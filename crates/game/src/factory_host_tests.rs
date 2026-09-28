//! Tests of the factory commands: commands go in, the factory state changes. No window and no
//! thread: the tests call `apply`, `tick` and `frame` like the simulation thread does.

use super::*;
use crate::demo::{self, Shape};
use foundry_content::Stack;
use foundry_factory::Click;

struct Game {
    host: FactoryHost,
    sim: Simulation,
    content: Arc<Content>,
}

impl Game {
    fn new() -> Self {
        let content = Arc::new(Content::load_default().unwrap());
        let guide = Arc::new(Guide::load_default().unwrap());
        let demo = demo::build(content.clone(), Shape::Box { width_chunks: 32, height_chunks: 16 }, 3);
        let mut sim = demo.sim;
        let host = FactoryHost::new_game(content.clone(), guide, &mut sim, demo.start_center.0 as i32).unwrap();
        let mut g = Self { host, sim, content };
        g.ticks(30); // The robot lands.
        g
    }

    fn apply(&mut self, cmd: FactoryCommand) {
        self.host.apply(cmd, &mut self.sim);
    }

    fn ticks(&mut self, n: u32) {
        for _ in 0..n {
            self.sim.tick();
            self.host.tick(&mut self.sim);
        }
    }

    fn frame(&mut self) -> FactoryFrame {
        let t = self.sim.tick_count();
        self.host.frame(&self.sim, t)
    }

    fn part(&self, id: &str) -> PartId {
        self.content.factory.part(id).unwrap()
    }

    fn count(&self, item: ItemRef) -> u32 {
        self.host.factory.player.count(item)
    }

    fn give(&mut self, item: ItemRef, n: u32) {
        let c = self.content.clone();
        self.host.factory.player.insert(&c, item, n);
    }

    fn hub(&self) -> (BuildingId, CellRect) {
        let (id, b) = self
            .host
            .factory
            .buildings
            .iter()
            .find(|(_, b)| self.content.factory.building_def(b.kind).kind == "hub")
            .expect("the Hub is placed");
        (id, b.cell_rect())
    }

    /// Take the building at a cell into the inventory at once (an action of its own).
    fn remove_at(&mut self, p: CellPos) {
        let action = self.host.action_id(0);
        if let Some(id) = self.host.removable_at(p) {
            self.host.remove_building(id, action, &mut self.sim);
        }
    }

    /// A free place for a building right of the robot.
    fn free_place(&self, kind: BuildingKindId) -> TilePos {
        self.host.free_place(kind, &self.sim).expect("a free place")
    }
}

#[test]
fn new_game_has_the_broken_hub_the_robot_and_the_start_inventory() {
    let mut g = Game::new();
    let (_, hub) = g.hub();
    assert_eq!(g.host.factory.progress.stage(), 0, "the Hub is not repaired yet");
    let next = g.host.factory.progress.milestone_view(&g.content).unwrap();
    assert_eq!(next.stage, 1);
    // The robot stands on the ground next to the Hub.
    let r = g.host.robot.rect();
    assert!(g.host.robot.on_ground);
    assert!((r.x0 - hub.x1).abs() < 40, "robot {r:?} hub {hub:?}");
    assert_eq!(g.count(ItemRef::Material(g.content.expect_material("wood"))), 30);
    assert_eq!(g.count(ItemRef::Part(g.part("crate"))), 1);
    let f = g.frame();
    assert!(f.robot.is_some());
    assert_eq!(f.hotbar[0], Some(ItemRef::Part(g.part("crate"))));
    assert!(f.guide.is_some(), "the first frame has the guide");
}

#[test]
fn walking_input_moves_the_robot() {
    let mut g = Game::new();
    let x = g.host.robot.left;
    let input = PlayerInput { movement: MoveInput { x: 1, jump: false }, ..Default::default() };
    g.apply(FactoryCommand::Input(input));
    g.ticks(30);
    assert!(g.host.robot.left > x + 10, "{} -> {}", x, g.host.robot.left);
}

