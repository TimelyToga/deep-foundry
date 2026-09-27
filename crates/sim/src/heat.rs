//! Heat flow and phase changes (technical design section 6.5).
//!
//! Owner: Milestone 1 task 1B. This is a stub until then.

use crate::world::World;
use foundry_content::MaterialTable;

/// Run the heat pass on the world. Called after movement.
pub fn step(_world: &mut World, _mats: &MaterialTable, _tick: u64, _seed: u64, _stamp: u64) {}
