//! The crucible pours molten metal through its tap into a mold: the full Tier 0 casting step in
//! the world, with no metal moved by hand.

mod common;

use common::*;
use foundry_core::TilePos;
use foundry_factory::Factory;

/// A campfire with the left half of a crucible on it, a plate mold under the tap (one tile away
/// from the fire), and a crate right of the mold. Ore and charcoal go into the crucible and wood
/// into the campfire. The molten tin runs out of the tap onto the mold, the mold casts tin plates,
/// and its part output puts them into the crate.
#[test]
fn the_crucible_pours_tin_into_a_plate_mold() {
    let c = content();
    let mut sim = world(&c, Some(96));
    let mut f = Factory::new(c.clone());
    research_all(&mut f);
    let campfire = f.place(kind(&c, "campfire"), TilePos::new(5, 11), 0, false, &mut sim).unwrap();
    let crucible = f.place(kind(&c, "crucible"), TilePos::new(5, 10), 0, false, &mut sim).unwrap();
    let mold = f.place(kind(&c, "plate_mold"), TilePos::new(7, 11), 0, false, &mut sim).unwrap();
    let crate_ = f.place(kind(&c, "crate"), TilePos::new(8, 11), 0, false, &mut sim).unwrap();
    assert_eq!(f.buildings.insert(&c, campfire, item(&c, "wood"), 100), 100);
    f.set_recipe(crucible, Some(c.factory.recipe("tin_smelting").unwrap())).unwrap();
    f.set_recipe(mold, Some(c.factory.recipe("tin_plate").unwrap())).unwrap();
    // 3 batches of 16 units: 3 plates.
    for _ in 0..3 {
        f.buildings.insert(&c, crucible, item(&c, "raw_cassiterite"), 16);
        f.buildings.insert(&c, crucible, item(&c, "charcoal"), 4);
        run(&mut f, &mut sim, 2400);
    }
    run(&mut f, &mut sim, 1200);
    let plate = item(&c, "tin_plate");
    let in_crate = f.buildings.inventory(crate_).unwrap().count(plate);
    let in_mold = f.building_view(mold).unwrap().outputs[0].count;
    let left = count_all(&sim, mat(&c, "molten_tin")) + count_all(&sim, mat(&c, "tin_block"));
    assert_eq!((in_crate, in_mold), (3, 0), "mold: {:?}; crucible: {:?}; tin cells in the world: {left}", f.building_view(mold), f.building_view(crucible));
    assert_eq!(left, 0, "all the tin went into the mold");
}

/// A hopper right on top of a stamp mill feeds it directly, and the mill's output port fills the
/// crate next to it. The hopper holds two ores; the mill crushes only malachite, so the hopper
/// gives the malachite past the cassiterite at the front of its queue.
#[test]
fn a_hopper_feeds_the_machine_under_it_past_a_refused_material() {
    let c = content();
    let mut sim = world(&c, Some(96));
    let mut f = Factory::new(c.clone());
    research_all(&mut f);
    let mill = f.place(kind(&c, "stamp_mill"), TilePos::new(5, 9), 0, false, &mut sim).unwrap();
    let hopper = f.place(kind(&c, "hopper"), TilePos::new(5, 8), 0, false, &mut sim).unwrap();
    let crate_ = f.place(kind(&c, "crate"), TilePos::new(7, 11), 0, false, &mut sim).unwrap();
    f.set_recipe(mill, Some(c.factory.recipe("crushed_malachite").unwrap())).unwrap();
    assert_eq!(f.buildings.insert(&c, hopper, item(&c, "raw_cassiterite"), 8), 8);
    assert_eq!(f.buildings.insert(&c, hopper, item(&c, "raw_malachite"), 32), 32);
    run(&mut f, &mut sim, 1500);
    let crushed = f.buildings.inventory(crate_).unwrap().count(item(&c, "crushed_malachite"));
    assert_eq!(crushed, 32, "hopper: {:?}; mill: {:?}", f.building_view(hopper), f.building_view(mill));
    // The cassiterite waits in the hopper; no ore lies in the world.
    assert_eq!(f.building_view(hopper).unwrap().inputs[0].count, 8);
    assert_eq!(count_all(&sim, mat(&c, "raw_malachite")) + count_all(&sim, mat(&c, "crushed_malachite")), 0);
}
