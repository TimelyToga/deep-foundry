//! Inventories: slots for parts and tanks for bulk material.
//!
//! - A slot holds one stack of one part, up to the part's stack size.
//! - A tank holds one material, in units (1 unit = 1 cell), up to its capacity.
//!
//! The slot rules follow Factorio. The mouse cursor can hold one part stack (`cursor`).
//! - Left click: pick up a stack, put it down, add to a stack of the same part, or swap.
//! - Right click: take half of a stack, or put down one piece.
//! - Shift click: move the stack to the other inventory.
//! - Ctrl click: move all pieces of that part to the other inventory.

use foundry_content::{Content, ItemRef, Stack};
use foundry_core::{MaterialId, PartId};
use serde::{Deserialize, Serialize};

/// Pieces of one part in a slot or in the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartStack {
    pub part: PartId,
    pub count: u32,
}

impl PartStack {
    pub const fn new(part: PartId, count: u32) -> Self {
        Self { part, count }
    }

    pub const fn to_stack(self) -> Stack {
        Stack { item: ItemRef::Part(self.part), count: self.count }
    }
}

/// A tank for one material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tank {
    /// `None` when the tank is empty.
    pub material: Option<MaterialId>,
    pub units: u32,
    pub capacity: u32,
}

impl Tank {
    pub const fn new(capacity: u32) -> Self {
        Self { material: None, units: 0, capacity }
    }

    /// Units of `m` this tank can still take.
    pub fn room_for(&self, m: MaterialId) -> u32 {
        match self.material {
            Some(x) if x == m => self.capacity - self.units,
            None => self.capacity,
            Some(_) => 0,
        }
    }

    /// Add up to `n` units. Returns the units that did not fit.
    pub fn fill(&mut self, m: MaterialId, n: u32) -> u32 {
        let add = self.room_for(m).min(n);
        if add > 0 {
            self.material = Some(m);
            self.units += add;
        }
        n - add
    }

    /// Take up to `n` units of `m`. Returns the units taken.
    pub fn drain(&mut self, m: MaterialId, n: u32) -> u32 {
        if self.material != Some(m) {
            return 0;
        }
        let take = self.units.min(n);
        self.units -= take;
        if self.units == 0 {
            self.material = None;
        }
        take
    }
}

/// Which mouse action on a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Click {
    Left,
    Right,
    /// Shift + left click.
    Shift,
    /// Ctrl + left click.
    Ctrl,
}

/// Part slots and material tanks.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inventory {
    pub slots: Vec<Option<PartStack>>,
    pub tanks: Vec<Tank>,
}

/// Pieces of `part` in one slot.
pub fn stack_size(content: &Content, part: PartId) -> u32 {
    content.factory.part_def(part).stack.max(1) as u32
}

impl Inventory {
    /// An inventory with `slots` empty slots and `tanks` empty tanks of `tank_capacity` units.
    pub fn new(slots: usize, tanks: usize, tank_capacity: u32) -> Self {
        Self { slots: vec![None; slots], tanks: vec![Tank::new(tank_capacity); tanks] }
    }

