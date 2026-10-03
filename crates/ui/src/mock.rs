//! Test data for the preview and the screenshot tests.
//!
//! - [`content`] loads the real content from `assets/data`. The real factory data is still a
//!   small starter set, so it adds extra Tier 0-2 parts, buildings, recipes and technologies from
//!   `crates/ui/mock_data/*.ron` (only ids that the real data does not have).
//! - [`model`] invents only the state: inventory, queue, machine state, power numbers, saves.
//! - [`MockGame`] applies `UiAction`s to the model the way the game will.

use crate::action::{GameMode, SettingChange, SlotClick, SlotRef, UiAction, WindowKind};
use crate::crafting::{Stock, craftable_count};
use crate::graph::{SAMPLES, TimeSeries};
use crate::item;
use crate::model::*;
use crate::slots::{self, SlotList};
use foundry_content::factory_defs::{BuildingDef, MilestoneDef, PartDef, RecipeDef, TechDef};
use foundry_content::{Content, FactoryContent, ItemRef, Stack};
use foundry_core::{BuildingId, BuildingKindId, CellPos, RecipeId, TechId};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

const EXTRA_PARTS: &str = include_str!("../mock_data/parts.ron");
const EXTRA_BUILDINGS: &str = include_str!("../mock_data/buildings.ron");
const EXTRA_RECIPES: &str = include_str!("../mock_data/recipes.ron");
const EXTRA_TECHS: &str = include_str!("../mock_data/tech.ron");

fn read_list<T: serde::de::DeserializeOwned>(dir: &Path) -> Result<Vec<T>, String> {
    let mut files: Vec<_> = match std::fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "ron")).collect(),
        Err(_) => return Ok(vec![]),
    };
    files.sort();
    let mut out = vec![];
    for f in files {
        let text = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        let list: Vec<T> = ron::from_str(&text).map_err(|e| format!("{}: {e}", f.display()))?;
        out.extend(list);
    }
    Ok(out)
}

fn parse<T: serde::de::DeserializeOwned>(name: &str, text: &str) -> Result<Vec<T>, String> {
    ron::from_str(text).map_err(|e| format!("mock_data/{name}: {e}"))
}

/// The real content with the extra mock entries added. Errors name the file.
pub fn load_content(assets: &Path) -> Result<Content, String> {
    let mut content = Content::load(assets).map_err(|e| e.to_string())?;
    let data = assets.join("data");
    let mut parts: Vec<PartDef> = read_list(&data.join("parts"))?;
    let mut buildings: Vec<BuildingDef> = read_list(&data.join("buildings"))?;
    let mut recipes: Vec<RecipeDef> = read_list(&data.join("recipes"))?;
    let mut techs: Vec<TechDef> = read_list(&data.join("tech"))?;
    let milestones: Vec<MilestoneDef> = read_list(&data.join("milestones"))?;

    let mut ids: HashSet<String> = parts.iter().map(|p| p.id.clone()).chain(buildings.iter().map(|b| b.id.clone())).collect();
    for p in parse::<PartDef>("parts.ron", EXTRA_PARTS)? {
        if content.material(&p.id).is_none() && ids.insert(p.id.clone()) {
            parts.push(p);
        }
    }
    for b in parse::<BuildingDef>("buildings.ron", EXTRA_BUILDINGS)? {
        if content.material(&b.id).is_none() && ids.insert(b.id.clone()) {
            buildings.push(b);
        }
    }
    let mut recipe_ids: HashSet<String> = recipes.iter().map(|r| r.id.clone()).collect();
    for r in parse::<RecipeDef>("recipes.ron", EXTRA_RECIPES)? {
        if recipe_ids.insert(r.id.clone()) {
            recipes.push(r);
        }
    }
    let mut unlocked: HashSet<String> = techs.iter().flat_map(|t| t.unlocks.iter().cloned()).collect();
    let mut tech_ids: HashSet<String> = techs.iter().map(|t| t.id.clone()).collect();
    for mut t in parse::<TechDef>("tech.ron", EXTRA_TECHS)? {
        if tech_ids.insert(t.id.clone()) {
            t.unlocks.retain(|r| recipe_ids.contains(r) && unlocked.insert(r.clone()));
            t.requires.retain(|r| tech_ids.contains(r));
            techs.push(t);
        }
    }
    let mut errors = vec![];
    let fc = FactoryContent::build(&content.materials, parts, buildings, recipes, techs, milestones, &mut errors);
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    content.factory = fc;
    Ok(content)
}

/// The mock content from the default assets folder. Panics with the error text if it does not load.
pub fn content() -> Arc<Content> {
    let assets = foundry_content::default_assets_dir();
    match load_content(&assets) {
        Ok(c) => Arc::new(c),
        Err(e) => panic!("the UI mock content does not load from {}:\n{e}", assets.display()),
    }
}

/// An item by string id. Panics if it does not exist (the mock data always has it).
pub fn it(c: &Content, id: &str) -> ItemRef {
    c.item(id).unwrap_or_else(|| panic!("mock: no item `{id}`"))
}

/// A building type by string id.
pub fn bk(c: &Content, id: &str) -> BuildingKindId {
    c.factory.building(id).unwrap_or_else(|| panic!("mock: no building `{id}`"))
}

/// A recipe by string id.
pub fn rid(c: &Content, id: &str) -> RecipeId {
    c.factory.recipe(id).unwrap_or_else(|| panic!("mock: no recipe `{id}`"))
}

/// A technology by string id.
pub fn tid(c: &Content, id: &str) -> TechId {
    c.factory.tech(id).unwrap_or_else(|| panic!("mock: no tech `{id}`"))
}

fn st(c: &Content, id: &str, n: u32) -> Option<Stack> {
    Some(Stack { item: it(c, id), count: n })
}

/// A smooth, repeatable wave with some noise, for graph test data.
fn wave(i: usize, seed: u32, base: f32, amp: f32, period: f32) -> f32 {
    let x = i as f32;
    let noise = ((i as u32).wrapping_mul(2_654_435_761).wrapping_add(seed.wrapping_mul(40_503)) >> 16) as f32 / 65_536.0;
    (base + amp * (x * std::f32::consts::TAU / period + seed as f32).sin() + amp * 0.35 * (noise - 0.5)).max(0.0)
}

/// Test time series around `base` for all ranges.
pub fn series(seed: u32, base: f32, amp: f32) -> TimeSeries {
    let mut s = TimeSeries::default();
    for (r, list) in s.ranges.iter_mut().enumerate() {
        let period = [90.0, 140.0, 70.0, 110.0, 60.0][r];
        list.extend((0..SAMPLES).map(|i| wave(i, seed + r as u32 * 17, base, amp, period)));
    }
    s
}

