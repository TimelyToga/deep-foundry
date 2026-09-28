//! Inventories: slots for parts and tanks for bulk material.
//!
//! - A slot holds one stack of one part, up to the part's stack size.
//! - A tank holds one material, in units (1 unit = 1 cell), up to its capacity.
//! - In a storage inventory with `mixed` slots (a crate, the Hub), part slot `i` and tank `i`
//!   are one slot: it holds one part stack or one material amount, never both.
//! - `takes` limits the materials of the tanks (a barrel keeps only liquids).
//!
//! The slot rules follow Factorio. The mouse cursor can hold one part stack (`cursor`).
//! - Left click: pick up a stack, put it down, add to a stack of the same part, or swap.
//! - Right click: take half of a stack, or put down one piece.
//! - Shift click: move the stack to the other inventory (shift + right: half of it).
//! - Ctrl click: move all pieces of that part to the other inventory (ctrl + right: half).
//!
//! The moves between the robot and a building window are in `transfer.rs`.

use foundry_content::{Content, ItemRef, Phase, Stack};
use foundry_core::{MaterialId, PartId};
use serde::{Deserialize, Serialize};

/// Tanks of the robot (game design section 6.4). One tank holds one material, so the robot can
/// carry 8 different materials.
pub const PLAYER_TANKS: usize = 8;
/// Units in one tank of the robot (1 unit = 1 cell).
///
/// The dig tool takes at most 12 cells of soft ground per tick (`tools::DIG_POWER`), which is
/// 720 units per second while the whole dig circle has ground in it. 8 tanks of 6,000 units
/// (48,000 units) take 67 seconds of that top speed. A player also walks, aims and digs harder
/// ground, so the tanks last a few minutes of digging.
pub const PLAYER_TANK_UNITS: u32 = 6_000;
/// Part slots of the robot.
pub const PLAYER_SLOTS: usize = 40;

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
    /// Shift + right click: like `Shift`, with half of the stack.
    ShiftRight,
    /// Ctrl + right click: like `Ctrl`, with half of the pieces.
    CtrlRight,
}

impl Click {
    /// True for the clicks that move items to the other inventory (Shift and Ctrl).
    pub fn is_move(self) -> bool {
        !matches!(self, Click::Left | Click::Right)
    }

    /// True for the right mouse button.
    pub fn is_right(self) -> bool {
        matches!(self, Click::Right | Click::ShiftRight | Click::CtrlRight)
    }
}

/// Which materials the tanks of an inventory take.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TankRule {
    /// Every material (the robot).
    #[default]
    Any,
    /// Only liquids (a barrel).
    Liquid,
    /// Only powders and solids (a crate).
    Bulk,
}

/// Part slots and material tanks.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inventory {
    pub slots: Vec<Option<PartStack>>,
    pub tanks: Vec<Tank>,
    /// Storage slots: part slot `i` and tank `i` are one slot. It holds one part stack or one
    /// material amount. `slots` and `tanks` then have the same length.
    #[serde(default)]
    pub mixed: bool,
    /// The materials that the tanks take.
    #[serde(default)]
    pub takes: TankRule,
}

/// Pieces of `part` in one slot.
pub fn stack_size(content: &Content, part: PartId) -> u32 {
    content.factory.part_def(part).stack.max(1) as u32
}

/// One slot of a storage building: a part slot or a tank. See [`Inventory::place`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Part(usize),
    Tank(usize),
}

impl Inventory {
    /// An inventory with `slots` empty slots and `tanks` empty tanks of `tank_capacity` units.
    pub fn new(slots: usize, tanks: usize, tank_capacity: u32) -> Self {
        Self { slots: vec![None; slots], tanks: vec![Tank::new(tank_capacity); tanks], mixed: false, takes: TankRule::Any }
    }

    /// A storage inventory with `n` slots. Each slot holds one part stack or up to `units` of one
    /// material that `takes` allows.
    pub fn mixed(n: usize, units: u32, takes: TankRule) -> Self {
        Self { slots: vec![None; n], tanks: vec![Tank::new(units); n], mixed: true, takes }
    }

    /// The robot's start inventory: `PLAYER_SLOTS` part slots and `PLAYER_TANKS` tanks of
    /// `PLAYER_TANK_UNITS` units.
    pub fn player() -> Self {
        Self::new(PLAYER_SLOTS, PLAYER_TANKS, PLAYER_TANK_UNITS)
    }

