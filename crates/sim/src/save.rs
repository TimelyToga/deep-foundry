//! Save and load the cell world (technical design section 10).
//!
//! Only changed chunks are saved: chunks that are not pristine. Every other chunk is made again by
//! the chunk source from the seed. So a large explored world that the player changed in a few
//! places saves small and fast. Packed chunks are written as they are (no new compression).
//!
//! File layout (little-endian):
//! - magic `DFSIM`, format version (u32)
//! - seed (u64), tick (u64)
//! - world height in chunks (i32), width in chunks (i32, 0 = no limit to the left and right)
//! - chunk source: name (u16 length + UTF-8), settings (u32 length + UTF-8)
//! - material names (u32 count, then u16 length + UTF-8 for each), so ids can change between versions
//! - air temperature by row (u32 count + i16 values)
//! - view: 1 byte (0 = none, 1 = present), then 4 × i32 if present
//! - anchors: next anchor id (u64), count (u32), then for each: id (u64), 4 × i32
//! - chunks (u32 count), each: x, y (i32), dirty rectangle (4 × i32), flags (u8: bit 0 = the
//!   cells are as the source made them; such a chunk is saved only for its dirty rectangle),
//!   packed cells (u32 length + an lz4 block of the cell arrays, see `pack::encode`)
//! - particles (see `Particles::write`)

use crate::chunk::LocalRect;
use crate::pack::{PackedChunk, RAW_CHUNK, decode, encode};
use crate::source::{self, ChunkSource};
use crate::{AnchorId, SimConfig, Simulation};
use foundry_content::Content;
use foundry_core::{CellRect, ChunkPos, MaterialId};
use std::io::{self, Read, Write};
use std::sync::Arc;

const MAGIC: &[u8; 5] = b"DFSIM";
/// Version 3: particles store their cell (i32) and the place inside it (f32).
const VERSION: u32 = 3;

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("read or write failed: {0}")]
    Io(#[from] io::Error),
    #[error("this is not a Deep Foundry world file")]
    NotAWorld,
    #[error("the world file has format version {0}; this game reads version {VERSION}")]
    Version(u32),
    #[error("the world file is damaged: {0}")]
    Damaged(String),
    #[error("the world was made by chunk source `{name}` ({settings}); {problem}")]
    Source { name: String, settings: String, problem: String },
}

/// What a load found that did not match the current content.
#[derive(Debug, Default, Clone)]
pub struct LoadReport {
    /// Material names in the file that the current data files do not have. Their cells became air.
    pub unknown_materials: Vec<String>,
    /// Number of chunks in the file.
    pub chunks: usize,
}

/// Finds the chunk source for a saved world from its name and settings. See
/// `Simulation::load_with_resolver`.
pub type SourceResolver = dyn Fn(&Content, &str, &str) -> Option<Arc<dyn ChunkSource>>;

enum SourcePick<'a> {
    Given(Option<Arc<dyn ChunkSource>>),
    Resolve(&'a SourceResolver),
}

impl Simulation {
    /// Write the world to `w`.
    pub fn save(&self, w: &mut impl Write) -> Result<(), SaveError> {
        w.write_all(MAGIC)?;
        put_u32(w, VERSION)?;
        w.write_all(&self.seed.to_le_bytes())?;
        w.write_all(&self.tick.to_le_bytes())?;
        put_i32(w, self.world.height_chunks())?;
        put_i32(w, self.world.width_chunks().unwrap_or(0))?;
        let source = self.world.source();
        put_text(w, source.name(), false)?;
        put_text(w, &source.settings(), true)?;
        let ids = &self.content.materials.ids;
        put_u32(w, ids.len() as u32)?;
        for id in ids {
            put_text(w, id, false)?;
        }
        put_u32(w, self.air_temperature.len() as u32)?;
        for t in &self.air_temperature {
            w.write_all(&t.to_le_bytes())?;
        }
        match self.view {
            Some(r) => {
                w.write_all(&[1])?;
                put_rect(w, r)?;
            }
            None => w.write_all(&[0])?,
        }
        w.write_all(&self.next_anchor.to_le_bytes())?;
        put_u32(w, self.anchors.len() as u32)?;
        for (id, r) in &self.anchors {
            w.write_all(&id.0.to_le_bytes())?;
            put_rect(w, *r)?;
        }
        let chunks = self.world.saved_positions();
        put_u32(w, chunks.len() as u32)?;
        for pos in chunks {
            let p = self.world.packed_copy(pos).expect("a saved chunk has cells");
            put_i32(w, pos.x)?;
            put_i32(w, pos.y)?;
            for v in [p.dirty.x0, p.dirty.y0, p.dirty.x1, p.dirty.y1] {
                put_i32(w, v)?;
            }
            w.write_all(&[p.pristine as u8])?;
            put_u32(w, p.bytes.len() as u32)?;
            w.write_all(&p.bytes)?;
        }
        self.particles.write(w)?;
        Ok(())
    }