fn sum_series(parts: &[&TimeSeries]) -> TimeSeries {
    let mut s = TimeSeries::default();
    for r in 0..5 {
        let n = parts.iter().map(|p| p.ranges[r].len()).max().unwrap_or(0);
        s.ranges[r] = (0..n).map(|i| parts.iter().map(|p| p.ranges[r].get(i).copied().unwrap_or(0.0)).sum()).collect();
    }
    s
}

fn slot(stack: Option<Stack>, filter: Option<ItemRef>) -> BuildingSlot {
    BuildingSlot { stack, filter, capacity: 0 }
}

/// A storage slot with some units of a material.
fn material_slot(c: &Content, id: &str, units: u32, capacity: u32) -> BuildingSlot {
    BuildingSlot { stack: Some(Stack { item: ItemRef::Material(c.expect_material(id)), count: units }), filter: None, capacity }
}

/// A steam assembler that makes vacuum tubes.
pub fn steam_assembler_view(c: &Content) -> BuildingView {
    BuildingView {
        id: BuildingId { index: 17, generation: 1 },
        kind: bk(c, "steam_assembler"),
        status: MachineStatus::Working,
        status_detail: String::new(),
        recipe: Some(rid(c, "vacuum_tube")),
        inputs: vec![
            slot(st(c, "glass_tube", 4), Some(it(c, "glass_tube"))),
            slot(st(c, "copper_wire", 12), Some(it(c, "copper_wire"))),
            slot(None, Some(it(c, "steel_bolt"))),
        ],
        outputs: vec![slot(st(c, "vacuum_tube", 3), None)],
        fuel: vec![],
        buffers: vec![MaterialBuffer {
            label: "Steam in".into(),
            material: Some(c.expect_material("steam")),
            units: 64,
            capacity: 200,
            output: false,
        }],
        progress: 0.45,
        speed: 1.0,
        power: None,
        temperature: Some(96.0),
        milestone: None,
        later_stages: vec![],
        room: None,
    }
}

/// A kiln that fires clay bricks: a closed room of 6 tiles, hot enough, charcoal in the fuel slot.
pub fn kiln_view(c: &Content) -> BuildingView {
    let charcoal = ItemRef::Material(c.expect_material("charcoal"));
    BuildingView {
        id: BuildingId { index: 12, generation: 1 },
        kind: bk(c, "kiln_controller"),
        status: MachineStatus::Working,
        status_detail: String::new(),
        recipe: Some(rid(c, "clay_brick")),
        inputs: vec![slot(st(c, "raw_clay_brick", 11), Some(it(c, "raw_clay_brick")))],
        outputs: vec![slot(st(c, "clay_brick", 5), Some(it(c, "clay_brick")))],
        fuel: vec![BuildingSlot { stack: Some(Stack { item: charcoal, count: 236 }), filter: Some(charcoal), capacity: 400 }],
        buffers: vec![],
        progress: 0.62,
        speed: 48.0,
        power: None,
        temperature: Some(212.0),
        milestone: None,
        later_stages: vec![],
        room: Some(RoomPanel {
            valid: true,
            problem: None,
            temperature: Some(931.0),
            needs: Some(900.0),
            tiles: 6,
            max_tiles: 24,
            hatches: 2,
            fuel_cells: 22,
            burning: 19,
            ash: 149,
            blast: 0.0,
            walls: "Clay brick wall or Firebrick wall".into(),
        }),
    }
}

/// A kiln whose room has a hole.
pub fn kiln_hole_view(c: &Content) -> BuildingView {
    let mut v = kiln_view(c);
    v.status = MachineStatus::RoomNotValid;
    v.status_detail = "The room has a hole".into();
    v.progress = 0.0;
    v.room = Some(RoomPanel {
        valid: false,
        problem: Some("The room has a hole 4 tiles right and 2 tiles up from the controller. Close it with a wall block.".into()),
        temperature: None,
        needs: Some(900.0),
        max_tiles: 24,
        walls: "Clay brick wall or Firebrick wall".into(),
        ..Default::default()
    });
    v
}

/// A small boiler with no fuel.
pub fn boiler_view(c: &Content) -> BuildingView {
    BuildingView {
        id: BuildingId { index: 4, generation: 2 },
        kind: bk(c, "small_boiler"),
        status: MachineStatus::NoFuel,
        status_detail: "Put coal or charcoal in the fuel slot.".into(),
        recipe: None,
        inputs: vec![],
        outputs: vec![],
        fuel: vec![slot(None, Some(it(c, "charcoal")))],
        buffers: vec![
            MaterialBuffer { label: "Water in".into(), material: Some(c.expect_material("water")), units: 180, capacity: 400, output: false },
            MaterialBuffer { label: "Steam out".into(), material: Some(c.expect_material("steam")), units: 12, capacity: 400, output: true },
        ],
        progress: 0.0,
        speed: 1.0,
        power: None,
        temperature: Some(212.0),
        milestone: None,
        later_stages: vec![],
        room: None,
    }
}

/// An electric furnace on a network that has too little power.
pub fn electric_furnace_view(c: &Content) -> BuildingView {
    BuildingView {
        id: BuildingId { index: 31, generation: 1 },
        kind: bk(c, "electric_furnace"),
        status: MachineStatus::LowPower,
        status_detail: "The network gives 86%.".into(),
        recipe: Some(rid(c, "smelt_cassiterite")),
        inputs: vec![],
        outputs: vec![],
        fuel: vec![],
        buffers: vec![
            MaterialBuffer { label: "Ore in".into(), material: Some(c.expect_material("crushed_cassiterite")), units: 96, capacity: 256, output: false },
            MaterialBuffer { label: "Charcoal in".into(), material: Some(c.expect_material("charcoal")), units: 20, capacity: 64, output: false },
            MaterialBuffer { label: "Tap".into(), material: Some(c.expect_material("molten_tin")), units: 140, capacity: 200, output: true },
        ],
        progress: 0.7,
        speed: 1.0,
        power: Some(PowerUse { use_w: 7740.0, max_w: 9000.0, voltage: Voltage::Lv, network_voltage: Some(Voltage::Lv), satisfaction: 0.86 }),
        temperature: Some(1140.0),
        milestone: None,
        later_stages: vec![],
        room: None,
    }
}