/// A jump press and its release arrive before one tick: the robot still jumps.
#[test]
fn a_jump_press_and_release_before_one_tick_still_jump() {
    let mut g = Game::new();
    let top = g.host.robot.top;
    let jump = PlayerInput { movement: MoveInput { x: 0, jump: true }, ..Default::default() };
    g.apply(FactoryCommand::Input(jump));
    g.apply(FactoryCommand::Input(PlayerInput::default()));
    let mut highest = top;
    for _ in 0..20 {
        g.ticks(1);
        highest = highest.min(g.host.robot.top);
    }
    assert!(highest < top, "the robot jumps: top {top}, highest {highest}");
}

/// A dig click that goes down and up before one tick digs for one tick.
#[test]
fn a_short_dig_click_digs_once() {
    let mut g = Game::new();
    let clay = g.content.expect_material("clay");
    let r = g.host.robot.rect();
    let aim = CellPos::new(r.x1 + 6, r.y1 + 3);
    for y in aim.y - 4..aim.y + 4 {
        for x in aim.x - 4..aim.x + 4 {
            g.sim.set_cell(CellPos::new(x, y), clay, None);
        }
    }
    g.apply(FactoryCommand::Input(PlayerInput { aim, dig: true, ..Default::default() }));
    g.apply(FactoryCommand::Input(PlayerInput { aim, ..Default::default() }));
    g.ticks(1);
    let once = g.count(ItemRef::Material(clay));
    assert!(once > 0, "the click digs");
    g.ticks(5);
    assert_eq!(g.count(ItemRef::Material(clay)), once, "only for one tick");
}

#[test]
fn dig_fills_the_tank_and_scan_discovers() {
    let mut g = Game::new();
    let clay = g.content.expect_material("clay");
    // Put clay under the robot's feet and dig it.
    let r = g.host.robot.rect();
    let aim = CellPos::new(r.x1 + 6, r.y1 + 3);
    for y in aim.y - 4..aim.y + 4 {
        for x in aim.x - 4..aim.x + 4 {
            g.sim.set_cell(CellPos::new(x, y), clay, None);
        }
    }
    g.apply(FactoryCommand::Input(PlayerInput { aim, scan: true, ..Default::default() }));
    g.ticks(1);
    assert!(g.host.factory.progress.is_material_discovered(clay));
    let notices = g.frame().notices;
    assert!(notices.iter().any(|n| n.starts_with("Discovered: Clay")), "{notices:?}");
    g.apply(FactoryCommand::Input(PlayerInput { aim, dig: true, ..Default::default() }));
    g.ticks(10);
    assert!(g.count(ItemRef::Material(clay)) >= 40, "dug {}", g.count(ItemRef::Material(clay)));
    assert!(g.frame().digging);
}