    /// Read a world written by `save`. Material names are matched to the current content.
    /// Works for worlds made with a built-in chunk source (`AirSource`, `LayerSource`). For other
    /// sources use `load_with_source`.
    pub fn load(content: Arc<Content>, r: &mut impl Read) -> Result<(Simulation, LoadReport), SaveError> {
        Simulation::load_with_source(content, None, r)
    }

    /// Read a world written by `save`, with the chunk source that made it. The source must have
    /// the same name and settings as the one in the file. `None`: use a built-in source.
    pub fn load_with_source(
        content: Arc<Content>,
        source: Option<Arc<dyn ChunkSource>>,
        r: &mut impl Read,
    ) -> Result<(Simulation, LoadReport), SaveError> {
        Simulation::load_inner(content, SourcePick::Given(source), r)
    }

    /// Read a world written by `save`. `resolve` gets the source name and settings from the
    /// file and returns the source that made the world, or `None` if it does not know it.
    /// Built-in sources are tried after `resolve`.
    pub fn load_with_resolver(content: Arc<Content>, resolve: &SourceResolver, r: &mut impl Read) -> Result<(Simulation, LoadReport), SaveError> {
        Simulation::load_inner(content, SourcePick::Resolve(resolve), r)
    }

    fn load_inner(content: Arc<Content>, source: SourcePick, r: &mut impl Read) -> Result<(Simulation, LoadReport), SaveError> {
        let mut magic = [0u8; 5];
        r.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(SaveError::NotAWorld);
        }
        let version = get_u32(r)?;
        if version != VERSION {
            return Err(SaveError::Version(version));
        }
        let seed = get_u64(r)?;
        let tick = get_u64(r)?;
        let (height, width) = (get_i32(r)?, get_i32(r)?);
        if !(1..=4096).contains(&height) || !(0..=1 << 24).contains(&width) {
            return Err(SaveError::Damaged(format!("world size {width} x {height} chunks")));
        }
        let source_name = get_text(r, false)?;
        let source_settings = get_text(r, true)?;
        let source = match source {
            SourcePick::Given(given) => given,
            SourcePick::Resolve(resolve) => resolve(&content, &source_name, &source_settings),
        };
        let source = match source {
            Some(s) if s.name() == source_name && s.settings() == source_settings => s,
            Some(s) => {
                return Err(SaveError::Source {
                    name: source_name,
                    settings: source_settings,
                    problem: format!("the given source is `{}` ({})", s.name(), s.settings()),
                });
            }
            None => source::built_in(&content, &source_name, &source_settings).ok_or_else(|| SaveError::Source {
                name: source_name.clone(),
                settings: source_settings.clone(),
                problem: "load it with `Simulation::load_with_source` and that source".into(),
            })?,
        };