/// The Hub with its 16 slots and the first repair stage (from the real milestone data). It holds
/// items for the next stage and copper wire for the stage after it.
pub fn hub_view(c: &Content) -> BuildingView {
    let view = |m: &foundry_content::Milestone, delivered: &dyn Fn(usize, u32) -> u32| MilestoneView {
        stage: m.stage,
        name: m.name.clone(),
        description: m.description.clone(),
        items: m.deliver.iter().enumerate().map(|(i, s)| Delivery { item: s.item, delivered: delivered(i, s.count), need: s.count }).collect(),
    };
    // Delivered so far: all of the first item, some of the second, a few of the rest.
    let milestone = c.factory.milestones.first().map(|m| {
        view(m, &|i, need| match i {
            0 => need,
            1 => need * 3 / 5,
            _ => need * 9 / 25,
        })
    });
    let later_stages = c.factory.milestones.iter().skip(1).map(|m| view(m, &|_, _| 0)).collect();
    let mut inputs = vec![slot(st(c, "bronze_gear", 12), None), slot(st(c, "clay_brick", 36), None), slot(st(c, "copper_wire", 120), None)];
    inputs.resize(16, BuildingSlot::default());
    BuildingView {
        id: BuildingId { index: 1, generation: 1 },
        kind: bk(c, "hub"),
        status: MachineStatus::Idle,
        status_detail: "Waiting for repair parts.".into(),
        recipe: None,
        inputs,
        outputs: vec![],
        fuel: vec![],
        buffers: vec![],
        progress: 0.0,
        speed: 1.0,
        power: None,
        temperature: None,
        milestone,
        later_stages,
        room: None,
    }
}

/// A crate with bulk materials and parts: each of its 8 slots holds a part stack or up to 6,000
/// units of one material.
pub fn crate_view(c: &Content) -> BuildingView {
    let mut inputs = vec![
        material_slot(c, "clay", 6000, 6000),
        material_slot(c, "clay", 2300, 6000),
        material_slot(c, "sand", 4100, 6000),
        material_slot(c, "raw_malachite", 950, 6000),
        slot(st(c, "raw_clay_brick", 24), None),
        slot(st(c, "workbench", 1), None),
    ];
    inputs.resize(8, BuildingSlot::default());
    BuildingView {
        id: BuildingId { index: 9, generation: 1 },
        kind: bk(c, "crate"),
        status: MachineStatus::Idle,
        status_detail: String::new(),
        recipe: None,
        inputs,
        outputs: vec![],
        fuel: vec![],
        buffers: vec![],
        progress: 0.0,
        speed: 1.0,
        power: None,
        temperature: Some(21.0),
        milestone: None,
        later_stages: vec![],
        room: None,
    }
}

/// The research window entries for all technologies of the content. `finished` are done,
/// `current` is researched now. The others are available when the technologies they need are
/// done, their tier is open (tier 0 and 1) and their discoveries are made. Else they are locked,
/// with the reasons.
pub fn tech_entries(c: &Content, finished: &[TechId], current: Option<(TechId, f32)>) -> Vec<TechEntry> {
    let scanned = ["malachite", "cassiterite"];
    c.factory
        .techs
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let id = TechId(i as u16);
            let mut e = TechEntry { id, state: TechState::Available, progress: 0.0, reasons: vec![], queue_position: None, can_queue: false };
            if finished.contains(&id) {
                e.state = TechState::Done;
                e.progress = 1.0;
            } else if let Some((_, p)) = current.filter(|(t, _)| *t == id) {
                e.state = TechState::Researching;
                e.progress = p;
            } else {
                if t.tier >= 2 {
                    e.reasons.push(format!("Repair the Hub (stage {}) to open Tier {}.", t.tier, t.tier));
                }
                let missing: Vec<&str> =
                    t.requires.iter().filter(|r| !finished.contains(r)).filter_map(|r| c.factory.techs.get(r.0 as usize)).map(|r| r.name.as_str()).collect();
                if !missing.is_empty() {
                    e.reasons.push(format!("Research {} first.", missing.join(" and ")));
                }
                let scans: Vec<String> = t.discoveries.iter().filter(|d| !scanned.contains(&d.as_str())).map(|d| item::discovery_name(c, d)).collect();
                if !scans.is_empty() {
                    e.reasons.push(format!("Scan {} with F.", scans.join(" and ")));
                }
                if !e.reasons.is_empty() {
                    e.state = TechState::Locked;
                }
            }
            e
        })
        .collect()
}

/// Guide goals, with texts from the real guide data (`assets/data/guide`). Some are done.
pub fn guide_goals() -> Vec<GuideGoal> {
    let g = |id: &str, tier: u8, title: &str, text: &str, done: bool, count: Option<(u32, u32)>, reward_points: u32| GuideGoal {
        id: id.into(),
        tier,
        title: title.into(),
        text: text.into(),
        done,
        count,
        reward_points,
        waits_for: None,
    };
    vec![
        g("t0_dig", 0, "Dig", "Hold the left mouse button to dig. The cells you dig go into your material tank. Dig 100 units of sand.", true, Some((100, 100)), 1),
        g("t0_clay", 0, "Find clay", "Clay is a brown powder. Look near water and under the dirt. You make bricks from clay. Dig 64 units of clay.", true, Some((64, 64)), 1),
        g("t0_wood", 0, "Cut a tree", "Dig the trunk of a tree to get wood. Wood is fuel, and your first buildings are made of it. Collect 60 units of wood.", true, Some((60, 60)), 1),
        g("t0_workbench", 0, "Build a workbench", "Open the crafting menu and make a workbench from 20 wood. Place it. Hand crafting is 2 times faster near a workbench.", true, Some((1, 1)), 1),
        g("t0_research_bronze", 0, "Research Bronze", "Open the tech tree and research Bronze. It needs scans of copper ore and tin ore.", true, None, 1),
        GuideGoal {
            waits_for: Some("the kiln".into()),
            ..g("t0_kiln", 0, "Build a kiln", "Build a closed room from clay brick walls. Put a kiln controller in the wall.", false, Some((0, 1)), 2)
        },
        g("t0_kits", 0, "Make research kits", "A bronze research kit needs a bronze gear, a clay brick and a tin plate. Put the kits in your labs.", false, Some((6, 10)), 1),
        g(
            "t0_hub",
            0,
            "Repair the Hub",
            "The Hub is your landing pod. Open it to see the parts the first repair needs, then deliver them. This opens Tier 1: Steam.",
            false,
            None,
            5,
        ),
        g(
            "t1_steam",
            1,
            "Make steam",
            "Build a small boiler. It burns fuel and turns water into steam. Steam machines take steam from pipes. Do not let a hot boiler run dry: cold water in a dry, hot boiler makes it explode.",
            true,
            Some((1, 1)),
            2,
        ),
        g("t1_stone", 1, "Dig into stone", "Research the bronze drill head. With it you can dig stone and go down into the upper stone layer.", false, None, 1),
        g("t1_coal", 1, "Find coal", "Coal is a black rock in the stone layer. You make coke from it. Dig 200 units of coal.", false, Some((120, 200)), 1),
        g(
            "t1_coke_oven",
            1,
            "Build a coke oven",
            "Build a closed room from brick walls with a coke oven controller. Coal that is heated with no air turns into coke. The oven also makes creosote (a liquid) and coal gas.",
            false,
            Some((0, 1)),
            2,
        ),
    ]
}