#[test]
fn hand_craft_place_open_and_take_back_a_workbench() {
    let mut g = Game::new();
    let workbench = g.part("workbench");
    let recipe = g.content.factory.recipe("workbench").unwrap();
    g.apply(FactoryCommand::Craft { recipe, count: 1 });
    assert_eq!(g.frame().crafting.len(), 1);
    g.ticks(4 * 60);
    assert_eq!(g.count(ItemRef::Part(workbench)), 1, "crafted");
    assert!(g.host.hotbar.contains(&Some(ItemRef::Part(workbench))), "a new building goes on the quickbar");
    assert_eq!(g.count(ItemRef::Material(g.content.expect_material("wood"))), 10);

    // A quickbar key takes it into the hand; the ghost is green at a free place.
    g.apply(FactoryCommand::PickToCursor(workbench));
    assert_eq!(g.host.factory.cursor.map(|c| c.part), Some(workbench));
    let kind = g.content.factory.building("workbench").unwrap();
    let at = g.free_place(kind);
    let ghost = GhostRequest { kind, at, rotation: 0, flip: false };
    g.apply(FactoryCommand::Input(PlayerInput { ghost: Some(ghost), ..Default::default() }));
    let f = g.frame();
    assert_eq!(f.ghost.as_ref().unwrap().error, None);
    // A ghost in the ground is red with the reason.
    let low = TilePos::new(at.x, at.y + 4);
    g.apply(FactoryCommand::Input(PlayerInput { ghost: Some(GhostRequest { at: low, ..ghost }), ..Default::default() }));
    let err = g.frame().ghost.unwrap().error.unwrap();
    assert!(err.contains("dig first") || err.contains("Blocked"), "{err}");

    g.apply(FactoryCommand::Place(Placement::new(kind, at, 0)));
    assert_eq!(g.host.factory.buildings.count_of(kind), 1);
    assert_eq!(g.host.factory.cursor, None);
    g.apply(FactoryCommand::OpenAt(at.origin()));
    let f = g.frame();
    assert_eq!(f.building.as_ref().map(|b| b.kind), Some(kind));
    g.ticks(2);
    assert!(g.frame().craft_speed > 1.0, "the workbench makes hand crafting faster");

    g.remove_at(at.origin().offset(3, 3));
    assert_eq!(g.host.factory.buildings.count_of(kind), 0);
    assert_eq!(g.count(ItemRef::Part(workbench)), 1, "back in the inventory");
    assert!(g.frame().building.is_none(), "the window closed");
}

#[test]
fn placing_needs_the_building_in_the_hand_and_reach() {
    let mut g = Game::new();
    let kind = g.content.factory.building("crate").unwrap();
    let at = g.free_place(kind);
    g.apply(FactoryCommand::Place(Placement::new(kind, at, 0)));
    assert_eq!(g.host.factory.buildings.count_of(kind), 0);
    assert!(g.frame().notices.iter().any(|n| n.contains("in the hand")));
    g.apply(FactoryCommand::PickToCursor(g.part("crate")));
    let far = TilePos::new(at.x, at.y - 20);
    assert_eq!(g.host.check_place(kind, far, 0, false, &g.sim), Err("Out of reach".to_string()));
}

#[test]
fn the_hub_takes_deliveries_and_cannot_be_removed() {
    let mut g = Game::new();
    let (hub, rect) = g.hub();
    let brick = ItemRef::Part(g.part("clay_brick"));
    g.give(brick, 30);
    g.apply(FactoryCommand::OpenAt(CellPos::new(rect.x0 + 2, rect.y0 + 2)));
    let f = g.frame();
    assert!(f.milestone.is_some(), "the Hub window shows the repair stage");
    // Shift + click moves the stack into the Hub. The Hub delivers it in its tick.
    let slot = g.host.factory.player.slots.iter().position(|s| s.is_some_and(|s| ItemRef::Part(s.part) == brick)).unwrap();
    g.apply(FactoryCommand::Click { target: SlotTarget::Inventory(slot), click: Click::Shift });
    assert_eq!(g.count(brick), 0);
    g.ticks(5);
    let m = g.host.factory.progress.milestone_view(&g.content).unwrap();
    let d = m.items.iter().find(|d| d.item == brick).unwrap();
    assert_eq!(d.delivered, 30);
    g.remove_at(CellPos::new(rect.x0 + 2, rect.y0 + 2));
    assert!(g.host.factory.buildings.get(hub).is_some());
    assert!(g.frame().notices.iter().any(|n| n.contains("cannot be removed")));
}

