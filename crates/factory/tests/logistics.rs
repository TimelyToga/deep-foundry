//! Hoppers and belts move real sand cells across the world.

mod common;

use common::*;
use foundry_core::{CellRect, TilePos};
use foundry_factory::{Factory, Status};

/// Stone from row 112 (tile row 14) down.
const GROUND: i32 = 112;

#[test]
fn hopper_and_belts_carry_sand_and_drop_it_at_the_end() {
    let c = content();
    let mut sim = world(&c, Some(GROUND));
    let mut f = Factory::new(c.clone());
    let (sand, stone) = (mat(&c, "sand"), mat(&c, "stone"));
    // Belts high above the ground on tile row 7 (cells 56..64), tiles x 3..=8, moving right.
    // The sand falls 48 cells from the end, so the pile there never reaches the belt.
    let belt = kind(&c, "test_belt");
    for x in 3..=8 {
        f.place(belt, TilePos::new(x, 7), 0, false, &mut sim).unwrap();
    }
    // A hopper two tiles above the first belt (one tile of air between them).
    let hopper = f.place(kind(&c, "test_hopper"), TilePos::new(3, 5), 0, false, &mut sim).unwrap();
    // Sand in a stone funnel above the hopper, so it all falls onto the hopper top.
    fill(&mut sim, CellRect::new(22, 8, 24, 40), stone, None);
    fill(&mut sim, CellRect::new(32, 8, 34, 40), stone, None);
    fill(&mut sim, CellRect::new(24, 16, 32, 36), sand, None);
    // A wall at the start of the belt line, so no sand slides off that end.
    fill(&mut sim, CellRect::new(22, 48, 24, 56), stone, None);
    let total = count_all(&sim, sand);
    assert_eq!(total, 160);
    let belt_end = 9 * 8; // the first cell right of the last belt
    let mut max_inside = 0;
    for _ in 0..900 {
        f.tick(&mut sim);
        sim.tick();
        let inside = f.building_view(hopper).map_or(0, |v| v.inputs.iter().map(|b| b.count).sum::<u32>());
        max_inside = max_inside.max(inside);
        assert_eq!(count_all(&sim, sand) + inside as usize, total, "no sand is lost or made");
    }
    assert!(max_inside > 0, "the hopper held sand on the way");
    // The sand passed through the hopper, along the belts and off the end of the line. Now it
    // is all in the pile on the ground below (the pile spreads to both sides of the end).
    assert_eq!(sim.count_material(CellRect::new(0, 64, 128, GROUND), sand), total, "all sand is on the ground");
    assert_eq!(sim.count_material(CellRect::new(0, 40, 128, 64), sand), 0, "nothing is left on the belts");
    assert!(sim.count_material(CellRect::new(belt_end, 64, 128, GROUND), sand) > 0);
    assert_eq!(sim.count_material(CellRect::new(24, 8, 32, 40), sand), 0, "the funnel is empty");
    assert_eq!(f.building_view(hopper).unwrap().status, Status::NoInput);
}

#[test]
fn a_flipped_belt_moves_sand_left() {
    let c = content();
    let mut sim = world(&c, Some(GROUND));
    let mut f = Factory::new(c.clone());
    let sand = mat(&c, "sand");
    let belt = kind(&c, "test_belt");
    for x in 6..=8 {
        f.place(belt, TilePos::new(x, 13), 0, true, &mut sim).unwrap();
    }
    // A small pile on the right belt.
    fill(&mut sim, CellRect::new(66, 100, 70, 104), sand, None);
    let total = count_all(&sim, sand);
    run(&mut f, &mut sim, 240);
    assert_eq!(count_all(&sim, sand), total);
    // The sand fell off the left end (x < 48).
    assert_eq!(sim.count_material(CellRect::new(0, 0, 48, GROUND), sand), total);
    // With nothing on them, the belts go to sleep.
    run(&mut f, &mut sim, 60);
    assert_eq!(f.buildings.awake_count(), 0);
}

#[test]
fn a_hopper_filter_keeps_other_powder_out() {
    let c = content();
    let mut sim = world(&c, Some(GROUND));
    let mut f = Factory::new(c.clone());
    let (sand, gravel) = (mat(&c, "sand"), mat(&c, "gravel"));
    let hopper = f.place(kind(&c, "test_hopper"), TilePos::new(6, 13), 0, false, &mut sim).unwrap();
    assert!(f.buildings.set_hopper_filter(hopper, Some(sand)));
    // Gravel on the hopper; it stays on top.
    fill(&mut sim, CellRect::new(48, 100, 56, 104), gravel, None);
    run(&mut f, &mut sim, 120);
    assert_eq!(count_all(&sim, gravel), 32, "the gravel stays outside");
    let v = f.building_view(hopper).unwrap();
    assert!(v.inputs.is_empty());
    assert_eq!(v.reason, "Empty");
}