/// An LV network with some load problems.
pub fn power_view(c: &Content) -> PowerNetworkView {
    let e = |id: &str, count: u32, watts: f64, seed: u32| PowerEntry {
        kind: bk(c, id),
        count,
        watts,
        history: series(seed, watts as f32, watts as f32 * 0.18),
    };
    let producers = vec![
        e("steam_turbine", 2, 40_000.0, 1),
        e("solar_panel", 6, 9_600.0, 2),
        e("battery_box", 2, 6_400.0, 3),
        e("water_turbine", 1, 3_200.0, 4),
    ];
    let consumers = vec![
        e("electric_furnace", 3, 23_200.0, 5),
        e("macerator", 2, 11_400.0, 6),
        e("assembler", 4, 12_600.0, 7),
        e("wire_mill", 2, 6_800.0, 8),
        e("lv_lab", 1, 3_900.0, 9),
        e("electric_pump", 1, 2_300.0, 10),
    ];
    let prod_refs: Vec<&TimeSeries> = producers.iter().map(|p| &p.history).collect();
    let cons_refs: Vec<&TimeSeries> = consumers.iter().map(|p| &p.history).collect();
    let production_history = sum_series(&prod_refs);
    let consumption_history = sum_series(&cons_refs);
    PowerNetworkView {
        id: 3,
        voltage: Voltage::Lv,
        satisfaction: 0.86,
        production_w: producers.iter().map(|p| p.watts).sum(),
        capacity_w: 62_000.0,
        consumption_w: consumers.iter().map(|p| p.watts).sum(),
        stored_j: 1_200_000.0,
        storage_capacity_j: 4_000_000.0,
        amps: 36.0,
        limit_amps: 32.0,
        producers,
        consumers,
        production_history,
        consumption_history,
        warnings: vec![PowerWarning::CableOverloaded { amps: 36.0, limit_amps: 32.0, cable: bk(c, "tin_cable") }, PowerWarning::NotEnoughPower],
    }
}

fn stats_view(c: &Content) -> ProductionStatsView {
    let row = |id: &str, made: f32, used: f32, seed: u32| ProductionRow {
        item: it(c, id),
        made: if made > 0.0 { series(seed, made, made * 0.3) } else { TimeSeries::default() },
        used: if used > 0.0 { series(seed + 50, used, used * 0.3) } else { TimeSeries::default() },
    };
    ProductionStatsView {
        rows: vec![
            row("raw_malachite", 960.0, 900.0, 1),
            row("crushed_malachite", 880.0, 850.0, 2),
            row("clay", 640.0, 480.0, 3),
            row("charcoal", 420.0, 510.0, 4),
            row("molten_copper", 540.0, 530.0, 5),
            row("molten_bronze", 300.0, 290.0, 6),
            row("bronze_plate", 36.0, 30.0, 7),
            row("bronze_gear", 12.0, 11.0, 8),
            row("brick", 24.0, 18.0, 9),
            row("bronze_kit", 6.0, 5.5, 10),
            row("steam", 360.0, 350.0, 11),
            row("water", 380.0, 360.0, 12),
            row("wood_belt", 4.0, 3.0, 13),
        ],
    }
}

/// A hover view of a building, for the entity info panel.
pub fn hover_building(c: &Content) -> HoverView {
    HoverView::Building {
        id: BuildingId { index: 9, generation: 1 },
        kind: bk(c, "steam_crusher"),
        status: MachineStatus::OutputFull,
        recipe: Some(rid(c, "crushed_malachite")),
        progress: 1.0,
        temperature: Some(64.0),
        power_w: None,
    }
}

/// More about the building of `hover_building`, for the hover box.
pub fn hover_building_detail() -> HoverDetail {
    HoverDetail { reason: "Output full: take out the Crushed malachite".into(), hit_points: Some((180, 200)), ..Default::default() }
}

/// More about the cell of the mock model, for the hover box.
pub fn hover_cell_detail() -> HoverDetail {
    HoverDetail { dig: Some(DigState::CanDig), ..Default::default() }
}

