//! World generation (Milestone 2A): the surface and the upper stone layer.
//!
//! `WorldGen` is a `foundry_sim::ChunkSource`. The world asks it for one chunk at a time, in any
//! order and on many threads. The cells of a chunk depend only on the settings, the world seed
//! and the chunk position (see the rules in `crates/sim/src/source.rs`).
//!
//! # The world
//!
//! - The sky is above the surface level (`WorldGenSettings::surface_y`, row 1024 in the default
//!   world). The ground goes up and down around it with hills, cliffs and dunes.
//! - Biomes follow each other along x in zones of about 3200 cells. The start zone at x = 0 is
//!   temperate. The zone to the right is a desert and the zone to the left is a tundra. Further
//!   out, each zone gets a biome from the seed. `surface.rs` makes the ground height and the
//!   surface features (lakes, rivers, salt flats, coal outcrops).
//! - The start area (x = -1536 to 1536) always has the same features, moved a little by the seed:
//!   a flat place for the Hub at x = 0, a lake, a river bed with tin gravel, a coal outcrop,
//!   malachite and clay near the Hub, and groups of trees (`surface::start_features`).
//! - Below the soil: the surface layer (to 600 cells deep), the upper stone layer (600 to 1800),
//!   then deep rock (granite, then basalt) and bedrock at the bottom. `chunk.rs` fills the cells:
//!   caves, ore veins, limestone bands, coal seams, gravel and sand pockets, water pockets and
//!   methane pockets.
//! - Trees are made in `trees.rs`.
//!
//! # Stable at the start
//!
//! New chunks sleep, and there is no settle step. So every powder cell must lie on support: after
//! a chunk is filled, a powder cell with air, gas or liquid below it or diagonally below it becomes
//! a solid (stone, or ice for snow). Lakes and water pockets have a flat top and closed walls.
//!
//! # Speed
//!
//! Noise is sampled every 8 cells and read with linear steps between the samples (`noise::Coarse`).
//! Chunks high in the sky return at once.

mod chunk;
pub mod noise;
mod surface;
mod trees;

use foundry_content::{Content, Phase};
use foundry_core::{CHUNK_SIZE, MaterialId};
use foundry_sim::{ChunkCells, ChunkSource};

pub use surface::Biome;

/// The version of the generator. It is part of the saved settings, so a world file made by an
/// older version is not loaded with a newer generator that would make different cells.
pub const VERSION: u32 = 1;

/// Depth of the surface layer below the surface level, in cells.
pub const SURFACE_LAYER: i32 = 600;
/// Depth where the upper stone layer ends.
pub const UPPER_STONE_END: i32 = 1800;
/// Depth where the deep rock layer (granite) ends.
pub const DEEP_ROCK_END: i32 = 3600;

/// The settings that change the cells. Saved in world files as text (`to_text`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldGenSettings {
    /// The surface level: the row where the ground is, about (the hills go up and down around it).
    pub surface_y: i32,
    /// The row below the last row of the world. The lowest rows are bedrock.
    pub bottom_y: i32,
}

impl Default for WorldGenSettings {
    /// The default world: 16 chunks of sky and 128 chunks below the surface level.
    fn default() -> Self {
        Self::for_world(foundry_sim::DEFAULT_SKY_CHUNKS, foundry_sim::DEFAULT_DEPTH_CHUNKS)
    }
}

impl WorldGenSettings {
    /// The settings for a world with this many chunks of sky and this many chunks below the
    /// surface level (the same numbers as `foundry_sim::SimConfig`).
    pub fn for_world(sky_chunks: i32, depth_chunks: i32) -> Self {
        Self { surface_y: sky_chunks * CHUNK_SIZE, bottom_y: (sky_chunks + depth_chunks) * CHUNK_SIZE }
    }

    /// The settings as short text, for example "v1 surface=1024 bottom=9216".
    pub fn to_text(&self) -> String {
        format!("v{VERSION} surface={} bottom={}", self.surface_y, self.bottom_y)
    }

    /// Read the text of `to_text`. `None` if the text is not valid or has another version.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split_whitespace();
        if parts.next()? != format!("v{VERSION}") {
            return None;
        }
        let (mut surface, mut bottom) = (None, None);
        for part in parts {
            let (key, value) = part.split_once('=')?;
            let value: i32 = value.parse().ok()?;
            match key {
                "surface" => surface = Some(value),
                "bottom" => bottom = Some(value),
                _ => return None,
            }
        }
        let s = Self { surface_y: surface?, bottom_y: bottom? };
        (s.surface_y > 0 && s.bottom_y > s.surface_y).then_some(s)
    }
}

