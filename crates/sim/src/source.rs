//! Chunk sources: they make the cells of new chunks.
//!
//! The world does not keep all of its chunks in memory. When it needs a chunk for the first time
//! (the view shows it, a cell in it is written, or it is next to a chunk that updates), it asks
//! its `ChunkSource` for the cells. The world also drops chunks that did not change and asks the
//! source for them again later. So the source must give the same cells every time.
//!
//! # How to write a chunk source (for example the world generator)
//!
//! 1. Make a type that holds the settings (material ids, noise settings, and so on) and
//!    implement `ChunkSource` for it. Look up material ids once, when you make the type.
//! 2. In `generate`, write the material of each cell into `cells.mat`, for the chunk at
//!    `cells.pos`. Use `foundry_core::local_index(x, y)` for the index (x and y are 0 to 63,
//!    row by row from the top). The world cell of local (0, 0) is `cells.pos.origin()`.
//!    Cells that you do not write stay air.
//! 3. Temperatures are optional. Each cell gets the default temperature of its material, unless
//!    you write a temperature into `cells.temp` (for example hot rock in the magma layer).
//! 4. The result must depend only on your settings, `cells.seed` and `cells.pos`. It must not
//!    depend on other chunks or on the order of calls. The world calls `generate` for many
//!    chunks at once on different threads, and again for a chunk that it dropped.
//! 5. Things that cross chunk borders (caves, ore veins, trees, lakes) must come from the world
//!    position. For example: noise sampled at the cell position, or a list of features made from
//!    the seed and a large region number, where each chunk draws only its own part.
//! 6. New chunks sleep: they do not update until something next to them changes. So the terrain
//!    must be stable (no floating sand or water), or set `cells.awake = true` for a chunk that must
//!    move at once.
//! 7. Keep it fast: about 50 microseconds per chunk or less. A view that moves quickly needs
//!    a few hundred new chunks per second.
//! 8. The world never asks for chunks above the top (y < 0) or below the bottom of the world.
//!    Put bedrock in the lowest rows.
//! 9. `name` and `settings` are saved in world files. A world file can only be loaded with a
//!    source that has the same name and settings.
//!
//! The world sets the shade, life, motion and flags of the new cells itself.

use foundry_content::Content;
use foundry_core::{CHUNK_AREA, CHUNK_SIZE, ChunkPos, MaterialId, local_index};
use std::sync::Arc;

/// The cells of a new chunk, for a `ChunkSource` to fill.
pub struct ChunkCells<'a> {
    /// The chunk position. The world cell of local (0, 0) is `pos.origin()`.
    pub pos: ChunkPos,
    /// The world seed. Use it for all random choices.
    pub seed: u64,
    /// Material id of each cell. Index: `foundry_core::local_index(x, y)`. Starts as air.
    pub mat: &'a mut [u16; CHUNK_AREA],
    /// Temperature of each cell in °C. Starts as `ChunkCells::MATERIAL_DEFAULT`: the cell gets
    /// the default temperature of its material.
    pub temp: &'a mut [i16; CHUNK_AREA],
    /// Set this to true if the chunk has cells that must move at once (for example a ball of water
    /// in the air). Otherwise the chunk sleeps until a cell next to it changes.
    pub awake: bool,
}

impl ChunkCells<'_> {
    /// The value in `temp` that means "the default temperature of the material".
    pub const MATERIAL_DEFAULT: i16 = i16::MIN;

    /// Set the material of one cell. `x` and `y` are local (0 to 63).
    #[inline]
    pub fn set(&mut self, x: i32, y: i32, m: MaterialId) {
        self.mat[local_index(x, y)] = m.0;
    }

    /// Set every cell to one material.
    pub fn fill(&mut self, m: MaterialId) {
        self.mat.fill(m.0);
    }

    /// World row (y) of local row 0.
    pub fn top(&self) -> i32 {
        self.pos.y * CHUNK_SIZE
    }

    /// World column (x) of local column 0.
    pub fn left(&self) -> i32 {
        self.pos.x * CHUNK_SIZE
    }
}

/// Makes the cells of new chunks. See the module documentation for the rules.
pub trait ChunkSource: Send + Sync {
    /// Write the cells of the chunk at `cells.pos`.
    fn generate(&self, cells: &mut ChunkCells);

    /// A short name, for example "layers". Saved in world files.
    fn name(&self) -> &str;

    /// The settings that change the output, as short text. Saved in world files. Do not include
    /// the world seed; the world saves it.
    fn settings(&self) -> String {
        String::new()
    }
}

/// All air. The source of a finite test box.
#[derive(Debug, Default, Clone, Copy)]
pub struct AirSource;

impl ChunkSource for AirSource {
    fn generate(&self, _cells: &mut ChunkCells) {}

    fn name(&self) -> &str {
        "air"
    }
}

/// A simple world: air above a flat surface line, stone below it, and 2 rows of bedrock at the
/// bottom of the world.
#[derive(Debug, Clone)]
pub struct LayerSource {
    /// The first row of stone.
    pub surface_y: i32,
    /// The row below the last row of the world. Rows `bottom_y - 2` and `bottom_y - 1` are bedrock.
    pub bottom_y: i32,
    stone: MaterialId,
    bedrock: MaterialId,
}

impl LayerSource {
    pub fn new(content: &Content, surface_y: i32, bottom_y: i32) -> Self {
        let stone = content.material("stone").unwrap_or(MaterialId::AIR);
        let bedrock = content.material("bedrock").unwrap_or(stone);
        Self { surface_y, bottom_y, stone, bedrock }
    }

    /// Make the source again from the text of `settings()` (for loading a world file).
    pub fn from_settings(content: &Content, settings: &str) -> Option<Self> {
        let mut surface = None;
        let mut bottom = None;
        for part in settings.split_whitespace() {
            let (key, value) = part.split_once('=')?;
            let value: i32 = value.parse().ok()?;
            match key {
                "surface" => surface = Some(value),
                "bottom" => bottom = Some(value),
                _ => return None,
            }
        }
        Some(Self::new(content, surface?, bottom?))
    }
}

impl ChunkSource for LayerSource {
    fn generate(&self, cells: &mut ChunkCells) {
        let top = cells.top();
        for ly in 0..CHUNK_SIZE {
            let y = top + ly;
            let m = if y >= self.bottom_y - 2 {
                self.bedrock
            } else if y >= self.surface_y {
                self.stone
            } else {
                continue;
            };
            let row = local_index(0, ly);
            cells.mat[row..row + CHUNK_SIZE as usize].fill(m.0);
        }
    }

    fn name(&self) -> &str {
        "layers"
    }

    fn settings(&self) -> String {
        format!("surface={} bottom={}", self.surface_y, self.bottom_y)
    }
}

/// Make a built-in source again from its saved name and settings. `None` for other sources.
pub fn built_in(content: &Content, name: &str, settings: &str) -> Option<Arc<dyn ChunkSource>> {
    match name {
        "air" => Some(Arc::new(AirSource)),
        "layers" => LayerSource::from_settings(content, settings).map(|s| Arc::new(s) as Arc<dyn ChunkSource>),
        _ => None,
    }
}
