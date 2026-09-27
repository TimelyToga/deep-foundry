//! The Factorio rules for clicks on item slots.
//!
//! The UI only reports clicks (`UiAction::ClickSlot`). The owner of the items (the game or
//! the simulation) applies the rules with [`apply`]. This module has no egui code, so any
//! crate can use it.
//!
//! | Click | Hand empty | Hand holds items |
//! |---|---|---|
//! | Left | Pick up the whole stack. | Put the stack down. Same item: add to the slot. Other item or full slot: swap. |
//! | Right | Take half (rounded up). | Put one item down. |
//! | Shift + left | Move the whole stack to the other inventory. | (same) |
//! | Shift + right | Move half of the stack to the other inventory. | (same) |
//! | Ctrl + left | Move all items of this type to the other inventory. | (same) |
//! | Ctrl + right | Move half of all items of this type to the other inventory. | (same) |
//!
//! Items move into stacks of the same item first, then into empty slots, in slot order.

use crate::action::{ClickButton, SlotClick};
use crate::item::{ItemId, ItemStack};

/// One list of slots (an inventory, the tank, the inputs of a building) and its limits.
pub struct SlotList<'a> {
    pub slots: &'a mut [Option<ItemStack>],
    /// The most of `item` that slot `index` can hold. 0 means the slot does not take this item
    /// (for example a part in a tank slot, or anything in an output slot).
    pub limit: &'a dyn Fn(usize, ItemId) -> u32,
}

impl<'a> SlotList<'a> {
    pub fn new(slots: &'a mut [Option<ItemStack>], limit: &'a dyn Fn(usize, ItemId) -> u32) -> Self {
        Self { slots, limit }
    }

    fn limit_of(&self, index: usize, item: ItemId) -> u32 {
        (self.limit)(index, item)
    }

    /// The number of `item` in all slots.
    pub fn count_of(&self, item: ItemId) -> u64 {
        self.slots.iter().flatten().filter(|s| s.item == item).map(|s| s.count as u64).sum()
    }
}

/// Apply a click on slot `index` of `this`.
///
/// `others` are the inventories that shift-click and ctrl-click move items into, in the order
/// to try (for example: the fuel slots, then the input slots of the open building).
/// Pass an empty list if no other inventory is open; then shift-click and ctrl-click do nothing.
pub fn apply(hand: &mut Option<ItemStack>, this: &mut SlotList, index: usize, others: &mut [SlotList], click: SlotClick) {
    if index >= this.slots.len() {
        return;
    }
    let half = click.button == ClickButton::Right;
    if click.ctrl {
        if let Some(item) = this.slots[index].map(|s| s.item) {
            move_all_of(this, item, others, half);
        }
    } else if click.shift {
        if let Some(stack) = this.slots[index] {
            let amount = if half { stack.count.div_ceil(2) } else { stack.count };
            move_from_slot(this, index, others, amount);
        }
    } else if half {
        right_click(hand, this, index);
    } else {
        left_click(hand, this, index);
    }
}

/// Left click: pick up, put down, add, or swap.
pub fn left_click(hand: &mut Option<ItemStack>, list: &mut SlotList, index: usize) {
    match (*hand, list.slots[index]) {
        (None, None) => {}
        (None, Some(stack)) => {
            *hand = Some(stack);
            list.slots[index] = None;
        }
        (Some(h), None) => {
            let n = h.count.min(list.limit_of(index, h.item));
            if n > 0 {
                list.slots[index] = Some(ItemStack::new(h.item, n));
                *hand = take(h, n);
            }
        }
        (Some(h), Some(s)) if h.item == s.item => {
            let limit = list.limit_of(index, h.item);
            if s.count < limit {
                let n = h.count.min(limit - s.count);
                list.slots[index] = Some(ItemStack::new(s.item, s.count + n));
                *hand = take(h, n);
            } else if limit > 0 && h.count <= limit {
                // The slot is full: swap, as in Factorio.
                list.slots[index] = Some(h);
                *hand = Some(s);
            }
        }
        (Some(h), Some(s)) => {
            if list.limit_of(index, h.item) >= h.count {
                list.slots[index] = Some(h);
                *hand = Some(s);
            }
        }
    }
}