/// The material ids that the generator uses. A material that is missing from the data files
/// becomes stone (or air for a gas), so the generator works with any content.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Mats {
    pub stone: u16,
    pub dirt: u16,
    pub grass: u16,
    pub sand: u16,
    pub gravel: u16,
    pub clay: u16,
    pub water: u16,
    pub ice: u16,
    pub snow: u16,
    pub frozen_ground: u16,
    pub wood: u16,
    pub rubber_wood: u16,
    pub leaves: u16,
    pub salt: u16,
    pub coal: u16,
    pub malachite: u16,
    pub cassiterite: u16,
    pub magnetite: u16,
    pub hematite: u16,
    pub chalcopyrite: u16,
    pub limestone: u16,
    pub granite: u16,
    pub basalt: u16,
    pub bedrock: u16,
    pub methane: u16,
}

impl Mats {
    fn new(content: &Content) -> Self {
        let stone = content.material("stone").unwrap_or(MaterialId::AIR).0;
        let id = |name: &str| content.material(name).map_or(stone, |m| m.0);
        Mats {
            stone,
            dirt: id("dirt"),
            grass: id("grass"),
            sand: id("sand"),
            gravel: id("gravel"),
            clay: id("clay"),
            water: id("water"),
            ice: id("ice"),
            snow: id("snow"),
            frozen_ground: id("frozen_ground"),
            wood: id("wood"),
            rubber_wood: id("rubber_wood"),
            leaves: id("leaves"),
            salt: id("salt"),
            coal: id("coal"),
            malachite: id("malachite"),
            cassiterite: id("cassiterite"),
            magnetite: id("magnetite"),
            hematite: id("hematite"),
            chalcopyrite: id("chalcopyrite"),
            limestone: id("limestone"),
            granite: id("granite"),
            basalt: id("basalt"),
            bedrock: id("bedrock"),
            methane: content.material("methane").map_or(0, |m| m.0),
        }
    }
}

/// Bits of `WorldGen::kind`.
pub(crate) const POWDER: u8 = 1;
/// Air, gas, fire or liquid: a powder cell above it falls or slides.
pub(crate) const OPEN: u8 = 2;

/// The world generator. See the module documentation.
pub struct WorldGen {
    settings: WorldGenSettings,
    pub(crate) m: Mats,
    /// `POWDER` and `OPEN` bits for each material id (all 65536 ids, so no bounds check).
    pub(crate) kind: Box<[u8; 65536]>,
    /// The solid that replaces a powder cell with no support (for each material id).
    pub(crate) support: Vec<u16>,
    /// Air temperature of each row, from the top of the world down.
    air: Vec<i16>,
    /// -1 to 1 for each cell of a 64 x 64 square, repeated over the world: small random changes
    /// at the edges of shapes, so they look rough. Faster than a hash for each cell.
    pub(crate) jitter: Box<[f32; 4096]>,
}

impl WorldGen {
    pub fn new(content: &Content, settings: WorldGenSettings) -> Self {
        let m = Mats::new(content);
        let mats = &content.materials;
        let mut kind = Box::new([0u8; 65536]);
        for (k, p) in kind.iter_mut().zip(&mats.phase) {
            *k = match p {
                Phase::Powder => POWDER,
                Phase::Solid => 0,
                _ => OPEN,
            };
        }
        let support = (0..mats.len()).map(|i| if i as u16 == m.snow { m.ice } else { m.stone }).collect();
        let air = (0..settings.bottom_y.max(1)).map(|y| air_temperature(&settings, y)).collect();
        // Random values, then smoothed twice with their 8 neighbors (the square wraps around),
        // so the edges get small bumps and not single loose cells.
        let mut jitter = Box::new([0.0f32; 4096]);
        for (i, j) in jitter.iter_mut().enumerate() {
            *j = noise::signed(noise::hash1(0x6a09_e667, i as i32));
        }
        for _ in 0..2 {
            let old = jitter.clone();
            for y in 0..64 {
                for x in 0..64 {
                    let mut sum = 0.0;
                    for dy in [63, 0, 1] {
                        for dx in [63, 0, 1] {
                            sum += old[((y + dy) % 64) * 64 + (x + dx) % 64];
                        }
                    }
                    jitter[y * 64 + x] = sum / 9.0;
                }
            }
        }
        // Scale to a spread of about -1 to 1 (standard deviation 0.5), and never more (the carve
        // blocks in chunk.rs count on it).
        let sd = (jitter.iter().map(|j| j * j).sum::<f32>() / 4096.0).sqrt().max(1e-6);
        for j in jitter.iter_mut() {
            *j = (*j * 0.5 / sd).clamp(-1.0, 1.0);
        }
        Self { settings, m, kind, support, air, jitter }
    }

    /// Make the generator again from the text of `ChunkSource::settings` (for loading a world file).
    pub fn from_settings(content: &Content, text: &str) -> Option<Self> {
        WorldGenSettings::parse(text).map(|s| Self::new(content, s))
    }

    /// The settings (the text form for world files is `ChunkSource::settings`).
    pub fn world_settings(&self) -> &WorldGenSettings {
        &self.settings
    }

    /// The air temperature of each row of cells (°C), from the top of the world down.
    /// For `Simulation::set_air_temperature`.
    pub fn air_temperature_rows(&self) -> &[i16] {
        &self.air
    }