    /// The robot's start inventory (game design section 6.4): 4 tanks of 2,000 units and
    /// 40 part slots.
    pub fn player() -> Self {
        Self::new(40, 4, 2000)
    }

    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(|s| s.is_none()) && self.tanks.iter().all(|t| t.units == 0)
    }

    /// How many of an item the inventory holds.
    pub fn count(&self, item: ItemRef) -> u32 {
        match item {
            ItemRef::Part(p) => self.slots.iter().flatten().filter(|s| s.part == p).map(|s| s.count).sum(),
            ItemRef::Material(m) => self.tanks.iter().filter(|t| t.material == Some(m)).map(|t| t.units).sum(),
        }
    }

    /// How many of an item would fit now (up to `want`).
    pub fn room_for(&self, content: &Content, item: ItemRef, want: u32) -> u32 {
        let room: u64 = match item {
            ItemRef::Part(p) => {
                let size = stack_size(content, p);
                self.slots
                    .iter()
                    .map(|s| match s {
                        None => size as u64,
                        Some(s) if s.part == p => size.saturating_sub(s.count) as u64,
                        Some(_) => 0,
                    })
                    .sum()
            }
            ItemRef::Material(m) => self.tanks.iter().map(|t| t.room_for(m) as u64).sum(),
        };
        room.min(want as u64) as u32
    }

    /// Add `count` of an item. Stacks of the same item fill first, then empty slots (or tanks).
    /// Returns the count that did not fit (the overflow).
    pub fn insert(&mut self, content: &Content, item: ItemRef, count: u32) -> u32 {
        let mut left = count;
        match item {
            ItemRef::Part(p) => {
                let size = stack_size(content, p);
                for s in self.slots.iter_mut().flatten() {
                    if left == 0 {
                        break;
                    }
                    if s.part == p && s.count < size {
                        let add = (size - s.count).min(left);
                        s.count += add;
                        left -= add;
                    }
                }
                for s in self.slots.iter_mut() {
                    if left == 0 {
                        break;
                    }
                    if s.is_none() {
                        let add = size.min(left);
                        *s = Some(PartStack::new(p, add));
                        left -= add;
                    }
                }
            }
            ItemRef::Material(m) => {
                for t in self.tanks.iter_mut().filter(|t| t.material == Some(m)) {
                    left = t.fill(m, left);
                }
                for t in self.tanks.iter_mut().filter(|t| t.material.is_none()) {
                    if left == 0 {
                        break;
                    }
                    left = t.fill(m, left);
                }
            }
        }
        left
    }

    /// Add a stack. Returns what did not fit, if anything.
    pub fn insert_stack(&mut self, content: &Content, stack: Stack) -> Option<Stack> {
        let left = self.insert(content, stack.item, stack.count);
        (left > 0).then_some(Stack { item: stack.item, count: left })
    }

    /// Take up to `count` of an item. The last slots are emptied first. Returns the count taken.
    pub fn remove(&mut self, item: ItemRef, count: u32) -> u32 {
        let mut taken = 0;
        match item {
            ItemRef::Part(p) => {
                for s in self.slots.iter_mut().rev() {
                    if taken == count {
                        break;
                    }
                    if let Some(st) = s
                        && st.part == p
                    {
                        let t = st.count.min(count - taken);
                        st.count -= t;
                        taken += t;
                        if st.count == 0 {
                            *s = None;
                        }
                    }
                }
            }
            ItemRef::Material(m) => {
                for t in self.tanks.iter_mut().rev() {
                    if taken == count {
                        break;
                    }
                    taken += t.drain(m, count - taken);
                }
            }
        }
        taken
    }

    /// Everything in the inventory: one stack per item, in slot order, then tanks.
    pub fn contents(&self) -> Vec<Stack> {
        let mut out: Vec<Stack> = vec![];
        let mut add = |item: ItemRef, n: u32| {
            if n == 0 {
                return;
            }
            match out.iter_mut().find(|s| s.item == item) {
                Some(s) => s.count += n,
                None => out.push(Stack { item, count: n }),
            }
        };
        for s in self.slots.iter().flatten() {
            add(ItemRef::Part(s.part), s.count);
        }
        for t in &self.tanks {
            if let Some(m) = t.material {
                add(ItemRef::Material(m), t.units);
            }
        }
        out
    }

    /// Empty the inventory and return what it held.
    pub fn take_all(&mut self) -> Vec<Stack> {
        let out = self.contents();
        self.slots.fill(None);
        for t in &mut self.tanks {
            t.material = None;
            t.units = 0;
        }
        out
    }

    /// Apply a mouse action to a slot. `other` is the inventory that shift and ctrl clicks move
    /// items to (for example the open crate). Returns true if anything changed.
    pub fn click(
        &mut self,
        content: &Content,
        slot: usize,
        click: Click,
        cursor: &mut Option<PartStack>,
        other: Option<&mut Inventory>,
    ) -> bool {
        match click {
            Click::Left => self.left_click(content, slot, cursor),
            Click::Right => self.right_click(content, slot, cursor),
            Click::Shift => other.is_some_and(|o| self.shift_click(content, slot, o) > 0),
            Click::Ctrl => other.is_some_and(|o| self.ctrl_click(content, slot, o) > 0),
        }
    }

    /// Left click. Empty cursor: pick up the stack. Empty slot: put the cursor stack down.
    /// Same part: add as much as fits; if the slot is already full, swap. Different parts: swap.
    pub fn left_click(&mut self, content: &Content, slot: usize, cursor: &mut Option<PartStack>) -> bool {
        let Some(s) = self.slots.get_mut(slot) else { return false };
        match (s.as_mut(), cursor.as_mut()) {
            (None, None) => false,
            (Some(_), None) | (None, Some(_)) => {
                std::mem::swap(s, cursor);
                true
            }
            (Some(a), Some(c)) if a.part == c.part => {
                let size = stack_size(content, a.part);
                if a.count >= size {
                    std::mem::swap(s, cursor);
                    return true;
                }
                let add = (size - a.count).min(c.count);
                a.count += add;
                c.count -= add;
                if c.count == 0 {
                    *cursor = None;
                }
                true
            }
            (Some(_), Some(_)) => {
                std::mem::swap(s, cursor);
                true
            }
        }
    }

    /// Right click. Empty cursor: take half of the stack (rounded up). Cursor with a part: put one
    /// piece into an empty slot or onto a stack of the same part that has room.
    pub fn right_click(&mut self, content: &Content, slot: usize, cursor: &mut Option<PartStack>) -> bool {
        let Some(s) = self.slots.get_mut(slot) else { return false };
        match (s.as_mut(), cursor.as_mut()) {
            (None, None) => false,
            (Some(a), None) => {
                let half = a.count.div_ceil(2);
                a.count -= half;
                *cursor = Some(PartStack::new(a.part, half));
                if a.count == 0 {
                    *s = None;
                }
                true
            }
            (None, Some(c)) => {
                *s = Some(PartStack::new(c.part, 1));
                c.count -= 1;
                if c.count == 0 {
                    *cursor = None;
                }
                true
            }
            (Some(a), Some(c)) => {
                if a.part != c.part || a.count >= stack_size(content, a.part) {
                    return false;
                }
                a.count += 1;
                c.count -= 1;
                if c.count == 0 {
                    *cursor = None;
                }
                true
            }
        }
    }

    /// Shift click: move the stack in `slot` to `to`, as much as fits. Returns the count moved.
    pub fn shift_click(&mut self, content: &Content, slot: usize, to: &mut Inventory) -> u32 {
        let Some(Some(st)) = self.slots.get(slot).copied() else { return 0 };
        let left = to.insert(content, ItemRef::Part(st.part), st.count);
        let moved = st.count - left;
        self.slots[slot] = (left > 0).then_some(PartStack::new(st.part, left));
        moved
    }

    /// Ctrl click: move all pieces of the part in `slot` to `to`, as much as fits.
    /// Returns the count moved.
    pub fn ctrl_click(&mut self, content: &Content, slot: usize, to: &mut Inventory) -> u32 {
        let Some(Some(st)) = self.slots.get(slot).copied() else { return 0 };
        let item = ItemRef::Part(st.part);
        let have = self.count(item);
        let fits = to.room_for(content, item, have);
        let moved = self.remove(item, fits);
        let left = to.insert(content, item, moved);
        debug_assert_eq!(left, 0);
        moved
    }

    /// Shift click on a tank: move its material to `to`, as much as fits. Returns the units moved.
    pub fn shift_click_tank(&mut self, content: &Content, tank: usize, to: &mut Inventory) -> u32 {
        let Some(t) = self.tanks.get(tank).copied() else { return 0 };
        let Some(m) = t.material else { return 0 };
        let fits = to.room_for(content, ItemRef::Material(m), t.units);
        let moved = self.tanks[tank].drain(m, fits);
        to.insert(content, ItemRef::Material(m), moved);
        moved
    }

    /// Data for the inventory screen.
    pub fn view(&self, content: &Content) -> InventoryView {
        InventoryView {
            slots: self
                .slots
                .iter()
                .map(|s| {
                    s.map(|s| SlotView {
                        item: ItemRef::Part(s.part),
                        name: content.factory.part_def(s.part).name.clone(),
                        count: s.count,
                        stack: stack_size(content, s.part),
                    })
                })
                .collect(),
            tanks: self
                .tanks
                .iter()
                .map(|t| TankView {
                    item: t.material.map(ItemRef::Material),
                    name: t.material.map(|m| content.materials.names[m.index()].clone()).unwrap_or_default(),
                    units: t.units,
                    capacity: t.capacity,
                })
                .collect(),
        }
    }
}

