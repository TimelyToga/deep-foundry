//! Moving items between the robot and the open building window, with the Factorio click rules.
//!
//! With a building window open:
//!
//! | Click | Robot tank | Robot part slot | Building slot |
//! |---|---|---|---|
//! | Left | Move the tank into the building. | Pick up the stack. | Part: pick up the stack. Material: move it to the robot. |
//! | Right | Move half of the tank. | Pick up half. | Part: pick up half. Material: move half to the robot. |
//! | Shift + left | Like left. | Move the stack into the building. | Move it to the robot. |
//! | Shift + right | Like right. | Move half of the stack. | Move half of it to the robot. |
//! | Ctrl + left | Move this material from every tank. | Move all of this part. | Move all of this item to the robot. |
//! | Ctrl + right | Move half of all of it. | Move half of all of it. | Move half of all of it. |
//!
//! - A machine with a fuel slot (the campfire): a tank click puts a fuel (wood, charcoal) into
//!   the fuel slot when no recipe input takes it. A click on the fuel slot gives it back.
//! - The Hub: a click on an item it holds gives it back to the robot. A part in the hand goes
//!   into the Hub, and only if a repair stage needs it (see `HubRule`).
//! - With no building window open, a click on a tank chooses the spray material (the game does
//!   that, not the factory).
//! - [`Factory::empty_tank`] deletes the material in a tank (the trash button).

use crate::Factory;
use crate::buildings::{Logic, is_deliverable};
use crate::inventory::{Click, Place, TankRule};
use foundry_content::ItemRef;
use foundry_core::BuildingId;

/// A slot of the robot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RobotSlot {
    /// A part slot.
    Part(usize),
    /// A material tank.
    Tank(usize),
}

/// The count a click moves out of `n`: all, or half (rounded up) for a right click.
fn amount(click: Click, n: u32) -> u32 {
    if click.is_right() { n.div_ceil(2) } else { n }
}

/// True for Ctrl clicks: they move the item from every slot, not only the clicked one.
fn every_slot(click: Click) -> bool {
    matches!(click, Click::Ctrl | Click::CtrlRight)
}

impl Factory {
    /// A click on a robot slot while the window of building `open` is open (or no window).
    /// Returns `Ok(true)` if items moved or the hand changed, and `Err` with a message for the
    /// player when the building takes none of the item.
    pub fn click_robot_slot(&mut self, slot: RobotSlot, click: Click, open: Option<BuildingId>) -> Result<bool, String> {
        match (slot, open) {
            (RobotSlot::Part(i), Some(id)) if click.is_move() => self.parts_to_building(i, click, id).map(|n| n > 0),
            (RobotSlot::Part(i), _) => {
                let content = self.content.clone();
                Ok(self.player.click(&content, i, click, &mut self.cursor, None))
            }
            (RobotSlot::Tank(t), Some(id)) => self.tank_to_building(t, click, id).map(|n| n > 0),
            (RobotSlot::Tank(_), None) => Ok(false),
        }
    }

    /// Move material from robot tank `tank` into building `id` (see the table at the top).
    /// Returns the units moved.
    pub fn tank_to_building(&mut self, tank: usize, click: Click, id: BuildingId) -> Result<u32, String> {
        let Some(t) = self.player.tanks.get(tank).copied() else { return Ok(0) };
        let Some(m) = t.material else { return Ok(0) };
        let item = ItemRef::Material(m);
        let have = if every_slot(click) { self.player.count(item) } else { t.units };
        let n = self.give_to_building(id, item, amount(click, have))?;
        // The clicked tank first, then the others.
        let from_tank = self.player.tanks[tank].drain(m, n);
        if from_tank < n {
            self.player.remove(item, n - from_tank);
        }
        Ok(n)
    }