    /// The air temperature at a cell, with the biome: the tundra is colder near the surface.
    /// The simulation has only one air temperature per row for now (see
    /// `docs/design/requests/worldgen.md`); this is the value that a column-based air
    /// temperature would use.
    pub fn air_temperature_at(&self, seed: u64, x: i32, y: i32) -> i16 {
        let row = self.air.get(y.max(0) as usize).copied().unwrap_or(foundry_core::DEFAULT_TEMPERATURE);
        let depth = y - self.settings.surface_y;
        if depth < 400 && self.biome_at(seed, x) == Biome::Tundra {
            let t = noise::smoothstep((depth - 150) as f32 / 250.0);
            return (-20.0 + (row as f32 + 20.0) * t).round() as i16;
        }
        row
    }

    /// The top row of the ground in column `x`: the first cell that is not air, water, ice on
    /// water, or part of a tree.
    pub fn ground_y(&self, seed: u64, x: i32) -> i32 {
        surface::Ctx::new(self, seed, x, x).ground_at(x).g
    }

    /// The top row of water in column `x`, or `None` if the column has no lake or river.
    pub fn water_y(&self, seed: u64, x: i32) -> Option<i32> {
        let g = surface::Ctx::new(self, seed, x, x).ground_at(x);
        (g.water < g.g).then_some(g.water)
    }

    /// The biome of the surface in column `x`.
    pub fn biome_at(&self, seed: u64, x: i32) -> Biome {
        surface::Ctx::new(self, seed, x, x).biome_at(x)
    }

    /// The start place: x = 0 and the row of the flat ground there (a multiple of the tile size,
    /// so the Hub stands on it with no gap).
    pub fn start(&self, seed: u64) -> (i32, i32) {
        (0, self.ground_y(seed, 0))
    }
}

/// The air temperature of row `y` (°C): 20 °C (`DEFAULT_TEMPERATURE`) in the sky and the surface
/// layer, and warmer with depth (see docs/design/01-game-design.md section 5.2).
///
/// The sky and the surface layer must be at `DEFAULT_TEMPERATURE`: new chunks put their cells
/// there at that temperature (`Fill::write`), and air is at rest only when it is at the air
/// temperature of its row. With a colder sky, every air cell cooled slowly with random ±1 °C
/// steps, so no chunk near the surface ever went to heat sleep (about 170 awake chunks around the
/// player, 4 ms per tick and spikes of 70 ms).
fn air_temperature(s: &WorldGenSettings, y: i32) -> i16 {
    // (depth below the surface level, °C). Straight lines between the points.
    const POINTS: [(f32, f32); 10] = [
        (-1024.0, 20.0),
        (0.0, 20.0),
        (600.0, 20.0),
        (1800.0, 30.0),
        (1900.0, 40.0),
        (3600.0, 80.0),
        (3700.0, 150.0),
        (5400.0, 400.0),
        (7200.0, 600.0),
        (7300.0, 800.0),
    ];
    let d = (y - s.surface_y) as f32;
    if d <= POINTS[0].0 {
        return POINTS[0].1 as i16;
    }
    for w in POINTS.windows(2) {
        let ((d0, t0), (d1, t1)) = (w[0], w[1]);
        if d < d1 {
            return (t0 + (t1 - t0) * (d - d0) / (d1 - d0)).round() as i16;
        }
    }
    POINTS[POINTS.len() - 1].1 as i16
}

impl ChunkSource for WorldGen {
    fn generate(&self, cells: &mut ChunkCells) {
        chunk::generate(self, cells);
    }

    fn name(&self) -> &str {
        "worldgen"
    }

    fn settings(&self) -> String {
        self.settings.to_text()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_text_round_trip() {
        let s = WorldGenSettings::for_world(16, 40);
        assert_eq!(s.to_text(), "v1 surface=1024 bottom=3584");
        assert_eq!(WorldGenSettings::parse(&s.to_text()), Some(s));
        assert_eq!(WorldGenSettings::parse("v0 surface=1024 bottom=3584"), None);
        assert_eq!(WorldGenSettings::parse("v1 surface=1024"), None);
        assert_eq!(WorldGenSettings::parse("v1 surface=1024 bottom=3584 x=1"), None);
    }

    #[test]
    fn air_is_warmer_below() {
        let s = WorldGenSettings::default();
        let t = |d: i32| air_temperature(&s, s.surface_y + d);
        // The sky and the surface layer are at the temperature of new cells (see `air_temperature`).
        assert_eq!(t(0), foundry_core::DEFAULT_TEMPERATURE);
        assert_eq!(t(-1024), foundry_core::DEFAULT_TEMPERATURE);
        assert_eq!(t(600), foundry_core::DEFAULT_TEMPERATURE);
        assert!(t(700) >= 20 && t(700) <= 25);
        assert!(t(2500) > 40 && t(2500) < 80);
        assert!(t(4000) > 150);
    }
}
