//! Commands go from the main thread to the simulation thread.
//! The simulation applies all queued commands at the start of the next tick, in order.

use crate::ids::MaterialId;
use crate::pos::{CellPos, CellRect, ChunkPos};

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Fill a circle with a material. `temperature: None` uses the material's default temperature.
    /// Painting air erases.
    Paint { center: CellPos, radius: u16, material: MaterialId, mode: PaintMode, temperature: Option<i16> },
    /// The renderer needs chunk data for this area (the view plus a margin).
    SetView { area: CellRect },
    /// The renderer dropped its data for these chunks. The simulation sends them again when needed.
    ForgetChunks(Vec<ChunkPos>),
    /// Send all chunks in the view again. Use it when the renderer lost its GPU data.
    ResendAll,
    /// Stop or start ticks.
    SetPaused(bool),
    /// Run exactly one tick while paused.
    Step,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaintMode {
    /// Replace every cell in the circle (except bedrock).
    #[default]
    Replace,
    /// Fill only air cells.
    OnlyAir,
}
