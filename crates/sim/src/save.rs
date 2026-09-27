//! Save and load the cell world (technical design section 10).
//!
//! File layout (little-endian):
//! - magic `DFSIM`, format version (u32)
//! - seed (u64), tick (u64), width and height in chunks (i32, i32)
//! - material names (u32 count, then u16 length + UTF-8 for each), so ids can change between versions
//! - air temperature by row (u32 count + i16 values)
//! - chunks (u32 count), each: x, y (i32), dirty rectangle (4 × i32), lz4 block of the cell arrays
//! - particles (see `Particles::write`)
//!
//! Chunks that were never written are not saved; they are air.

use crate::chunk::{Chunk, LocalRect};
use crate::{SimConfig, Simulation};
use foundry_content::Content;
use foundry_core::{CHUNK_AREA, ChunkPos, MaterialId};
use std::io::{self, Read, Write};
use std::sync::Arc;

const MAGIC: &[u8; 5] = b"DFSIM";
const VERSION: u32 = 1;
/// Bytes of one chunk's cell arrays: mat, temp (2 bytes each), shade, life, motion, flags.
const RAW_CHUNK: usize = CHUNK_AREA * 8;

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
}

/// What a load found that did not match the current content.
#[derive(Debug, Default, Clone)]
pub struct LoadReport {
    /// Material names in the file that the current data files do not have. Their cells became air.
    pub unknown_materials: Vec<String>,
}

impl Simulation {
    /// Write the world to `w`.
    pub fn save(&self, w: &mut impl Write) -> Result<(), SaveError> {
        w.write_all(MAGIC)?;
        put_u32(w, VERSION)?;
        w.write_all(&self.seed.to_le_bytes())?;
        w.write_all(&self.tick.to_le_bytes())?;
        put_i32(w, self.world.width_chunks)?;
        put_i32(w, self.world.height_chunks)?;
        let ids = &self.content.materials.ids;
        put_u32(w, ids.len() as u32)?;
        for id in ids {
            w.write_all(&(id.len() as u16).to_le_bytes())?;
            w.write_all(id.as_bytes())?;
        }
        put_u32(w, self.air_temperature.len() as u32)?;
        for t in &self.air_temperature {
            w.write_all(&t.to_le_bytes())?;
        }
        let chunks: Vec<ChunkPos> = self.world.loaded_chunks().collect();
        put_u32(w, chunks.len() as u32)?;
        let mut raw = vec![0u8; RAW_CHUNK];
        for pos in chunks {
            let c = self.world.chunk(pos).expect("loaded chunk");
            put_i32(w, pos.x)?;
            put_i32(w, pos.y)?;
            for v in [c.dirty.x0, c.dirty.y0, c.dirty.x1, c.dirty.y1] {
                put_i32(w, v)?;
            }
            encode_chunk(c, &mut raw);
            let packed = lz4_flex::compress_prepend_size(&raw);
            put_u32(w, packed.len() as u32)?;
            w.write_all(&packed)?;
        }
        self.particles.write(w)?;
        Ok(())
    }

