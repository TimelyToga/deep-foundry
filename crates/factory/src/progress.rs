//! Research, labs, Hub milestones, discovery and the guide. Owner: task "progression".
//! This is a stub until then.

use foundry_content::Content;

/// The player's progress.
#[derive(Default)]
pub struct Progress {}

impl Progress {
    pub fn new(_content: &Content) -> Self {
        Self {}
    }

    /// Run one tick.
    pub fn tick(&mut self, _content: &Content) {}
}
