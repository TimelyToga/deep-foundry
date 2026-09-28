//! Commands go from the main thread to the simulation thread.
//! The simulation applies all queued commands at the start of the next tick, in order.

use crate::ids::MaterialId;
use std::path::PathBuf;
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
    /// Turn debug data in snapshots on or off (see `Snapshot::debug_chunks`).
    SetDebug(bool),
    /// Change a number setting of the simulation, by its key (see `SimSettings::sliders` in
    /// `foundry_sim`). Unknown keys are ignored.
    SetSimSetting { key: String, value: f32 },
    /// An explosion at a cell (a debug tool). `strength` uses the hardness scale (0 to 255, see
    /// `foundry_sim::explode`); `heat` is in °C (0: no heat).
    Explode { center: CellPos, strength: f32, heat: i16 },
    /// Save the world to a file. The thread that owns the `Simulation` handles this command
    /// (with `Simulation::save_file`), not `Simulation::apply`. It reports the result in
    /// `Snapshot::notices`.
    SaveWorld { path: PathBuf },
    /// Replace the world with one loaded from a file. Handled like `SaveWorld`
    /// (with `Simulation::load_file`).
    LoadWorld { path: PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaintMode {
    /// Replace every cell in the circle (except bedrock).
    #[default]
    Replace,
    /// Fill only air cells.
    OnlyAir,
}
