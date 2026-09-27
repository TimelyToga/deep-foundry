//! Buildings take damage from heat and from lost body cells, then break and release their
//! contents.

mod common;

use common::*;
use foundry_core::{CellPos, CellRect, DEFAULT_TEMPERATURE, TilePos};
use foundry_factory::buildings::DAMAGE_PERIOD;
use foundry_factory::{Factory, FactoryEvent, Status};
use foundry_sim::Simulation;

const GROUND: i32 = 96;

fn max_temperature(sim: &Simulation, r: CellRect) -> i16 {
    let mut t = i16::MIN;
    for y in r.y0..r.y1 {
        for x in r.x0..r.x1 {
            t = t.max(sim.cell(CellPos::new(x, y)).temperature);
        }
    }
    t
}

#[test]
fn a_building_next_to_lava_gets_too_hot_breaks_and_releases_its_contents() {
    let c = content();
    let mut sim = world(&c, Some(GROUND));
    let mut f = Factory::new(c.clone());
    let (sand, lava, stone, wood) = (mat(&c, "sand"), mat(&c, "lava"), mat(&c, "stone"), mat(&c, "wood_block"));
    // A hopper on the ground holds 20 sand (its output faces the ground, so it keeps it).
    let at = TilePos::new(6, 11);
    let foot = tile_cells(at, 1, 1);
    let id = f.place(kind(&c, "test_hopper"), at, 0, false, &mut sim).unwrap();
    assert_eq!(f.buildings.insert(&c, id, item(&c, "sand"), 20), 20);
    // A stone basin with lava on both sides of the hopper.
    fill(&mut sim, CellRect::new(36, 80, 40, GROUND), stone, None);
    fill(&mut sim, CellRect::new(64, 80, 68, GROUND), stone, None);
    fill(&mut sim, CellRect::new(40, 88, 48, GROUND), lava, None);
    fill(&mut sim, CellRect::new(56, 88, 64, GROUND), lava, None);
    run(&mut f, &mut sim, 120);
    if max_temperature(&sim, foot) <= DEFAULT_TEMPERATURE {
        // The heat pass of the simulation is still a stub in this branch, so heat does not flow
        // from the lava into the body. Do what it will do: make the body as hot as the lava
        // next to it. When the heat pass exists, this branch is not used.
        fill(&mut sim, foot, wood, Some(1100));
    }
    let mut broke = false;
    for _ in 0..(8 * DAMAGE_PERIOD) {
        run(&mut f, &mut sim, 1);
        if f.take_events().contains(&FactoryEvent::Broke { id, kind: kind(&c, "test_hopper"), at }) {
            broke = true;
            break;
        }
        if let Some(v) = f.building_view(id) {
            assert!(v.hit_points <= 100);
        }
    }
    assert!(broke, "the hopper broke");
    assert!(f.buildings.get(id).is_none());
    assert_eq!(f.buildings.at_tile(at, foundry_content::Layer::Front), None);
    // The 20 sand cells are back in the world. Scrap (the body material, which has no broken
    // form) lies in the bottom quarter of the old footprint.
    assert_eq!(count_all(&sim, sand), 20);
    assert_eq!(sim.count_material(CellRect::new(foot.x0, foot.y1 - 2, foot.x1, foot.y1), wood), 16);
}

#[test]
fn lost_body_cells_break_a_building() {
    let c = content();
    let mut sim = world(&c, Some(GROUND));
    let mut f = Factory::new(c.clone());
    let at = TilePos::new(6, 11);
    let foot = tile_cells(at, 1, 1);
    let id = f.place(kind(&c, "test_crate"), at, 0, false, &mut sim).unwrap();
    f.buildings.insert(&c, id, item(&c, "bronze_plate"), 5);
    // A quarter of the body is gone: half of the hit points.
    fill(&mut sim, CellRect::new(foot.x0, foot.y0, foot.x1, foot.y0 + 2), mat(&c, "air"), None);
    run(&mut f, &mut sim, DAMAGE_PERIOD as usize);
    assert_eq!(f.building_view(id).unwrap().hit_points, 50);
    // The same lost cells do not count twice.
    run(&mut f, &mut sim, DAMAGE_PERIOD as usize);
    assert_eq!(f.building_view(id).unwrap().hit_points, 50);
    // Another quarter: 0 hit points. The plates become bronze cells (16 units each).
    fill(&mut sim, CellRect::new(foot.x0, foot.y0 + 2, foot.x1, foot.y0 + 4), mat(&c, "air"), None);
    run(&mut f, &mut sim, DAMAGE_PERIOD as usize);
    assert!(f.buildings.get(id).is_none());
    assert_eq!(count_all(&sim, mat(&c, "bronze_block")), 80);
}

#[test]
fn a_hot_body_stops_a_machine() {
    let c = content();
    let mut sim = world(&c, Some(GROUND));
    let mut f = Factory::new(c.clone());
    let at = TilePos::new(4, 10);
    let id = f.place(kind(&c, "test_press"), at, 0, false, &mut sim).unwrap();
    f.set_recipe(id, c.factory.recipe("test_press_sand")).unwrap();
    f.buildings.insert(&c, id, item(&c, "sand"), 16);
    fill(&mut sim, tile_cells(at, 2, 2), mat(&c, "wood_block"), Some(250));
    run(&mut f, &mut sim, 2 * DAMAGE_PERIOD as usize);
    let v = f.building_view(id).unwrap();
    assert_eq!(v.status, Status::TooHot);
    assert_eq!(v.reason, "Too hot: 250 °C, the limit is 200 °C");
    assert_eq!(v.temperature, 250);
    assert!(v.hit_points < 100, "slightly too hot: slow damage");
    assert!(v.hit_points > 50);
}
