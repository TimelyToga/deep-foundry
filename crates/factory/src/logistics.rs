//! State of the simple Tier 0 logistics buildings: hopper, belt and lab.
//! Storage (crate) and the Hub use an `Inventory`. The logic that moves cells is in `buildings.rs`.
//!
//! Data parameters (`params` in the building data):
//! - storage: `slots` (default 8), `tanks` (default 0), `capacity` units per tank (default 1000)
//! - hopper: `capacity` cells inside (default 64), `rate` cells released per second (default 16)
//! - belt: `belt_speed` cells per second (default 8)
//! - hub: `slots` (default 16), `tanks` (default 4), `capacity` units per tank (default 1000)
//! - lab: `kit_buffer` kits of each type it holds (default 10); `speed` is the lab speed
//! - workbench: `reach` in tiles (default 6); `speed` is the hand crafting speed near it

use crate::progress_link::{KitBuffer, LabStatus};
use foundry_core::{MaterialId, TICKS_PER_SECOND};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// A hopper: a small store of powder cells, oldest first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hopper {
    pub cells: VecDeque<MaterialId>,
    pub capacity: u32,
    /// Only this material goes in. `None`: every powder.
    pub filter: Option<MaterialId>,
}

impl Hopper {
    pub fn new(capacity: u32) -> Self {
        Self { cells: VecDeque::new(), capacity, filter: None }
    }

    pub fn is_full(&self) -> bool {
        self.cells.len() as u32 >= self.capacity
    }

    pub fn accepts(&self, m: MaterialId) -> bool {
        !self.is_full() && self.filter.is_none_or(|f| f == m)
    }

    /// Add one cell if it is accepted. Returns true if it was added.
    pub fn push(&mut self, m: MaterialId) -> bool {
        if !self.accepts(m) {
            return false;
        }
        self.cells.push_back(m);
        true
    }

    /// The cells as (material, count), in the order they came in.
    pub fn counts(&self) -> Vec<(MaterialId, u32)> {
        let mut out: Vec<(MaterialId, u32)> = vec![];
        for &m in &self.cells {
            match out.iter_mut().find(|(x, _)| *x == m) {
                Some((_, n)) => *n += 1,
                None => out.push((m, 1)),
            }
        }
        out
    }
}

/// A belt. It moves the powder that rests on its top.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Belt {
    /// Belt steps in a row that moved nothing.
    pub idle_steps: u32,
}

/// A lab: a small buffer of research kits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lab {
    pub kits: KitBuffer,
    /// Kits of each type the lab takes.
    pub kit_limit: u32,
    /// What the last lab tick did. `None` before the first tick and without power. Not saved.
    #[serde(skip)]
    pub last: Option<LabStatus>,
}

impl Lab {
    pub fn new(kit_limit: u32) -> Self {
        Self { kits: KitBuffer::default(), kit_limit: kit_limit.max(1), last: None }
    }
}

/// How many steps a thing that runs `per_second` times per second makes in tick `tick`.
/// All buildings with the same rate step in the same ticks, so a line of belts moves together.
pub fn steps_in_tick(tick: u64, per_second: f32) -> u32 {
    if per_second <= 0.0 {
        return 0;
    }
    let rate = per_second as f64 / TICKS_PER_SECOND as f64;
    let now = (tick as f64 * rate).floor();
    let before = (tick.saturating_sub(1) as f64 * rate).floor();
    (now - before).max(0.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_add_up_to_the_rate() {
        for rate in [1.0f32, 8.0, 16.0, 30.0, 60.0, 90.0] {
            let total: u32 = (1..=600).map(|t| steps_in_tick(t, rate)).sum();
            assert_eq!(total, (rate * 10.0) as u32, "rate {rate}");
        }
        assert_eq!(steps_in_tick(5, 0.0), 0);
    }

    #[test]
    fn hopper_filter_and_capacity() {
        let (sand, clay) = (MaterialId(3), MaterialId(4));
        let mut h = Hopper::new(2);
        h.filter = Some(sand);
        assert!(!h.push(clay));
        assert!(h.push(sand));
        assert!(h.push(sand));
        assert!(!h.push(sand), "full");
        assert_eq!(h.counts(), vec![(sand, 2)]);
    }
}
