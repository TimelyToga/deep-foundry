//! One chunk: 64 × 64 cells stored as separate arrays.

use foundry_core::{CHUNK_AREA, DEFAULT_TEMPERATURE};

/// Cell flag: the "updated" parity bit. A cell was updated in this tick if this bit equals `tick & 1`.
pub const FLAG_PARITY: u8 = 1 << 0;
/// Cell flag: the cell is part of a building body (used from Milestone 3).
pub const FLAG_BUILDING: u8 = 1 << 1;

pub struct Chunk {
    pub mat: [u16; CHUNK_AREA],
    /// Temperature in whole °C.
    pub temp: [i16; CHUNK_AREA],
    /// Color shade. Set when the cell is made. Moves with the cell.
    pub shade: [u8; CHUNK_AREA],
    /// Timer for materials that fade, and for slow reactions.
    pub life: [u8; CHUNK_AREA],
    /// Fall speed and other movement state.
    pub motion: [u8; CHUNK_AREA],
    pub flags: [u8; CHUNK_AREA],
    /// Changes each time a cell in this chunk changes. The snapshot code uses it.
    pub version: u64,
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
        })
    }

    /// True if every cell is air.
    pub fn is_all_air(&self) -> bool {
        self.mat.iter().all(|&m| m == 0)
    }
}
