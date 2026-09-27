//! `KitBuffer`: the research kits inside one lab.

use foundry_core::PartId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Small differences below this value are rounding errors.
pub(crate) const EPS: f64 = 1e-9;

/// The research kits inside one lab. The lab building holds one `KitBuffer` and gives it to
/// [`super::Progress::lab_tick`] each tick.
///
/// A lab uses kits a little at a time. When it needs a kit, it takes one whole kit from `counts`
/// and keeps the unused rest of it in `partly_used`. One kit is enough for one research unit.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct KitBuffer {
    /// Whole kits.
    counts: BTreeMap<PartId, u32>,
    /// The unused rest (0 to 1) of a kit that the lab started to use. It is not in `counts`.
    partly_used: BTreeMap<PartId, f64>,
}

impl KitBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Put `count` whole kits in the buffer.
    pub fn add(&mut self, part: PartId, count: u32) {
        if count > 0 {
            *self.counts.entry(part).or_insert(0) += count;
        }
    }

    /// Take up to `count` whole kits out. Returns how many it took.
    pub fn take(&mut self, part: PartId, count: u32) -> u32 {
        let Some(have) = self.counts.get_mut(&part) else { return 0 };
        let taken = count.min(*have);
        *have -= taken;
        if *have == 0 {
            self.counts.remove(&part);
        }
        taken
    }

    /// Whole kits of this type.
    pub fn count(&self, part: PartId) -> u32 {
        self.counts.get(&part).copied().unwrap_or(0)
    }

    /// All whole kits.
    pub fn total(&self) -> u32 {
        self.counts.values().sum()
    }

    /// True if there are no whole kits. (A partly used kit does not count.)
    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }

    /// Whole kits by type, in id order.
    pub fn iter(&self) -> impl Iterator<Item = (PartId, u32)> + '_ {
        self.counts.iter().map(|(p, n)| (*p, *n))
    }

    /// The unused rest (0 to 1) of a kit of this type that the lab started to use.
    pub fn partly_used(&self, part: PartId) -> f64 {
        self.partly_used.get(&part).copied().unwrap_or(0.0)
    }

    /// Kits of this type that the lab can still use: whole kits plus the unused rest.
    pub(crate) fn available(&self, part: PartId) -> f64 {
        self.count(part) as f64 + self.partly_used(part)
    }

    /// Use `amount` kits (can be less than 1). Takes whole kits as needed.
    /// The caller checks `available` first.
    pub(crate) fn use_kits(&mut self, part: PartId, amount: f64) {
        let mut rest = self.partly_used(part);
        while rest < amount - EPS {
            if self.take(part, 1) == 0 {
                break;
            }
            rest += 1.0;
        }
        rest -= amount;
        if rest < EPS {
            self.partly_used.remove(&part);
        } else {
            self.partly_used.insert(part, rest);
        }
    }
}
