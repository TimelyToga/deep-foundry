//! Heat flow and phase changes (technical design section 6.5).
//!
//! Owner: Milestone 1 task "heat". This is a stub until then.

use crate::world::World;
use foundry_content::MaterialTable;

/// Run the heat pass on the world. Called after movement and particles.
/// `air_temperature[y]` is the air temperature of cell row y.
pub fn step(
    _world: &mut World,
    _mats: &MaterialTable,
    _air_temperature: &[i16],
    _tick: u64,
    _seed: u64,
    _stamp: u64,
    _pool: Option<&rayon::ThreadPool>,
) {
}