        let mut report = LoadReport::default();
        let n = get_u32(r)? as usize;
        if n > u16::MAX as usize + 1 {
            return Err(SaveError::Damaged(format!("{n} materials")));
        }
        let mut remap = Vec::with_capacity(n);
        for _ in 0..n {
            let name = get_text(r, false)?;
            remap.push(content.material(&name).unwrap_or_else(|| {
                report.unknown_materials.push(name.clone());
                MaterialId::AIR
            }));
        }
        let same_ids = remap.iter().enumerate().all(|(i, m)| m.index() == i);
        let rows = get_u32(r)? as usize;
        if rows > 1 << 20 {
            return Err(SaveError::Damaged(format!("{rows} rows of air temperature")));
        }
        let mut air = Vec::with_capacity(rows);
        for _ in 0..rows {
            air.push(get_i16(r)?);
        }
        let mut flag = [0u8; 1];
        r.read_exact(&mut flag)?;
        let view = if flag[0] == 1 { Some(get_rect(r)?) } else { None };
        let next_anchor = get_u64(r)?;
        let anchor_count = get_u32(r)?;
        if anchor_count > 1 << 20 {
            return Err(SaveError::Damaged(format!("{anchor_count} anchors")));
        }
        let mut anchors = Vec::with_capacity(anchor_count as usize);
        for _ in 0..anchor_count {
            anchors.push((AnchorId(get_u64(r)?), get_rect(r)?));
        }

        let config = SimConfig {
            seed,
            width_chunks: (width > 0).then_some(width),
            sky_chunks: 0,
            depth_chunks: height,
            bedrock_border: false,
            source: Some(source),
        };
        let mut sim = Simulation::new(content, config);
        sim.tick = tick;
        sim.set_air_temperature(&air);
        sim.view = view;
        sim.anchors = anchors;
        sim.next_anchor = next_anchor;
        let stamp = sim.stamp;
        let count = get_u32(r)?;
        let mut raw = vec![0u8; RAW_CHUNK];
        for _ in 0..count {
            let pos = ChunkPos::new(get_i32(r)?, get_i32(r)?);
            let dirty = LocalRect { x0: get_i32(r)?, y0: get_i32(r)?, x1: get_i32(r)?, y1: get_i32(r)? };
            let mut flags = [0u8; 1];
            r.read_exact(&mut flags)?;
            let pristine = flags[0] & 1 != 0;
            let len = get_u32(r)? as usize;
            if len > RAW_CHUNK * 2 + 64 {
                return Err(SaveError::Damaged(format!("chunk {pos:?} is {len} bytes")));
            }
            if !sim.world.chunk_in_bounds(pos) {
                return Err(SaveError::Damaged(format!("chunk {pos:?} is outside the world")));
            }
            let mut bytes = vec![0u8; len];
            r.read_exact(&mut bytes)?;
            // Check the cells now, so a damaged file is an error here and not a crash later.
            let cells = lz4_flex::decompress_size_prepended(&bytes)
                .map_err(|e| SaveError::Damaged(format!("chunk {pos:?}: {e}")))?;
            if cells.len() != RAW_CHUNK {
                return Err(SaveError::Damaged(format!("chunk {pos:?} has {} bytes of cells", cells.len())));
            }
            if !same_ids {
                // Material ids changed: decode with the new ids and pack again.
                let mut c = crate::chunk::Chunk::new_air();
                decode(&cells, &mut c, Some(&remap));
                encode(&c, &mut raw);
                bytes = lz4_flex::compress_prepend_size(&raw);
            }
            let version = if pristine { crate::chunk::GENERATED_VERSION } else { stamp };
            sim.world.insert_packed(pos, PackedChunk { bytes: bytes.into_boxed_slice(), dirty, version, pristine });
        }
        report.chunks = count as usize;
        sim.particles = crate::particles::Particles::read(r, &remap)?;
        report.unknown_materials.sort();
        report.unknown_materials.dedup();
        Ok((sim, report))
    }

    /// Save to a file (written to a temporary file first, so a failed save never breaks an old one).
    pub fn save_file(&self, path: &std::path::Path) -> Result<(), SaveError> {
        let tmp = path.with_extension("tmp");
        {
            let mut f = io::BufWriter::new(std::fs::File::create(&tmp)?);
            self.save(&mut f)?;
            f.flush()?;
        }
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Load from a file. See `load`.
    pub fn load_file(content: Arc<Content>, path: &std::path::Path) -> Result<(Simulation, LoadReport), SaveError> {
        Simulation::load_file_with_source(content, None, path)
    }

    /// Load from a file. See `load_with_resolver`.
    pub fn load_file_with_resolver(content: Arc<Content>, resolve: &SourceResolver, path: &std::path::Path) -> Result<(Simulation, LoadReport), SaveError> {
        let mut f = io::BufReader::new(std::fs::File::open(path)?);
        Simulation::load_with_resolver(content, resolve, &mut f)
    }

    /// Load from a file with the chunk source that made the world. See `load_with_source`.
    pub fn load_file_with_source(
        content: Arc<Content>,
        source: Option<Arc<dyn ChunkSource>>,
        path: &std::path::Path,
    ) -> Result<(Simulation, LoadReport), SaveError> {
        let mut f = io::BufReader::new(std::fs::File::open(path)?);
        Simulation::load_with_source(content, source, &mut f)
    }
}