/// Right click: take half (rounded up), or put one item down.
pub fn right_click(hand: &mut Option<ItemStack>, list: &mut SlotList, index: usize) {
    match (*hand, list.slots[index]) {
        (None, None) => {}
        (None, Some(s)) => {
            let n = s.count.div_ceil(2);
            *hand = Some(ItemStack::new(s.item, n));
            list.slots[index] = take(s, n);
        }
        (Some(h), None) => {
            if list.limit_of(index, h.item) >= 1 {
                list.slots[index] = Some(ItemStack::new(h.item, 1));
                *hand = take(h, 1);
            }
        }
        (Some(h), Some(s)) if h.item == s.item => {
            if s.count < list.limit_of(index, s.item) {
                list.slots[index] = Some(ItemStack::new(s.item, s.count + 1));
                *hand = take(h, 1);
            }
        }
        (Some(_), Some(_)) => {}
    }
}

/// Put up to `count` of `item` into the list. Stacks of the same item first, then empty slots.
/// Returns how many went in.
pub fn insert(list: &mut SlotList, item: ItemId, count: u32) -> u32 {
    let mut left = count;
    // Pass 1: add to stacks of the same item.
    for i in 0..list.slots.len() {
        if left == 0 {
            break;
        }
        if let Some(s) = list.slots[i]
            && s.item == item
        {
            let room = list.limit_of(i, item).saturating_sub(s.count);
            let n = room.min(left);
            if n > 0 {
                list.slots[i] = Some(ItemStack::new(item, s.count + n));
                left -= n;
            }
        }
    }
    // Pass 2: empty slots.
    for i in 0..list.slots.len() {
        if left == 0 {
            break;
        }
        if list.slots[i].is_none() {
            let n = list.limit_of(i, item).min(left);
            if n > 0 {
                list.slots[i] = Some(ItemStack::new(item, n));
                left -= n;
            }
        }
    }
    count - left
}

/// Move up to `amount` from slot `index` of `from` into `to` (tried in order). Returns how many moved.
pub fn move_from_slot(from: &mut SlotList, index: usize, to: &mut [SlotList], amount: u32) -> u32 {
    let Some(stack) = from.slots[index] else { return 0 };
    let mut left = amount.min(stack.count);
    for target in to.iter_mut() {
        if left == 0 {
            break;
        }
        left -= insert(target, stack.item, left);
    }
    let moved = amount.min(stack.count) - left;
    from.slots[index] = take(stack, moved);
    moved
}

/// Move all items of one type (or half of them, rounded up) from `from` into `to`.
/// Returns how many moved.
pub fn move_all_of(from: &mut SlotList, item: ItemId, to: &mut [SlotList], half: bool) -> u32 {
    let total = from.count_of(item).min(u32::MAX as u64) as u32;
    let mut want = if half { total.div_ceil(2) } else { total };
    let mut moved = 0;
    for i in 0..from.slots.len() {
        if want == 0 {
            break;
        }
        if from.slots[i].is_some_and(|s| s.item == item) {
            let n = move_from_slot(from, i, to, want);
            moved += n;
            want -= n;
            if from.slots[i].is_some() {
                break; // The other inventories are full.
            }
        }
    }
    moved
}