    /// True if the tanks take this material.
    pub fn takes_material(&self, content: &Content, m: MaterialId) -> bool {
        let phase = content.materials.phase.get(m.index()).copied().unwrap_or_default();
        match self.takes {
            TankRule::Any => true,
            TankRule::Liquid => phase == Phase::Liquid,
            TankRule::Bulk => matches!(phase, Phase::Powder | Phase::Solid),
        }
    }

    /// True if part slot `i` may hold a part (in a mixed inventory, its tank must be empty).
    fn part_slot_open(&self, i: usize) -> bool {
        !self.mixed || self.tanks.get(i).is_none_or(|t| t.material.is_none())
    }

    /// True if tank `i` may hold material (in a mixed inventory, its part slot must be empty).
    fn tank_open(&self, i: usize) -> bool {
        !self.mixed || self.slots.get(i).is_none_or(|s| s.is_none())
    }

    /// The number of slots a storage window shows: one per mixed slot, else the part slots and
    /// then the tanks.
    pub fn place_count(&self) -> usize {
        if self.mixed { self.slots.len() } else { self.slots.len() + self.tanks.len() }
    }

    /// What slot `i` of a storage window is. In a mixed inventory, it is the tank if the tank
    /// has material, else the part slot.
    pub fn place(&self, i: usize) -> Option<Place> {
        if self.mixed {
            let has_material = self.tanks.get(i).is_some_and(|t| t.material.is_some());
            return (i < self.slots.len()).then_some(if has_material { Place::Tank(i) } else { Place::Part(i) });
        }
        if i < self.slots.len() {
            Some(Place::Part(i))
        } else {
            (i - self.slots.len() < self.tanks.len()).then_some(Place::Tank(i - self.slots.len()))
        }
    }

    /// The item and the count in slot `i` of a storage window.
    pub fn place_stack(&self, i: usize) -> Option<Stack> {
        match self.place(i)? {
            Place::Part(j) => self.slots[j].map(|s| s.to_stack()),
            Place::Tank(j) => {
                let t = self.tanks[j];
                t.material.map(|m| Stack { item: ItemRef::Material(m), count: t.units })
            }
        }
    }

