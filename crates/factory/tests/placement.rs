//! Placing and removing buildings in a real simulation.

mod common;

use common::*;
use foundry_content::{ItemRef, PortKind, Side, Stack};
use foundry_core::{CellPos, CellRect, TilePos};
use foundry_factory::{Factory, InvTarget, PlaceError, RemoveError, Transform};
use foundry_sim::Simulation;

/// Stone from row 96 (tile row 12) down.
const GROUND: i32 = 96;

fn setup() -> (Factory, Simulation) {
    let c = content();
    (Factory::new(c.clone()), world(&c, Some(GROUND)))
}

#[test]
fn place_on_stone_ground_writes_the_body_and_takes_the_tiles() {
    let (mut f, mut sim) = setup();
    let c = f.content.clone();
    let press = kind(&c, "test_press");
    let at = TilePos::new(4, 10); // 2 × 2 tiles: cells y 80..96, just above the ground
    assert_eq!(f.can_place(press, at, 0, false, &sim), Ok(()));
    let id = f.place(press, at, 0, false, &mut sim).expect("placed");
    let wood = mat(&c, "wood_block");
    assert_eq!(sim.count_material(tile_cells(at, 2, 2), wood), 256);
    // Every tile of the footprint is taken.
    for t in [TilePos::new(4, 10), TilePos::new(5, 11)] {
        assert_eq!(f.buildings.at_tile(t, foundry_content::Layer::Front), Some(id));
    }
    let err = f.can_place(kind(&c, "test_crate"), TilePos::new(5, 11), 0, false, &sim).unwrap_err();
    assert!(matches!(err, PlaceError::TileTaken { by, .. } if by == id));
    assert_eq!(err.to_string(), "Tile taken by Test press");
    // The body cells are marked as building cells (for explosions and the light).
    let r = tile_cells(at, 2, 2);
    assert!(sim.is_building_cell(CellPos::new(r.x0, r.y0)) && sim.is_building_cell(CellPos::new(r.x1 - 1, r.y1 - 1)));
    assert!(!sim.is_building_cell(CellPos::new(r.x0 - 1, r.y0)), "the cell next to it is not");
    // The building still stands after some ticks.
    run(&mut f, &mut sim, 100);
    assert!(f.buildings.get(id).is_some());
    assert_eq!(sim.count_material(tile_cells(at, 2, 2), wood), 256);
}

