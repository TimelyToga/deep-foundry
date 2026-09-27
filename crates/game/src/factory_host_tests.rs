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
    assert_eq!(g.count(ItemRef::Material(g.content.expect_material("wood"))), 10);

    // A quickbar key takes it into the hand; the ghost is green at a free place.
    g.apply(FactoryCommand::PickToCursor(workbench));
    assert_eq!(g.host.factory.cursor.map(|c| c.part), Some(workbench));
    let kind = g.content.factory.building("workbench").unwrap();
    let at = g.free_place(kind);
    let ghost = GhostRequest { kind, at, rotation: 0 };
    g.apply(FactoryCommand::Input(PlayerInput { ghost: Some(ghost), ..Default::default() }));
    let f = g.frame();
    assert_eq!(f.ghost.as_ref().unwrap().error, None);
    // A ghost in the ground is red with the reason.
    let low = TilePos::new(at.x, at.y + 4);
    g.apply(FactoryCommand::Input(PlayerInput { ghost: Some(GhostRequest { at: low, ..ghost }), ..Default::default() }));
    let err = g.frame().ghost.unwrap().error.unwrap();
    assert!(err.contains("dig first") || err.contains("Blocked"), "{err}");

    g.apply(FactoryCommand::Place { kind, at, rotation: 0 });
    assert_eq!(g.host.factory.buildings.count_of(kind), 1);
    assert_eq!(g.host.factory.cursor, None);
    g.apply(FactoryCommand::OpenAt(at.origin()));
    let f = g.frame();
    assert_eq!(f.building.as_ref().map(|b| b.kind), Some(kind));
    g.ticks(2);
    assert!(g.frame().craft_speed > 1.0, "the workbench makes hand crafting faster");

    g.apply(FactoryCommand::RemoveAt(at.origin().offset(3, 3)));
    assert_eq!(g.host.factory.buildings.count_of(kind), 0);
    assert_eq!(g.count(ItemRef::Part(workbench)), 1, "back in the inventory");
    assert!(g.frame().building.is_none(), "the window closed");
}

#[test]
fn placing_needs_the_building_in_the_hand_and_reach() {
    let mut g = Game::new();
    let kind = g.content.factory.building("crate").unwrap();
    let at = g.free_place(kind);
    g.apply(FactoryCommand::Place { kind, at, rotation: 0 });
    assert_eq!(g.host.factory.buildings.count_of(kind), 0);
    assert!(g.frame().notices.iter().any(|n| n.contains("in the hand")));
    g.apply(FactoryCommand::PickToCursor(g.part("crate")));
    let far = TilePos::new(at.x + 40, at.y);
    assert_eq!(g.host.check_place(kind, far, 0, &g.sim), Err("Too far away".to_string()));
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
    g.apply(FactoryCommand::RemoveAt(CellPos::new(rect.x0 + 2, rect.y0 + 2)));
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
    g.apply(FactoryCommand::Place { kind, at, rotation: 0 });
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
    g.apply(FactoryCommand::Place { kind, at, rotation: 0 });
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