    /// Take up to `n` of the item in slot `i` of a storage window. Returns the count taken.
    pub fn take_from_place(&mut self, i: usize, n: u32) -> u32 {
        match self.place(i) {
            Some(Place::Part(j)) => {
                let Some(s) = self.slots[j].as_mut() else { return 0 };
                let t = s.count.min(n);
                s.count -= t;
                if s.count == 0 {
                    self.slots[j] = None;
                }
                t
            }
            Some(Place::Tank(j)) => match self.tanks[j].material {
                Some(m) => self.tanks[j].drain(m, n),
                None => 0,
            },
            None => 0,
        }
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
                    .enumerate()
                    .map(|(i, s)| match s {
                        None if self.part_slot_open(i) => size as u64,
                        None => 0,
                        Some(s) if s.part == p => size.saturating_sub(s.count) as u64,
                        Some(_) => 0,
                    })
                    .sum()
            }
            ItemRef::Material(m) if self.takes_material(content, m) => {
                self.tanks.iter().enumerate().filter(|(i, _)| self.tank_open(*i)).map(|(_, t)| t.room_for(m) as u64).sum()
            }
            ItemRef::Material(_) => 0,
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
                for i in 0..self.slots.len() {
                    if left == 0 {
                        break;
                    }
                    if self.slots[i].is_none() && self.part_slot_open(i) {
                        let add = size.min(left);
                        self.slots[i] = Some(PartStack::new(p, add));
                        left -= add;
                    }
                }
            }
            ItemRef::Material(m) if self.takes_material(content, m) => {
                for t in self.tanks.iter_mut().filter(|t| t.material == Some(m)) {
                    left = t.fill(m, left);
                }
                for i in 0..self.tanks.len() {
                    if left == 0 {
                        break;
                    }
                    if self.tanks[i].material.is_none() && self.tank_open(i) {
                        left = self.tanks[i].fill(m, left);
                    }
                }
            }
            ItemRef::Material(_) => {}
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
            Click::Shift | Click::ShiftRight => other.is_some_and(|o| self.shift_click(content, slot, o, click.is_right()) > 0),
            Click::Ctrl | Click::CtrlRight => other.is_some_and(|o| self.ctrl_click(content, slot, o, click.is_right()) > 0),
        }
    }

    /// Left click. Empty cursor: pick up the stack. Empty slot: put the cursor stack down.
    /// Same part: add as much as fits; if the slot is already full, swap. Different parts: swap.
    pub fn left_click(&mut self, content: &Content, slot: usize, cursor: &mut Option<PartStack>) -> bool {
        let open = self.part_slot_open(slot);
        let Some(s) = self.slots.get_mut(slot) else { return false };
        match (s.as_mut(), cursor.as_mut()) {
            (None, None) => false,
            // A material is in this slot (a mixed storage slot).
            (None, Some(_)) if !open => false,
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
        let open = self.part_slot_open(slot);
        let Some(s) = self.slots.get_mut(slot) else { return false };
        match (s.as_mut(), cursor.as_mut()) {
            (None, None) => false,
            (None, Some(_)) if !open => false,
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

    /// Shift click: move the stack in `slot` (`half`: half of it) to `to`, as much as fits.
    /// Returns the count moved.
    pub fn shift_click(&mut self, content: &Content, slot: usize, to: &mut Inventory, half: bool) -> u32 {
        let Some(Some(st)) = self.slots.get(slot).copied() else { return 0 };
        let want = if half { st.count.div_ceil(2) } else { st.count };
        let left = to.insert(content, ItemRef::Part(st.part), want);
        let moved = want - left;
        let rest = st.count - moved;
        self.slots[slot] = (rest > 0).then_some(PartStack::new(st.part, rest));
        moved
    }

    /// Ctrl click: move all pieces of the part in `slot` (`half`: half of them) to `to`, as much
    /// as fits. Returns the count moved.
    pub fn ctrl_click(&mut self, content: &Content, slot: usize, to: &mut Inventory, half: bool) -> u32 {
        let Some(Some(st)) = self.slots.get(slot).copied() else { return 0 };
        let item = ItemRef::Part(st.part);
        let have = self.count(item);
        let want = if half { have.div_ceil(2) } else { have };
        let fits = to.room_for(content, item, want);
        let moved = self.remove(item, fits);
        let left = to.insert(content, item, moved);
        debug_assert_eq!(left, 0);
        moved
    }

    /// Make each tank hold at least `capacity` units and have at least `n` tanks. The material
    /// in the tanks stays. (For saves from a version with smaller tanks.)
    pub fn grow_tanks(&mut self, n: usize, capacity: u32) {
        for t in &mut self.tanks {
            t.capacity = t.capacity.max(capacity);
        }
        if self.tanks.len() < n {
            self.tanks.resize(n, Tank::new(capacity));
        }
    }

    /// Delete the material in a tank. Returns the units deleted.
    pub fn empty_tank(&mut self, tank: usize) -> u32 {
        let Some(t) = self.tanks.get_mut(tank) else { return 0 };
        let units = t.units;
        t.units = 0;
        t.material = None;
        units
    }

    /// Data for the inventory screen.
    pub fn view(&self, content: &Content) -> InventoryView {
        InventoryView {
            mixed: self.mixed,
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
    /// Part slot `i` and tank `i` are one slot (see [`Inventory::mixed`]).
    pub mixed: bool,
}

/// One slot of a storage window: a part stack, a tank, or an empty slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaceView {
    pub item: Option<ItemRef>,
    pub count: u32,
    /// Units of a tank; 0 for a part slot.
    pub capacity: u32,
}

impl InventoryView {
    /// The slots of a storage window, in the order of [`Inventory::place`].
    pub fn places(&self) -> Vec<PlaceView> {
        let part = |s: &Option<SlotView>| PlaceView { item: s.as_ref().map(|s| s.item), count: s.as_ref().map_or(0, |s| s.count), capacity: 0 };
        let tank = |t: &TankView| PlaceView { item: t.item, count: t.units, capacity: t.capacity };
        if self.mixed {
            self.slots.iter().zip(&self.tanks).map(|(s, t)| if t.item.is_some() { tank(t) } else { part(s) }).collect()
        } else {
            self.slots.iter().map(part).chain(self.tanks.iter().map(tank)).collect()
        }
    }
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
        assert_eq!(a.shift_click(&c, 0, &mut b, false), 30);
        assert_eq!(a.slots[0], Some(PartStack::new(gear, 30)));
        assert_eq!(b.slots[0], Some(PartStack::new(gear, 100)));
        assert_eq!(a.shift_click(&c, 0, &mut b, false), 0);
        // Back the other way: all 100 fit into a (room 70 + 40 = 110).
        assert_eq!(b.shift_click(&c, 0, &mut a, false), 100);
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
        assert_eq!(b.ctrl_click(&c, 0, &mut small, false), 5);
        assert_eq!(b.count(ItemRef::Part(gear)), 25);
    }

    #[test]
    fn shift_right_click_moves_half_the_stack() {
        let c = content();
        let gear = part(&c, "bronze_gear");
        let mut a = Inventory::new(1, 0, 0);
        let mut b = Inventory::new(1, 0, 0);
        a.slots[0] = Some(PartStack::new(gear, 9));
        assert!(a.click(&c, 0, Click::ShiftRight, &mut None, Some(&mut b)));
        assert_eq!((a.count(ItemRef::Part(gear)), b.count(ItemRef::Part(gear))), (4, 5));
    }

    #[test]
    fn a_mixed_slot_holds_a_part_stack_or_a_material() {
        let c = content();
        let gear = part(&c, "bronze_gear");
        let (sand, water) = (c.expect_material("sand"), c.expect_material("water"));
        let mut crate_ = Inventory::mixed(3, 500, TankRule::Bulk);
        // Parts use slot 0; sand fills the two other slots, then no room is left.
        assert_eq!(crate_.insert(&c, ItemRef::Part(gear), 30), 0);
        assert_eq!(crate_.room_for(&c, ItemRef::Material(sand), u32::MAX), 1000);
        assert_eq!(crate_.insert(&c, ItemRef::Material(sand), 1200), 200);
        assert_eq!(crate_.room_for(&c, ItemRef::Part(gear), u32::MAX), 70);
        assert_eq!(crate_.insert(&c, ItemRef::Part(part(&c, "tin_plate")), 1), 1, "no free slot for a new part");
        assert_eq!(crate_.place_count(), 3);
        assert_eq!(crate_.place(0), Some(Place::Part(0)));
        assert_eq!(crate_.place(1), Some(Place::Tank(1)));
        assert_eq!(crate_.place_stack(2), Some(Stack { item: ItemRef::Material(sand), count: 500 }));
        // A crate keeps bulk materials only.
        assert_eq!(crate_.room_for(&c, ItemRef::Material(water), 10), 0);
        // The cursor cannot put a part on a slot that holds material.
        let mut cursor = Some(PartStack::new(gear, 5));
        assert!(!crate_.left_click(&c, 1, &mut cursor));
        assert!(!crate_.right_click(&c, 1, &mut cursor));
        // A slot is free again when its material is taken.
        assert_eq!(crate_.take_from_place(1, 800), 500);
        assert_eq!(crate_.place(1), Some(Place::Part(1)));
        assert!(crate_.left_click(&c, 1, &mut cursor));
        assert_eq!(crate_.slots[1], Some(PartStack::new(gear, 5)));
        // The view shows one slot per place.
        let places = crate_.view(&c).places();
        assert_eq!(places.len(), 3);
        assert_eq!(places[2], PlaceView { item: Some(ItemRef::Material(sand)), count: 500, capacity: 500 });
        assert_eq!(places[1], PlaceView { item: Some(ItemRef::Part(gear)), count: 5, capacity: 0 });
    }

    #[test]
    fn a_barrel_keeps_liquids_only() {
        let c = content();
        let (sand, water) = (c.expect_material("sand"), c.expect_material("water"));
        let mut barrel = Inventory::new(0, 1, 500);
        barrel.takes = TankRule::Liquid;
        assert_eq!(barrel.insert(&c, ItemRef::Material(sand), 10), 10);
        assert_eq!(barrel.insert(&c, ItemRef::Material(water), 600), 100);
        // Not mixed: the window shows the part slots (none), then the tank.
        assert_eq!(barrel.place(0), Some(Place::Tank(0)));
        assert_eq!(barrel.place(1), None);
    }

    #[test]
    fn the_robot_has_large_tanks() {
        let inv = Inventory::player();
        assert_eq!(inv.tanks.len(), PLAYER_TANKS);
        assert!(inv.tanks.iter().all(|t| t.capacity == PLAYER_TANK_UNITS));
        // At the top dig speed (12 cells per tick, 60 ticks per second) the tanks take about
        // a minute to fill: a player who also walks and aims digs for a few minutes.
        let seconds = (PLAYER_TANKS as u32 * PLAYER_TANK_UNITS) as f32 / (12.0 * 60.0);
        assert!(seconds >= 60.0, "{seconds}");
    }

    #[test]
    fn empty_tank_deletes_the_material() {
        let c = content();
        let sand = c.expect_material("sand");
        let mut inv = Inventory::player();
        inv.insert(&c, ItemRef::Material(sand), 7000);
        assert_eq!(inv.empty_tank(0), PLAYER_TANK_UNITS);
        assert_eq!(inv.count(ItemRef::Material(sand)), 1000);
        assert_eq!(inv.tanks[0].material, None);
        assert_eq!(inv.empty_tank(99), 0);
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
