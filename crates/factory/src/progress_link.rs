//! The link between buildings and the progression system (labs and the Hub).
//!
//! - A lab keeps a [`KitBuffer`]. Each tick the lab calls `lab_tick(&content, lab_speed, &mut kits)`
//!   and shows the [`LabStatus`] it gets back.
//! - The Hub calls `deliver(&content, stack)`. It returns how many items the progression took.
//!
//! `Buildings::tick` takes `impl ProgressLink`, so tests can give a small test progress. The game
//! gives the real [`Progress`].

pub use crate::progress::{KitBuffer, LabStatus};

use crate::machines::Status;
use crate::progress::Progress;
use foundry_content::{Content, Stack};

/// The progression calls that buildings make.
pub trait ProgressLink {
    /// One tick of work in one lab. See [`Progress::lab_tick`].
    fn lab_tick(&mut self, content: &Content, lab_speed: f32, kits: &mut KitBuffer) -> LabStatus;
    /// Give items to the Hub. Returns how many were taken. See [`Progress::deliver`].
    fn deliver(&mut self, content: &Content, stack: Stack) -> u32;
    /// The last Hub repair stage that is done (0: none). The Hub takes only items that a later
    /// stage asks for.
    fn hub_stage(&self) -> u8 {
        0
    }
    /// What the Hub repair stages still need (see `HubRule::need`). `None`: no limit beyond
    /// `hub_stage`.
    fn hub_need(&self, _content: &Content) -> Option<Vec<Stack>> {
        None
    }
}

impl ProgressLink for Progress {
    fn lab_tick(&mut self, content: &Content, lab_speed: f32, kits: &mut KitBuffer) -> LabStatus {
        Progress::lab_tick(self, content, lab_speed, kits)
    }

    fn deliver(&mut self, content: &Content, stack: Stack) -> u32 {
        Progress::deliver(self, content, stack)
    }

    fn hub_stage(&self) -> u8 {
        self.stage()
    }

    fn hub_need(&self, content: &Content) -> Option<Vec<Stack>> {
        Some(Progress::hub_need(self, content))
    }
}

/// The building status for a lab status.
pub fn lab_status(s: &LabStatus) -> Status {
    match s {
        LabStatus::Working | LabStatus::Done(_) => Status::Working,
        LabStatus::NoResearch => Status::Idle,
        LabStatus::MissingKits(_) => Status::NoInput,
    }
}

/// The reason text for a lab status, for the building window.
pub fn lab_reason(content: &Content, s: &LabStatus) -> String {
    match s {
        LabStatus::Working | LabStatus::Done(_) => "Researching".into(),
        LabStatus::NoResearch => "No research selected".into(),
        LabStatus::MissingKits(list) if list.is_empty() => "Needs research kits".into(),
        LabStatus::MissingKits(list) => {
            let names: Vec<&str> = list.iter().map(|k| content.item_name(k.item)).collect();
            format!("Needs {}", names.join(", "))
        }
    }
}
