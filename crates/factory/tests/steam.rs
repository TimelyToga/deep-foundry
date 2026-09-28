//! The tier-one boiler feeds a crusher through placed bronze pipe sections.

mod common;

use common::*;
use foundry_content::ItemRef;
use foundry_core::{CellPos, TilePos};
use foundry_factory::steam::{FluidTank, SteamState};
use foundry_factory::{Click, Factory, RobotSlot};

#[test]
fn boiler_heats_water_and_runs_a_crusher_through_bronze_pipes() {
    let c = content();
    let mut sim = world(&c, None);
    let mut f = Factory::new(c.clone());
    research_all(&mut f);

    let boiler = f
        .place(
            kind(&c, "small_boiler"),
            TilePos::new(3, 10),
            0,
            false,
            &mut sim,
        )
        .unwrap();
    let pipe_tiles = [
        TilePos::new(4, 10),
        TilePos::new(5, 10),
        TilePos::new(6, 10),
        TilePos::new(7, 10),
        TilePos::new(7, 11),
        TilePos::new(8, 11),
        TilePos::new(3, 11),
        TilePos::new(2, 11),
    ];
    for tile in pipe_tiles.into_iter().rev() {
        f.place(kind(&c, "bronze_pipe"), tile, 0, false, &mut sim)
            .unwrap();
    }
    let crusher = f
        .place(
            kind(&c, "steam_crusher"),
            TilePos::new(8, 10),
            0,
            false,
            &mut sim,
        )
        .unwrap();
    f.set_recipe(crusher, c.factory.recipe("crushed_magnetite"))
        .unwrap();

    // Fuel and cold water are taken from the boiler's own intake ports.
    let charcoal = mat(&c, "charcoal");
    let water = mat(&c, "water");
    let ore = mat(&c, "raw_magnetite");
    for y in 76..80 {
        for x in 24..32 {
            sim.set_cell(CellPos::new(x, y), charcoal, None);
        }
    }
    for y in 88..96 {
        for x in 0..16 {
            sim.set_cell(CellPos::new(x, y), water, None);
        }
    }
    for y in 76..80 {
        for x in 64..72 {
            sim.set_cell(CellPos::new(x, y), ore, None);
        }
    }

    run(&mut f, &mut sim, 1);
    let b = f.buildings.get(boiler).unwrap();
    assert!(b.temperature < 100, "the boiler starts cold");
    assert!(f.building_view(boiler).unwrap().reason.contains("Heating water"));
    assert!(
        matches!(b.steam, SteamState::Boiler { steam: 0.0, .. }),
        "cold water must not make steam"
    );

    run(&mut f, &mut sim, 1000);
    let boiler_view = f.building_view(boiler).unwrap();
    let crusher_view = f.building_view(crusher).unwrap();
    assert!(
        boiler_view.temperature >= 100,
        "boiler body heats from burning fuel: {boiler_view:?}"
    );
    assert!(
        count_all(&sim, mat(&c, "crushed_magnetite")) > 0,
        "the steam crusher completes its recipe"
    );
    assert!(
        crusher_view.reason != "No power",
        "the stop reason names missing steam when the network is dry"
    );
}

#[test]
fn steam_lab_stops_without_steam_then_researches_and_iron_belt_moves_cells() {
    let c = content();
    let mut sim = world(&c, Some(112));
    let mut f = Factory::new(c.clone());
    let steam_power = c.factory.tech("steam_power").unwrap();
    let steam_machines = c.factory.tech("steam_machines_1").unwrap();
    f.progress.debug_unlock_tier(1);
    f.progress.debug_complete(&c, steam_power);
    f.progress.start_research(&c, steam_machines).unwrap();
    let lab = f
        .place(
            kind(&c, "steam_lab"),
            TilePos::new(5, 10),
            0,
            false,
            &mut sim,
        )
        .unwrap();
    let pipe = f
        .place(
            kind(&c, "bronze_pipe"),
            TilePos::new(6, 10),
            0,
            false,
            &mut sim,
        )
        .unwrap();
    assert_eq!(f.buildings.insert(&c, lab, item(&c, "bronze_kit"), 10), 10);
    run(&mut f, &mut sim, 5);
    let stopped = f.building_view(lab).unwrap();
    assert_eq!(stopped.status, foundry_factory::Status::NoPower);
    assert!(stopped.reason.contains("Needs steam"));
    assert_eq!(f.progress.research_status(&c).unwrap().progress, 0.0);

    f.buildings.get_mut(pipe).unwrap().steam = SteamState::Pipe(FluidTank {
        material: Some(mat(&c, "steam")),
        amount: 10.0,
        capacity: 200.0,
    });
    f.buildings.wake(lab);
    run(&mut f, &mut sim, 60);
    assert!(f.progress.research_status(&c).unwrap().progress > 0.0);

    let iron_belt = kind(&c, "iron_belt");
    for x in 8..=10 {
        f.place(iron_belt, TilePos::new(x, 13), 0, false, &mut sim)
            .unwrap();
    }
    let sand = mat(&c, "sand");
    fill(
        &mut sim,
        foundry_core::CellRect::new(66, 100, 70, 104),
        sand,
        None,
    );
    let total = count_all(&sim, sand);
    run(&mut f, &mut sim, 240);
    assert!(
        sim.count_material(foundry_core::CellRect::new(88, 64, 128, 112), sand) > 0,
        "iron belt moves a real powder cell"
    );
    assert_eq!(count_all(&sim, sand), total);
}

