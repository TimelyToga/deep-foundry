//! Running production lines for the offscreen screenshots: the Tier 0 smelting site and a Tier 1
//! automated line. Each setup makes a flat site right of the robot, places real buildings, runs
//! the normal tick paths until the line works. The picture shows it in the alt mode (product
//! icons on the machines). Returns a cell for the mouse (the hover box).

use crate::factory_host::FactoryHost;
use foundry_content::{Content, ItemRef};
use foundry_core::{BuildingId, CellPos, MaterialId, TILE_SIZE, TechId, TilePos};
use foundry_factory::steam::SteamState;
use foundry_sim::Simulation;

/// Make tiles `x0..x0 + w` flat with the ground top at tile row `row`: air above (12 tile rows),
/// dirt in the 2 rows below. Returns the tile row where buildings stand.
fn flat(sim: &mut Simulation, content: &Content, x0: i32, w: i32, row: i32) -> i32 {
    let dirt = content.expect_material("dirt");
    for x in x0 * TILE_SIZE..(x0 + w) * TILE_SIZE {
        for y in (row - 12) * TILE_SIZE..row * TILE_SIZE {
            sim.set_cell(CellPos::new(x, y), MaterialId::AIR, None);
        }
        for y in row * TILE_SIZE..(row + 2) * TILE_SIZE {
            sim.set_cell(CellPos::new(x, y), dirt, None);
        }
    }
    row - 1
}

fn place(host: &mut FactoryHost, sim: &mut Simulation, content: &Content, id: &str, at: TilePos) -> BuildingId {
    let kind = content.factory.building(id).unwrap_or_else(|| panic!("no building {id}"));
    host.factory.place(kind, at, 0, false, sim).unwrap_or_else(|e| panic!("place {id} at {at:?}: {e}"))
}

fn recipe(host: &mut FactoryHost, content: &Content, id: BuildingId, r: &str) {
    host.factory.set_recipe(id, content.factory.recipe(r)).unwrap_or_else(|e| panic!("recipe {r}: {e}"));
}

fn item(content: &Content, id: &str) -> ItemRef {
    content.item(id).unwrap_or_else(|| panic!("no item {id}"))
}

/// The site: from 3 tiles right of the robot, `w` tiles wide, on the ground there.
fn site(host: &FactoryHost, sim: &mut Simulation, content: &Content, w: i32) -> (i32, i32) {
    let x0 = host.robot.rect().x1.div_euclid(TILE_SIZE) + 3;
    let row = crate::factory_host::ground_top(sim, content, (x0 + w / 2) * TILE_SIZE).div_euclid(TILE_SIZE);
    (x0, flat(sim, content, x0 - 1, w + 2, row))
}

/// The Tier 0 smelting site: bellows, a campfire with the crucible on it, a plate mold under the
/// tap and a crate. The crucible smelts copper.
pub fn smelter(host: &mut FactoryHost, sim: &mut Simulation, content: &Content) -> CellPos {
    for i in 0..content.factory.techs.len() {
        host.factory.progress.debug_complete(content, TechId(i as u16));
    }
    let (x, b) = site(host, sim, content, 8);
    place(host, sim, content, "bellows", TilePos::new(x, b));
    let fire = place(host, sim, content, "campfire", TilePos::new(x + 3, b));
    let crucible = place(host, sim, content, "crucible", TilePos::new(x + 3, b - 1));
    let mold = place(host, sim, content, "plate_mold", TilePos::new(x + 5, b));
    let crate_ = place(host, sim, content, "crate", TilePos::new(x + 6, b));
    recipe(host, content, crucible, "copper_smelting");
    recipe(host, content, mold, "copper_plate");
    host.factory.buildings.insert(content, fire, item(content, "wood"), 100);
    host.factory.buildings.insert(content, crate_, item(content, "copper_plate"), 3);
    for _ in 0..4 {
        host.factory.buildings.insert(content, crucible, item(content, "raw_malachite"), 32);
        host.factory.buildings.insert(content, crucible, item(content, "charcoal"), 8);
        for _ in 0..1500 {
            sim.tick();
            host.tick(sim);
        }
    }
    TilePos::new(x + 5, b).origin().offset(4, 2)
}

/// Fill the steam tank of a machine (the screenshot has no boiler).
fn steam(host: &mut FactoryHost, content: &Content, id: BuildingId) {
    let s = content.expect_material("steam");
    if let Some(b) = host.factory.buildings.get_mut(id)
        && let SteamState::Machine(t) = &mut b.steam
    {
        t.add(s, 200.0);
    }
    host.factory.buildings.wake(id);
}

/// A Tier 1 line with no player action: a steam drill over a malachite vein → crate → arm →
/// steam furnace (charcoal from a hopper on it) → plate mold → crate → arm → steam assembler
/// (bronze gears from the plates of another crate) → arm → crate.
pub fn automation(host: &mut FactoryHost, sim: &mut Simulation, content: &Content) -> CellPos {
    for i in 0..content.factory.techs.len() {
        host.factory.progress.debug_complete(content, TechId(i as u16));
    }
    let (x, b) = site(host, sim, content, 16);
    // A malachite vein under the drill.
    let vein = content.expect_material("malachite");
    for cy in (b + 1) * TILE_SIZE..(b + 6) * TILE_SIZE {
        for cx in x * TILE_SIZE..(x + 2) * TILE_SIZE {
            sim.set_cell(CellPos::new(cx, cy), vein, None);
        }
    }
    let drill = place(host, sim, content, "steam_drill", TilePos::new(x, b - 1));
    place(host, sim, content, "crate", TilePos::new(x + 2, b));
    place(host, sim, content, "arm", TilePos::new(x + 3, b));
    let furnace = place(host, sim, content, "steam_furnace", TilePos::new(x + 4, b - 1));
    let hopper = place(host, sim, content, "hopper", TilePos::new(x + 4, b - 2));
    let mold = place(host, sim, content, "plate_mold", TilePos::new(x + 6, b));
    place(host, sim, content, "crate", TilePos::new(x + 7, b));
    // The gear line: plates in a crate → arm → assembler → arm → crate.
    let plates = place(host, sim, content, "crate", TilePos::new(x + 9, b));
    place(host, sim, content, "arm", TilePos::new(x + 10, b));
    let assembler = place(host, sim, content, "steam_assembler", TilePos::new(x + 11, b - 1));
    place(host, sim, content, "arm", TilePos::new(x + 13, b));
    place(host, sim, content, "crate", TilePos::new(x + 14, b));
    recipe(host, content, furnace, "copper_smelting");
    recipe(host, content, mold, "copper_plate");
    recipe(host, content, assembler, "bronze_gear");
    host.factory.buildings.insert(content, hopper, item(content, "charcoal"), 64);
    host.factory.buildings.insert(content, plates, item(content, "bronze_plate"), 40);
    for k in 0..60 * 40 {
        if k % 300 == 0 {
            for id in [drill, furnace, assembler] {
                steam(host, content, id);
            }
        }
        sim.tick();
        host.tick(sim);
    }
    for id in [drill, furnace, assembler] {
        steam(host, content, id);
    }
    TilePos::new(x + 4, b - 1).origin().offset(8, 8)
}