    /// Read a world written by `save`. Material names are matched to the current content.
    pub fn load(content: Arc<Content>, r: &mut impl Read) -> Result<(Simulation, LoadReport), SaveError> {
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
        let (wc, hc) = (get_i32(r)?, get_i32(r)?);
        if !(1..=4096).contains(&wc) || !(1..=4096).contains(&hc) {
            return Err(SaveError::Damaged(format!("world size {wc} x {hc} chunks")));
        }
        let mut report = LoadReport::default();
        let n = get_u32(r)? as usize;
        let mut remap = Vec::with_capacity(n);
        for _ in 0..n {
            let len = get_u16(r)? as usize;
            let mut name = vec![0u8; len];
            r.read_exact(&mut name)?;
            let name = String::from_utf8(name).map_err(|_| SaveError::Damaged("material name".into()))?;
            remap.push(content.material(&name).unwrap_or_else(|| {
                report.unknown_materials.push(name.clone());
                MaterialId::AIR
            }));
        }
        let rows = get_u32(r)? as usize;
        let mut air = Vec::with_capacity(rows);
        for _ in 0..rows {
            air.push(get_i16(r)?);
        }

        let config = SimConfig { width_chunks: wc, height_chunks: hc, seed, bedrock_border: false };
        let mut sim = Simulation::new(content, config);
        sim.tick = tick;
        sim.set_air_temperature(&air);
        let count = get_u32(r)?;
        let mut raw = vec![0u8; RAW_CHUNK];
        for _ in 0..count {
            let pos = ChunkPos::new(get_i32(r)?, get_i32(r)?);
            let dirty = LocalRect { x0: get_i32(r)?, y0: get_i32(r)?, x1: get_i32(r)?, y1: get_i32(r)? };
            let len = get_u32(r)? as usize;
            if len > RAW_CHUNK * 2 + 64 {
                return Err(SaveError::Damaged(format!("chunk {pos:?} is {len} bytes")));
            }
            let mut packed = vec![0u8; len];
            r.read_exact(&mut packed)?;
            let bytes = lz4_flex::decompress_size_prepended(&packed)
                .map_err(|e| SaveError::Damaged(format!("chunk {pos:?}: {e}")))?;
            if bytes.len() != RAW_CHUNK {
                return Err(SaveError::Damaged(format!("chunk {pos:?} has {} bytes", bytes.len())));
            }
            raw.copy_from_slice(&bytes);
            let stamp = sim.stamp;
            let Some(c) = sim.world.chunk_mut(pos) else {
                return Err(SaveError::Damaged(format!("chunk {pos:?} is outside the world")));
            };
            decode_chunk(&raw, c, &remap);
            c.dirty = dirty;
            c.version = stamp;
        }
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

    /// Load from a file.
    pub fn load_file(content: Arc<Content>, path: &std::path::Path) -> Result<(Simulation, LoadReport), SaveError> {
        let mut f = io::BufReader::new(std::fs::File::open(path)?);
        Simulation::load(content, &mut f)
    }
}

fn encode_chunk(c: &Chunk, out: &mut [u8]) {
    let (mat, rest) = out.split_at_mut(CHUNK_AREA * 2);
    let (temp, rest) = rest.split_at_mut(CHUNK_AREA * 2);
    for i in 0..CHUNK_AREA {
        mat[i * 2..i * 2 + 2].copy_from_slice(&c.mat[i].to_le_bytes());
        temp[i * 2..i * 2 + 2].copy_from_slice(&c.temp[i].to_le_bytes());
    }
    rest[..CHUNK_AREA].copy_from_slice(&c.shade);
    rest[CHUNK_AREA..CHUNK_AREA * 2].copy_from_slice(&c.life);
    rest[CHUNK_AREA * 2..CHUNK_AREA * 3].copy_from_slice(&c.motion);
    rest[CHUNK_AREA * 3..].copy_from_slice(&c.flags);
}

fn decode_chunk(raw: &[u8], c: &mut Chunk, remap: &[MaterialId]) {
    let (mat, rest) = raw.split_at(CHUNK_AREA * 2);
    let (temp, rest) = rest.split_at(CHUNK_AREA * 2);
    for i in 0..CHUNK_AREA {
        let saved = u16::from_le_bytes([mat[i * 2], mat[i * 2 + 1]]) as usize;
        c.mat[i] = remap.get(saved).copied().unwrap_or(MaterialId::AIR).0;
        c.temp[i] = i16::from_le_bytes([temp[i * 2], temp[i * 2 + 1]]);
    }
    c.shade.copy_from_slice(&rest[..CHUNK_AREA]);
    c.life.copy_from_slice(&rest[CHUNK_AREA..CHUNK_AREA * 2]);
    c.motion.copy_from_slice(&rest[CHUNK_AREA * 2..CHUNK_AREA * 3]);
    c.flags.copy_from_slice(&rest[CHUNK_AREA * 3..]);
}

fn put_u32(w: &mut impl Write, v: u32) -> io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

fn put_i32(w: &mut impl Write, v: i32) -> io::Result<()> {
    w.write_all(&v.to_le_bytes())
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
        let mut s = Simulation::new(content, SimConfig { width_chunks: 6, height_chunks: 4, seed: 11, bedrock_border: true });
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
        let mut a = Simulation::new(with.clone(), SimConfig { width_chunks: 2, height_chunks: 2, seed: 1, bedrock_border: true });
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
}
