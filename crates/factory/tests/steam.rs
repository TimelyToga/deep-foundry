//! The tier-one boiler feeds a crusher through placed bronze pipe sections.

mod common;

use common::*;
use foundry_core::{CellPos, TilePos};
use foundry_factory::Factory;
use foundry_factory::steam::SteamState;

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
        for x in 0..24 {
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
