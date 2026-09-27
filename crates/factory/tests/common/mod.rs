//! Shared helpers for the factory integration tests.
//!
//! The content is the real content (`assets/data`) plus a few test buildings and recipes with
//! ids that start with `test_`. The real data does not have Tier 0 logistics buildings yet.

#![allow(dead_code)]

use foundry_content::factory_defs::{BuildingDef, MilestoneDef, PartDef, RecipeDef, TechDef};
use foundry_content::{Content, FactoryContent, ItemRef, default_assets_dir};
use foundry_core::{BuildingKindId, CellPos, CellRect, MaterialId, TechId, TilePos};
use foundry_factory::Factory;
use foundry_sim::{SimConfig, Simulation};
use serde::de::DeserializeOwned;
use std::path::Path;
use std::sync::{Arc, OnceLock};

const TEST_BUILDINGS: &str = r#"[
    Building(id: "test_crate", name: "Test crate", kind: "storage", size: (1, 1), tier: 0, body: "wood_block",
        params: {"slots": 8.0}),
    Building(id: "test_hopper", name: "Test hopper", kind: "hopper", size: (1, 1), tier: 0, body: "wood_block",
        ports: [(kind: BulkIn, tile: (0, 0), side: Up), (kind: BulkOut, tile: (0, 0), side: Down)],
        params: {"capacity": 64.0, "rate": 30.0}),
    Building(id: "test_belt", name: "Test belt", kind: "belt", size: (1, 1), tier: 0, body: "wood_block",
        params: {"belt_speed": 30.0}),
    Building(id: "test_press", name: "Test press", kind: "crafter", size: (2, 2), tier: 0, body: "wood_block",
        crafts: ["test_pressing"],
        ports: [(kind: BulkIn, tile: (0, 0), side: Up), (kind: BulkOut, tile: (1, 1), side: Down)]),
    Building(id: "test_press_mk2", name: "Test press Mk2", kind: "crafter", size: (2, 2), tier: 1, body: "wood_block",
        crafts: ["test_pressing"],
        power: Some((tier: 1, use_w: 100.0, idle_w: 5.0)),
        ports: [(kind: BulkIn, tile: (0, 0), side: Up), (kind: BulkOut, tile: (1, 1), side: Down)]),
    Building(id: "test_assembler", name: "Test assembler", kind: "crafter", size: (1, 1), tier: 0, body: "wood_block",
        crafts: ["test_assembling"],
        ports: [(kind: PartIn, tile: (0, 0), side: Left), (kind: PartOut, tile: (0, 0), side: Right)]),
    Building(id: "test_oven", name: "Test oven", kind: "furnace", size: (1, 1), tier: 0, body: "terracotta", max_temp: 1500,
        crafts: ["test_heating"],
        ports: [(kind: Heat, tile: (0, 0), side: Down)]),
    Building(id: "test_vent", name: "Test vent", kind: "crafter", size: (1, 1), tier: 0, body: "wood_block",
        crafts: ["test_venting"],
        ports: [(kind: Exhaust, tile: (0, 0), side: Up)]),
    Building(id: "test_hub", name: "Test hub", kind: "hub", size: (2, 2), tier: 0, body: "wood_block",
        ports: [(kind: PartIn, tile: (0, 0), side: Left)]),
    Building(id: "test_post", name: "Test post", kind: "wall", size: (1, 1), tier: 0, body: "wood_block",
        params: {"needs_floor": 1.0}),
    Building(id: "test_boiler", name: "Test boiler", kind: "crafter", size: (1, 1), tier: 0, body: "terracotta", max_temp: 1500,
        crafts: ["test_boiling"],
        ports: [(kind: FluidIn, tile: (0, 0), side: Left), (kind: FluidOut, tile: (0, 0), side: Up)]),
    Building(id: "test_campfire", name: "Test campfire", kind: "crafter", size: (1, 1), tier: 0, body: "terracotta", max_temp: 1500,
        crafts: ["test_burning"],
        power: Some((tier: 0, burn_w: 20000.0)),
        ports: [(kind: Heat, tile: (0, 0), side: Up)],
        params: {"heat_temp": 800.0}),
]"#;