    /// Move parts from robot slot `slot` into building `id` (Shift: the stack; Ctrl: all of this
    /// part; right button: half). Returns the count moved.
    pub fn parts_to_building(&mut self, slot: usize, click: Click, id: BuildingId) -> Result<u32, String> {
        let Some(Some(st)) = self.player.slots.get(slot).copied() else { return Ok(0) };
        let item = ItemRef::Part(st.part);
        let have = if every_slot(click) { self.player.count(item) } else { st.count };
        let n = self.give_to_building(id, item, amount(click, have))?;
        // The clicked slot first, then the others.
        let from_slot = n.min(st.count);
        let rest = st.count - from_slot;
        self.player.slots[slot] = (rest > 0).then_some(crate::PartStack::new(st.part, rest));
        if from_slot < n {
            self.player.remove(item, n - from_slot);
        }
        Ok(n)
    }

    /// A click on slot `index` of a storage window (a crate, a barrel, the Hub). See the table at
    /// the top. Returns `Ok(true)` if anything changed.
    pub fn click_storage_slot(&mut self, id: BuildingId, index: usize, click: Click) -> Result<bool, String> {
        let hub = self.buildings.is_hub(id);
        let Some(inv) = self.buildings.inventory(id) else { return Ok(false) };
        let Some(place) = inv.place(index) else { return Ok(false) };
        let has = inv.place_stack(index).is_some();
        if hub {
            // A part in the hand goes into the Hub (a right click: one piece). Else a click gives
            // the item back to the robot.
            if let Some(c) = self.cursor {
                let want = if click.is_right() { 1 } else { c.count };
                let n = self.give_to_building(id, ItemRef::Part(c.part), want)?;
                let left = c.count - n;
                self.cursor = (left > 0).then_some(crate::PartStack::new(c.part, left));
                return Ok(true);
            }
            return if has { self.storage_to_robot(id, index, click).map(|n| n > 0) } else { Ok(false) };
        }
        match place {
            Place::Part(i) if !click.is_move() => {
                let content = self.content.clone();
                let Some(inv) = self.buildings.inventory_mut(id) else { return Ok(false) };
                let changed = if click == Click::Right {
                    inv.right_click(&content, i, &mut self.cursor)
                } else {
                    inv.left_click(&content, i, &mut self.cursor)
                };
                if changed {
                    self.buildings.wake(id);
                }
                Ok(changed)
            }
            _ if has => self.storage_to_robot(id, index, click).map(|n| n > 0),
            _ => Ok(false),
        }
    }

    /// Move the item in slot `index` of a storage building to the robot (all, half, or with Ctrl
    /// all of that item in the building). Returns the count moved.
    pub fn storage_to_robot(&mut self, id: BuildingId, index: usize, click: Click) -> Result<u32, String> {
        let content = self.content.clone();
        let Some(inv) = self.buildings.inventory(id) else { return Ok(0) };
        let Some(s) = inv.place_stack(index) else { return Ok(0) };
        let have = if every_slot(click) { inv.count(s.item) } else { s.count };
        let want = amount(click, have);
        let n = self.player.room_for(&content, s.item, want);
        if n == 0 {
            let name = content.item_name(s.item);
            return Err(match s.item {
                ItemRef::Material(_) => format!("Tanks full: no room for {name}"),
                ItemRef::Part(_) => "The inventory is full".to_string(),
            });
        }
        let Some(inv) = self.buildings.inventory_mut(id) else { return Ok(0) };
        let from_place = inv.take_from_place(index, n);
        if from_place < n {
            inv.remove(s.item, n - from_place);
        }
        self.player.insert(&content, s.item, n);
        self.buildings.wake(id);
        Ok(n)
    }

    /// A click on the fuel slot of a machine window: the fuel goes back into the robot's tanks
    /// (all of it, or half with the right button). Returns the units moved.
    pub fn fuel_to_robot(&mut self, id: BuildingId, click: Click) -> Result<u32, String> {
        let Some(f) = self.buildings.view(&self.content, id).and_then(|v| v.fuel) else { return Ok(0) };
        let Some(m) = f.material else { return Ok(0) };
        let item = ItemRef::Material(m);
        let n = self.player.room_for(&self.content, item, amount(click, f.units));
        if n == 0 {
            return Err(format!("Tanks full: no room for {}", self.content.item_name(item)));
        }
        let Some((m, n)) = self.buildings.take_fuel(id, n) else { return Ok(0) };
        self.player.insert(&self.content.clone(), ItemRef::Material(m), n);
        Ok(n)
    }