/// A stack with `n` fewer items, or `None` if nothing is left.
fn take(stack: ItemStack, n: u32) -> Option<ItemStack> {
    let left = stack.count.saturating_sub(n);
    (left > 0).then_some(ItemStack::new(stack.item, left))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::PartId;

    const GEAR: ItemId = ItemId::Part(PartId(1));
    const PLATE: ItemId = ItemId::Part(PartId(2));

    fn st(item: ItemId, n: u32) -> Option<ItemStack> {
        Some(ItemStack::new(item, n))
    }

    fn limit50(_: usize, _: ItemId) -> u32 {
        50
    }

    #[test]
    fn left_click_picks_up_whole_stack() {
        let mut slots = [st(GEAR, 20), None];
        let mut hand = None;
        left_click(&mut hand, &mut SlotList::new(&mut slots, &limit50), 0);
        assert_eq!(hand, st(GEAR, 20));
        assert_eq!(slots[0], None);
    }

    #[test]
    fn left_click_puts_stack_down() {
        let mut slots = [None, None];
        let mut hand = st(GEAR, 20);
        left_click(&mut hand, &mut SlotList::new(&mut slots, &limit50), 1);
        assert_eq!(hand, None);
        assert_eq!(slots[1], st(GEAR, 20));
    }

    #[test]
    fn left_click_adds_same_item_and_keeps_the_rest() {
        let mut slots = [st(GEAR, 40)];
        let mut hand = st(GEAR, 20);
        left_click(&mut hand, &mut SlotList::new(&mut slots, &limit50), 0);
        assert_eq!(slots[0], st(GEAR, 50));
        assert_eq!(hand, st(GEAR, 10));
    }

    #[test]
    fn left_click_on_full_stack_of_same_item_swaps() {
        let mut slots = [st(GEAR, 50)];
        let mut hand = st(GEAR, 10);
        left_click(&mut hand, &mut SlotList::new(&mut slots, &limit50), 0);
        assert_eq!(slots[0], st(GEAR, 10));
        assert_eq!(hand, st(GEAR, 50));
    }

    #[test]
    fn left_click_swaps_different_items() {
        let mut slots = [st(PLATE, 7)];
        let mut hand = st(GEAR, 3);
        left_click(&mut hand, &mut SlotList::new(&mut slots, &limit50), 0);
        assert_eq!(slots[0], st(GEAR, 3));
        assert_eq!(hand, st(PLATE, 7));
    }

    #[test]
    fn left_click_does_nothing_if_slot_does_not_take_the_item() {
        let only_plates = |_: usize, item: ItemId| if item == PLATE { 50 } else { 0 };
        let mut slots = [None];
        let mut hand = st(GEAR, 3);
        left_click(&mut hand, &mut SlotList::new(&mut slots, &only_plates), 0);
        assert_eq!(slots[0], None);
        assert_eq!(hand, st(GEAR, 3));
    }

    #[test]
    fn right_click_takes_half_rounded_up() {
        let mut slots = [st(GEAR, 7)];
        let mut hand = None;
        right_click(&mut hand, &mut SlotList::new(&mut slots, &limit50), 0);
        assert_eq!(hand, st(GEAR, 4));
        assert_eq!(slots[0], st(GEAR, 3));

        let mut slots = [st(GEAR, 1)];
        let mut hand = None;
        right_click(&mut hand, &mut SlotList::new(&mut slots, &limit50), 0);
        assert_eq!(hand, st(GEAR, 1));
        assert_eq!(slots[0], None);
    }

    #[test]
    fn right_click_puts_one_down() {
        let mut slots = [None, st(GEAR, 5), st(PLATE, 5)];
        let mut hand = st(GEAR, 3);
        let limit = limit50;
        let mut list = SlotList::new(&mut slots, &limit);
        right_click(&mut hand, &mut list, 0);
        right_click(&mut hand, &mut list, 1);
        right_click(&mut hand, &mut list, 2); // Other item: nothing happens.
        assert_eq!(slots, [st(GEAR, 1), st(GEAR, 6), st(PLATE, 5)]);
        assert_eq!(hand, st(GEAR, 1));
    }

    #[test]
    fn shift_click_moves_stack_to_other_inventory() {
        let mut inv = [st(GEAR, 30), None];
        let mut chest = [st(GEAR, 45), st(PLATE, 10), None];
        let mut hand = None;
        {
            let mut this = SlotList::new(&mut inv, &limit50);
            let mut others = [SlotList::new(&mut chest, &limit50)];
            apply(&mut hand, &mut this, 0, &mut others, SlotClick::SHIFT_LEFT);
        }
        // 5 fill the stack of 45 first, then 25 go to the empty slot.
        assert_eq!(chest, [st(GEAR, 50), st(PLATE, 10), st(GEAR, 25)]);
        assert_eq!(inv[0], None);
        assert_eq!(hand, None);
    }

    #[test]
    fn shift_click_keeps_what_does_not_fit() {
        let mut inv = [st(GEAR, 30)];
        let mut chest = [st(GEAR, 45)];
        let mut hand = None;
        {
            let mut this = SlotList::new(&mut inv, &limit50);
            let mut others = [SlotList::new(&mut chest, &limit50)];
            apply(&mut hand, &mut this, 0, &mut others, SlotClick::SHIFT_LEFT);
        }
        assert_eq!(chest, [st(GEAR, 50)]);
        assert_eq!(inv, [st(GEAR, 25)]);
    }

    #[test]
    fn shift_right_click_moves_half() {
        let mut inv = [st(GEAR, 9)];
        let mut chest = [None];
        let mut hand = None;
        {
            let mut this = SlotList::new(&mut inv, &limit50);
            let mut others = [SlotList::new(&mut chest, &limit50)];
            apply(&mut hand, &mut this, 0, &mut others, SlotClick::SHIFT_RIGHT);
        }
        assert_eq!(chest, [st(GEAR, 5)]);
        assert_eq!(inv, [st(GEAR, 4)]);
    }

    #[test]
    fn shift_click_tries_other_inventories_in_order() {
        let fuel_only_plates = |_: usize, item: ItemId| if item == PLATE { 10 } else { 0 };
        let mut inv = [st(PLATE, 15)];
        let mut fuel = [None];
        let mut input = [None];
        let mut hand = None;
        {
            let mut this = SlotList::new(&mut inv, &limit50);
            let mut others = [SlotList::new(&mut fuel, &fuel_only_plates), SlotList::new(&mut input, &limit50)];
            apply(&mut hand, &mut this, 0, &mut others, SlotClick::SHIFT_LEFT);
        }
        assert_eq!(fuel, [st(PLATE, 10)]);
        assert_eq!(input, [st(PLATE, 5)]);
        assert_eq!(inv, [None]);
    }

    #[test]
    fn ctrl_click_moves_all_of_the_same_item() {
        let mut inv = [st(GEAR, 10), st(PLATE, 5), st(GEAR, 20), st(GEAR, 3)];
        let mut chest = [None, None];
        let mut hand = None;
        {
            let mut this = SlotList::new(&mut inv, &limit50);
            let mut others = [SlotList::new(&mut chest, &limit50)];
            apply(&mut hand, &mut this, 2, &mut others, SlotClick::CTRL_LEFT);
        }
        assert_eq!(chest, [st(GEAR, 33), None]);
        assert_eq!(inv, [None, st(PLATE, 5), None, None]);
    }

    #[test]
    fn ctrl_right_click_moves_half_of_all() {
        let mut inv = [st(GEAR, 10), st(GEAR, 11)];
        let mut chest = [None];
        let mut hand = None;
        {
            let mut this = SlotList::new(&mut inv, &limit50);
            let mut others = [SlotList::new(&mut chest, &limit50)];
            apply(&mut hand, &mut this, 0, &mut others, SlotClick::CTRL_RIGHT);
        }
        // 21 in total, half rounded up is 11.
        assert_eq!(chest, [st(GEAR, 11)]);
        assert_eq!(inv, [None, st(GEAR, 10)]);
    }

    #[test]
    fn ctrl_click_stops_when_other_inventory_is_full() {
        let mut inv = [st(GEAR, 30), st(GEAR, 30)];
        let mut chest = [st(GEAR, 40)];
        let mut hand = None;
        {
            let mut this = SlotList::new(&mut inv, &limit50);
            let mut others = [SlotList::new(&mut chest, &limit50)];
            apply(&mut hand, &mut this, 0, &mut others, SlotClick::CTRL_LEFT);
        }
        assert_eq!(chest, [st(GEAR, 50)]);
        assert_eq!(inv, [st(GEAR, 20), st(GEAR, 30)]);
    }

    #[test]
    fn modifier_clicks_do_nothing_without_other_inventory() {
        let mut inv = [st(GEAR, 10)];
        let mut hand = None;
        let mut this = SlotList::new(&mut inv, &limit50);
        apply(&mut hand, &mut this, 0, &mut [], SlotClick::SHIFT_LEFT);
        apply(&mut hand, &mut this, 0, &mut [], SlotClick::CTRL_LEFT);
        assert_eq!(inv, [st(GEAR, 10)]);
        assert_eq!(hand, None);
    }

    #[test]
    fn tank_slots_hold_materials_in_units() {
        use foundry_core::MaterialId;
        let clay = ItemId::Material(MaterialId(5));
        let tank_limit = |_: usize, item: ItemId| if item.is_bulk() { 2000 } else { 0 };
        let mut tank = [st(clay, 1500), None];
        let mut hand = st(clay, 800);
        let mut list = SlotList::new(&mut tank, &tank_limit);
        left_click(&mut hand, &mut list, 0);
        assert_eq!(tank[0], st(clay, 2000));
        assert_eq!(hand, st(clay, 300));
        // A part does not go into a tank slot.
        let mut hand = st(GEAR, 1);
        let mut list = SlotList::new(&mut tank, &tank_limit);
        left_click(&mut hand, &mut list, 1);
        assert_eq!(tank[1], None);
    }
}
