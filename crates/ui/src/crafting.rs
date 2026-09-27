//! Hand crafting rules: how many times a recipe can run, and how many a click asks for.
//!
//! Clicks on a recipe, as in Factorio:
//! - left click: craft 1
//! - right click: craft 5
//! - shift + left click: craft all you can
//!
//! A click never asks for more than the player can make. The simulation checks again.

use crate::model::PlayerView;
use foundry_content::{ItemRef, Recipe};
use std::collections::HashMap;

/// How many of each item the player has (part slots and material tank together).
#[derive(Debug, Clone, Default)]
pub struct Stock {
    counts: HashMap<ItemRef, u64>,
}

impl Stock {
    pub fn from_player(player: &PlayerView) -> Self {
        let mut s = Stock::default();
        s.fill(player);
        s
    }

    /// Count the items of the player again. Reuses the memory.
    pub fn fill(&mut self, player: &PlayerView) {
        self.counts.clear();
        for stack in player.inventory.iter().flatten() {
            *self.counts.entry(stack.item).or_default() += stack.count as u64;
        }
        for slot in &player.tank {
            if let Some(m) = slot.material {
                *self.counts.entry(ItemRef::Material(m)).or_default() += slot.units as u64;
            }
        }
    }

    pub fn set(&mut self, item: ItemRef, count: u64) {
        self.counts.insert(item, count);
    }

    pub fn get(&self, item: ItemRef) -> u64 {
        self.counts.get(&item).copied().unwrap_or(0)
    }
}

/// How many times the recipe can run with the items in `stock`. 0 if it has no inputs.
pub fn craftable_count(recipe: &Recipe, stock: &Stock) -> u32 {
    let mut best: Option<u64> = None;
    for input in &recipe.inputs {
        if input.count == 0 {
            continue;
        }
        let times = stock.get(input.item) / input.count as u64;
        best = Some(best.map_or(times, |b| b.min(times)));
    }
    best.unwrap_or(0).min(u32::MAX as u64) as u32
}

/// How many crafts a click on a recipe asks for. 0 means the click does nothing.
pub fn click_count(right_button: bool, shift: bool, craftable: u32) -> u32 {
    let want = if shift {
        craftable
    } else if right_button {
        5
    } else {
        1
    };
    want.min(craftable)
}

/// How many runs a click on a crafting queue job cancels (left 1, right 5, shift all).
pub fn cancel_count(right_button: bool, shift: bool, job_count: u32) -> u32 {
    let want = if shift {
        job_count
    } else if right_button {
        5
    } else {
        1
    };
    want.min(job_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TankSlot;
    use foundry_content::Stack;
    use foundry_core::{MaterialId, PartId};

    const PLATE: ItemRef = ItemRef::Part(PartId(1));
    const GEAR: ItemRef = ItemRef::Part(PartId(2));
    const CLAY: ItemRef = ItemRef::Material(MaterialId(3));

    fn recipe(inputs: &[(ItemRef, u32)]) -> Recipe {
        Recipe {
            id: "test".into(),
            name: "Test".into(),
            category: "hand".into(),
            hand: true,
            inputs: inputs.iter().map(|&(item, count)| Stack { item, count }).collect(),
            outputs: vec![Stack { item: GEAR, count: 1 }],
            byproducts: vec![],
            time: 1.0,
            tier: 0,
            min_temp: None,
            group: "intermediate".into(),
            unlocked_by: None,
        }
    }

    fn player() -> PlayerView {
        let s = |item, count| Some(Stack { item, count });
        PlayerView {
            inventory: vec![s(PLATE, 5), None, s(PLATE, 2), s(GEAR, 5)],
            tank: vec![TankSlot { material: Some(MaterialId(3)), units: 100, capacity: 2000 }, TankSlot::default()],
            ..Default::default()
        }
    }

    #[test]
    fn stock_counts_all_slots_and_tank() {
        let s = Stock::from_player(&player());
        assert_eq!(s.get(PLATE), 7);
        assert_eq!(s.get(GEAR), 5);
        assert_eq!(s.get(CLAY), 100);
        assert_eq!(s.get(ItemRef::Part(PartId(99))), 0);
    }

    #[test]
    fn craft_all_you_can_uses_the_scarcest_ingredient() {
        let s = Stock::from_player(&player());
        assert_eq!(craftable_count(&recipe(&[(PLATE, 2), (GEAR, 1)]), &s), 3);
        assert_eq!(craftable_count(&recipe(&[(PLATE, 1), (GEAR, 1)]), &s), 5);
        assert_eq!(craftable_count(&recipe(&[(CLAY, 16)]), &s), 6);
        assert_eq!(craftable_count(&recipe(&[(PLATE, 8)]), &s), 0);
        assert_eq!(craftable_count(&recipe(&[]), &s), 0);
    }

    #[test]
    fn click_counts_follow_factorio() {
        assert_eq!(click_count(false, false, 10), 1);
        assert_eq!(click_count(true, false, 10), 5);
        assert_eq!(click_count(false, true, 10), 10);
        // Never more than can be made.
        assert_eq!(click_count(true, false, 3), 3);
        assert_eq!(click_count(false, false, 0), 0);
        assert_eq!(click_count(false, true, 0), 0);
    }

    #[test]
    fn cancel_counts() {
        assert_eq!(cancel_count(false, false, 7), 1);
        assert_eq!(cancel_count(true, false, 7), 5);
        assert_eq!(cancel_count(true, false, 2), 2);
        assert_eq!(cancel_count(false, true, 7), 7);
    }
}