/// A realistic model: the player is in Tier 1, with a full inventory, a crafting queue,
/// alerts and research. No window is open.
pub fn model(content: Arc<Content>) -> UiModel {
    let c = &*content;
    let mut inventory = vec![
        st(c, "bronze_plate", 58),
        st(c, "bronze_gear", 17),
        st(c, "tin_plate", 42),
        st(c, "copper_plate", 12),
        st(c, "brick", 86),
        st(c, "raw_clay_brick", 24),
        st(c, "bronze_rod", 30),
        st(c, "copper_wire", 64),
        st(c, "bronze_pipe_section", 6),
        st(c, "steel_plate", 8),
        st(c, "wood_belt", 100),
        st(c, "wood_belt", 48),
        st(c, "hopper", 7),
        st(c, "crate", 4),
        st(c, "barrel", 3),
        st(c, "crucible", 2),
        st(c, "ingot_mold", 3),
        st(c, "plate_mold", 2),
        st(c, "clay_brick_wall", 100),
        st(c, "clay_brick_wall", 14),
        st(c, "ladder", 20),
        st(c, "steam_crusher", 2),
        st(c, "small_boiler", 1),
        st(c, "steam_assembler", 1),
        st(c, "bronze_pipe", 36),
        st(c, "bronze_kit", 12),
        st(c, "glass_vial", 9),
        st(c, "circuit_board", 3),
        st(c, "workbench", 1),
        st(c, "kiln_controller", 1),
    ];
    inventory.resize(60, None);
    let m = |id: &str| Some(c.expect_material(id));
    let mut tank = vec![
        TankSlot { material: m("clay"), units: 4240, capacity: 6000 },
        TankSlot { material: m("raw_malachite"), units: 860, capacity: 6000 },
        TankSlot { material: m("charcoal"), units: 5990, capacity: 6000 },
        TankSlot { material: m("wood"), units: 420, capacity: 6000 },
        TankSlot { material: m("sand"), units: 2600, capacity: 6000 },
    ];
    tank.resize(8, TankSlot { material: None, units: 0, capacity: 6000 });
    let hb = |id: &str| Some(it(c, id));
    let hotbar = vec![
        hb("wood_belt"),
        hb("hopper"),
        hb("crate"),
        hb("bronze_pipe"),
        hb("steam_crusher"),
        hb("small_boiler"),
        hb("steam_assembler"),
        hb("clay_brick_wall"),
        hb("ladder"),
        None,
        hb("crucible"),
        hb("ingot_mold"),
        hb("plate_mold"),
        hb("kiln_controller"),
        hb("workbench"),
        hb("steam_drill"),
        None,
        None,
        None,
        None,
    ];
    let mut model = UiModel::new(content.clone());
    model.state = GameState::Playing;
    model.player = PlayerView {
        hull: 82.0,
        hull_max: 100.0,
        temperature: 41.0,
        heat_limit: 80.0,
        inventory,
        tank,
        spray: m("clay"),
        tanks_full: false,
        hand: None,
        hotbar,
        selected_hotbar: Some(0),
        crafting: vec![
            CraftJobView { recipe: rid(c, "bronze_gear"), count: 4, progress: 0.35 },
            CraftJobView { recipe: rid(c, "wood_belt"), count: 10, progress: 0.0 },
            CraftJobView { recipe: rid(c, "bronze_kit"), count: 2, progress: 0.0 },
        ],
        craft_speed: 2.0,
        // Dug materials: the robot throws out dirt and gravel.
        dig: ["charcoal", "clay", "dirt", "gravel", "raw_cassiterite", "raw_malachite", "sand", "wood"]
            .iter()
            .map(|id| DigRule { material: c.expect_material(id), keep: !matches!(*id, "dirt" | "gravel") })
            .collect(),
    };
    model.finished_techs = ["bronze", "research", "steam_power", "steam_machines_1", "iron_logistics"].iter().map(|t| tid(c, t)).collect();
    model.hover = Some(HoverView::Cell { pos: CellPos { x: 4133, y: 612 }, material: c.expect_material("raw_malachite"), temperature: 24.0 });
    model.hover_detail = hover_cell_detail();
    model.research = Some(ResearchView { tech: tid(c, "steam_machines_2"), progress: 0.62 });
    model.techs = tech_entries(c, &model.finished_techs, Some((tid(c, "steam_machines_2"), 0.62)));
    // A technology that started and then stopped, and one in the queue.
    for e in &mut model.techs {
        if e.id == tid(c, "bronze_drill_head") {
            e.progress = 0.3;
        }
        if e.id == tid(c, "glass") {
            e.queue_position = Some(0);
        }
    }
    model.discovery_points = 7;
    model.guide = guide_goals();
    model.alerts = vec![
        AlertView { id: 1, kind: AlertKind::MachineStopped, text: "Steam crusher stopped: output full".into(), count: 2, pos: Some(CellPos { x: 4200, y: 640 }) },
        AlertView { id: 2, kind: AlertKind::Fire, text: "Fire next to the kiln".into(), count: 1, pos: Some(CellPos { x: 4050, y: 590 }) },
        AlertView { id: 3, kind: AlertKind::Leak, text: "Steam leak: bronze pipe".into(), count: 3, pos: Some(CellPos { x: 4310, y: 700 }) },
    ];
    model.stats = stats_view(c);
    model.saves = vec![
        SaveInfo { id: "autosave-1".into(), name: "Autosave".into(), date: "2026-09-27 16:40".into(), play_time_s: 4 * 3600 + 23 * 60, world: "Seed 81234, 8192 × 8192".into() },
        SaveInfo { id: "copper-valley".into(), name: "Copper valley".into(), date: "2026-09-27 15:12".into(), play_time_s: 4 * 3600 + 2 * 60, world: "Seed 81234, 8192 × 8192".into() },
        SaveInfo { id: "first-kiln".into(), name: "First kiln".into(), date: "2026-09-26 21:05".into(), play_time_s: 58 * 60, world: "Seed 81234, 8192 × 8192".into() },
        SaveInfo { id: "desert-test".into(), name: "Desert test".into(), date: "2026-09-25 18:30".into(), play_time_s: 17 * 60, world: "Seed 5, 4096 × 4096".into() },
    ];
    model.settings = Settings { show_fps: true, ..Default::default() };
    model.fps = 120.0;
    model
}

/// A sandbox model like the one the game uses now: every material with no limit in the
/// inventory, brush materials in the quickbar, sand in the hand, and performance numbers.
pub fn sandbox_model(content: Arc<Content>) -> UiModel {
    let c = content.clone();
    let mut m = UiModel::new(content);
    m.state = GameState::Playing;
    m.player.inventory = c
        .materials
        .all()
        .filter(|id| c.materials.ids[id.index()] != "bedrock")
        .map(|id| Some(Stack { item: ItemRef::Material(id), count: 1 }))
        .collect();
    let ids = ["sand", "water", "oil", "lava", "stone", "wood", "fire", "steam", "methane", "air"];
    m.player.hotbar = ids.iter().map(|id| c.material(id).map(ItemRef::Material)).collect();
    m.player.hotbar.resize(20, None);
    m.player.selected_hotbar = Some(0);
    m.player.hand = Some(Stack { item: it(&c, "sand"), count: 1 });
    m.sandbox = Some(SandboxView { brush_radius: 6, sim_paused: false });
    m.perf = Some(PerfView { fps: 120.0, tick_ms: 1.4, ticks_per_second: 60.0, awake_chunks: 42, loaded_chunks: 512 });
    m.hover = Some(HoverView::Cell { pos: CellPos { x: 1012, y: 588 }, material: c.expect_material("water"), temperature: 18.0 });
    m.settings.show_fps = true;
    m
}

/// Example simulation settings (the liquid settings will look like this).
pub fn example_sim_settings() -> Vec<SimSetting> {
    vec![
        SimSetting {
            key: "liquid_spread".into(),
            label: "Liquid spread".into(),
            help: "How far a liquid cell moves to the side in one tick.".into(),
            value: 6.0,
            min: 1.0,
            max: 16.0,
            step: 1.0,
        },
        SimSetting {
            key: "splash".into(),
            label: "Splash strength".into(),
            help: "How much liquid jumps up when something falls into it.".into(),
            value: 0.4,
            min: 0.0,
            max: 1.0,
            step: 0.05,
        },
    ]
}

/// Applies `UiAction`s to a model, the way the game will. For the preview and for tests.
pub struct MockGame {
    pub model: UiModel,
    /// Set by `UiAction::QuitGame`.
    pub quit: bool,
    save_counter: u32,
}

impl MockGame {
    pub fn new(content: Arc<Content>) -> Self {
        Self { model: model(content), quit: false, save_counter: 0 }
    }

    pub fn content(&self) -> Arc<Content> {
        self.model.content.clone()
    }

    /// Open a building window (the game does this when the player clicks a building).
    pub fn open_building(&mut self, view: BuildingView) {
        self.model.building = Some(view);
    }

