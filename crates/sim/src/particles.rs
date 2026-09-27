//! Free-flying cells and visual particles (technical design section 6.6).
//!
//! Owner: Milestone 1 task "explosions and particles". This is a stub until then.

use crate::world::World;
use foundry_content::MaterialTable;
use foundry_core::{CellRect, ParticleView};

/// All particles in the world.
#[derive(Default)]
pub struct Particles {}

impl Particles {
    /// Move all particles one tick. Particles that hit something become cells again.
    pub fn step(&mut self, _world: &mut World, _mats: &MaterialTable, _tick: u64, _seed: u64, _stamp: u64) {}

    /// Particles inside an area, for the snapshot.
    pub fn views(&self, _area: CellRect, _out: &mut Vec<ParticleView>) {}

    pub fn len(&self) -> usize {
        0
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
