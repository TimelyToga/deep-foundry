//! Tier 2 electric power: a steam turbine, copper cables and machines that use power.

mod common;

use common::*;
use foundry_core::{BuildingId, TilePos};
use foundry_factory::steam::SteamState;
use foundry_factory::{Factory, Status};

fn give_steam(f: &mut Factory, id: BuildingId) {
    let steam = f.content.expect_material("steam");
    if let Some(b) = f.buildings.get_mut(id)
        && let SteamState::Machine(t) = &mut b.steam
    {
        t.add(steam, 200.0);
    }
    f.buildings.wake(id);
}

/// A turbine (tiles 2-3, 10-11; power port at tile 3, 11), cables along row 11 to the right.
fn setup(cables_to: i32) -> (std::sync::Arc<foundry_content::Content>, foundry_sim::Simulation, Factory, BuildingId) {
    let c = content();
    let mut sim = world(&c, Some(96));
    let mut f = Factory::new(c.clone());
    research_all(&mut f);
    let turbine = f.place(kind(&c, "steam_turbine"), TilePos::new(2, 10), 0, false, &mut sim).unwrap();
    for x in 3..cables_to {
        f.place(kind(&c, "copper_cable"), TilePos::new(x, 11), 0, false, &mut sim).unwrap();
    }
    (c, sim, f, turbine)
}

#[test]
fn a_turbine_powers_a_macerator_through_cables() {
    let (c, mut sim, mut f, turbine) = setup(9);
    // The macerator's power port is its bottom-left tile (8, 11), on the cable.
    let mac = f.place(kind(&c, "macerator"), TilePos::new(8, 10), 0, false, &mut sim).unwrap();
    f.set_recipe(mac, c.factory.recipe("crushed_malachite")).unwrap();
    f.buildings.insert(&c, mac, item(&c, "raw_malachite"), 32);
    for _ in 0..10 {
        give_steam(&mut f, turbine);
        run(&mut f, &mut sim, 60);
    }
    let v = f.building_view(mac).unwrap();
    assert!(v.outputs[0].count > 0 || count_all(&sim, mat(&c, "crushed_malachite")) > 0, "{v:?}");
    let nets = f.buildings.power_nets();
    assert_eq!(nets.len(), 1);
    assert_eq!((nets[0].generators.len(), nets[0].consumers.len()), (1, 1));
}

#[test]
fn no_steam_no_power_and_no_cable_no_power() {
    let (c, mut sim, mut f, _turbine) = setup(9);
    let mac = f.place(kind(&c, "macerator"), TilePos::new(8, 10), 0, false, &mut sim).unwrap();
    // A second macerator with no cable behind its power port.
    let alone = f.place(kind(&c, "macerator"), TilePos::new(12, 10), 0, false, &mut sim).unwrap();
    for id in [mac, alone] {
        f.set_recipe(id, c.factory.recipe("crushed_malachite")).unwrap();
        f.buildings.insert(&c, id, item(&c, "raw_malachite"), 32);
    }
    run(&mut f, &mut sim, 120);
    for id in [mac, alone] {
        let v = f.building_view(id).unwrap();
        assert_eq!(v.status, Status::NoPower, "{v:?}");
        assert_eq!(v.outputs[0].count, 0);
    }
}

/// Two electric furnaces want more than one turbine makes: both get the same part of their power
/// and work slower.
#[test]
fn too_little_power_slows_every_machine() {
    let (c, mut sim, mut f, turbine) = setup(12);
    let a = f.place(kind(&c, "electric_furnace"), TilePos::new(5, 10), 0, false, &mut sim).unwrap();
    let b = f.place(kind(&c, "electric_furnace"), TilePos::new(8, 10), 0, false, &mut sim).unwrap();
    for id in [a, b] {
        f.set_recipe(id, c.factory.recipe("copper_smelting")).unwrap();
        f.buildings.insert(&c, id, item(&c, "raw_malachite"), 64);
        f.buildings.insert(&c, id, item(&c, "charcoal"), 16);
    }
    give_steam(&mut f, turbine);
    run(&mut f, &mut sim, 5);
    let net = f.buildings.power_nets()[0].clone();
    // Each furnace: 300 W × 16 (a tier 2 machine on a tier 0 recipe) = 4.8 kW; together 9.6 kW
    // of the turbine's 12 kW.
    assert_eq!(net.consumers.len(), 2);
    assert!((net.demand_w - 9600.0).abs() < 1.0, "{net:?}");
    assert_eq!(net.satisfaction, 1.0);
    // A third furnace: 14.4 kW wanted, 12 kW made.
    let third = f.place(kind(&c, "electric_furnace"), TilePos::new(11, 10), 0, false, &mut sim).unwrap();
    f.set_recipe(third, c.factory.recipe("copper_smelting")).unwrap();
    f.buildings.insert(&c, third, item(&c, "raw_malachite"), 64);
    f.buildings.insert(&c, third, item(&c, "charcoal"), 16);
    give_steam(&mut f, turbine);
    run(&mut f, &mut sim, 5);
    let net = f.buildings.power_nets()[0].clone();
    assert!((net.satisfaction - 12000.0 / 14400.0).abs() < 0.01, "{net:?}");
    assert!((f.buildings.get(a).unwrap().power_factor - net.satisfaction).abs() < 0.01);
}

/// A battery fills while the turbine makes more than the machines use, and powers a macerator
/// when the turbine has no steam.
#[test]
fn a_battery_stores_power_and_gives_it_back() {
    let (c, mut sim, mut f, turbine) = setup(11);
    let mac = f.place(kind(&c, "macerator"), TilePos::new(8, 10), 0, false, &mut sim).unwrap();
    // The battery's power port is its bottom tile (10, 11), on the cable.
    let battery = f.place(kind(&c, "battery"), TilePos::new(10, 10), 0, false, &mut sim).unwrap();
    // The macerator has no recipe: the turbine's power goes into the battery (5 kW).
    for _ in 0..10 {
        give_steam(&mut f, turbine);
        run(&mut f, &mut sim, 60);
    }
    let charge = f.buildings.get(battery).unwrap().charge_j;
    assert!((40_000.0..=50_001.0).contains(&charge), "{charge} J after 10 s at 5 kW");
    let net = f.buildings.power_nets()[0].clone();
    assert_eq!(net.batteries, vec![battery]);
    assert!(f.building_view(battery).unwrap().reason.starts_with("Charging"), "{:?}", f.building_view(battery));
    // No steam now; the macerator works on battery power.
    if let Some(b) = f.buildings.get_mut(turbine)
        && let SteamState::Machine(t) = &mut b.steam
    {
        t.amount = 0.0;
    }
    f.set_recipe(mac, c.factory.recipe("crushed_malachite")).unwrap();
    f.buildings.insert(&c, mac, item(&c, "raw_malachite"), 32);
    run(&mut f, &mut sim, 300);
    let v = f.building_view(mac).unwrap();
    assert_ne!(v.status, Status::NoPower, "{v:?}");
    assert!(v.outputs[0].count > 0 || count_all(&sim, mat(&c, "crushed_malachite")) > 0, "{v:?}");
    assert!(f.buildings.get(battery).unwrap().charge_j < charge, "the battery gave energy");
}
