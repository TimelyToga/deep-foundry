//! Sizes and rates used everywhere.

/// A chunk is `CHUNK_SIZE` × `CHUNK_SIZE` cells.
pub const CHUNK_SIZE: i32 = 64;
/// `cell >> CHUNK_SHIFT` gives the chunk coordinate (floor division, also for negative values).
pub const CHUNK_SHIFT: u32 = 6;
/// `cell & CHUNK_MASK` gives the coordinate inside the chunk.
pub const CHUNK_MASK: i32 = CHUNK_SIZE - 1;
/// Cells in one chunk.
pub const CHUNK_AREA: usize = (CHUNK_SIZE * CHUNK_SIZE) as usize;

/// A tile is `TILE_SIZE` × `TILE_SIZE` cells. Buildings snap to tiles.
pub const TILE_SIZE: i32 = 8;
pub const TILE_SHIFT: u32 = 3;
pub const TILES_PER_CHUNK: i32 = CHUNK_SIZE / TILE_SIZE;

/// Simulation ticks per second.
pub const TICKS_PER_SECOND: u32 = 60;
pub const TICK_SECONDS: f64 = 1.0 / TICKS_PER_SECOND as f64;

/// A cell moves at most this many cells in one tick.
/// The 4-pass parallel chunk update depends on this limit: it must stay ≤ CHUNK_SIZE / 2.
pub const MAX_CELL_MOVE: i32 = 32;

/// Temperature of new air and of cells that have no set temperature, in °C.
pub const DEFAULT_TEMPERATURE: i16 = 20;
