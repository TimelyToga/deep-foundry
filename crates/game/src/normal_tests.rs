//! Tests of the main-thread side of the normal mode: UI actions and clicks become factory
//! commands, and the factory views become the UI model.

use super::*;
use crate::demo::{self, Shape};
use crate::factory_host::FactoryHost;
use foundry_factory::Guide;
use foundry_core::TilePos;
use std::sync::Arc;

/// A normal game on this thread, and the main-thread side with its first frame.
fn setup() -> (FactoryHost, foundry_sim::Simulation, NormalMode, Arc<Content>) {
    let content = Arc::new(Content::load_default().unwrap());
    let d = demo::build(content.clone(), Shape::Box { width_chunks: 32, height_chunks: 16 }, 3);
    let mut sim = d.sim;
    let mut host = FactoryHost::new_game(content.clone(), Arc::new(Guide::default()), &mut sim, d.start_center.0 as i32).unwrap();
    let mut n = NormalMode::new();
    n.take_frame(host.frame(&sim, 0), Instant::now());
    (host, sim, n, content)
}

fn apply_all(host: &mut FactoryHost, sim: &mut foundry_sim::Simulation, cmds: Vec<GameCommand>) {
    for c in cmds {
        if let GameCommand::Factory(c) = c {
            host.apply(c, sim);
        }
    }
}

#[test]
fn model_comes_from_the_frame() {
    let (_, _, n, content) = setup();
    let mut model = UiModel::new(content.clone());
    n.fill_model(&mut model);
    assert!(model.sandbox.is_none());
    assert_eq!(model.player.tank[0].material, content.material("wood"));
    assert_eq!(model.player.tank[0].units, 30);
    assert_eq!(model.player.inventory.iter().flatten().count(), 1);
    assert_eq!(model.player.hotbar.len(), 20);
    assert_eq!(model.player.hotbar[0], content.factory.part("crate").map(ItemRef::Part));
}

#[test]
fn tank_click_chooses_the_spray_material_and_quickbar_takes_buildings() {
    let (mut host, mut sim, mut n, content) = setup();
    let mut cmds = vec![];
    assert!(n.action(&UiAction::ClickSlot { slot: SlotRef::Tank(0), click: SlotClick::LEFT }, &mut cmds));
    assert_eq!(n.spray, content.material("wood"));
    assert_eq!(n.spray_material(), content.material("wood"));
    let mut model = UiModel::new(content.clone());
    n.fill_model(&mut model);
    assert_eq!(model.player.hand.map(|h| h.count), Some(30), "the hand shows the spray material");
    // Quickbar slot 1 has the crate: it goes into the hand.
    cmds.clear();
    n.action(&UiAction::SelectHotbar(0), &mut cmds);
    assert_eq!(n.spray, None);
    apply_all(&mut host, &mut sim, cmds);
    assert_eq!(host.factory.cursor.map(|c| c.part), content.factory.part("crate"));
    n.take_frame(host.frame(&sim, 1), Instant::now());
    assert_eq!(n.building_in_hand(&content), content.factory.building("crate"));
    let g = n.ghost(&content, CellPos::new(100, 100)).unwrap();
    assert_eq!(g.request.at, TilePos::new(12, 12));
}

#[test]
fn clicks_in_the_world_open_or_dig() {
    let (mut host, mut sim, mut n, content) = setup();
    let hub = host.factory.buildings.iter().next().map(|(_, b)| b.cell_rect()).unwrap();
    let on_hub = CellPos::new(hub.x0 + 3, hub.y0 + 3);
    // The host reports the building under the mouse.
    let input = n.input(&content, on_hub, CellRect::new(0, 0, 2048, 1024)).unwrap();
    apply_all(&mut host, &mut sim, vec![input]);
    n.take_frame(host.frame(&sim, 1), Instant::now());
    assert!(matches!(n.hover(on_hub), Some(HoverView::Building { .. })));
    let cmds = n.press(&content, true, on_hub, Mods::default());
    assert_eq!(cmds.len(), 1);
    apply_all(&mut host, &mut sim, cmds);
    n.take_frame(host.frame(&sim, 2), Instant::now());
    let mut model = UiModel::new(content.clone());
    n.fill_model(&mut model);
    let b = model.building.as_ref().expect("the Hub window is open");
    assert_eq!(b.inputs.len(), 16);
    assert_eq!(b.milestone.as_ref().map(|m| m.stage), Some(1));
    // A press on open ground digs.
    let cmds = n.press(&content, true, CellPos::new(hub.x1 + 40, hub.y1), Mods::default());
    assert!(cmds.is_empty());
    assert!(n.held.dig);
}

