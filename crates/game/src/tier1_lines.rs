//! Tier 1 in the real game: a steam factory set up with the player actions. The robot digs a
//! pool and sprays water into it, places the boiler, the pipes from the pool to its water port and
//! from its steam port to a steam furnace, fuels the boiler through its window, and loads the
//! furnace. The furnace pours copper into a mold, and the mold fills a crate.
//!
//! The research and the parts are given (the Tier 0 test plays how to get them).

use super::tier0_tests::Player;
use crate::factory_host::{FactoryCommand, PlayerInput};
use foundry_core::{CellPos, MaterialId, TILE_SIZE, TechId, TilePos};

impl Player {
    /// Put items into the robot's inventory (a test shortcut for parts made in Tier 0).
    fn give(&mut self, id: &str, n: u32) {
        let item = self.content.item(id).unwrap_or_else(|| panic!("no item {id}"));
        let content = self.content.clone();
        self.host.factory.player.insert(&content, item, n);
    }
}

#[test]
fn a_steam_furnace_runs_on_a_boiler_with_water_from_a_sprayed_pool() {
    let mut p = Player::new();
    let content = p.content.clone();
    for i in 0..content.factory.techs.len() {
        p.host.factory.progress.debug_complete(&content, TechId(i as u16));
    }
    for (id, n) in [("bronze_plate", 60), ("bronze_gear", 20), ("bronze_pipe_section", 10), ("clay_brick", 30), ("wood", 200)] {
        p.give(id, n);
    }
    for (id, n) in [("small_boiler", 1), ("steam_furnace", 1), ("bronze_pipe", 4), ("hopper", 1), ("plate_mold", 1), ("crate", 1)] {
        p.craft(id, n).unwrap();
    }
    p.give("charcoal", 400);
    p.give("water", 3000);
    p.give("raw_malachite", 128);
    let (x, b) = p.site().unwrap();
    // A pool 3 tiles wide and 1 tile deep in the ground left of the boiler.
    let ground = (b + 1) * TILE_SIZE;
    for cy in ground..ground + TILE_SIZE {
        for cx in (x - 3) * TILE_SIZE..x * TILE_SIZE {
            p.sim.set_cell(CellPos::new(cx, cy), MaterialId::AIR, None);
        }
    }
    p.walk_to((x - 5) * TILE_SIZE).unwrap();
    // Spray the water into the pool, as a player does (right mouse button).
    let water = content.expect_material("water");
    let aim = CellPos::new((x - 2) * TILE_SIZE, ground + 3);
    for _ in 0..200 {
        p.apply(FactoryCommand::Input(PlayerInput { aim, spray: Some(water), ..Default::default() }));
        p.ticks(2);
    }
    p.apply(FactoryCommand::Input(PlayerInput::default()));
    p.ticks(60);
    let pool = foundry_core::CellRect::new((x - 3) * TILE_SIZE, ground, x * TILE_SIZE, ground + TILE_SIZE);
    assert!(p.sim.count_material(pool, water) >= 100, "the pool has {} water", p.sim.count_material(pool, water));

    // The boiler, its water pipe from the pool and its steam pipe to the furnace.
    p.walk_to((x + 3) * TILE_SIZE + 4).unwrap();
    let boiler = p.place_at("small_boiler", TilePos::new(x, b - 1)).unwrap();
    for t in [TilePos::new(x - 2, b + 1), TilePos::new(x - 1, b + 1), TilePos::new(x, b + 1), TilePos::new(x, b)] {
        p.place_at("bronze_pipe", t).unwrap();
    }
    for t in [TilePos::new(x + 1, b - 1), TilePos::new(x + 2, b - 1), TilePos::new(x + 2, b), TilePos::new(x + 3, b)] {
        p.place_at("bronze_pipe", t).unwrap();
    }
    p.walk_to((x + 9) * TILE_SIZE).unwrap();
    // Leaves and dirt that the walk shook loose: the player digs them away.
    p.clear_area((x + 3) * TILE_SIZE, (x + 7) * TILE_SIZE, (b - 3) * TILE_SIZE, (b + 1) * TILE_SIZE);
    let furnace = p.place_at("steam_furnace", TilePos::new(x + 3, b - 1)).unwrap();
    let hopper = p.place_at("hopper", TilePos::new(x + 3, b - 2)).unwrap();
    let mold = p.place_at("plate_mold", TilePos::new(x + 5, b)).unwrap();
    let crate_ = p.place_at("crate", TilePos::new(x + 6, b)).unwrap();

    // Fuel for the boiler; recipes; ore and charcoal into the hopper on the furnace.
    p.open_id(boiler).unwrap();
    p.click_tank("charcoal");
    p.open_id(mold).unwrap();
    p.choose(mold, "copper_plate").unwrap();
    p.open_id(furnace).unwrap();
    p.choose(furnace, "copper_smelting").unwrap();
    // The hopper holds 64 cells: 16 charcoal and 48 ore (a player clicks with the right button,
    // or puts a hopper for each).
    for (id, n) in [("charcoal", 16), ("raw_malachite", 48)] {
        let it = content.item(id).unwrap();
        p.host.factory.player.remove(it, n);
        p.host.factory.buildings.insert(&content, hopper, it, n);
    }
    let plate = "copper_plate";
    for _ in 0..60 {
        if p.stored(crate_, plate) >= 3 {
            break;
        }
        p.ticks(120);
    }
    let boiler_view = p.host.factory.building_view(boiler);
    assert!(
        p.stored(crate_, plate) >= 3,
        "{} plates; boiler {:?}; furnace {:?}; hopper {:?}",
        p.stored(crate_, plate),
        boiler_view,
        p.host.factory.building_view(furnace),
        p.host.factory.building_view(hopper)
    );
}