#[test]
fn storage_slot_clicks_move_items() {
    let mut g = Game::new();
    let crate_part = g.part("crate");
    let kind = g.content.factory.building("crate").unwrap();
    g.apply(FactoryCommand::PickToCursor(crate_part));
    let at = g.free_place(kind);
    g.apply(FactoryCommand::Place(Placement::new(kind, at, 0)));
    g.apply(FactoryCommand::OpenAt(at.origin()));
    let id = g.host.building_at(at.origin()).unwrap();
    let brick = ItemRef::Part(g.part("raw_clay_brick"));
    g.give(brick, 7);
    let slot = g.host.factory.player.slots.iter().position(|s| s.is_some()).unwrap();
    g.apply(FactoryCommand::Click { target: SlotTarget::Inventory(slot), click: Click::Shift });
    assert_eq!(g.host.factory.buildings.inventory(id).unwrap().count(brick), 7);
    // Left click on the crate slot picks the stack up into the hand.
    g.apply(FactoryCommand::Click { target: SlotTarget::Building { id, group: SlotGroup::Input, index: 0 }, click: Click::Left });
    assert_eq!(g.host.factory.cursor.map(|c| c.to_stack()), Some(Stack { item: brick, count: 7 }));
    g.apply(FactoryCommand::ClearCursor);
    assert_eq!(g.count(brick), 7);
}

/// Place the start crate right of the robot and open its window. Returns its id.
fn open_crate(g: &mut Game) -> BuildingId {
    let kind = g.content.factory.building("crate").unwrap();
    g.apply(FactoryCommand::PickToCursor(g.part("crate")));
    let at = g.free_place(kind);
    g.apply(FactoryCommand::Place(Placement::new(kind, at, 0)));
    g.apply(FactoryCommand::OpenAt(at.origin()));
    g.host.building_at(at.origin()).unwrap()
}

#[test]
fn full_tanks_stop_digging_then_a_crate_takes_the_material() {
    let mut g = Game::new();
    let clay = g.content.expect_material("clay");
    // Fill every tank with other materials.
    let others = ["sand", "dirt", "gravel", "wood", "ash", "raw_malachite", "raw_cassiterite", "charcoal"];
    let units = foundry_factory::inventory::PLAYER_TANK_UNITS;
    g.host.factory.player.empty_tank(0);
    for id in others {
        g.give(ItemRef::Material(g.content.expect_material(id)), units);
    }
    let r = g.host.robot.rect();
    let aim = CellPos::new(r.x1 + 6, r.y1 + 3);
    // Clay under the whole dig circle (the robot throws out dirt, so dirt would still dig).
    let d = crate::tools::DIG_RADIUS + 1;
    for y in aim.y - d..aim.y + d {
        for x in aim.x - d..aim.x + d {
            g.sim.set_cell(CellPos::new(x, y), clay, None);
        }
    }
    g.apply(FactoryCommand::Input(PlayerInput { aim, dig: true, ..Default::default() }));
    g.ticks(5);
    assert_eq!(g.count(ItemRef::Material(clay)), 0, "no room: the clay stays in the ground");
    assert_eq!(g.sim.cell(aim).material, clay);
    let f = g.frame();
    assert!(f.tanks_full && !f.digging);
    assert!(f.notices.iter().any(|n| n == TANKS_FULL), "{:?}", f.notices);
    // Stop digging, put the sand tank into a crate (a click on the tank), then dig again.
    g.apply(FactoryCommand::Input(PlayerInput::default()));
    let id = open_crate(&mut g);
    let sand = ItemRef::Material(g.content.expect_material("sand"));
    let tank = g.host.factory.player.tanks.iter().position(|t| t.material.map(ItemRef::Material) == Some(sand)).unwrap();
    g.apply(FactoryCommand::Click { target: SlotTarget::Tank(tank), click: Click::Left });
    assert_eq!(g.host.factory.buildings.inventory(id).unwrap().count(sand), units);
    assert_eq!(g.count(sand), 0);
    g.apply(FactoryCommand::Input(PlayerInput { aim, dig: true, ..Default::default() }));
    g.ticks(5);
    assert!(g.count(ItemRef::Material(clay)) > 0);
    // After a while without a full tank, the HUD warning goes away.
    g.apply(FactoryCommand::Input(PlayerInput::default()));
    g.ticks(TANKS_FULL_TICKS as u32 + 1);
    assert!(!g.frame().tanks_full);
}