#[test]
fn pipette_takes_the_building_with_its_turn_and_recipe_and_empties_the_hand_elsewhere() {
    let (mut host, mut sim, mut n, content) = setup();
    let crusher = content.factory.building("steam_crusher").unwrap();
    let part = content.factory.part("steam_crusher").unwrap();
    host.factory.player.insert(&content, ItemRef::Part(part), 3);
    host.factory.cursor = Some(foundry_factory::PartStack::new(part, 1));
    let at = host.free_place(crusher, &sim).unwrap();
    let recipe = content.factory.recipe("crushed_limestone").unwrap();
    let place = crate::factory_host::Placement { kind: crusher, at, rotation: 1, flip: true, recipe: None, action: 1 };
    host.apply(FactoryCommand::Place(place), &mut sim);
    let id = host.building_at(at.origin()).unwrap();
    host.factory.buildings.set_recipe(&content, id, Some(recipe)).unwrap();
    // The mouse is on the crusher and the hand is empty.
    let on = at.origin().offset(4, 4);
    let input = n.input(&content, on, CellRect::new(0, 0, 2048, 1024)).unwrap();
    apply_all(&mut host, &mut sim, vec![input]);
    n.take_frame(host.frame(&sim, 1), Instant::now());
    let cmds = n.pipette(&content, on);
    assert!(matches!(cmds.as_slice(), [GameCommand::Factory(FactoryCommand::PickToCursor(p))] if *p == part));
    assert_eq!(n.build.transform(crusher), (1, true), "the rotation and flip of the building");
    assert_eq!(n.build.recipe_for(crusher), Some(recipe));
    apply_all(&mut host, &mut sim, cmds);
    n.take_frame(host.frame(&sim, 2), Instant::now());
    assert_eq!(n.building_in_hand(&content), Some(crusher));
    // The ghost has the same turn.
    let g = n.ghost(&content, on).unwrap();
    assert_eq!((g.request.rotation, g.request.flip), (1, true));
    // Q on empty ground empties the hand.
    let empty = CellPos::new(on.x, on.y - 200);
    let cmds = n.pipette(&content, empty);
    assert!(cmds.iter().any(|c| matches!(c, GameCommand::Factory(FactoryCommand::ClearCursor))));
    assert_eq!(n.build.hand_recipe, None);
}

#[test]
fn a_press_with_a_building_starts_a_drag_line_and_the_release_ends_it() {
    let (mut host, mut sim, mut n, content) = setup();
    let belt = content.factory.part("wood_belt").unwrap();
    host.factory.player.insert(&content, ItemRef::Part(belt), 10);
    apply_all(&mut host, &mut sim, vec![FactoryCommand::PickToCursor(belt).into()]);
    n.take_frame(host.frame(&sim, 1), Instant::now());
    let start = CellPos::new(1000, 900);
    let cmds = n.press(&content, true, start, Mods::default());
    assert!(matches!(cmds.as_slice(), [GameCommand::Factory(FactoryCommand::Place(_))]));
    // Two tiles to the left: two more belts, and the first belt turns to face left.
    let cmds = n.update(start.offset(-16, 0));
    let turn = crate::factory_host::Turn::To { rotation: 2, flip: false };
    assert!(matches!(cmds[0], GameCommand::Factory(FactoryCommand::Turn { turn: t, .. }) if t == turn));
    assert_eq!(cmds.len(), 3);
    assert_eq!(n.build.transform(content.factory.building("wood_belt").unwrap()), (2, false), "the hand keeps the direction");
    // While dragging there is no ghost to check.
    let input = n.input(&content, start, CellRect::new(0, 0, 10, 10));
    assert!(input.is_some_and(|c| matches!(c, GameCommand::Factory(FactoryCommand::Input(i)) if i.ghost.is_none())));
    n.release(true);
    assert!(n.build.drag.is_none());
    assert!(n.update(start.offset(-40, 0)).is_empty());
}