    /// Delete the material in robot tank `tank` (the trash button). Returns the units deleted.
    pub fn empty_tank(&mut self, tank: usize) -> u32 {
        self.player.empty_tank(tank)
    }

    /// Put up to `n` of an item into building `id`. Returns the count it took, or a message for
    /// the player when it takes none.
    fn give_to_building(&mut self, id: BuildingId, item: ItemRef, n: u32) -> Result<u32, String> {
        if n == 0 {
            return Ok(0);
        }
        let content = self.content.clone();
        let room = self.buildings.room_for(&content, id, item);
        let taken = self.buildings.insert(&content, id, item, n.min(room));
        if taken == 0 {
            return Err(self.refusal(id, item));
        }
        Ok(taken)
    }

    /// Why building `id` does not take an item: a message for the player.
    pub fn refusal(&self, id: BuildingId, item: ItemRef) -> String {
        let content = &self.content;
        let Some(b) = self.buildings.get(id) else { return String::new() };
        let name = &content.factory.building_def(b.kind).name;
        let what = content.item_name(item);
        match (&b.logic, item) {
            (Logic::Storage(inv), ItemRef::Material(m)) if !inv.takes_material(content, m) => match inv.takes {
                TankRule::Liquid => format!("{name} takes only liquids"),
                _ => format!("{name} does not take {what}: put liquids in a barrel"),
            },
            (Logic::Storage(inv), ItemRef::Part(_)) if inv.slots.is_empty() => format!("{name} takes only liquids"),
            (Logic::Storage(_), _) => format!("{name} is full"),
            (Logic::Hub(_), _) if !is_deliverable(content, item, self.progress.stage()) => {
                format!("The Hub does not take {what}: no repair stage needs it")
            }
            (Logic::Hub(inv), _) if inv.room_for(content, item, 1) == 0 => "The Hub is full".to_string(),
            (Logic::Hub(_), _) => format!("The Hub has all the {what} that the repair stages need"),
            _ => format!("{name} does not take {what}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PartStack;
    use crate::buildings::HubRule;
    use crate::inventory::{PLAYER_TANK_UNITS, Place};
    use foundry_content::{Content, Stack};
    use foundry_core::TilePos;
    use foundry_sim::{SimConfig, Simulation};
    use std::sync::Arc;

    fn setup() -> (Factory, Simulation) {
        let content = Arc::new(Content::load_default().expect("content loads"));
        let sim = Simulation::new(content.clone(), SimConfig::finite(4, 4, 1));
        (Factory::new(content), sim)
    }

    fn place(f: &mut Factory, sim: &mut Simulation, id: &str, at: TilePos) -> BuildingId {
        let kind = f.content.factory.building(id).unwrap();
        f.place(kind, at, 0, false, sim).unwrap()
    }

    fn mat(f: &Factory, id: &str) -> ItemRef {
        ItemRef::Material(f.content.expect_material(id))
    }

    fn part(f: &Factory, id: &str) -> ItemRef {
        ItemRef::Part(f.content.factory.part(id).unwrap())
    }

    fn stored(f: &Factory, id: BuildingId, item: ItemRef) -> u32 {
        f.buildings.inventory(id).unwrap().count(item)
    }

    #[test]
    fn tank_clicks_move_material_into_a_crate() {
        let (mut f, mut sim) = setup();
        let c = f.content.clone();
        let crate_ = place(&mut f, &mut sim, "crate", TilePos::new(4, 4));
        let (clay, sand) = (mat(&f, "clay"), mat(&f, "sand"));
        f.player.insert(&c, clay, 9000); // tank 0 full, 3000 in tank 1
        f.player.insert(&c, sand, 800); // tank 2
        // Left: the whole tank. Right: half of a tank.
        assert_eq!(f.click_robot_slot(RobotSlot::Tank(0), Click::Left, Some(crate_)), Ok(true));
        assert_eq!((stored(&f, crate_, clay), f.player.count(clay)), (PLAYER_TANK_UNITS, 3000));
        assert_eq!(f.tank_to_building(2, Click::Right, crate_), Ok(400));
        assert_eq!(f.player.count(sand), 400);
        // Ctrl: this material from every tank.
        f.player.insert(&c, clay, 2000);
        assert_eq!(f.tank_to_building(1, Click::Ctrl, crate_), Ok(5000));
        assert_eq!(f.player.count(clay), 0);
        // One crate slot holds up to 6,000 units: clay uses two slots, sand one.
        let inv = f.buildings.inventory(crate_).unwrap();
        assert_eq!(inv.place(0), Some(Place::Tank(0)));
        assert_eq!(inv.place_stack(1), Some(Stack { item: sand, count: 400 }));
        assert_eq!(inv.place_stack(2), Some(Stack { item: clay, count: 5000 }));
        // Without a window, a tank click moves nothing (the game chooses the spray material).
        assert_eq!(f.click_robot_slot(RobotSlot::Tank(2), Click::Left, None), Ok(false));
    }

    #[test]
    fn a_crate_does_not_take_liquids_and_says_why() {
        let (mut f, mut sim) = setup();
        let c = f.content.clone();
        let crate_ = place(&mut f, &mut sim, "crate", TilePos::new(4, 4));
        let barrel = place(&mut f, &mut sim, "barrel", TilePos::new(8, 4));
        f.player.insert(&c, mat(&f, "water"), 700);
        let err = f.click_robot_slot(RobotSlot::Tank(0), Click::Left, Some(crate_)).unwrap_err();
        assert!(err.contains("put liquids in a barrel"), "{err}");
        // The barrel takes water (500 units), but not sand.
        assert_eq!(f.tank_to_building(0, Click::Left, barrel), Ok(500));
        f.player.insert(&c, mat(&f, "sand"), 10);
        let err = f.tank_to_building(1, Click::Left, barrel).unwrap_err();
        assert!(err.contains("only liquids"), "{err}");
    }

    #[test]
    fn building_slot_clicks_move_items_back_to_the_robot() {
        let (mut f, mut sim) = setup();
        let c = f.content.clone();
        let crate_ = place(&mut f, &mut sim, "crate", TilePos::new(4, 4));
        let (clay, gear) = (mat(&f, "clay"), part(&f, "bronze_gear"));
        f.buildings.insert(&c, crate_, clay, 4000);
        f.buildings.insert(&c, crate_, gear, 30);
        // Slot 0 holds clay, slot 1 the gears. A right click on material moves half.
        assert_eq!(f.click_storage_slot(crate_, 0, Click::Right), Ok(true));
        assert_eq!((f.player.count(clay), stored(&f, crate_, clay)), (2000, 2000));
        // Shift + click moves it back to the robot.
        assert_eq!(f.click_storage_slot(crate_, 0, Click::Shift), Ok(true));
        assert_eq!(f.player.count(clay), 4000);
        // A plain click on a part picks it up, as in Factorio; shift + click moves it.
        assert_eq!(f.click_storage_slot(crate_, 1, Click::Left), Ok(true));
        let ItemRef::Part(gp) = gear else { unreachable!() };
        assert_eq!(f.cursor, Some(PartStack::new(gp, 30)));
        assert_eq!(f.click_storage_slot(crate_, 1, Click::Left), Ok(true));
        assert_eq!(f.click_storage_slot(crate_, 1, Click::ShiftRight), Ok(true));
        assert_eq!((f.player.count(gear), stored(&f, crate_, gear)), (15, 15));
        // Part slots of the robot: shift moves the stack, ctrl all of the part.
        let slot = f.player.slots.iter().position(|s| s.is_some_and(|s| s.part == gp)).unwrap();
        assert_eq!(f.click_robot_slot(RobotSlot::Part(slot), Click::Ctrl, Some(crate_)), Ok(true));
        assert_eq!(stored(&f, crate_, gear), 30);
    }

    #[test]
    fn the_robot_tanks_full_stops_the_move_back() {
        let (mut f, mut sim) = setup();
        let c = f.content.clone();
        let crate_ = place(&mut f, &mut sim, "crate", TilePos::new(4, 4));
        let names = ["clay", "sand", "dirt", "gravel", "wood", "ash", "raw_malachite", "raw_cassiterite"];
        for n in names {
            f.player.insert(&c, mat(&f, n), PLAYER_TANK_UNITS);
        }
        f.buildings.insert(&c, crate_, mat(&f, "charcoal"), 100);
        let err = f.click_storage_slot(crate_, 0, Click::Left).unwrap_err();
        assert_eq!(err, "Tanks full: no room for Charcoal");
        assert_eq!(f.empty_tank(3), PLAYER_TANK_UNITS);
        assert_eq!(f.click_storage_slot(crate_, 0, Click::Left), Ok(true));
    }

    #[test]
    fn an_old_save_gets_the_new_tanks_and_crates() {
        let (mut f, mut sim) = setup();
        let c = f.content.clone();
        let crate_ = place(&mut f, &mut sim, "crate", TilePos::new(4, 4));
        let (clay, gear) = (mat(&f, "clay"), part(&f, "bronze_gear"));
        // The shapes of the version before: 4 robot tanks of 2,000 units, crates for parts only.
        f.player = crate::Inventory::new(40, 4, 2000);
        f.player.insert(&c, clay, 1500);
        let Some(Logic::Storage(inv)) = f.buildings.get_mut(crate_).map(|b| &mut b.logic) else { panic!() };
        *inv = crate::Inventory::new(8, 0, 1000);
        inv.slots[3] = Some(PartStack::new(f.content.factory.part("bronze_gear").unwrap(), 7));
        let save = f.save();
        f.load(save);
        assert_eq!(f.player.tanks.len(), crate::inventory::PLAYER_TANKS);
        assert!(f.player.tanks.iter().all(|t| t.capacity == PLAYER_TANK_UNITS));
        assert_eq!(f.player.count(clay), 1500);
        let inv = f.buildings.inventory(crate_).unwrap();
        assert!(inv.mixed);
        assert_eq!(inv.place_stack(3), Some(Stack { item: gear, count: 7 }), "the part keeps its slot");
        assert_eq!(f.tank_to_building(0, Click::Left, crate_), Ok(1500));
    }

    #[test]
    fn items_in_crates_count_for_the_guide() {
        use crate::progress::GuideState;
        let (mut f, mut sim) = setup();
        let c = f.content.clone();
        let crate_ = place(&mut f, &mut sim, "crate", TilePos::new(4, 4));
        let sand = mat(&f, "sand");
        f.player.insert(&c, sand, 60);
        f.buildings.insert(&c, crate_, sand, 50);
        assert_eq!(f.item_count(sand), 110);
    }

    #[test]
    fn the_first_dig_discovers_the_material() {
        let (mut f, _) = setup();
        let malachite = f.content.expect_material("malachite");
        let raw = f.content.expect_material("raw_malachite");
        assert_eq!(f.take_dug_cell(malachite), crate::Dug::Kept);
        assert_eq!(f.player.count(ItemRef::Material(raw)), 1, "the vein breaks into raw ore");
        assert!(f.progress.is_material_discovered(malachite) && f.progress.is_material_discovered(raw));
        let events: Vec<_> = f.progress.drain_events().collect();
        assert_eq!(events.len(), 2, "one discovery each: {events:?}");
        assert_eq!(f.take_dug_cell(malachite), crate::Dug::Kept);
        assert_eq!(f.progress.drain_events().count(), 0, "a second dig discovers nothing");
        // No room: nothing is taken.
        for t in &mut f.player.tanks {
            t.material = Some(f.content.expect_material("sand"));
            t.units = t.capacity;
        }
        assert_eq!(f.take_dug_cell(malachite), crate::Dug::NoRoom(raw));
    }

    #[test]
    fn the_campfire_fires_raw_clay_bricks_with_wood_fuel() {
        let (mut f, mut sim) = setup();
        let c = f.content.clone();
        let fire = place(&mut f, &mut sim, "campfire", TilePos::new(4, 4));
        // A machine with a fuel slot and one recipe starts with that recipe.
        assert_eq!(f.building_view(fire).unwrap().recipe, c.factory.recipe("pit_fired_clay_brick"));
        let (raw, brick, wood) = (part(&f, "raw_clay_brick"), part(&f, "clay_brick"), mat(&f, "wood"));
        f.player.insert(&c, raw, 3);
        let slot = f.player.slots.iter().position(|s| s.is_some()).unwrap();
        assert_eq!(f.parts_to_building(slot, Click::Shift, fire), Ok(3));
        for _ in 0..20 {
            f.tick(&mut sim);
        }
        let v = f.building_view(fire).unwrap();
        assert_eq!((v.status, v.reason.as_str()), (crate::Status::NoFuel, "No fuel: put wood in the fuel slot"));
        // A tank click puts the wood into the fuel slot (wood is not a recipe input).
        f.player.insert(&c, wood, 50);
        assert_eq!(f.tank_to_building(0, Click::Left, fire), Ok(50));
        // 3 bricks of 30 seconds; one unit of wood burns for 5 seconds.
        for _ in 0..3 * 30 * 60 + 5 {
            f.tick(&mut sim);
        }
        let v = f.building_view(fire).unwrap();
        assert_eq!(v.outputs[0].count, 3, "{v:?}");
        let fuel = v.fuel.unwrap();
        // The fire keeps burning while idle after the third brick, so it has started a
        // nineteenth unit by the time the output is ready.
        assert_eq!((fuel.material, fuel.units), (Some(c.expect_material("wood")), 31));
        // The output goes to the robot; a click on the fuel slot gives the wood back.
        assert_eq!(f.take_outputs_to_player(fire), vec![Stack { item: brick, count: 3 }]);
        assert_eq!(f.fuel_to_robot(fire, Click::Left), Ok(31));
        assert_eq!(f.player.count(wood), 31);
    }

    #[test]
    fn the_hub_takes_only_what_a_stage_needs_and_gives_it_back() {
        let (mut f, mut sim) = setup();
        let c = f.content.clone();
        let hub = place(&mut f, &mut sim, "hub", TilePos::new(4, 4));
        f.buildings.set_hub_rule(HubRule { stage: 0, need: Some(Arc::new(f.progress.hub_need(&c))) });
        let (plate, wire) = (part(&f, "bronze_plate"), part(&f, "copper_wire"));
        f.player.insert(&c, mat(&f, "wood"), 50);
        let err = f.tank_to_building(0, Click::Left, hub).unwrap_err();
        assert!(err.contains("no repair stage needs it"), "{err}");
        // Stage 1 needs 24 bronze plates: the Hub takes 24 of 100.
        f.player.insert(&c, plate, 100);
        let slot = f.player.slots.iter().position(|s| s.is_some()).unwrap();
        assert_eq!(f.parts_to_building(slot, Click::Shift, hub), Ok(24));
        assert_eq!(f.player.count(plate), 76);
        let err = f.parts_to_building(slot, Click::Shift, hub).unwrap_err();
        assert!(err.contains("has all the Bronze plate"), "{err}");
        // A later stage item (copper wire for stage 2) waits in the Hub; a click gives it back.
        f.player.insert(&c, wire, 30);
        let wslot = f.player.slots.iter().position(|s| s.is_some_and(|s| ItemRef::Part(s.part) == wire)).unwrap();
        assert_eq!(f.parts_to_building(wslot, Click::Shift, hub), Ok(30));
        let index = (0..16).find(|&i| f.buildings.inventory(hub).unwrap().place_stack(i).is_some_and(|s| s.item == wire)).unwrap();
        assert_eq!(f.click_storage_slot(hub, index, Click::Left), Ok(true));
        assert_eq!((f.player.count(wire), stored(&f, hub, wire)), (30, 0));
        // A part in the hand goes in only if a stage needs it.
        let ItemRef::Part(wp) = wire else { unreachable!() };
        f.player.remove(wire, 5);
        f.cursor = Some(PartStack::new(wp, 5));
        assert_eq!(f.click_storage_slot(hub, 15, Click::Right), Ok(true));
        assert_eq!(f.cursor, Some(PartStack::new(wp, 4)));
        f.cursor = Some(PartStack::new(f.content.factory.part("crate").unwrap(), 1));
        assert!(f.click_storage_slot(hub, 15, Click::Left).is_err());
    }
}