const TEST_RECIPES: &str = r#"[
    Recipe(id: "test_press_sand", category: "test_pressing", inputs: [("sand", 8)], outputs: [("gravel", 4)], time: 1.0),
    Recipe(id: "test_make_gear", category: "test_assembling", inputs: [("bronze_plate", 3)], outputs: [("bronze_gear", 1)],
        byproducts: [(item: "tin_plate", count: 1, chance: 0.5)], time: 0.5),
    Recipe(id: "test_bake_sand", category: "test_heating", inputs: [("sand", 4)], outputs: [("gravel", 4)], time: 1.0,
        min_temp: Some(500)),
    Recipe(id: "test_vent_gas", category: "test_venting", inputs: [("sand", 1)], outputs: [("carbon_dioxide", 4)], time: 0.1),
    Recipe(id: "test_boil", category: "test_boiling", inputs: [("water", 4)], outputs: [("steam", 4)], time: 0.2),
    Recipe(id: "test_burn_charcoal", category: "test_burning", inputs: [("charcoal", 1)], outputs: [("ash", 1)], time: 2.0),
]"#;

fn read_dir<T: DeserializeOwned>(dir: &Path) -> Vec<T> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "ron"))
        .collect();
    files.sort();
    let mut out = vec![];
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        let list: Vec<T> = ron::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        out.extend(list);
    }
    out
}

/// The real content plus the test buildings and recipes.
pub fn content() -> Arc<Content> {
    static C: OnceLock<Arc<Content>> = OnceLock::new();
    C.get_or_init(|| {
        let mut c = Content::load_default().expect("content loads");
        let data = default_assets_dir().join("data");
        let parts: Vec<PartDef> = read_dir(&data.join("parts"));
        let mut buildings: Vec<BuildingDef> = read_dir(&data.join("buildings"));
        buildings.extend(ron::from_str::<Vec<BuildingDef>>(TEST_BUILDINGS).expect("test buildings parse"));
        let mut recipes: Vec<RecipeDef> = read_dir(&data.join("recipes"));
        recipes.extend(ron::from_str::<Vec<RecipeDef>>(TEST_RECIPES).expect("test recipes parse"));
        let techs: Vec<TechDef> = read_dir(&data.join("tech"));
        let milestones: Vec<MilestoneDef> = read_dir(&data.join("milestones"));
        let mut errors = vec![];
        c.factory = FactoryContent::build(&c.materials, parts, buildings, recipes, techs, milestones, &mut errors);
        assert!(errors.is_empty(), "{errors:#?}");
        Arc::new(c)
    })
    .clone()
}

/// A 128 × 128 cell world (16 × 16 tiles) with a bedrock border and stone from row `ground_y`
/// down (use `None` for no ground).
pub fn world(content: &Arc<Content>, ground_y: Option<i32>) -> Simulation {
    let mut sim = Simulation::new(content.clone(), SimConfig::finite(2, 2, 7));
    sim.set_threads(1);
    if let Some(g) = ground_y {
        let stone = content.expect_material("stone");
        let (w, h) = sim.size_cells();
        for y in g..h - 2 {
            for x in 2..w - 2 {
                sim.set_cell(CellPos::new(x, y), stone, None);
            }
        }
    }
    sim
}

pub fn kind(content: &Content, id: &str) -> BuildingKindId {
    content.factory.building(id).unwrap_or_else(|| panic!("no building {id}"))
}

pub fn item(content: &Content, id: &str) -> ItemRef {
    content.item(id).unwrap_or_else(|| panic!("no item {id}"))
}

pub fn mat(content: &Content, id: &str) -> MaterialId {
    content.expect_material(id)
}

/// Cells of a material in the whole world.
pub fn count_all(sim: &Simulation, m: MaterialId) -> usize {
    let (w, h) = sim.size_cells();
    sim.count_material(CellRect::new(0, 0, w, h), m)
}

/// Fill a rectangle of cells with a material.
pub fn fill(sim: &mut Simulation, r: CellRect, m: MaterialId, temperature: Option<i16>) {
    for y in r.y0..r.y1 {
        for x in r.x0..r.x1 {
            sim.set_cell(CellPos::new(x, y), m, temperature);
        }
    }
}

/// The cells of a rectangle of tiles.
pub fn tile_cells(at: TilePos, w: i32, h: i32) -> CellRect {
    let o = at.origin();
    CellRect::new(o.x, o.y, o.x + w * 8, o.y + h * 8)
}

/// Finish all technologies, so the player knows all recipes.
pub fn research_all(factory: &mut Factory) {
    let c = factory.content.clone();
    for i in 0..c.factory.techs.len() {
        factory.progress.debug_complete(&c, TechId(i as u16));
    }
}

/// Run the factory and the simulation for `n` ticks.
pub fn run(factory: &mut Factory, sim: &mut Simulation, n: usize) {
    for _ in 0..n {
        factory.tick(sim);
        sim.tick();
    }
}
