//! One chunk: 64 × 64 cells stored as separate arrays.

use foundry_core::{CHUNK_AREA, CHUNK_SIZE, DEFAULT_TEMPERATURE};

/// Cell flag: the "updated" parity bit. A cell counts as updated in this tick if this bit equals `tick & 1`.
/// Cells that did not move for a while keep an old bit, so a match can be stale. The update loop
/// handles this by checking such a cell again in the next tick.
pub const FLAG_PARITY: u8 = 1 << 0;
/// Cell flag: the cell is part of a building body (used from Milestone 3).
pub const FLAG_BUILDING: u8 = 1 << 1;
/// Cell flag: the cell burns (set by `react.rs`, for the renderer). Flags do not move with a cell,
/// so the flag can stay behind for a short time when a burning liquid or powder moves, or when
/// other code replaces a burning cell. Use it only on materials that have burn data, and never on
/// air. The burning state itself is in the cell's life byte (see `react.rs`).
pub const FLAG_BURNING: u8 = 1 << 2;

/// `motion` bits 0-4: fall speed (0 to 31).
pub const MOTION_SPEED: u8 = 0x1f;
/// `motion` bit 5: the side a liquid last flowed to (set = right).
pub const MOTION_RIGHT: u8 = 1 << 5;
/// `motion` bits 6-7: sideways momentum of a liquid (0 to 3).
pub const MOTION_MOMENTUM: u8 = 0xc0;
pub const MOTION_MOMENTUM_SHIFT: u32 = 6;

/// A rectangle inside one chunk, in local cell coordinates (0 to 64). `x1` and `y1` are exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalRect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl LocalRect {
    pub const EMPTY: LocalRect = LocalRect { x0: CHUNK_SIZE, y0: CHUNK_SIZE, x1: 0, y1: 0 };
    pub const FULL: LocalRect = LocalRect { x0: 0, y0: 0, x1: CHUNK_SIZE, y1: CHUNK_SIZE };

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    /// Add one cell.
    #[inline(always)]
    pub fn add_point(&mut self, x: i32, y: i32) {
        self.x0 = self.x0.min(x);
        self.y0 = self.y0.min(y);
        self.x1 = self.x1.max(x + 1);
        self.y1 = self.y1.max(y + 1);
    }

    /// Add a rectangle. The result is clamped to the chunk.
    #[inline(always)]
    pub fn add_rect(&mut self, o: LocalRect) {
        if o.is_empty() {
            return;
        }
        self.x0 = self.x0.min(o.x0.max(0));
        self.y0 = self.y0.min(o.y0.max(0));
        self.x1 = self.x1.max(o.x1.min(CHUNK_SIZE));
        self.y1 = self.y1.max(o.y1.min(CHUNK_SIZE));
    }
}

pub struct Chunk {
    pub mat: [u16; CHUNK_AREA],
    /// Temperature in whole °C.
    pub temp: [i16; CHUNK_AREA],
    /// Color shade. Set when the cell is made. Moves with the cell.
    pub shade: [u8; CHUNK_AREA],
    /// Timer for materials that fade, and for slow reactions.
    pub life: [u8; CHUNK_AREA],
    /// Fall speed and flow direction. See the `MOTION_*` constants.
    pub motion: [u8; CHUNK_AREA],
    pub flags: [u8; CHUNK_AREA],
    /// Changes each time a cell in this chunk changes. The snapshot code uses it.
    /// 0: all air since the chunk was made. `GENERATED_VERSION`: as the chunk source made it.
    pub version: u64,
    /// The cells to update in the next tick. Empty means the chunk sleeps.
    pub dirty: LocalRect,
    /// True while every cell is exactly as the chunk source made it. The world can drop such a
    /// chunk from memory and make it again later. Any write to a cell sets it to false.
    pub pristine: bool,
    /// The chunk is in the world's awake list or paused set. Only `World` changes this.
    pub(crate) queued: bool,
    /// Rows (bit y) that may hold a cell with a fall speed. The fall pass of the movement tick
    /// looks only at these rows (see `schedule.rs`). A bit that is set with no falling cell in the
    /// row only costs a look; a new chunk has all bits set.
    pub falling_rows: u64,
    /// Heat: the stamp of the last tick in which the heat pass worked on this chunk. In that tick,
    /// `heat_edge_temp` and `heat_edge_mat` hold the edge cells as they were before the pass.
    /// See `heat.rs`.
    pub heat_stamp: u64,
    /// Heat: the temperatures of the 4 edges at the start of the heat pass of tick `heat_stamp`.
    /// Order: top row (y = 0), bottom row (y = 63), left column (x = 0), right column (x = 63),
    /// 64 cells each.
    pub heat_edge_temp: [i16; 256],
    /// Heat: the materials of the same edge cells.
    pub heat_edge_mat: [u16; 256],
    /// Heat: ticks since a temperature in this chunk last changed, at most `heat::QUIET_TICKS`.
    /// At `heat::QUIET_TICKS` the chunk is at rest for heat.
    pub heat_quiet: u8,
    /// Heat: how much the temperatures of glowing cells may have changed since `version` last
    /// changed (°C). The heat pass sets a new version when it gets large (see `heat.rs`).
    pub glow_drift: u16,
}

/// The version of a chunk that the chunk source made and that did not change since.
/// Stamps of real changes start above it.
pub const GENERATED_VERSION: u64 = 1;

impl Chunk {
    /// A chunk full of air at the default temperature.
    pub fn new_air() -> Box<Chunk> {
        Box::new(Chunk {
            mat: [0; CHUNK_AREA],
            temp: [DEFAULT_TEMPERATURE; CHUNK_AREA],
            shade: [0; CHUNK_AREA],
            life: [0; CHUNK_AREA],
            motion: [0; CHUNK_AREA],
            flags: [0; CHUNK_AREA],
            version: 0,
            dirty: LocalRect::EMPTY,
            pristine: true,
            queued: false,
            falling_rows: u64::MAX,
            heat_stamp: 0,
            heat_edge_temp: [0; 256],
            heat_edge_mat: [0; 256],
            heat_quiet: crate::heat::QUIET_TICKS,
            glow_drift: 0,
        })
    }

    /// True if every cell is air.
    pub fn is_all_air(&self) -> bool {
        self.mat.iter().all(|&m| m == 0)
    }
}
