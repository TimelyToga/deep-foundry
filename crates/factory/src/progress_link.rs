//! The link between buildings and the progression system (labs and the Hub).
//!
//! The contract (agreed with task "progression"):
//! - A lab keeps a kit buffer, `KitBuffer` (a map from `PartId` to a count with `add` and `take`).
//!   Each tick the lab calls `progress.lab_tick(&content, lab_speed, &mut kits) -> LabStatus`.
//! - The Hub calls `progress.deliver(&content, stack) -> u32`. It returns how many it took.
//!
//! STAND-INS. The progression task defines `KitBuffer`, `LabStatus`, `Progress::lab_tick` and
//! `Progress::deliver` in `progress.rs`. They were not in this branch yet, so this file has small
//! stand-ins with the same names and signatures. To join the two branches:
//! 1. Delete the stand-in `KitBuffer` and `LabStatus` below and add
//!    `pub use crate::progress::{KitBuffer, LabStatus};`.
//! 2. Keep the `ProgressLink` trait. Change `impl ProgressLink for Progress` so that it calls the
//!    real `Progress::lab_tick` and `Progress::deliver`. (Buildings take `impl ProgressLink`, so
//!    tests can give a small test progress.)
//! 3. Change `lab_status` below to match the real `LabStatus` variants.
//!
//! The factory code uses these `KitBuffer` methods: `default`, `add`, `take`, `count`, `iter`,
//! `is_empty`, and needs `Debug`, `Clone`, `Serialize` and `Deserialize` on it.

use crate::machines::Status;
use crate::progress::Progress;
use foundry_content::{Content, Stack};
use foundry_core::PartId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Research kits in a lab. Stand-in; see the file comment.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KitBuffer {
    kits: BTreeMap<PartId, u32>,
}

impl KitBuffer {
    pub fn add(&mut self, part: PartId, count: u32) {
        if count > 0 {
            *self.kits.entry(part).or_insert(0) += count;
        }
    }

    /// Take up to `count`. Returns the number taken.
    pub fn take(&mut self, part: PartId, count: u32) -> u32 {
        let Some(have) = self.kits.get_mut(&part) else { return 0 };
        let t = (*have).min(count);
        *have -= t;
        if *have == 0 {
            self.kits.remove(&part);
        }
        t
    }

    pub fn count(&self, part: PartId) -> u32 {
        self.kits.get(&part).copied().unwrap_or(0)
    }

    /// All kits, in `PartId` order.
    pub fn iter(&self) -> impl Iterator<Item = (PartId, u32)> + '_ {
        self.kits.iter().map(|(p, n)| (*p, *n))
    }

    pub fn is_empty(&self) -> bool {
        self.kits.is_empty()
    }
}

/// What a lab did in one tick. Stand-in; see the file comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LabStatus {
    /// The lab used kits and made research progress.
    Working,
    /// No technology is being researched.
    #[default]
    NoResearch,
    /// The current technology needs kits that the lab does not have.
    MissingKits,
}

/// The progression calls that buildings make. Stand-in; see the file comment.
pub trait ProgressLink {
    fn lab_tick(&mut self, content: &Content, lab_speed: f32, kits: &mut KitBuffer) -> LabStatus;
    fn deliver(&mut self, content: &Content, stack: Stack) -> u32;
}

impl ProgressLink for Progress {
    fn lab_tick(&mut self, _content: &Content, _lab_speed: f32, _kits: &mut KitBuffer) -> LabStatus {
        LabStatus::NoResearch
    }

    fn deliver(&mut self, _content: &Content, _stack: Stack) -> u32 {
        0
    }
}

/// The building status and the reason text for a lab status.
pub fn lab_status(s: LabStatus) -> (Status, &'static str) {
    match s {
        LabStatus::Working => (Status::Working, "Researching"),
        LabStatus::NoResearch => (Status::Idle, "No research selected"),
        LabStatus::MissingKits => (Status::NoInput, "Needs research kits"),
    }
}