/// One slot on the inventory screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotView {
    pub item: ItemRef,
    pub name: String,
    pub count: u32,
    /// Pieces in a full stack.
    pub stack: u32,
}

/// One tank on the inventory screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TankView {
    /// `None` when empty.
    pub item: Option<ItemRef>,
    /// Empty when the tank is empty.
    pub name: String,
    pub units: u32,
    pub capacity: u32,
}

/// An inventory for the screen: slots then tanks, in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InventoryView {
    pub slots: Vec<Option<SlotView>>,
    pub tanks: Vec<TankView>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, OnceLock};

    fn content() -> Arc<Content> {
        static C: OnceLock<Arc<Content>> = OnceLock::new();
        C.get_or_init(|| Arc::new(Content::load_default().expect("content loads"))).clone()
    }

    fn part(c: &Content, id: &str) -> PartId {
        c.factory.part(id).unwrap_or_else(|| panic!("no part {id}"))
    }

    #[test]
    fn insert_fills_stacks_then_empty_slots_and_returns_overflow() {
        let c = content();
        let gear = part(&c, "bronze_gear"); // stack 100
        let mut inv = Inventory::new(3, 0, 0);
        inv.slots[1] = Some(PartStack::new(gear, 90));
        assert_eq!(inv.insert(&c, ItemRef::Part(gear), 30), 0);
        assert_eq!(inv.slots[1], Some(PartStack::new(gear, 100)));
        assert_eq!(inv.slots[0], Some(PartStack::new(gear, 20)));
        // 2 free spaces are 80 + 100; 250 leaves 70.
        assert_eq!(inv.insert(&c, ItemRef::Part(gear), 250), 70);
        assert_eq!(inv.count(ItemRef::Part(gear)), 300);
        // No tanks: materials never fit.
        let sand = c.expect_material("sand");
        assert_eq!(inv.insert(&c, ItemRef::Material(sand), 5), 5);
    }

    #[test]
    fn tanks_hold_one_material_each() {
        let c = content();
        let (sand, clay) = (c.expect_material("sand"), c.expect_material("clay"));
        let mut inv = Inventory::new(0, 2, 100);
        assert_eq!(inv.insert(&c, ItemRef::Material(sand), 150), 0);
        assert_eq!(inv.insert(&c, ItemRef::Material(clay), 10), 10, "both tanks hold sand");
        assert_eq!(inv.remove(ItemRef::Material(sand), 60), 60);
        assert_eq!(inv.count(ItemRef::Material(sand)), 90);
        // The second tank (50 sand) is drained first and becomes free for clay.
        assert_eq!(inv.tanks[1].material, None);
        assert_eq!(inv.insert(&c, ItemRef::Material(clay), 10), 0);
    }

    #[test]
    fn remove_takes_from_the_last_slots() {
        let c = content();
        let gear = part(&c, "bronze_gear");
        let mut inv = Inventory::new(3, 0, 0);
        inv.insert(&c, ItemRef::Part(gear), 150);
        assert_eq!(inv.remove(ItemRef::Part(gear), 60), 60);
        assert_eq!(inv.slots[0], Some(PartStack::new(gear, 90)));
        assert_eq!(inv.slots[1], None);
        assert_eq!(inv.remove(ItemRef::Part(gear), 1000), 90);
        assert!(inv.is_empty());
    }

    #[test]
    fn left_click_picks_up_puts_down_merges_and_swaps() {
        let c = content();
        let (gear, plate) = (part(&c, "bronze_gear"), part(&c, "tin_plate"));
        let mut inv = Inventory::new(2, 0, 0);
        inv.slots[0] = Some(PartStack::new(gear, 10));
        let mut cursor = None;
        // Pick up.
        assert!(inv.left_click(&c, 0, &mut cursor));
        assert_eq!((inv.slots[0], cursor), (None, Some(PartStack::new(gear, 10))));
        // Put down.
        assert!(inv.left_click(&c, 1, &mut cursor));
        assert_eq!((inv.slots[1], cursor), (Some(PartStack::new(gear, 10)), None));
        // Merge: cursor 95 gears onto 10 gears: the slot gets 100, 5 stay in the cursor.
        cursor = Some(PartStack::new(gear, 95));
        assert!(inv.left_click(&c, 1, &mut cursor));
        assert_eq!((inv.slots[1], cursor), (Some(PartStack::new(gear, 100)), Some(PartStack::new(gear, 5))));
        // Full slot of the same part: swap.
        assert!(inv.left_click(&c, 1, &mut cursor));
        assert_eq!((inv.slots[1], cursor), (Some(PartStack::new(gear, 5)), Some(PartStack::new(gear, 100))));
        // Different part: swap.
        cursor = Some(PartStack::new(plate, 3));
        assert!(inv.left_click(&c, 1, &mut cursor));
        assert_eq!((inv.slots[1], cursor), (Some(PartStack::new(plate, 3)), Some(PartStack::new(gear, 5))));
        // Empty slot, empty cursor: nothing.
        let mut empty = None;
        assert!(!inv.left_click(&c, 0, &mut empty));
        assert!(!inv.left_click(&c, 9, &mut cursor), "slot out of range");
    }

    #[test]
    fn right_click_takes_half_and_puts_one() {
        let c = content();
        let (gear, plate) = (part(&c, "bronze_gear"), part(&c, "tin_plate"));
        let mut inv = Inventory::new(2, 0, 0);
        inv.slots[0] = Some(PartStack::new(gear, 7));
        let mut cursor = None;
        assert!(inv.right_click(&c, 0, &mut cursor));
        assert_eq!((inv.slots[0], cursor), (Some(PartStack::new(gear, 3)), Some(PartStack::new(gear, 4))));
        assert!(inv.right_click(&c, 1, &mut cursor));
        assert_eq!((inv.slots[1], cursor), (Some(PartStack::new(gear, 1)), Some(PartStack::new(gear, 3))));
        assert!(inv.right_click(&c, 0, &mut cursor));
        assert_eq!((inv.slots[0], cursor), (Some(PartStack::new(gear, 4)), Some(PartStack::new(gear, 2))));
        // A slot of 1: take half gives 1 and empties the slot.
        let mut c2 = None;
        assert!(inv.right_click(&c, 1, &mut c2));
        assert_eq!((inv.slots[1], c2), (None, Some(PartStack::new(gear, 1))));
        // Different part: nothing happens.
        let mut other = Some(PartStack::new(plate, 5));
        assert!(!inv.right_click(&c, 0, &mut other));
        // The last piece empties the cursor.
        let mut one = Some(PartStack::new(gear, 1));
        assert!(inv.right_click(&c, 0, &mut one));
        assert_eq!((inv.slots[0], one), (Some(PartStack::new(gear, 5)), None));
    }

    #[test]
    fn shift_click_moves_one_stack_and_keeps_the_rest() {
        let c = content();
        let gear = part(&c, "bronze_gear");
        let mut a = Inventory::new(2, 0, 0);
        let mut b = Inventory::new(1, 0, 0);
        a.slots[0] = Some(PartStack::new(gear, 60));
        a.slots[1] = Some(PartStack::new(gear, 60));
        b.slots[0] = Some(PartStack::new(gear, 70));
        // b has room for 30.
        assert_eq!(a.shift_click(&c, 0, &mut b), 30);
        assert_eq!(a.slots[0], Some(PartStack::new(gear, 30)));
        assert_eq!(b.slots[0], Some(PartStack::new(gear, 100)));
        assert_eq!(a.shift_click(&c, 0, &mut b), 0);
        // Back the other way: all 100 fit into a (room 70 + 40 = 110).
        assert_eq!(b.shift_click(&c, 0, &mut a), 100);
        assert_eq!(b.slots[0], None);
        assert_eq!(a.count(ItemRef::Part(gear)), 190);
    }

    #[test]
    fn ctrl_click_moves_all_of_that_part() {
        let c = content();
        let (gear, plate) = (part(&c, "bronze_gear"), part(&c, "tin_plate"));
        let mut a = Inventory::new(4, 0, 0);
        a.slots[0] = Some(PartStack::new(gear, 10));
        a.slots[1] = Some(PartStack::new(plate, 5));
        a.slots[3] = Some(PartStack::new(gear, 20));
        let mut b = Inventory::new(4, 0, 0);
        let mut cursor = None;
        assert!(a.click(&c, 3, Click::Ctrl, &mut cursor, Some(&mut b)));
        assert_eq!(a.count(ItemRef::Part(gear)), 0);
        assert_eq!(a.count(ItemRef::Part(plate)), 5);
        assert_eq!(b.slots[0], Some(PartStack::new(gear, 30)));
        // Only part of it fits: the rest stays.
        let mut small = Inventory::new(1, 0, 0);
        small.slots[0] = Some(PartStack::new(gear, 95));
        assert_eq!(b.ctrl_click(&c, 0, &mut small), 5);
        assert_eq!(b.count(ItemRef::Part(gear)), 25);
    }

    #[test]
    fn shift_click_on_a_tank_moves_units() {
        let c = content();
        let sand = c.expect_material("sand");
        let mut a = Inventory::new(0, 1, 100);
        let mut b = Inventory::new(0, 1, 30);
        a.insert(&c, ItemRef::Material(sand), 80);
        assert_eq!(a.shift_click_tank(&c, 0, &mut b), 30);
        assert_eq!(a.count(ItemRef::Material(sand)), 50);
        assert_eq!(b.count(ItemRef::Material(sand)), 30);
    }

    #[test]
    fn view_lists_slots_and_tanks() {
        let c = content();
        let gear = part(&c, "bronze_gear");
        let mut inv = Inventory::new(2, 1, 50);
        inv.insert(&c, ItemRef::Part(gear), 3);
        inv.insert(&c, ItemRef::Material(c.expect_material("sand")), 7);
        let v = inv.view(&c);
        assert_eq!(v.slots[0].as_ref().unwrap().name, "Bronze gear");
        assert_eq!(v.slots[0].as_ref().unwrap().stack, 100);
        assert!(v.slots[1].is_none());
        assert_eq!((v.tanks[0].name.as_str(), v.tanks[0].units, v.tanks[0].capacity), ("Sand", 7, 50));
    }
}