    pub fn open_power(&mut self) {
        self.model.power = Some(power_view(&self.model.content));
    }

    pub fn apply_all(&mut self, actions: Vec<UiAction>) {
        for a in actions {
            self.apply(a);
        }
    }

    pub fn apply(&mut self, action: UiAction) {
        let content = self.model.content.clone();
        let md = &mut self.model;
        match action {
            UiAction::OpenWindow(_) => {}
            UiAction::CloseWindow(WindowKind::Building) => md.building = None,
            UiAction::CloseWindow(WindowKind::PowerNetwork) => md.power = None,
            UiAction::CloseWindow(_) => {}
            UiAction::OpenPowerNetwork(_) => md.power = Some(power_view(&content)),
            UiAction::ClickSlot { slot, click } => self.click_slot(slot, click),
            UiAction::EmptyTank(i) => {
                if let Some(t) = md.player.tank.get_mut(i) {
                    t.material = None;
                    t.units = 0;
                }
            }
            UiAction::SetKeep { material, keep } => {
                if let Some(r) = md.player.dig.iter_mut().find(|r| r.material == material) {
                    r.keep = keep;
                }
            }
            UiAction::SelectHotbar(i) => {
                if md.player.hotbar.get(i).copied().flatten().is_some() {
                    md.player.selected_hotbar = Some(i);
                }
            }
            UiAction::SetHotbar { index, item } => {
                if index < md.player.hotbar.len() {
                    md.player.hotbar[index] = item;
                }
            }
            UiAction::ClearHand => {
                if let Some(h) = md.player.hand.take() {
                    let put = self.give(h.item, h.count);
                    if put < h.count {
                        self.model.player.hand = Some(Stack { item: h.item, count: h.count - put });
                    }
                }
            }
            UiAction::Craft { recipe, count } => self.craft(recipe, count),
            UiAction::CancelCraft { index, count } => self.cancel(index, count),
            UiAction::SetRecipe { recipe, .. } => {
                if let Some(bv) = md.building.as_mut() {
                    bv.recipe = recipe;
                    bv.status = if recipe.is_some() { MachineStatus::NoInput } else { MachineStatus::NoRecipe };
                    bv.progress = 0.0;
                    let inputs: Vec<ItemRef> =
                        recipe.and_then(|r| item::recipe(&content, r)).map(|r| r.inputs.iter().filter(|s| !item::is_bulk(s.item)).map(|s| s.item).collect()).unwrap_or_default();
                    bv.inputs.resize(inputs.len(), BuildingSlot::default());
                    for (i, s) in bv.inputs.iter_mut().enumerate() {
                        s.filter = inputs.get(i).copied();
                    }
                }
            }
            UiAction::ShowAlert(id) => md.message = format!("The camera moves to alert {id}."),
            UiAction::StartResearch(tech) => {
                let progress = md.techs.iter().find(|e| e.id == tech).map(|e| e.progress).unwrap_or(0.0);
                md.research = Some(ResearchView { tech, progress });
                for e in &mut md.techs {
                    if e.id == tech {
                        e.state = TechState::Researching;
                        e.queue_position = None;
                    } else if e.state == TechState::Researching {
                        e.state = TechState::Available;
                    }
                }
            }
            UiAction::NewGame { seed, size, mode } => {
                *md = match mode {
                    GameMode::Normal => model(content),
                    GameMode::Sandbox => sandbox_model(content),
                };
                let (w, h) = size.cells();
                md.message = format!("New world: seed {seed}, {w} × {h} cells.");
            }
            UiAction::Continue => {
                md.state = GameState::Playing;
                md.message = "Loaded the newest save.".into();
            }
            UiAction::Pause => md.state = GameState::Paused,
            UiAction::Resume => md.state = GameState::Playing,
            UiAction::Save { name, overwrite } => {
                if overwrite {
                    md.saves.retain(|s| s.name != name);
                }
                self.save_counter += 1;
                md.saves.insert(
                    0,
                    SaveInfo {
                        id: format!("save-{}", self.save_counter),
                        name: name.clone(),
                        date: "2026-09-27 17:00".into(),
                        play_time_s: 4 * 3600 + 30 * 60,
                        world: "Seed 81234, 8192 × 8192".into(),
                    },
                );
                md.message = format!("Saved \"{name}\".");
            }
            UiAction::Load(id) => {
                let name = md.saves.iter().find(|s| s.id == id).map(|s| s.name.clone()).unwrap_or(id);
                md.state = GameState::Playing;
                md.message = format!("Loaded \"{name}\".");
            }
            UiAction::DeleteSave(id) => md.saves.retain(|s| s.id != id),
            UiAction::QuitToMenu => {
                md.state = GameState::MainMenu;
                md.building = None;
                md.power = None;
            }
            UiAction::QuitGame => self.quit = true,
            UiAction::ChangeSetting(s) => match s {
                SettingChange::UiScale(v) => md.settings.ui_scale = v,
                SettingChange::Vsync(v) => md.settings.vsync = v,
                SettingChange::ShowFps(v) => md.settings.show_fps = v,
                SettingChange::ShowDebug(v) => md.settings.show_debug = v,
                SettingChange::Simulation { key, value } => {
                    if let Some(s) = md.settings.simulation.iter_mut().find(|s| s.key == key) {
                        s.value = value;
                    }
                }
                SettingChange::KeysByLetter(v) => md.settings.keys_by_letter = v,
                // The mock has no key bindings to change; it only shows the waiting row.
                SettingChange::RebindKey(id) => {
                    md.settings.key_waiting = if md.settings.key_waiting.as_deref() == Some(id.as_str()) { None } else { Some(id) };
                }
                SettingChange::ResetKeys => md.settings.key_bindings = crate::model::default_key_bindings(),
            },
        }
    }

    /// Advance hand crafting and the open building by `dt` seconds.
    pub fn tick(&mut self, dt: f32) {
        let content = self.model.content.clone();
        let md = &mut self.model;
        if md.state != GameState::Playing {
            return;
        }
        if let Some(bv) = md.building.as_mut()
            && bv.status == MachineStatus::Working
        {
            bv.progress = (bv.progress + dt / 5.0).fract();
        }
        let Some(job) = md.player.crafting.first_mut() else { return };
        let Some(r) = item::recipe(&content, job.recipe) else {
            md.player.crafting.remove(0);
            return;
        };
        job.progress += dt * md.player.craft_speed.max(0.1) / r.time.max(0.1);
        if job.progress >= 1.0 {
            job.progress = 0.0;
            job.count -= 1;
            if job.count == 0 {
                md.player.crafting.remove(0);
            }
            for out in &r.outputs {
                self.give(out.item, out.count);
            }
        }
    }