#[test]
fn a_crate_refuses_water_and_a_tank_can_be_emptied() {
    let mut g = Game::new();
    let water = ItemRef::Material(g.content.expect_material("water"));
    g.give(water, 300);
    let id = open_crate(&mut g);
    let tank = g.host.factory.player.tanks.iter().position(|t| t.material.map(ItemRef::Material) == Some(water)).unwrap();
    g.apply(FactoryCommand::Click { target: SlotTarget::Tank(tank), click: Click::Left });
    assert_eq!(g.host.factory.buildings.inventory(id).unwrap().count(water), 0);
    assert!(g.frame().notices.iter().any(|n| n.contains("put liquids in a barrel")));
    g.apply(FactoryCommand::EmptyTank(tank));
    assert_eq!(g.count(water), 0);
    assert!(g.frame().notices.iter().any(|n| n.starts_with("Emptied a tank: 300 units of Water")));
}

#[test]
fn the_hub_window_shows_later_stages_and_gives_items_back() {
    let mut g = Game::new();
    let (hub, rect) = g.hub();
    let wire = ItemRef::Part(g.part("copper_wire"));
    g.give(wire, 50);
    g.apply(FactoryCommand::OpenAt(CellPos::new(rect.x0 + 2, rect.y0 + 2)));
    let f = g.frame();
    assert!(f.later_milestones.iter().any(|m| m.stage == 2 && m.items.iter().any(|d| d.item == wire)));
    // Copper wire is for stage 2: the Hub holds it, and a click gives it back.
    let slot = g.host.factory.player.slots.iter().position(|s| s.is_some_and(|s| ItemRef::Part(s.part) == wire)).unwrap();
    g.apply(FactoryCommand::Click { target: SlotTarget::Inventory(slot), click: Click::Shift });
    g.ticks(5);
    assert_eq!(g.host.factory.buildings.inventory(hub).unwrap().count(wire), 50, "stage 1 does not use it");
    let places = g.frame().building.unwrap().inventory.unwrap().places();
    let index = places.iter().position(|p| p.item == Some(wire)).unwrap();
    g.apply(FactoryCommand::Click { target: SlotTarget::Building { id: hub, group: SlotGroup::Input, index }, click: Click::Left });
    assert_eq!(g.count(wire), 50);
    // The Hub does not take what no stage needs.
    let crate_part = ItemRef::Part(g.part("crate"));
    let slot = g.host.factory.player.slots.iter().position(|s| s.is_some_and(|s| ItemRef::Part(s.part) == crate_part)).unwrap();
    g.apply(FactoryCommand::Click { target: SlotTarget::Inventory(slot), click: Click::Shift });
    assert_eq!(g.count(crate_part), 1);
    assert!(g.frame().notices.iter().any(|n| n.contains("no repair stage needs it")));
}

#[test]
fn research_needs_discoveries_then_starts() {
    let mut g = Game::new();
    let bronze = g.content.factory.tech("bronze").unwrap();
    g.apply(FactoryCommand::StartResearch(bronze));
    assert!(!g.host.factory.progress.is_researched(bronze));
    assert!(g.frame().notices.iter().any(|n| n.contains("Needs discovery")));
    for id in ["malachite", "cassiterite"] {
        g.host.factory.scan(g.content.expect_material(id));
    }
    g.apply(FactoryCommand::StartResearch(bronze));
    g.ticks(1);
    // Bronze needs no kits: it is done at once.
    assert!(g.host.factory.progress.is_researched(bronze));
    g.apply(FactoryCommand::Windows { research: true, guide: false });
    let f = g.host.frame(&g.sim, 0);
    assert!(f.techs.is_some_and(|t| !t.is_empty()));
    assert!(f.finished_techs.contains(&bronze));
}