#[test]
fn solid_rock_blocks_with_the_right_reason() {
    let (f, sim) = setup();
    let c = f.content.clone();
    let press = kind(&c, "test_press");
    // Tile row 11 and 12: the lower row is in the stone.
    let err = f.can_place(press, TilePos::new(4, 11), 0, false, &sim).unwrap_err();
    match &err {
        PlaceError::Blocked { name, can_dig, at, .. } => {
            assert_eq!(name, "Stone");
            assert!(*can_dig);
            assert_eq!(at.y, GROUND);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(err.to_string(), "Blocked by stone: dig first");
    // The bedrock border (cells x 0 and 1) cannot be dug.
    let err = f.can_place(kind(&c, "test_crate"), TilePos::new(0, 5), 0, false, &sim).unwrap_err();
    assert_eq!(err.to_string(), "Blocked by bedrock");
    // Outside the world.
    assert_eq!(f.can_place(press, TilePos::new(-1, 5), 0, false, &sim), Err(PlaceError::OutsideWorld));
    assert_eq!(f.can_place(press, TilePos::new(15, 3), 0, false, &sim), Err(PlaceError::OutsideWorld));
    assert_eq!(f.can_place(press, TilePos::new(4, -1), 0, false, &sim), Err(PlaceError::OutsideWorld));
}

#[test]
fn loose_sand_and_water_are_pushed_away_and_nothing_is_lost() {
    let (mut f, mut sim) = setup();
    let c = f.content.clone();
    let (sand, water) = (mat(&c, "sand"), mat(&c, "water"));
    let at = TilePos::new(6, 10);
    let foot = tile_cells(at, 2, 2);
    // Sand fills the lower half of the footprint and more around it; water fills a corner.
    fill(&mut sim, CellRect::new(foot.x0 - 6, 88, foot.x1 + 6, GROUND), sand, None);
    fill(&mut sim, CellRect::new(foot.x0, 80, foot.x0 + 6, 86), water, None);
    let (sand0, water0) = (count_all(&sim, sand), count_all(&sim, water));
    let id = f.place(kind(&c, "test_press"), at, 0, false, &mut sim).expect("placed");
    assert_eq!(count_all(&sim, sand), sand0, "no sand lost");
    assert_eq!(count_all(&sim, water), water0, "no water lost");
    assert_eq!(sim.count_material(foot, mat(&c, "wood_block")), 256);
    run(&mut f, &mut sim, 200);
    assert!(f.buildings.get(id).is_some());
    assert_eq!(count_all(&sim, sand), sand0, "no sand lost after the cells settle");
    assert_eq!(count_all(&sim, water), water0);
}

#[test]
fn no_room_for_loose_cells() {
    let c = content();
    let mut sim = world(&c, Some(GROUND));
    let f = Factory::new(c.clone());
    // A closed stone box with sand inside: the sand has nowhere to go.
    let stone = mat(&c, "stone");
    let at = TilePos::new(6, 10);
    let foot = tile_cells(at, 2, 2);
    fill(&mut sim, CellRect::new(foot.x0 - 2, foot.y0 - 2, foot.x1 + 2, GROUND), stone, None);
    fill(&mut sim, foot, mat(&c, "sand"), None);
    let err = f.can_place(kind(&c, "test_press"), at, 0, false, &sim).unwrap_err();
    assert_eq!(err.to_string(), "No room to push the sand out of the way");
}

#[test]
fn needs_floor() {
    let (f, sim) = setup();
    let c = f.content.clone();
    let post = kind(&c, "test_post");
    assert_eq!(f.can_place(post, TilePos::new(4, 5), 0, false, &sim), Err(PlaceError::NeedsFloor));
    assert_eq!(PlaceError::NeedsFloor.to_string(), "Needs a floor");
    assert_eq!(f.can_place(post, TilePos::new(4, 11), 0, false, &sim), Ok(()));
}

#[test]
fn remove_gives_back_the_item_and_the_contents() {
    let (mut f, mut sim) = setup();
    let c = f.content.clone();
    let crate_kind = kind(&c, "test_crate");
    let at = TilePos::new(3, 11);
    let id = f.place(crate_kind, at, 0, false, &mut sim).unwrap();
    let plate = item(&c, "bronze_plate");
    assert_eq!(f.buildings.insert(&c, id, plate, 250), 250);
    let got = f.remove(id, &mut sim).unwrap();
    assert_eq!(got[0], Stack { item: ItemRef::Part(c.factory.part("test_crate").unwrap()), count: 1 });
    assert_eq!(got[1], Stack { item: plate, count: 250 });
    assert_eq!(sim.count_material(tile_cells(at, 1, 1), mat(&c, "wood_block")), 0, "the body is air again");
    let r = tile_cells(at, 1, 1);
    assert!(!sim.is_building_cell(CellPos::new(r.x0, r.y0)), "the air is not a building cell");
    assert_eq!(f.remove(id, &mut sim), Err(RemoveError::NotFound));
    // The tile is free again, and a new building gets a new id.
    let id2 = f.place(crate_kind, at, 0, false, &mut sim).unwrap();
    assert_ne!(id, id2);
    assert!(f.buildings.get(id).is_none());
}

#[test]
fn remove_to_player_drops_bulk_that_does_not_fit() {
    let (mut f, mut sim) = setup();
    let c = f.content.clone();
    let press = kind(&c, "test_press");
    let at = TilePos::new(4, 10);
    let id = f.place(press, at, 0, false, &mut sim).unwrap();
    f.set_recipe(id, c.factory.recipe("test_press_sand")).unwrap();
    let sand = mat(&c, "sand");
    assert_eq!(f.buildings.insert(&c, id, ItemRef::Material(sand), 16), 16);
    // The player's tanks are full of clay, so the sand cannot go in.
    let clay = item(&c, "clay");
    let tanks = f.player.tanks.len() as u32;
    f.player.insert(&c, clay, tanks * foundry_factory::inventory::PLAYER_TANK_UNITS);
    let sand0 = count_all(&sim, sand);
    let report = f.remove_to_player(id, &mut sim).unwrap();
    assert_eq!(report.taken, vec![Stack { item: ItemRef::Part(c.factory.part("test_press").unwrap()), count: 1 }]);
    assert_eq!(report.dropped, vec![Stack { item: ItemRef::Material(sand), count: 16 }]);
    assert!(report.left.is_empty());
    assert_eq!(count_all(&sim, sand), sand0 + 16);
}

#[test]
fn rotation_moves_the_ports_and_belts_only_face_sideways() {
    let (mut f, mut sim) = setup();
    let c = f.content.clone();
    // Test press ports: BulkIn at (0,0) Up, BulkOut at (1,1) Down. Turned once clockwise:
    // (0,0) -> (1,0) facing Right; (1,1) -> (0,1) facing Left.
    let at = TilePos::new(4, 10);
    let id = f.place(kind(&c, "test_press"), at, 1, false, &mut sim).unwrap();
    let v = f.building_view(id).unwrap();
    let find = |k: PortKind| v.ports.iter().find(|p| p.kind == k).unwrap().clone();
    assert_eq!((find(PortKind::BulkIn).tile, find(PortKind::BulkIn).side), (TilePos::new(5, 10), Side::Right));
    assert_eq!((find(PortKind::BulkOut).tile, find(PortKind::BulkOut).side), (TilePos::new(4, 11), Side::Left));
    // The ghost shows the same ports.
    let ghost = f.ghost_ports(kind(&c, "test_press"), at, 1, false);
    assert_eq!(ghost, v.ports);
    // Belts: rotation 1 and 3 are refused.
    let belt = kind(&c, "test_belt");
    assert_eq!(f.can_place(belt, TilePos::new(9, 11), 1, false, &sim), Err(PlaceError::CannotTurn));
    assert_eq!(f.can_place(belt, TilePos::new(9, 11), 2, false, &sim), Ok(()));
    assert_eq!(foundry_factory::buildings::belt_direction(Transform::new(0, true)), -1);
    assert_eq!(foundry_factory::buildings::belt_direction(Transform::new(2, true)), 1);
}

#[test]
fn slot_rules_between_the_player_and_a_crate() {
    let (mut f, mut sim) = setup();
    let c = f.content.clone();
    let id = f.place(kind(&c, "test_crate"), TilePos::new(3, 11), 0, false, &mut sim).unwrap();
    let gear = item(&c, "bronze_gear");
    let plate = item(&c, "bronze_plate");
    f.player.insert(&c, gear, 150);
    f.player.insert(&c, plate, 10);
    // Shift click the first player slot (100 gears) into the crate.
    assert!(f.click(InvTarget::Player, 0, foundry_factory::Click::Shift, Some(InvTarget::Building(id))));
    assert_eq!(f.buildings.inventory(id).unwrap().count(gear), 100);
    // Ctrl click the other gear slot: all remaining gears move.
    let slot = f.player.slots.iter().position(|s| s.is_some_and(|s| ItemRef::Part(s.part) == gear)).unwrap();
    assert!(f.click(InvTarget::Player, slot, foundry_factory::Click::Ctrl, Some(InvTarget::Building(id))));
    assert_eq!(f.player.count(gear), 0);
    assert_eq!(f.buildings.inventory(id).unwrap().count(gear), 150);
    // Pick up a crate stack with the cursor and put it in an empty player slot.
    assert!(f.click(InvTarget::Building(id), 0, foundry_factory::Click::Left, None));
    assert_eq!(f.cursor.map(|s| s.count), Some(100));
    let empty = f.player.slots.iter().position(|s| s.is_none()).unwrap();
    assert!(f.click(InvTarget::Player, empty, foundry_factory::Click::Right, None));
    assert_eq!(f.player.count(gear), 1);
    assert!(f.click(InvTarget::Player, empty, foundry_factory::Click::Left, None));
    assert_eq!(f.player.count(gear), 100);
    assert!(f.cursor.is_none());
    // The crate view shows the rest.
    let v = f.building_view(id).unwrap();
    let inv = v.inventory.unwrap();
    let n: u32 = inv.slots.iter().flatten().map(|s| s.count).sum();
    assert_eq!(n, 50);
}

#[test]
fn save_and_load_keep_the_buildings() {
    let (mut f, mut sim) = setup();
    let c = f.content.clone();
    let id = f.place(kind(&c, "test_press"), TilePos::new(4, 10), 0, false, &mut sim).unwrap();
    f.set_recipe(id, c.factory.recipe("test_press_sand")).unwrap();
    f.buildings.insert(&c, id, item(&c, "sand"), 8);
    run(&mut f, &mut sim, 30);
    let text = ron::to_string(&f.save()).expect("save");
    let mut g = Factory::new(c.clone());
    g.load(ron::from_str(&text).expect("load"));
    assert_eq!(g.building_view(id), f.building_view(id));
    assert_eq!(g.buildings.at_tile(TilePos::new(5, 11), foundry_content::Layer::Front), Some(id));
    // Both go on the same way.
    let mut sim2 = world(&c, Some(GROUND));
    for y in 80..96 {
        for x in 32..48 {
            let p = CellPos::new(x, y);
            let cell = sim.cell(p);
            sim2.set_cell(p, cell.material, Some(cell.temperature));
        }
    }
    run(&mut f, &mut sim, 40);
    run(&mut g, &mut sim2, 40);
    assert_eq!(g.building_view(id), f.building_view(id));
}

#[test]
fn a_placed_building_turns_where_it_stands() {
    let (mut f, mut sim) = setup();
    let c = f.content.clone();
    let at = TilePos::new(4, 10);
    let id = f.place(kind(&c, "test_press"), at, 0, false, &mut sim).unwrap();
    f.set_recipe(id, c.factory.recipe("test_press_sand")).unwrap();
    // A 2 × 2 building turns in place; the ports move as for a new placement.
    assert_eq!(f.set_transform(id, 1, false), Ok(()));
    let v = f.building_view(id).unwrap();
    assert_eq!((v.rotation, v.flip), (1, false));
    assert_eq!(v.ports, f.ghost_ports(kind(&c, "test_press"), at, 1, false));
    assert!(v.recipe.is_some(), "the recipe stays");
    assert_eq!(sim.count_material(tile_cells(at, 2, 2), mat(&c, "wood_block")), 256, "the body stays");
    // Belts face only left or right.
    let belt = f.place(kind(&c, "test_belt"), TilePos::new(8, 11), 0, false, &mut sim).unwrap();
    assert_eq!(f.set_transform(belt, 1, false), Err(PlaceError::CannotTurn));
    assert_eq!(f.set_transform(belt, 2, false), Ok(()));
    assert_eq!(foundry_factory::buildings::belt_direction(f.buildings.get(belt).unwrap().transform), -1);
    // An old id changes nothing.
    f.remove(belt, &mut sim).unwrap();
    assert!(f.set_transform(belt, 0, false).is_err());
}