fn put_u32(w: &mut impl Write, v: u32) -> io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

fn put_i32(w: &mut impl Write, v: i32) -> io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

fn put_rect(w: &mut impl Write, r: CellRect) -> io::Result<()> {
    for v in [r.x0, r.y0, r.x1, r.y1] {
        put_i32(w, v)?;
    }
    Ok(())
}

/// Text with a u16 length, or a u32 length if `long`.
fn put_text(w: &mut impl Write, s: &str, long: bool) -> io::Result<()> {
    if long {
        put_u32(w, s.len() as u32)?;
    } else {
        let len = u16::try_from(s.len()).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "text too long"))?;
        w.write_all(&len.to_le_bytes())?;
    }
    w.write_all(s.as_bytes())
}

fn get_text(r: &mut impl Read, long: bool) -> Result<String, SaveError> {
    let len = if long { get_u32(r)? as usize } else { get_u16(r)? as usize };
    if len > 1 << 20 {
        return Err(SaveError::Damaged(format!("text of {len} bytes")));
    }
    let mut bytes = vec![0u8; len];
    r.read_exact(&mut bytes)?;
    String::from_utf8(bytes).map_err(|_| SaveError::Damaged("text is not UTF-8".into()))
}

fn get_rect(r: &mut impl Read) -> io::Result<CellRect> {
    Ok(CellRect::new(get_i32(r)?, get_i32(r)?, get_i32(r)?, get_i32(r)?))
}

fn get_u16(r: &mut impl Read) -> io::Result<u16> {
    let mut b = [0u8; 2];
    r.read_exact(&mut b)?;
    Ok(u16::from_le_bytes(b))
}

fn get_i16(r: &mut impl Read) -> io::Result<i16> {
    let mut b = [0u8; 2];
    r.read_exact(&mut b)?;
    Ok(i16::from_le_bytes(b))
}

fn get_u32(r: &mut impl Read) -> io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn get_i32(r: &mut impl Read) -> io::Result<i32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(i32::from_le_bytes(b))
}

