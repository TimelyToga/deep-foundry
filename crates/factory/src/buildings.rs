//! Placed buildings. Owner: task "factory-core". This is a stub until then.

use crate::progress::Progress;
use foundry_content::Content;
use foundry_sim::Simulation;

/// All placed buildings.
#[derive(Default)]
pub struct Buildings {}

impl Buildings {
    pub fn new(_content: &Content) -> Self {
        Self {}
    }

    /// Run all building logic for one tick.
    pub fn tick(&mut self, _content: &Content, _sim: &mut Simulation, _progress: &mut Progress) {}
}