    /// Put items into the inventory or the tank. Returns how many went in.
    fn give(&mut self, it: ItemRef, count: u32) -> u32 {
        let pl = &mut self.model.player;
        match it {
            ItemRef::Material(mat) => {
                let mut left = count;
                for t in pl.tank.iter_mut() {
                    if left == 0 {
                        break;
                    }
                    if t.material == Some(mat) || t.material.is_none() {
                        let n = (t.capacity - t.units).min(left);
                        if n > 0 {
                            t.material = Some(mat);
                            t.units += n;
                            left -= n;
                        }
                    }
                }
                count - left
            }
            ItemRef::Part(_) => {
                let content = self.model.content.clone();
                let limit = |_: usize, x: ItemRef| if item::is_bulk(x) { 0 } else { item::stack_size(&content, x, 0) };
                let mut list = SlotList::new(&mut pl.inventory, &limit);
                slots::insert(&mut list, it, count)
            }
        }
    }

    fn take(&mut self, it: ItemRef, mut count: u32) {
        let pl = &mut self.model.player;
        match it {
            ItemRef::Material(mat) => {
                for t in pl.tank.iter_mut().rev() {
                    if t.material == Some(mat) {
                        let n = t.units.min(count);
                        t.units -= n;
                        count -= n;
                        if t.units == 0 {
                            t.material = None;
                        }
                    }
                }
            }
            ItemRef::Part(_) => {
                for s in pl.inventory.iter_mut().rev() {
                    if let Some(x) = s
                        && x.item == it
                    {
                        let n = x.count.min(count);
                        x.count -= n;
                        count -= n;
                        if x.count == 0 {
                            *s = None;
                        }
                    }
                }
            }
        }
    }

    fn craft(&mut self, recipe: RecipeId, count: u32) {
        let content = self.model.content.clone();
        let Some(r) = item::recipe(&content, recipe) else { return };
        let stock = Stock::from_player(&self.model.player);
        let n = count.min(craftable_count(r, &stock));
        if n == 0 {
            return;
        }
        for input in &r.inputs {
            self.take(input.item, input.count * n);
        }
        let q = &mut self.model.player.crafting;
        // Add to the last job if it is the same recipe and not the one in progress.
        let len = q.len();
        match q.last_mut() {
            Some(last) if last.recipe == recipe && len > 1 => last.count += n,
            _ => q.push(CraftJobView { recipe, count: n, progress: 0.0 }),
        }
    }

    fn cancel(&mut self, index: usize, count: u32) {
        let content = self.model.content.clone();
        let q = &mut self.model.player.crafting;
        let Some(job) = q.get_mut(index) else { return };
        let n = count.min(job.count);
        job.count -= n;
        let recipe = job.recipe;
        if job.count == 0 {
            q.remove(index);
        }
        if let Some(r) = item::recipe(&content, recipe) {
            for input in &r.inputs {
                self.give(input.item, input.count * n);
            }
        }
    }

    fn click_slot(&mut self, slot: SlotRef, click: SlotClick) {
        let content = self.model.content.clone();
        let md = &mut self.model;
        let inv_limit = |_: usize, x: ItemRef| if item::is_bulk(x) { 0 } else { item::stack_size(&content, x, 0) };
        let caps: Vec<u32> = md.player.tank.iter().map(|t| t.capacity).collect();
        let tank_limit = |i: usize, x: ItemRef| if item::is_bulk(x) { caps.get(i).copied().unwrap_or(0) } else { 0 };
        let mut tank: Vec<Option<Stack>> = md.player.tank.iter().map(|t| t.material.map(|m| Stack { item: ItemRef::Material(m), count: t.units })).collect();
        let hand = &mut md.player.hand;
        // Building slots, as plain lists.
        let (mut b_in, mut b_out, mut b_fuel, filters_in, filters_fuel) = match md.building.as_ref() {
            Some(bv) => (
                bv.inputs.iter().map(|s| s.stack).collect::<Vec<_>>(),
                bv.outputs.iter().map(|s| s.stack).collect::<Vec<_>>(),
                bv.fuel.iter().map(|s| s.stack).collect::<Vec<_>>(),
                bv.inputs.iter().map(|s| s.filter).collect::<Vec<_>>(),
                bv.fuel.iter().map(|s| s.filter).collect::<Vec<_>>(),
            ),
            None => (vec![], vec![], vec![], vec![], vec![]),
        };
        let in_limit = |i: usize, x: ItemRef| match filters_in.get(i).copied().flatten() {
            Some(f) if f != x => 0,
            _ => item::stack_size(&content, x, 200),
        };
        let fuel_limit = |i: usize, x: ItemRef| match filters_fuel.get(i).copied().flatten() {
            Some(f) if f == x => item::stack_size(&content, x, 200),
            _ => 0,
        };
        let no_limit = |_: usize, _: ItemRef| 0;
        let building_open = md.building.is_some();
        match slot {
            SlotRef::Inventory(i) | SlotRef::Tank(i) => {
                let is_tank = matches!(slot, SlotRef::Tank(_));
                let mut this = if is_tank { SlotList::new(&mut tank, &tank_limit) } else { SlotList::new(&mut md.player.inventory, &inv_limit) };
                if building_open {
                    let mut others = [SlotList::new(&mut b_fuel, &fuel_limit), SlotList::new(&mut b_in, &in_limit)];
                    slots::apply(hand, &mut this, i, &mut others, click);
                } else {
                    slots::apply(hand, &mut this, i, &mut [], click);
                }
            }
            SlotRef::Building { group, index, .. } => {
                let (list, limit): (&mut Vec<Option<Stack>>, &dyn Fn(usize, ItemRef) -> u32) = match group {
                    BuildingSlots::Input => (&mut b_in, &in_limit),
                    BuildingSlots::Output => (&mut b_out, &no_limit),
                    BuildingSlots::Fuel => (&mut b_fuel, &fuel_limit),
                };
                let mut this = SlotList::new(list, limit);
                let mut others = [SlotList::new(&mut md.player.inventory, &inv_limit), SlotList::new(&mut tank, &tank_limit)];
                slots::apply(hand, &mut this, index, &mut others, click);
            }
        }
        // Write the plain lists back.
        for (t, s) in md.player.tank.iter_mut().zip(tank) {
            match s {
                Some(Stack { item: ItemRef::Material(m), count }) => {
                    t.material = Some(m);
                    t.units = count;
                }
                _ => {
                    t.material = None;
                    t.units = 0;
                }
            }
        }
        if let Some(bv) = md.building.as_mut() {
            for (s, v) in bv.inputs.iter_mut().zip(b_in) {
                s.stack = v;
            }
            for (s, v) in bv.outputs.iter_mut().zip(b_out) {
                s.stack = v;
            }
            for (s, v) in bv.fuel.iter_mut().zip(b_fuel) {
                s.stack = v;
            }
        }
    }
}

