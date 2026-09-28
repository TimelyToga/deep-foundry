//! A real, running tier-one steam line used by the offscreen screenshot fixture.

use crate::factory_host::{FactoryCommand, FactoryHost};
use foundry_content::{Content, Layer};
use foundry_core::{CellPos, TechId, TilePos};
use foundry_sim::Simulation;

pub fn setup(host: &mut FactoryHost, sim: &mut Simulation, content: &Content) -> CellPos {
    for i in 0..content.factory.techs.len() {
        host.factory
            .progress
            .debug_complete(content, TechId(i as u16));
    }
    let crusher_kind = content
        .factory
        .building("steam_crusher")
        .expect("steam crusher data");
    let boiler_kind = content
        .factory
        .building("small_boiler")
        .expect("boiler data");
    let pipe_kind = content.factory.building("bronze_pipe").expect("pipe data");
    let crusher_at = host
        .free_place(crusher_kind, sim)
        .expect("a nearby place for the steam crusher");
    make_flat_site(sim, content, crusher_at);
    let boiler_at = (7..=12)
        .map(|dx| TilePos::new(crusher_at.x + dx, crusher_at.y))
        .find(|at| {
            host.factory
                .can_place(boiler_kind, *at, 0, false, sim)
                .is_ok()
        })
        .expect("a spot for the boiler next to the crusher");

    place(host, sim, "steam_crusher", crusher_kind, crusher_at);
    place(host, sim, "small_boiler", boiler_kind, boiler_at);
    let pipes = [
        TilePos::new(crusher_at.x, crusher_at.y + 1),
        TilePos::new(crusher_at.x + 1, crusher_at.y + 1),
        TilePos::new(crusher_at.x + 2, crusher_at.y + 1),
        TilePos::new(crusher_at.x + 3, crusher_at.y + 1),
        TilePos::new(crusher_at.x + 4, crusher_at.y + 1),
        TilePos::new(boiler_at.x - 2, boiler_at.y + 1),
        TilePos::new(boiler_at.x - 2, boiler_at.y),
        TilePos::new(boiler_at.x - 2, boiler_at.y - 1),
        TilePos::new(boiler_at.x - 1, boiler_at.y - 1),
        TilePos::new(boiler_at.x, boiler_at.y - 1),
        TilePos::new(boiler_at.x + 1, boiler_at.y - 1),
        TilePos::new(boiler_at.x + 1, boiler_at.y),
        TilePos::new(boiler_at.x, boiler_at.y + 1),
    ];
    for at in pipes {
        place(host, sim, "bronze_pipe", pipe_kind, at);
    }

    let crusher = host
        .factory
        .buildings
        .at_tile(crusher_at, Layer::Front)
        .expect("placed crusher");
    let recipe = content
        .factory
        .recipe("crushed_magnetite")
        .expect("crusher recipe");
    host.factory
        .set_recipe(crusher, Some(recipe))
        .expect("known crusher recipe");
    add_world_cells(sim, content, boiler_at, crusher_at);

    let output_area = foundry_core::CellRect::new(
        crusher_at.x * 8 - 32,
        crusher_at.y * 8 - 16,
        boiler_at.x * 8 + 48,
        boiler_at.y * 8 + 48,
    );
    let result = content.expect_material("crushed_magnetite");
    // Run the normal cell and factory tick paths until the crusher has produced ore and is working.
    for _ in 0..1600 {
        sim.tick();
        host.tick(sim);
        let running = host
            .factory
            .building_view(crusher)
            .is_some_and(|v| v.status == foundry_factory::Status::Working);
        if running && sim.count_material(output_area, result) > 0 {
            break;
        }
    }
    let view = host.factory.building_view(crusher).expect("crusher window");
    assert!(
        sim.count_material(output_area, result) > 0,
        "the screenshot steam line must produce crushed ore: {view:?}"
    );
    assert!(
        view.inputs.iter().any(|b| b.name == "Steam" && b.count > 0),
        "the crusher must show steam from its connected pipes: {view:?}"
    );
    host.apply(FactoryCommand::OpenAt(crusher_at.origin()), sim);
    CellPos::new(crusher_at.x * 8 + 8, crusher_at.y * 8 + 8)
}

fn make_flat_site(sim: &mut Simulation, content: &Content, at: TilePos) {
    let stone = content.expect_material("stone");
    let x0 = at.x * 8;
    let y0 = at.y * 8;
    for y in y0 - 8..y0 + 16 {
        for x in x0..x0 + 15 * 8 {
            sim.set_cell(CellPos::new(x, y), foundry_core::MaterialId::AIR, None);
        }
    }
    for x in x0..x0 + 15 * 8 {
        sim.set_cell(CellPos::new(x, y0 + 16), stone, None);
    }
    for y in y0 + 16..y0 + 40 {
        for x in (at.x + 1) * 8..(at.x + 2) * 8 {
            sim.set_cell(CellPos::new(x, y), foundry_core::MaterialId::AIR, None);
        }
    }
    for x in (at.x + 1) * 8..(at.x + 2) * 8 {
        sim.set_cell(CellPos::new(x, y0 + 40), stone, None);
    }
}

fn place(
    host: &mut FactoryHost,
    sim: &mut Simulation,
    part_name: &str,
    kind: foundry_core::BuildingKindId,
    at: TilePos,
) {
    host.factory
        .place(kind, at, 0, false, sim)
        .unwrap_or_else(|e| panic!("place {part_name} at {at:?}: {e}"));
}

fn add_world_cells(sim: &mut Simulation, content: &Content, boiler: TilePos, crusher: TilePos) {
    let charcoal = content.expect_material("charcoal");
    let water = content.expect_material("water");
    let ore = content.expect_material("raw_magnetite");
    let stone = content.expect_material("stone");
    let (bx, by) = (boiler.x * 8, boiler.y * 8);
    let (cx, cy) = (crusher.x * 8, crusher.y * 8);
    for y in by - 4..by {
        for x in bx..bx + 8 {
            sim.set_cell(CellPos::new(x, y), charcoal, None);
        }
    }
    for y in cy - 8..cy {
        for x in cx..cx + 8 {
            sim.set_cell(CellPos::new(x, y), ore, None);
        }
    }
    for y in by + 8..by + 16 {
        sim.set_cell(CellPos::new(bx - 5, y), stone, None);
        for x in bx - 4..bx {
            sim.set_cell(CellPos::new(x, y), water, None);
        }
    }
}