fn get_u64(r: &mut impl Read) -> io::Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundry_core::{CellPos, PaintMode};

    fn world() -> Simulation {
        let content = Arc::new(Content::load_default().unwrap());
        let mut s = Simulation::new(content, SimConfig::finite(6, 4, 11));
        let c = s.content().clone();
        s.paint(CellPos::new(100, 60), 20, c.expect_material("sand"), PaintMode::Replace, None);
        s.paint(CellPos::new(250, 60), 25, c.expect_material("water"), PaintMode::Replace, None);
        s.paint(CellPos::new(300, 200), 8, c.expect_material("lava"), PaintMode::Replace, None);
        for _ in 0..50 {
            s.tick();
        }
        s
    }

    #[test]
    fn save_then_load_gives_the_same_world_and_future() {
        let mut a = world();
        let mut bytes = vec![];
        a.save(&mut bytes).unwrap();
        let (mut b, report) = Simulation::load(a.content().clone(), &mut bytes.as_slice()).unwrap();
        assert!(report.unknown_materials.is_empty());
        assert_eq!(a.tick_count(), b.tick_count());
        assert_eq!(a.size_cells(), b.size_cells());
        assert_eq!(a.world_hash(), b.world_hash());
        // The loaded world continues exactly like the original.
        for _ in 0..200 {
            a.tick();
            b.tick();
        }
        assert_eq!(a.world_hash(), b.world_hash());
    }

    #[test]
    fn unknown_materials_become_air_and_are_reported() {
        let base = r##"[Material(id: "air", name: "Air", phase: Empty, colors: ["#000000"]),
            Material(id: "bedrock", name: "Bedrock", phase: Solid, colors: ["#111111"], hardness: 255),
            Material(id: "sand", name: "Sand", phase: Powder, colors: ["#ddcc88"], density: 1600)]"##;
        let extra = r##"[Material(id: "glowdust", name: "Glow dust", phase: Powder, colors: ["#88ff88"], density: 900)]"##;
        let with = Arc::new(Content::from_ron(&[base, extra], &[]).unwrap());
        let without = Arc::new(Content::from_ron(&[base], &[]).unwrap());
        let mut a = Simulation::new(with.clone(), SimConfig::finite(2, 2, 1));
        a.set_cell(CellPos::new(10, 10), with.expect_material("sand"), None);
        a.set_cell(CellPos::new(20, 10), with.expect_material("glowdust"), None);
        let mut bytes = vec![];
        a.save(&mut bytes).unwrap();
        let (b, report) = Simulation::load(without.clone(), &mut bytes.as_slice()).unwrap();
        assert_eq!(report.unknown_materials, vec!["glowdust".to_string()]);
        assert_eq!(b.cell(CellPos::new(10, 10)).material, without.expect_material("sand"));
        assert!(b.cell(CellPos::new(20, 10)).material.is_air());
        let mut bad = bytes.clone();
        bad[0] = b'X';
        assert!(matches!(Simulation::load(with, &mut bad.as_slice()), Err(SaveError::NotAWorld)));
    }

    #[test]
    fn a_truncated_file_is_an_error() {
        let a = world();
        let mut bytes = vec![];
        a.save(&mut bytes).unwrap();
        bytes.truncate(bytes.len() / 2);
        assert!(Simulation::load(a.content().clone(), &mut bytes.as_slice()).is_err());
    }

    /// A source that is not built in must be given to the load, with the same settings.
    #[test]
    fn a_custom_source_must_match() {
        struct Stripes(u16);
        impl ChunkSource for Stripes {
            fn generate(&self, cells: &mut crate::ChunkCells) {
                if cells.pos.y >= 2 {
                    cells.mat.fill(self.0);
                }
            }
            fn name(&self) -> &str {
                "stripes"
            }
            fn settings(&self) -> String {
                format!("material={}", self.0)
            }
        }
        let content = Arc::new(Content::load_default().unwrap());
        let stone = content.expect_material("stone").0;
        let sand = content.expect_material("sand").0;
        let source: Arc<dyn ChunkSource> = Arc::new(Stripes(stone));
        let mut a = Simulation::new(content.clone(), SimConfig { sky_chunks: 2, depth_chunks: 4, ..SimConfig::infinite(3, Some(source.clone())) });
        a.paint(CellPos::new(10, 100), 5, content.expect_material("sand"), PaintMode::Replace, None);
        let mut bytes = vec![];
        a.save(&mut bytes).unwrap();
        assert!(matches!(Simulation::load(content.clone(), &mut bytes.as_slice()), Err(SaveError::Source { .. })));
        let other: Arc<dyn ChunkSource> = Arc::new(Stripes(sand));
        assert!(matches!(
            Simulation::load_with_source(content.clone(), Some(other), &mut bytes.as_slice()),
            Err(SaveError::Source { .. })
        ));
        let (b, _) = Simulation::load_with_source(content, Some(source), &mut bytes.as_slice()).unwrap();
        assert_eq!(a.world_hash(), b.world_hash());
        assert_eq!(b.cell(CellPos::new(500, 200)).material.0, stone);
    }
}
