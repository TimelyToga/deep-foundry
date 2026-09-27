//! The factory: buildings, ports, machines, inventories, crafting, logistics and progression.
//!
//! The factory runs on the simulation thread, once per tick, before the cell simulation
//! (technical design section 7.1). It changes the world only through the public API of
//! `foundry_sim::Simulation`.
//!
//! Module owners (docs/design/04-build-plan.md):
//! - `buildings` (and new modules it needs: placement, ports, inventory, crafting, machines,
//!   logistics): task "factory-core"
//! - `progress` (technologies, labs, milestones, discovery, guide): task "progression"
//! - networks (power, fluids, signals): a later task

pub mod buildings;
pub mod progress;

use foundry_content::Content;
use foundry_sim::Simulation;
use std::sync::Arc;

/// All factory state.
pub struct Factory {
    pub content: Arc<Content>,
    pub buildings: buildings::Buildings,
    pub progress: progress::Progress,
}

impl Factory {
    pub fn new(content: Arc<Content>) -> Self {
        Self { buildings: buildings::Buildings::new(&content), progress: progress::Progress::new(&content), content }
    }

    /// Run one tick. Call it before `Simulation::advance` in the same tick.
    pub fn tick(&mut self, sim: &mut Simulation) {
        self.buildings.tick(&self.content, sim, &mut self.progress);
        self.progress.tick(&self.content);
    }
}
