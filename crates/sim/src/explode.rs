//! Explosions (technical design section 6.7).
//!
//! Owner: Milestone 1 task "explosions and particles". This is a stub until then.

use crate::SimEvent;
use crate::particles::Particles;
use crate::world::World;
use foundry_content::MaterialTable;

/// Handle the explosion events of this tick. Runs on one thread after the movement passes.
/// Explosions may add new events (chain reactions) for the next tick.
pub fn process(
    _world: &mut World,
    _mats: &MaterialTable,
    _events: &[SimEvent],
    _particles: &mut Particles,
    _tick: u64,
    _seed: u64,
    _stamp: u64,
) {
}