#[test]
fn research_queues_the_technologies_it_needs_first() {
    let mut g = Game::new();
    let (bronze, research) = (g.content.factory.tech("bronze").unwrap(), g.content.factory.tech("research").unwrap());
    // Research needs Bronze, and Bronze needs discoveries: both wait in the queue, in order.
    g.apply(FactoryCommand::StartResearch(research));
    assert_eq!(g.host.factory.progress.queue(), &[bronze, research]);
    // The research window offers the Queue button for such a technology.
    let views = g.host.factory.progress.tech_views(&g.content);
    let entry = crate::normal::tech_entry(&views[research.0 as usize]);
    assert!(!entry.can_queue, "it is queued already");
    let steam = g.content.factory.tech("steam_power").unwrap();
    let entry = crate::normal::tech_entry(&views[steam.0 as usize]);
    assert!(!entry.can_queue, "Tier 1 is not open yet");
    // When the discoveries are made, Bronze runs, then Research follows from the queue.
    for id in ["malachite", "cassiterite"] {
        g.host.factory.scan(g.content.expect_material(id));
    }
    g.ticks(2);
    let p = &g.host.factory.progress;
    assert!(p.is_researched(bronze));
    assert!(p.is_researched(research) || p.current() == Some(research), "{:?} {:?}", p.current(), p.queue());
}

#[test]
fn cancel_craft_gives_the_ingredients_back() {
    let mut g = Game::new();
    let wood = ItemRef::Material(g.content.expect_material("wood"));
    let recipe = g.content.factory.recipe("campfire").unwrap();
    g.apply(FactoryCommand::Craft { recipe, count: 2 });
    assert_eq!(g.count(wood), 10);
    g.apply(FactoryCommand::CancelCraft { index: 0 });
    assert_eq!(g.count(wood), 30);
    assert!(g.frame().crafting.is_empty());
}

#[test]
fn save_text_round_trip() {
    let mut g = Game::new();
    let kind = g.content.factory.building("crate").unwrap();
    g.apply(FactoryCommand::PickToCursor(g.part("crate")));
    let at = g.free_place(kind);
    g.apply(FactoryCommand::Place(Placement::new(kind, at, 0)));
    g.apply(FactoryCommand::SetHotbar { index: 3, item: Some(ItemRef::Material(g.content.expect_material("wood"))) });
    let text = g.host.save_text().unwrap();
    let loaded = FactoryHost::from_save_text(g.content.clone(), g.host.factory.guide.clone(), &text).unwrap();
    assert_eq!(loaded.robot, g.host.robot);
    assert_eq!(loaded.hotbar, g.host.hotbar);
    assert_eq!(loaded.factory.player, g.host.factory.player);
    assert_eq!(loaded.factory.buildings.count_of(kind), 1);
    assert_eq!(loaded.factory.buildings.len(), g.host.factory.buildings.len());
    assert!(FactoryHost::from_save_text(g.content.clone(), g.host.factory.guide.clone(), "not a save").is_err());
}

#[test]
fn mailbox_keeps_notices_and_lists() {
    let mb = FactoryMailbox::default();
    mb.publish(FactoryFrame { tick: 1, notices: vec!["a".into()], guide: Some(vec![]), ..Default::default() });
    mb.publish(FactoryFrame { tick: 2, notices: vec!["b".into()], ..Default::default() });
    let f = mb.take().unwrap();
    assert_eq!(f.tick, 2);
    assert_eq!(f.notices, vec!["a".to_string(), "b".to_string()]);
    assert!(f.guide.is_some());
    assert!(mb.take().is_none());
}

