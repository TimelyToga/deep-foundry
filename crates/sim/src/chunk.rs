//! One chunk: 64 × 64 cells stored as separate arrays.

use foundry_core::{CHUNK_AREA, CHUNK_SIZE, DEFAULT_TEMPERATURE};

/// Cell flag: the "updated" parity bit. A cell counts as updated in this tick if this bit equals `tick & 1`.
/// Cells that did not move for a while keep an old bit, so a match can be stale. The update loop
/// handles this by checking such a cell again in the next tick.
pub const FLAG_PARITY: u8 = 1 << 0;
/// Cell flag: the cell is part of a building body (used from Milestone 3).
pub const FLAG_BUILDING: u8 = 1 << 1;

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
    pub version: u64,
    /// The cells to update in the next tick. Empty means the chunk sleeps.
    pub dirty: LocalRect,
}

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
        })
    }

    /// True if every cell is air.
    pub fn is_all_air(&self) -> bool {
        self.mat.iter().all(|&m| m == 0)
    }
}
