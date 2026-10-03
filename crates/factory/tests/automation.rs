//! Tier 1 automation: arms, the steam assembler, the steam furnace and the steam drill in full
//! production lines that run with no player action.

mod common;

use common::*;
use foundry_core::{BuildingId, CellPos, CellRect, TilePos};
use foundry_factory::Factory;
use foundry_factory::steam::SteamState;

/// Fill the steam tank of a machine, as a boiler and pipes would.
fn give_steam(f: &mut Factory, id: BuildingId) {
    let steam = f.content.expect_material("steam");
    if let Some(b) = f.buildings.get_mut(id)
        && let SteamState::Machine(t) = &mut b.steam
    {
        t.add(steam, 200.0);
    }
    f.buildings.wake(id);
}

/// Run `n` ticks and keep the machines in `steam` supplied.
fn run_with_steam(f: &mut Factory, sim: &mut foundry_sim::Simulation, steam: &[BuildingId], n: usize) {
    for k in 0..n {
        if k % 600 == 0 {
            for &id in steam {
                give_steam(f, id);
            }
        }
        f.tick(sim);
        sim.tick();
    }
}

/// crate → arm → steam assembler → arm → crate: bronze plates in, bronze gears out.
#[test]
fn arms_feed_an_assembler_that_makes_gears() {
    let c = content();
    let mut sim = world(&c, Some(96));
    let mut f = Factory::new(c.clone());
    research_all(&mut f);
    let input = f.place(kind(&c, "crate"), TilePos::new(3, 11), 0, false, &mut sim).unwrap();
    f.place(kind(&c, "arm"), TilePos::new(4, 11), 0, false, &mut sim).unwrap();
    let assembler = f.place(kind(&c, "steam_assembler"), TilePos::new(5, 10), 0, false, &mut sim).unwrap();
    f.place(kind(&c, "arm"), TilePos::new(7, 11), 0, false, &mut sim).unwrap();
    let output = f.place(kind(&c, "crate"), TilePos::new(8, 11), 0, false, &mut sim).unwrap();
    f.set_recipe(assembler, c.factory.recipe("bronze_gear")).unwrap();
    assert_eq!(f.buildings.insert(&c, input, item(&c, "bronze_plate"), 20), 20);
    run_with_steam(&mut f, &mut sim, &[assembler], 60 * 40);
    let gears = f.buildings.inventory(output).unwrap().count(item(&c, "bronze_gear"));
    assert_eq!(gears, 10, "assembler: {:?}", f.building_view(assembler));
    assert_eq!(f.buildings.inventory(input).unwrap().count(item(&c, "bronze_plate")), 0);
}

/// An arm with no steam use still needs a building on both sides, and says so.
#[test]
fn an_arm_alone_says_what_it_needs() {
    let c = content();
    let mut sim = world(&c, Some(96));
    let mut f = Factory::new(c.clone());
    research_all(&mut f);
    let arm = f.place(kind(&c, "arm"), TilePos::new(4, 11), 0, false, &mut sim).unwrap();
    run(&mut f, &mut sim, 30);
    assert_eq!(f.building_view(arm).unwrap().reason, "Needs a building on both sides");
}

/// A full copper line with no player action:
/// steam drill over a malachite vein → crate → arm → steam furnace (charcoal from a hopper on
/// it) → its tap pours into a plate mold → the mold puts the plates into a crate.
#[test]
fn a_drill_furnace_and_mold_make_copper_plates_on_their_own() {
    let c = content();
    let mut sim = world(&c, Some(96));
    let mut f = Factory::new(c.clone());
    research_all(&mut f);
    // A malachite vein under the drill.
    fill(&mut sim, CellRect::new(16, 96, 32, 140), mat(&c, "malachite"), None);
    let drill = f.place(kind(&c, "steam_drill"), TilePos::new(2, 10), 0, false, &mut sim).unwrap();
    let ore_crate = f.place(kind(&c, "crate"), TilePos::new(4, 11), 0, false, &mut sim).unwrap();
    f.place(kind(&c, "arm"), TilePos::new(5, 11), 0, false, &mut sim).unwrap();
    let furnace = f.place(kind(&c, "steam_furnace"), TilePos::new(6, 10), 0, false, &mut sim).unwrap();
    let hopper = f.place(kind(&c, "hopper"), TilePos::new(6, 9), 0, false, &mut sim).unwrap();
    let mold = f.place(kind(&c, "plate_mold"), TilePos::new(8, 11), 0, false, &mut sim).unwrap();
    let plates = f.place(kind(&c, "crate"), TilePos::new(9, 11), 0, false, &mut sim).unwrap();
    f.set_recipe(furnace, c.factory.recipe("copper_smelting")).unwrap();
    f.set_recipe(mold, c.factory.recipe("copper_plate")).unwrap();
    assert_eq!(f.buildings.insert(&c, hopper, item(&c, "charcoal"), 40), 40);
    run_with_steam(&mut f, &mut sim, &[drill, furnace], 60 * 90);
    let made = f.buildings.inventory(plates).unwrap().count(item(&c, "copper_plate"));
    assert!(
        made >= 5,
        "{made} plates; drill {:?}; ore crate {:?}; furnace {:?}; mold {:?}",
        f.building_view(drill),
        f.buildings.inventory(ore_crate).map(|i| i.contents()),
        f.building_view(furnace),
        f.building_view(mold)
    );
    // The drill dug the vein under it: raw malachite, no vein cells moved by hand.
    assert!(count_all(&sim, mat(&c, "malachite")) < 16 * 44);
}