#[test]
fn items_can_be_taken_back_out_of_machine_input_slots() {
    let mut g = Game::new();
    let drawer = g.content.factory.building("steam_wire_drawer").unwrap();
    g.give(ItemRef::Part(g.part("steam_wire_drawer")), 1);
    g.apply(FactoryCommand::PickToCursor(g.part("steam_wire_drawer")));
    let at = g.free_place(drawer);
    g.apply(FactoryCommand::Place(Placement::new(drawer, at, 0)));
    let id = g.host.building_at(at.origin()).unwrap();
    let c = g.content.clone();
    g.host.factory.buildings.set_recipe(&c, id, c.factory.recipe("copper_wire")).unwrap();
    let plate = ItemRef::Part(g.part("copper_plate"));
    assert_eq!(g.host.factory.buildings.insert(&c, id, plate, 2), 2);
    let slot = |click| FactoryCommand::Click { target: SlotTarget::Building { id, group: SlotGroup::Input, index: 0 }, click };
    let input = |g: &mut Game| g.frame().building.map_or(0, |b| b.inputs[0].count);
    g.apply(FactoryCommand::OpenAt(at.origin()));
    // A right click with an empty hand takes half, a left click the rest.
    g.apply(slot(Click::Right));
    assert_eq!(g.host.factory.cursor.map(|c| c.count), Some(1));
    assert_eq!(input(&mut g), 1);
    // With plates in the hand, a left click puts them back.
    g.apply(slot(Click::Left));
    assert_eq!(g.host.factory.cursor, None);
    assert_eq!(input(&mut g), 2);
    g.apply(slot(Click::Left));
    assert_eq!(g.host.factory.cursor.map(|c| c.count), Some(2));
    g.apply(slot(Click::Left));
    // Shift + click moves them into the inventory.
    g.apply(slot(Click::Shift));
    assert_eq!(input(&mut g), 0);
    assert_eq!(g.count(plate), 2);
}

/// A small closed stone cup at `center` with a row of lava on a row of water, so the two react.
fn lava_on_water(g: &mut Game, center: CellPos) {
    let m = |name: &str| g.content.expect_material(name);
    let (stone, water, lava) = (m("stone"), m("water"), m("lava"));
    for dy in -4..=2 {
        for dx in -5i32..=5 {
            let inside = dx.abs() <= 4 && (-3..=1).contains(&dy);
            let material = match (inside, dy) {
                (false, _) => stone,
                (true, 1) | (true, 0) => water,
                (true, -1) => lava,
                _ => MaterialId::AIR,
            };
            g.sim.set_cell(CellPos::new(center.x + dx, center.y + dy), material, None);
        }
    }
}

/// Run ticks until the simulation sends a reaction event (at most `max` ticks). Returns the first one.
fn tick_until_reaction(g: &mut Game, max: u32) -> Option<(u16, CellPos)> {
    for _ in 0..max {
        g.ticks(1);
        let found = g.sim.events().iter().find_map(|e| if let SimEvent::Reaction { index, at } = *e { Some((index, at)) } else { None });
        if found.is_some() {
            return found;
        }
    }
    None
}

/// A reaction near the robot is discovered (with a notice); the same reaction far from the robot
/// is not.
#[test]
fn reactions_near_the_robot_are_discovered() {
    let mut g = Game::new();
    let robot = g.host.robot.center_cell();
    // Far: more than REACTION_SEE_RANGE cells from the robot, in the air above the ground.
    lava_on_water(&mut g, CellPos::new(robot.x + foundry_factory::REACTION_SEE_RANGE + 40, robot.y - 40));
    let (index, at) = tick_until_reaction(&mut g, 120).expect("lava and water react");
    let key = foundry_factory::progress::reaction_key(&g.content, &g.content.reactions[index as usize]);
    assert!((at.x - robot.x).abs() > foundry_factory::REACTION_SEE_RANGE, "the reaction is far: {at:?}");
    assert!(!g.host.factory.progress.is_reaction_discovered(&key), "a far reaction is not seen");
    g.ticks(60);
    // Near: 20 cells from the robot.
    lava_on_water(&mut g, CellPos::new(robot.x + 20, robot.y - 30));
    let mut seen = false;
    for _ in 0..120 {
        g.ticks(1);
        if g.host.factory.progress.is_reaction_discovered(&key) {
            seen = true;
            break;
        }
    }
    assert!(seen, "the reaction {key} near the robot is discovered");
    let notices = g.frame().notices;
    assert!(notices.iter().any(|n| n.starts_with("Discovered a reaction")), "{notices:?}");
}
