//! Reactions between touching cells (technical design section 6.4).
//!
//! Owner: Milestone 1 task 1C. This is a stub until then.

use crate::hood::Hood;
use foundry_content::Content;
use foundry_core::MaterialId;

/// Lookup tables for reactions, built once from the content.
#[derive(Debug, Default)]
pub struct ReactTable {}

impl ReactTable {
    pub fn new(_content: &Content) -> Self {
        Self {}
    }
}

/// What a reaction attempt did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing can happen here.
    None,
    /// The cell changed. Do not move it in this tick.
    Changed,
    /// A reaction is possible but did not happen in this tick (chance). Check again next tick.
    KeepAwake,
}

/// Try the reactions of the cell at (x, y). Called before movement.
#[inline]
pub fn try_react(_h: &mut Hood, _x: i32, _y: i32, _m: MaterialId) -> Outcome {
    Outcome::None
}