/// Paint a simple side view of the world behind the HUD, so screenshots show the UI on a
/// realistic background. Only for the preview and the tests.
pub fn paint_world(p: &egui::Painter, screen: egui::Rect) {
    use egui::{Color32, Mesh, Rect, Shape, pos2, vec2};
    let cell = 6.0;
    let surface = screen.top() + screen.height() * 0.46;
    let mut mesh = Mesh::default();
    // Sky.
    let top_c = Color32::from_rgb(98, 146, 196);
    let bot_c = Color32::from_rgb(176, 202, 222);
    mesh.colored_vertex(screen.left_top(), top_c);
    mesh.colored_vertex(screen.right_top(), top_c);
    mesh.colored_vertex(pos2(screen.right(), screen.bottom()), bot_c);
    mesh.colored_vertex(pos2(screen.left(), screen.bottom()), bot_c);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    let hash = |x: i32, y: i32| -> u32 {
        let mut h = (x as u32).wrapping_mul(0x27d4_eb2d) ^ (y as u32).wrapping_mul(0x1656_67b1);
        h ^= h >> 15;
        h = h.wrapping_mul(0x85eb_ca6b);
        h ^ (h >> 13)
    };
    let cols = (screen.width() / cell) as i32 + 1;
    let rows = (screen.height() / cell) as i32 + 1;
    for cx in 0..cols {
        let x = screen.left() + cx as f32 * cell;
        let ground = surface + ((cx as f32 * 0.05).sin() * 22.0 + (cx as f32 * 0.013).sin() * 40.0);
        for cy in 0..rows {
            let y = screen.top() + cy as f32 * cell;
            if y < ground {
                continue;
            }
            let d = y - ground;
            let n = hash(cx, cy);
            let v = (n % 24) as f32 / 100.0 + 0.88;
            let cave = ((cx as f32 - cols as f32 * 0.62).powi(2) / 900.0 + (cy as f32 - rows as f32 * 0.74).powi(2) / 60.0) < 1.0;
            let lava = cave && (cy as f32) > rows as f32 * 0.76;
            let base = if lava {
                Color32::from_rgb(255, 120, 30)
            } else if cave {
                Color32::from_rgb(28, 24, 26)
            } else if d < cell * 2.0 {
                Color32::from_rgb(84, 140, 60)
            } else if d < 90.0 {
                Color32::from_rgb(110, 78, 50)
            } else if (n % 97) < 3 {
                Color32::from_rgb(40, 130, 90) // malachite
            } else {
                Color32::from_rgb(104, 104, 110)
            };
            let c = crate::theme::shade(base, v * if lava { 1.0 } else { (1.0 - d / screen.height() * 0.6).max(0.35) });
            mesh.add_colored_rect(Rect::from_min_size(pos2(x, y), vec2(cell, cell)), c);
        }
    }
    p.add(Shape::mesh(mesh));
    // The robot.
    let robot = Rect::from_min_size(pos2(screen.center().x - 12.0, surface - 44.0), vec2(24.0, 48.0));
    p.rect_filled(robot, 3.0, Color32::from_rgb(220, 170, 60));
    p.rect_filled(Rect::from_min_size(robot.min + vec2(4.0, 6.0), vec2(16.0, 10.0)), 2.0, Color32::from_rgb(40, 60, 80));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_content_loads_and_has_the_mock_ids() {
        let c = content();
        for id in ["bronze_plate", "brick", "steam_assembler", "vacuum_tube", "tin_cable", "wood_belt"] {
            assert!(c.item(id).is_some(), "{id}");
        }
        let m = model(c.clone());
        assert_eq!(m.player.inventory.len(), 60);
        assert!(m.recipe_known(rid(&c, "bronze_gear")));
        // Electricity is not researched: its recipes are hidden.
        assert!(!m.recipe_known(rid(&c, "steam_turbine")));
    }

    #[test]
    fn mock_research_guide_and_hub() {
        let c = content();
        let m = model(c.clone());
        assert_eq!(m.techs.len(), c.factory.techs.len());
        for state in [TechState::Done, TechState::Researching, TechState::Available, TechState::Locked] {
            assert!(m.techs.iter().any(|e| e.state == state), "{state:?}");
        }
        assert!(m.techs.iter().filter(|e| e.state == TechState::Locked).all(|e| !e.reasons.is_empty()));
        assert!(m.guide.iter().any(|g| g.done) && m.guide.iter().any(|g| !g.done));
        let hub = hub_view(&c);
        assert_eq!(hub.inputs.len(), 16);
        assert!(hub.buffers.is_empty());
        assert!(!hub.later_stages.is_empty());
        let crate_ = crate_view(&c);
        assert_eq!(crate_.inputs.len(), 8);
        assert!(crate_.inputs.iter().any(|s| s.capacity > 0));
        assert_eq!(hub.milestone.as_ref().map(|m| m.stage), Some(1));
        let mut g = MockGame::new(c.clone());
        let drill = tid(&c, "bronze_drill_head");
        g.apply(UiAction::StartResearch(drill));
        assert_eq!(g.model.research.as_ref().map(|r| r.tech), Some(drill));
        assert_eq!(g.model.techs.iter().filter(|e| e.state == TechState::Researching).count(), 1);
    }

    #[test]
    fn mock_game_crafts_and_cancels() {
        let c = content();
        let mut g = MockGame::new(c.clone());
        let plate = it(&c, "bronze_plate");
        let before = Stock::from_player(&g.model.player).get(plate);
        g.apply(UiAction::Craft { recipe: rid(&c, "bronze_gear"), count: 2 });
        assert_eq!(Stock::from_player(&g.model.player).get(plate), before - 4);
        let last = g.model.player.crafting.len() - 1;
        g.apply(UiAction::CancelCraft { index: last, count: 2 });
        assert_eq!(Stock::from_player(&g.model.player).get(plate), before);
    }

    #[test]
    fn mock_game_moves_items_into_a_building() {
        let c = content();
        let mut g = MockGame::new(c.clone());
        g.open_building(steam_assembler_view(&c));
        let wire = it(&c, "copper_wire");
        let slot = g.model.player.inventory.iter().position(|s| s.is_some_and(|s| s.item == wire)).unwrap();
        g.apply(UiAction::ClickSlot { slot: SlotRef::Inventory(slot), click: SlotClick::SHIFT_LEFT });
        let b = g.model.building.as_ref().unwrap();
        assert_eq!(b.inputs[1].stack.map(|s| s.count), Some(12 + 64));
        assert!(g.model.player.inventory[slot].is_none());
    }
}
