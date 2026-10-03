//! Arms: a 1-tile building that moves items from the building behind it to the building in front
//! of it. With no turn, it takes from the building on its left and gives to the building on its
//! right; turning the arm turns that direction.
//!
//! - It takes finished products of a machine (parts or material), stacks from a crate, or cells
//!   from a hopper.
//! - It gives to whatever the other building accepts now: recipe inputs and the fuel slot of a
//!   machine, a crate, a hopper (powder), a lab (research kits), the Hub (what a repair stage
//!   needs), a boiler (fuel).
//! - It takes only an item that the other building has room for, so it never holds an item it
//!   cannot put down. It moves one part or `MATERIAL_PER_MOVE` units of material each move, once
//!   every `period` ticks (param "period", default 20: 3 moves per second).
//! - An arm needs steam when its data has a steam use (`power.steam_per_s`).

use crate::buildings::{Buildings, Logic};
use crate::geometry::neighbor_tile;
use crate::machines::output_stack;
use crate::steam::{consume_steam, steam_ready};
use foundry_content::{Content, ItemRef, Side};
use foundry_core::{BuildingId, MaterialId};
use serde::{Deserialize, Serialize};

/// Units of a material that an arm moves in one move.
pub const MATERIAL_PER_MOVE: u32 = 16;

/// The state of an arm.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Arm {
    /// The tick of the next move.
    pub next_move: u64,
    /// Items moved since the arm was placed (for the window).
    pub moved: u64,
}

impl Buildings {
    /// The building an arm takes from and the building it gives to.
    pub(crate) fn arm_ends(&self, i: u32) -> Option<(BuildingId, BuildingId)> {
        let b = self.at_index(i)?;
        let from = neighbor_tile(b.at, b.transform.side(Side::Left));
        let to = neighbor_tile(b.at, b.transform.side(Side::Right));
        let (&f, &t) = (self.front.get(&from)?, self.front.get(&to)?);
        (f.index != i && t.index != i && f != t).then_some((f, t))
    }

    /// One tick of an arm. Returns true if it moved something.
    pub(crate) fn arm_tick(&mut self, i: u32, content: &Content) -> bool {
        let now = self.now;
        let Some(b) = self.at_index(i) else { return false };
        let Logic::Arm(arm) = &b.logic else { return false };
        if now < arm.next_move {
            return false;
        }
        let def = content.factory.building_def(b.kind);
        let steam_use = def.power.as_ref().map_or(0.0, |p| p.steam_per_s as f64);
        if !steam_ready(b.steam, steam_use) {
            if let Some(b) = self.at_index_mut(i) {
                b.status = crate::Status::NoPower;
                b.steam_reason = Some("Needs steam from a connected bronze pipe".into());
            }
            return false;
        }
        if crate::power::is_consumer(content, b) && b.power_factor <= 0.0 {
            if let Some(b) = self.at_index_mut(i) {
                b.status = crate::Status::NoPower;
                b.steam_reason = Some("Needs power from a connected cable".into());
            }
            return false;
        }
        let period = def.param("period", 20.0).max(1.0) as u64;
        let Some((from, to)) = self.arm_ends(i) else {
            if let Some(b) = self.at_index_mut(i) {
                b.status = crate::Status::NoInput;
            }
            return false;
        };
        let moved = self.arm_move(from, to, content);
        if let Some(b) = self.at_index_mut(i) {
            // Waiting for items or room is normal for an arm: it is idle, not a problem.
            b.status = if moved > 0 { crate::Status::Working } else { crate::Status::Idle };
            if let Logic::Arm(arm) = &mut b.logic {
                arm.next_move = now + period;
                arm.moved += moved as u64;
            }
            if moved > 0 {
                // Steam for a whole move.
                consume_steam(&mut b.steam, steam_use * period as f64 / foundry_core::TICKS_PER_SECOND as f64);
            }
        }
        if moved > 0 {
            self.wake(from);
            self.wake(to);
        }
        moved > 0
    }

    /// Move one load from building `from` to building `to`. Returns the count moved.
    fn arm_move(&mut self, from: BuildingId, to: BuildingId, content: &Content) -> u32 {
        for (item, have) in self.arm_offers(from, content) {
            let per_move = match item {
                ItemRef::Part(_) => 1,
                ItemRef::Material(_) => MATERIAL_PER_MOVE,
            };
            let n = have.min(per_move).min(self.room_for(content, to, item));
            if n == 0 {
                continue;
            }
            let taken = self.arm_take(from, item, n, content);
            if taken == 0 {
                continue;
            }
            let given = self.insert(content, to, item, taken);
            if given < taken {
                // Cannot happen (the room was checked); put the rest back.
                self.insert(content, from, item, taken - given);
            }
            return given;
        }
        0
    }

    /// What an arm can take out of a building: (item, count), in order.
    fn arm_offers(&self, id: BuildingId, content: &Content) -> Vec<(ItemRef, u32)> {
        let Some(b) = self.get(id) else { return vec![] };
        match &b.logic {
            Logic::Machine(m) => {
                let Some(r) = m.recipe else { return vec![] };
                let recipe = content.factory.recipe_def(r);
                (0..m.outputs.len()).filter(|&j| m.outputs[j] > 0).map(|j| (output_stack(recipe, j).item, m.outputs[j])).collect()
            }
            Logic::Storage(inv) => inv.contents().into_iter().map(|s| (s.item, s.count)).collect(),
            Logic::Hopper(h) => h.counts().into_iter().map(|(m, n)| (ItemRef::Material(m), n)).collect(),
            // The parts on a belt tile, the front one first.
            Logic::Belt(belt) => belt.parts.iter().map(|p| (ItemRef::Part(p.part), 1)).collect(),
            _ => vec![],
        }
    }

    /// Take up to `n` of an item that `arm_offers` listed. Returns the count taken.
    fn arm_take(&mut self, id: BuildingId, item: ItemRef, n: u32, content: &Content) -> u32 {
        if matches!(self.get(id).map(|b| &b.logic), Some(Logic::Machine(_))) {
            return self.take_output(content, id, item, n);
        }
        let Some(b) = self.get_mut(id) else { return 0 };
        match &mut b.logic {
            Logic::Storage(inv) => inv.remove(item, n),
            Logic::Hopper(h) => {
                let ItemRef::Material(m) = item else { return 0 };
                take_cells(&mut h.cells, m, n)
            }
            Logic::Belt(belt) => {
                let ItemRef::Part(p) = item else { return 0 };
                match belt.parts.iter().position(|x| x.part == p) {
                    Some(k) if n > 0 => {
                        belt.parts.remove(k);
                        1
                    }
                    _ => 0,
                }
            }
            _ => 0,
        }
    }
}

/// Remove up to `n` cells of `m` from a hopper queue. Returns the count removed.
fn take_cells(cells: &mut std::collections::VecDeque<MaterialId>, m: MaterialId, n: u32) -> u32 {
    let mut taken = 0;
    cells.retain(|&c| {
        if c == m && taken < n {
            taken += 1;
            false
        } else {
            true
        }
    });
    taken
}