/// The drill does not dig what is too hard for it (bedrock), and says when nothing is left.
#[test]
fn a_drill_stops_when_nothing_is_left_in_reach() {
    let c = content();
    let mut sim = world(&c, Some(96));
    let mut f = Factory::new(c.clone());
    research_all(&mut f);
    // Under the drill: 2 rows of dirt, then bedrock.
    fill(&mut sim, CellRect::new(16, 96, 32, 98), mat(&c, "dirt"), None);
    fill(&mut sim, CellRect::new(16, 98, 32, 160), mat(&c, "bedrock"), None);
    let drill = f.place(kind(&c, "steam_drill"), TilePos::new(2, 10), 0, false, &mut sim).unwrap();
    let out = f.place(kind(&c, "crate"), TilePos::new(4, 11), 0, false, &mut sim).unwrap();
    run_with_steam(&mut f, &mut sim, &[drill], 600);
    // Dirt is waste: the drill throws it out to the left, not into the ore crate.
    run(&mut f, &mut sim, 240);
    assert_eq!(f.buildings.inventory(out).unwrap().count(item(&c, "dirt")), 0);
    assert_eq!(sim.count_material(CellRect::new(-40, 0, 16, 96), mat(&c, "dirt")), 32);
    assert!(f.building_view(drill).unwrap().reason.contains("Nothing left to dig"), "{:?}", f.building_view(drill));
}

/// Dirt over a malachite vein: the ore goes into the crate on the right, the dirt out of the left
/// port onto the ground.
#[test]
fn a_drill_sorts_ore_from_waste() {
    let c = content();
    let mut sim = world(&c, Some(96));
    let mut f = Factory::new(c.clone());
    research_all(&mut f);
    fill(&mut sim, CellRect::new(16, 96, 32, 104), mat(&c, "dirt"), None);
    fill(&mut sim, CellRect::new(16, 104, 32, 112), mat(&c, "malachite"), None);
    fill(&mut sim, CellRect::new(16, 112, 32, 160), mat(&c, "bedrock"), None);
    let drill = f.place(kind(&c, "steam_drill"), TilePos::new(2, 10), 0, false, &mut sim).unwrap();
    let out = f.place(kind(&c, "crate"), TilePos::new(4, 11), 0, false, &mut sim).unwrap();
    run_with_steam(&mut f, &mut sim, &[drill], 1800);
    let inv = f.buildings.inventory(out).unwrap();
    assert_eq!(inv.count(item(&c, "raw_malachite")), 128, "{:?}", inv.contents());
    assert_eq!(inv.count(item(&c, "dirt")), 0);
    run(&mut f, &mut sim, 240);
    assert_eq!(sim.count_material(CellRect::new(-40, 0, 16, 96), mat(&c, "dirt")), 128);
}

/// An arm takes coal from a crate and puts it into a boiler's fuel.
#[test]
fn an_arm_fuels_a_boiler() {
    let c = content();
    let mut sim = world(&c, Some(96));
    let mut f = Factory::new(c.clone());
    research_all(&mut f);
    let fuel = f.place(kind(&c, "crate"), TilePos::new(3, 11), 0, false, &mut sim).unwrap();
    f.place(kind(&c, "arm"), TilePos::new(4, 11), 0, false, &mut sim).unwrap();
    let boiler = f.place(kind(&c, "small_boiler"), TilePos::new(5, 10), 0, false, &mut sim).unwrap();
    assert_eq!(f.buildings.insert(&c, fuel, item(&c, "charcoal"), 100), 100);
    run(&mut f, &mut sim, 120);
    let units = f.building_view(boiler).unwrap().fuel.map_or(0, |x| x.units);
    assert!(units >= 16, "the boiler got {units} fuel");
    let _ = CellPos::new(0, 0);
}

