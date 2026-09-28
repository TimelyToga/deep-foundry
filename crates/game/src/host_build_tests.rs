//! Tests of construction on the simulation thread: drag lines, turning, the remove button, copy
//! and paste, undo and redo. No window and no thread.

use super::*;
use crate::demo::{self, Shape};
use crate::factory_host::{FactoryCommand, FactoryFrame, PlayerInput};
use foundry_content::Content;
use foundry_core::TILE_SIZE;
use foundry_factory::Guide;
use std::sync::Arc;

struct Game {
    host: FactoryHost,
    sim: Simulation,
    content: Arc<Content>,
}

impl Game {
    fn new() -> Self {
        let content = Arc::new(Content::load_default().unwrap());
        let demo = demo::build(content.clone(), Shape::Box { width_chunks: 32, height_chunks: 16 }, 3);
        let mut sim = demo.sim;
        let host = FactoryHost::new_game(content.clone(), Arc::new(Guide::default()), &mut sim, demo.start_center.0 as i32).unwrap();
        let mut g = Self { host, sim, content };
        g.ticks(30); // The robot lands.
        g
    }

    /// Finish all technologies, so the player knows all recipes.
    fn research_all(&mut self) {
        let c = self.content.clone();
        for i in 0..c.factory.techs.len() {
            self.host.factory.progress.debug_complete(&c, foundry_core::TechId(i as u16));
        }
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

    fn kind(&self, id: &str) -> BuildingKindId {
        self.content.factory.building(id).unwrap()
    }

    /// Put `n` of a building into the inventory and a stack of it into the hand.
    fn hold(&mut self, id: &str, n: u32) {
        let part = self.content.factory.part(id).unwrap();
        let c = self.content.clone();
        self.host.factory.player.insert(&c, ItemRef::Part(part), n);
        self.apply(FactoryCommand::PickToCursor(part));
    }

    /// Items of a building in the inventory and in the hand.
    fn items(&self, id: &str) -> u32 {
        let part = self.content.factory.part(id).unwrap();
        let hand = self.host.factory.cursor.filter(|c| c.part == part).map_or(0, |c| c.count);
        self.host.factory.player.count(ItemRef::Part(part)) + hand
    }

    fn count(&self, id: &str) -> u32 {
        self.host.factory.buildings.count_of(self.kind(id))
    }

    /// A free row of tiles in the air, four tiles above the first free place right of the robot.
    fn air_row(&self) -> TilePos {
        let at = self.host.free_place(self.kind("wood_belt"), &self.sim).expect("a free place");
        TilePos::new(at.x, at.y - 4)
    }

    fn place(&mut self, id: &str, at: TilePos, rotation: u8, action: u32) {
        let kind = self.kind(id);
        self.apply(FactoryCommand::Place(Placement { kind, at, rotation, flip: false, recipe: None, action }));
    }

    fn building(&self, id: &str, at: TilePos) -> Option<&foundry_factory::Building> {
        let bid = self.host.find(self.kind(id), at)?;
        self.host.factory.buildings.get(bid)
    }

    /// Hold the remove button while the mouse moves along these cells.
    fn remove_along(&mut self, action: u32, cells: &[CellPos]) {
        for &aim in cells {
            self.apply(FactoryCommand::Input(PlayerInput { aim, remove: Some(action), ..Default::default() }));
        }
    }

    fn release(&mut self) {
        self.apply(FactoryCommand::Input(PlayerInput::default()));
    }
}

fn middle(at: TilePos) -> CellPos {
    at.origin().offset(TILE_SIZE / 2, TILE_SIZE / 2)
}

fn right(at: TilePos, n: i32) -> TilePos {
    TilePos::new(at.x + n, at.y)
}

#[test]
fn a_dragged_line_is_one_undo_step_and_redo_places_it_again() {
    let mut g = Game::new();
    g.hold("wood_belt", 10);
    let row = g.air_row();
    for i in 0..4 {
        g.place("wood_belt", right(row, i), 0, 5);
    }
    assert_eq!(g.count("wood_belt"), 4);
    assert_eq!(g.items("wood_belt"), 6);
    assert_eq!(g.host.undo.undo_len(), 1, "one drag is one undo entry");

    g.apply(FactoryCommand::Undo);
    assert_eq!(g.count("wood_belt"), 0);
    assert_eq!(g.items("wood_belt"), 10, "the belts are back");
    assert_eq!((g.host.undo.undo_len(), g.host.undo.redo_len()), (0, 1));

    g.apply(FactoryCommand::Redo);
    assert_eq!(g.count("wood_belt"), 4);
    assert_eq!(g.items("wood_belt"), 6);
    assert_eq!((g.host.undo.undo_len(), g.host.undo.redo_len()), (1, 0));

    // A new action after an undo clears the redo list.
    g.apply(FactoryCommand::Undo);
    g.place("wood_belt", row, 0, 6);
    assert_eq!(g.host.undo.redo_len(), 0);
    g.apply(FactoryCommand::Redo);
    assert!(g.frame().notices.iter().any(|n| n == "Nothing to redo"));
}

#[test]
fn a_line_stops_at_the_first_place_that_fails() {
    let mut g = Game::new();
    g.hold("wood_wall", 10);
    let row = g.air_row();
    let hub = g.host.factory.buildings.iter().next().map(|(_, b)| b.at).unwrap();
    // The second place is on the Hub, so the third is not placed.
    g.place("wood_wall", row, 0, 9);
    g.place("wood_wall", hub, 0, 9);
    g.place("wood_wall", right(row, 1), 0, 9);
    assert_eq!(g.count("wood_wall"), 1);
    let stop = g.frame().drag_stop.expect("the line stopped");
    assert_eq!((stop.action, stop.at), (9, hub));
    assert!(stop.reason.contains("Tile taken"), "{}", stop.reason);
    // The next action places again.
    g.place("wood_wall", right(row, 1), 0, 10);
    assert_eq!(g.count("wood_wall"), 2);
    // Out of reach: the reason for the ghost.
    let far = TilePos::new(row.x + 30, row.y);
    assert_eq!(g.host.check_place(g.kind("wood_wall"), far, 0, false, &g.sim), Err("Out of reach".to_string()));
}

#[test]
fn the_remove_button_takes_the_buildings_under_the_path_one_after_another() {
    let mut g = Game::new();
    g.hold("wood_belt", 3);
    let row = g.air_row();
    for i in 0..3 {
        g.place("wood_belt", right(row, i), 0, 1);
    }
    assert_eq!(g.items("wood_belt"), 0);
    // The mouse moves fast from the first belt to the third: the second is on the path.
    g.remove_along(2, &[middle(row), middle(right(row, 2))]);
    let f = g.frame();
    assert_eq!(f.remove_queue.len(), 2, "two wait after the first");
    assert_eq!(f.removing.map(|r| r.progress), Some(0.0));
    g.ticks(remove_ticks((1, 1)) / 2);
    let p = g.frame().removing.unwrap().progress;
    assert!(p > 0.3 && p < 0.7, "{p}");
    g.ticks(remove_ticks((1, 1)));
    assert_eq!(g.count("wood_belt"), 2, "one after another");
    g.ticks(3 * remove_ticks((1, 1)));
    assert_eq!(g.count("wood_belt"), 0);
    assert_eq!(g.items("wood_belt"), 3, "back in the inventory");
    // The whole press is one undo entry.
    g.release();
    g.apply(FactoryCommand::Undo);
    assert_eq!(g.count("wood_belt"), 3);
    // Letting go stops the removal.
    g.remove_along(3, &[middle(row)]);
    g.ticks(remove_ticks((1, 1)) - 2);
    g.release();
    g.ticks(20);
    assert_eq!(g.count("wood_belt"), 3);
    assert!(g.frame().removing.is_none());
}

#[test]
fn undo_puts_a_removed_building_back_with_its_turn_and_recipe() {
    let mut g = Game::new();
    g.research_all();
    g.hold("steam_wire_drawer", 1);
    let row = g.air_row();
    g.place("steam_wire_drawer", row, 0, 1);
    let recipe = g.content.factory.recipe("copper_wire").unwrap();
    let id = g.host.find(g.kind("steam_wire_drawer"), row).unwrap();
    g.host.factory.buildings.set_recipe(&g.content, id, Some(recipe)).unwrap();
    // The drawer is 2 × 1: in place it turns by half turns.
    g.apply(FactoryCommand::Turn { at: middle(row), turn: Turn::Clockwise, action: 2 });
    assert_eq!(g.building("steam_wire_drawer", row).unwrap().transform.rotation, 2);
    g.remove_along(3, &[middle(row)]);
    g.ticks(remove_ticks((2, 1)) + 1);
    g.release();
    assert_eq!(g.count("steam_wire_drawer"), 0);
    assert_eq!(g.items("steam_wire_drawer"), 1);

    g.apply(FactoryCommand::Undo);
    let b = g.building("steam_wire_drawer", row).expect("placed again");
    assert_eq!(b.transform.rotation, 2);
    assert!(matches!(&b.logic, foundry_factory::Logic::Machine(m) if m.recipe == Some(recipe)));
    assert_eq!(g.items("steam_wire_drawer"), 0);
    g.apply(FactoryCommand::Undo);
    assert_eq!(g.building("steam_wire_drawer", row).unwrap().transform.rotation, 0, "the turn is undone");
    g.apply(FactoryCommand::Undo);
    assert_eq!(g.count("steam_wire_drawer"), 0, "the placement is undone");
    assert_eq!(g.items("steam_wire_drawer"), 1);
}

#[test]
fn undo_of_a_removal_needs_the_item() {
    let mut g = Game::new();
    g.hold("wood_wall", 1);
    let row = g.air_row();
    g.place("wood_wall", row, 0, 1);
    g.remove_along(2, &[middle(row)]);
    g.ticks(remove_ticks((1, 1)) + 1);
    g.release();
    // The wall item is gone from the inventory (for example, the player crafted with it).
    let c = g.content.clone();
    let part = c.factory.part("wood_wall").unwrap();
    g.host.factory.player.remove(ItemRef::Part(part), 1);
    g.apply(FactoryCommand::Undo);
    assert_eq!(g.count("wood_wall"), 0);
    assert!(g.frame().notices.iter().any(|n| n.contains("no Wood block in the inventory")), "the player is told why");
}

#[test]
fn copy_and_paste_the_recipe() {
    let mut g = Game::new();
    g.research_all();
    g.hold("steam_crusher", 2);
    let row = g.air_row();
    g.place("steam_crusher", row, 0, 1);
    g.place("steam_crusher", right(row, 3), 0, 2);
    let recipe = g.content.factory.recipe("crushed_limestone").unwrap();
    let first = g.host.find(g.kind("steam_crusher"), row).unwrap();
    g.host.factory.buildings.set_recipe(&g.content, first, Some(recipe)).unwrap();
    g.apply(FactoryCommand::PasteSettings(middle(right(row, 3))));
    assert!(g.frame().notices.iter().any(|n| n.starts_with("Nothing to paste")));
    g.apply(FactoryCommand::CopySettings(middle(row)));
    g.apply(FactoryCommand::PasteSettings(middle(right(row, 3))));
    let second = g.building("steam_crusher", right(row, 3)).unwrap();
    assert!(matches!(&second.logic, foundry_factory::Logic::Machine(m) if m.recipe == Some(recipe)));
    // Only on the same building kind.
    g.hold("crate", 1);
    g.place("crate", right(row, 6), 0, 3);
    g.apply(FactoryCommand::PasteSettings(middle(right(row, 6))));
    assert!(g.frame().notices.iter().any(|n| n.contains("Paste works only on a Steam crusher")));
}

#[test]
fn the_building_window_closes_out_of_reach() {
    let mut g = Game::new();
    g.hold("crate", 1);
    let row = g.air_row();
    g.place("crate", row, 0, 1);
    g.apply(FactoryCommand::OpenAt(middle(row)));
    assert!(g.frame().building.is_some());
    // The robot is far away now.
    let r = g.host.robot.rect();
    g.host.robot.left = r.x0 + 200;
    assert!(g.frame().building.is_none());
    g.host.robot.left = r.x0;
    g.apply(FactoryCommand::OpenAt(middle(row)));
    assert!(g.frame().building.is_some(), "it opens again in reach");
}
