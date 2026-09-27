//! Snapshots go from the simulation thread to the main thread after each tick.

use crate::consts::CHUNK_AREA;
use crate::pos::ChunkPos;

/// One cell as the GPU sees it. The layout matches the `Rgba16Uint` world texture:
///
/// - `[0]` material id
/// - `[1]` temperature in °C, as the bits of an `i16`
/// - `[2]` shade in the low 8 bits, life in the high 8 bits
/// - `[3]` flags (the same bits as the simulation cell flags)
pub type CellTexel = [u16; 4];

#[inline(always)]
pub fn pack_texel(material: u16, temperature: i16, shade: u8, life: u8, flags: u8) -> CellTexel {
    [material, temperature as u16, shade as u16 | ((life as u16) << 8), flags as u16]
}

/// The cells of one chunk, row by row from the top. Always `CHUNK_AREA` texels.
#[derive(Clone)]
pub struct ChunkImage {
    pub pos: ChunkPos,
    pub texels: Box<[CellTexel]>,
}

impl ChunkImage {
    pub fn new_air(pos: ChunkPos) -> Self {
        Self { pos, texels: vec![[0u16; 4]; CHUNK_AREA].into_boxed_slice() }
    }
}

impl std::fmt::Debug for ChunkImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChunkImage").field("pos", &self.pos).finish_non_exhaustive()
    }
}

/// Numbers about the simulation, for the timings panel and the benchmarks.
#[derive(Debug, Clone, Default)]
pub struct SimStats {
    pub tick: u64,
    /// Time of the last tick in milliseconds.
    pub tick_ms: f32,
    /// Chunks that were updated in the last tick.
    pub awake_chunks: u32,
    /// Chunks that exist in memory.
    pub loaded_chunks: u32,
    /// Time of each part of the last tick in milliseconds, for example ("movement", 1.2).
    pub sections: Vec<(&'static str, f32)>,
}

/// A free-flying cell (from explosions, spray, digging) or a visual-only particle, for drawing.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ParticleView {
    /// Position in cells (x right, y down).
    pub x: f32,
    pub y: f32,
    /// Velocity in cells per tick, for motion blur and interpolation.
    pub vx: f32,
    pub vy: f32,
    pub material: u16,
    pub temperature: i16,
    pub shade: u8,
}

/// Debug data for one awake chunk (only when debug data is on, see `Command::SetDebug`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DebugChunk {
    pub pos: ChunkPos,
    /// The cells updated in the last tick, in world cells. Empty if the chunk slept.
    pub updated: crate::pos::CellRect,
}

/// What the renderer and the UI need from one tick.
///
/// Rule for `chunks`: the renderer draws a chunk it has no data for as air.
/// The simulation sends each chunk in the view when it first enters the view and after each change.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub tick: u64,
    pub paused: bool,
    /// World size in cells (width, height).
    pub world_cells: (i32, i32),
    /// Chunks in the view that changed. At most one image for each chunk position.
    pub chunks: Vec<ChunkImage>,
    /// Free-flying particles in the view (all of them, every tick).
    pub particles: Vec<ParticleView>,
    /// Awake chunks and their update areas, if debug data is on. Empty otherwise.
    pub debug_chunks: Vec<DebugChunk>,
    pub stats: SimStats,
}

impl Snapshot {
    /// Add the chunk images of an older snapshot that this snapshot does not replace.
    /// Used when the reader did not take the older snapshot in time, so that no update is lost.
    pub fn merge_older(&mut self, older: Snapshot) {
        if older.chunks.is_empty() {
            return;
        }
        let newer: std::collections::HashSet<ChunkPos> = self.chunks.iter().map(|c| c.pos).collect();
        self.chunks.extend(older.chunks.into_iter().filter(|c| !newer.contains(&c.pos)));
    }
}