#[test]
fn removing_steam_buildings_returns_stored_resources_once() {
    let c = content();
    let mut sim = world(&c, None);
    let mut f = Factory::new(c.clone());
    let boiler = f
        .place(
            kind(&c, "small_boiler"),
            TilePos::new(3, 10),
            0,
            false,
            &mut sim,
        )
        .unwrap();
    f.buildings.get_mut(boiler).unwrap().steam = SteamState::Boiler {
        fuel: Some(mat(&c, "charcoal")),
        fuel_units: 2,
        burn_ticks: 30,
        fraction: 0.0,
        water: 2.8,
        steam: 3.2,
    };
    let removed = f.remove(boiler, &mut sim).unwrap();
    let count = |id: &str| {
        removed
            .iter()
            .filter(|s| s.item == item(&c, id))
            .map(|s| s.count)
            .sum::<u32>()
    };
    assert_eq!(count("charcoal"), 2);
    assert_eq!(count("water"), 2);
    assert_eq!(count("steam"), 3);
}

#[test]
fn boiler_window_transfers_fuel_water_and_steam() {
    let c = content();
    let mut sim = world(&c, None);
    let mut f = Factory::new(c.clone());
    let boiler = f
        .place(
            kind(&c, "small_boiler"),
            TilePos::new(3, 10),
            0,
            false,
            &mut sim,
        )
        .unwrap();
    let charcoal = mat(&c, "charcoal");
    let water = mat(&c, "water");
    let steam = mat(&c, "steam");
    f.player.insert(&c, ItemRef::Material(charcoal), 12);
    f.player.insert(&c, ItemRef::Material(water), 24);
    for material in [charcoal, water] {
        let tank = f
            .player
            .tanks
            .iter()
            .position(|t| t.material == Some(material))
            .unwrap();
        assert_eq!(
            f.click_robot_slot(RobotSlot::Tank(tank), Click::Left, Some(boiler)),
            Ok(true)
        );
    }
    let loaded = f.building_view(boiler).unwrap();
    assert_eq!(loaded.fuel.unwrap().units, 12);
    assert_eq!(
        loaded
            .inputs
            .iter()
            .find(|b| b.item == ItemRef::Material(water))
            .unwrap()
            .count,
        24
    );

    // Window clicks return the water buffer and fuel to robot tanks.
    assert_eq!(
        f.buildings
            .take_input(&c, boiler, ItemRef::Material(water), 7),
        7
    );
    assert_eq!(f.fuel_to_robot(boiler, Click::Right), Ok(6));
    assert_eq!(
        f.buildings.insert(&c, boiler, ItemRef::Material(water), 7),
        7
    );
    f.buildings.get_mut(boiler).unwrap().steam = SteamState::Boiler {
        fuel: Some(charcoal),
        fuel_units: 6,
        burn_ticks: 0,
        fraction: 0.0,
        water: 24.0,
        steam: 5.0,
    };
    assert_eq!(
        f.buildings
            .take_output(&c, boiler, ItemRef::Material(steam), 2),
        2
    );
    assert_eq!(f.building_view(boiler).unwrap().outputs[0].count, 3);
}

#[test]
fn old_saves_initialize_new_steam_states() {
    let c = content();
    let mut sim = world(&c, None);
    let mut f = Factory::new(c.clone());
    let boiler = f
        .place(
            kind(&c, "small_boiler"),
            TilePos::new(3, 10),
            0,
            false,
            &mut sim,
        )
        .unwrap();
    f.buildings.get_mut(boiler).unwrap().steam = SteamState::None;
    let save = f.save();
    f.load(save);
    assert!(matches!(
        f.buildings.get(boiler).unwrap().steam,
        SteamState::Boiler { .. }
    ));
}