/// barrel of molten pig iron → arm → steel converter (steam) → its tap pours down into a plate
/// mold → the mold puts steel plates into a crate. The slag comes out of the side port.
#[test]
fn a_steel_converter_line_makes_steel_plates() {
    let c = content();
    let mut sim = world(&c, Some(96));
    let mut f = Factory::new(c.clone());
    research_all(&mut f);
    let barrel = f.place(kind(&c, "barrel"), TilePos::new(4, 8), 0, false, &mut sim).unwrap();
    f.place(kind(&c, "arm"), TilePos::new(5, 8), 0, false, &mut sim).unwrap();
    let converter = f.place(kind(&c, "steel_converter"), TilePos::new(6, 8), 0, false, &mut sim).unwrap();
    let mold = f.place(kind(&c, "plate_mold"), TilePos::new(7, 11), 0, false, &mut sim).unwrap();
    let plates = f.place(kind(&c, "crate"), TilePos::new(8, 11), 0, false, &mut sim).unwrap();
    let slag = f.place(kind(&c, "crate"), TilePos::new(8, 9), 0, false, &mut sim).unwrap();
    f.set_recipe(converter, c.factory.recipe("steel_converting")).unwrap();
    f.set_recipe(mold, c.factory.recipe("steel_plate")).unwrap();
    assert_eq!(f.buildings.insert(&c, barrel, item(&c, "molten_pig_iron"), 128), 128);
    run_with_steam(&mut f, &mut sim, &[converter], 60 * 120);
    // 2 crafts: 120 units of steel, 7 plates (8 units wait in the mold).
    let made = f.buildings.inventory(plates).unwrap().count(item(&c, "steel_plate"));
    assert!(made == 7, "{made} plates; converter {:?}; mold {:?}", f.building_view(converter), f.building_view(mold));
    assert_eq!(f.buildings.inventory(slag).unwrap().count(item(&c, "crushed_slag")), 8);
}

/// Sand and ash from a hopper → steam furnace → its tap pours molten glass into a vial mold.
#[test]
fn a_steam_furnace_makes_glass_vials() {
    let c = content();
    let mut sim = world(&c, Some(96));
    let mut f = Factory::new(c.clone());
    research_all(&mut f);
    let furnace = f.place(kind(&c, "steam_furnace"), TilePos::new(6, 10), 0, false, &mut sim).unwrap();
    let hopper = f.place(kind(&c, "hopper"), TilePos::new(6, 9), 0, false, &mut sim).unwrap();
    let mold = f.place(kind(&c, "vial_mold"), TilePos::new(8, 11), 0, false, &mut sim).unwrap();
    let vials = f.place(kind(&c, "crate"), TilePos::new(9, 11), 0, false, &mut sim).unwrap();
    f.set_recipe(furnace, c.factory.recipe("molten_glass")).unwrap();
    f.set_recipe(mold, c.factory.recipe("glass_vial")).unwrap();
    f.buildings.insert(&c, hopper, item(&c, "sand"), 32);
    f.buildings.insert(&c, hopper, item(&c, "ash"), 8);
    run_with_steam(&mut f, &mut sim, &[furnace], 60 * 40);
    let made = f.buildings.inventory(vials).unwrap().count(item(&c, "glass_vial"));
    assert_eq!(made, 4, "furnace {:?}; mold {:?}", f.building_view(furnace), f.building_view(mold));
}

/// Rubber: steam extractor (rubber tree wood) → barrel under its outlet → arm → steam furnace
/// (Rubber) → arm → steam press → crate of rubber sheets.
#[test]
fn a_rubber_line_makes_rubber_sheets() {
    let c = content();
    let mut sim = world(&c, Some(96));
    let mut f = Factory::new(c.clone());
    research_all(&mut f);
    let extractor = f.place(kind(&c, "steam_extractor"), TilePos::new(2, 9), 0, false, &mut sim).unwrap();
    let barrel = f.place(kind(&c, "barrel"), TilePos::new(3, 11), 0, false, &mut sim).unwrap();
    f.place(kind(&c, "arm"), TilePos::new(4, 11), 0, false, &mut sim).unwrap();
    let furnace = f.place(kind(&c, "steam_furnace"), TilePos::new(5, 10), 0, false, &mut sim).unwrap();
    f.place(kind(&c, "arm"), TilePos::new(7, 10), 0, false, &mut sim).unwrap();
    // The press stands on the crate: its part output (bottom right, down) fills it.
    let sheets = f.place(kind(&c, "crate"), TilePos::new(9, 11), 0, false, &mut sim).unwrap();
    let press = f.place(kind(&c, "steam_press"), TilePos::new(8, 9), 0, false, &mut sim).unwrap();
    f.set_recipe(extractor, c.factory.recipe("resin")).unwrap();
    f.set_recipe(furnace, c.factory.recipe("rubber")).unwrap();
    f.set_recipe(press, c.factory.recipe("rubber_sheet")).unwrap();
    assert_eq!(f.buildings.insert(&c, extractor, item(&c, "rubber_wood"), 32), 32);
    let mut done = 0;
    for _ in 0..8 {
        f.buildings.insert(&c, extractor, item(&c, "rubber_wood"), 32);
        run_with_steam(&mut f, &mut sim, &[extractor, furnace, press], 600);
        done = f.buildings.inventory(sheets).unwrap().count(item(&c, "rubber_sheet"));
    }
    assert!(
        done >= 2,
        "{done} sheets; barrel {:?}; furnace {:?}; press {:?}",
        f.buildings.inventory(barrel).map(|i| i.contents()),
        f.building_view(furnace),
        f.building_view(press)
    );
}
