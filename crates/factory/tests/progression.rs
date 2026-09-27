//! The factory and the progression together, in a real simulation: labs do research, the Hub
//! takes milestone deliveries, research decides the known recipes, the guide checks goals, scans
//! and reactions give discoveries, and save and load keep all of it.
//!
//! The content is small and made only for these tests (all ids start with `test_`), so changes
//! to `assets/data` do not break them.

use foundry_content::factory_defs::{BuildingDef, MilestoneDef, PartDef, RecipeDef, TechDef};
use foundry_content::{Content, FactoryContent, ItemRef, Stack};
use foundry_core::{BuildingKindId, CellPos, RecipeId, TechId, TilePos};
use foundry_factory::progress::{Discovered, LockReason, POINTS_PER_MATERIAL, POINTS_PER_REACTION};
use foundry_factory::{
    CraftError, Factory, GUIDE_PERIOD, Guide, GuideState, PartStack, ProgressEvent, REACTION_SEE_RANGE, RecipeError,
    Status,
};
use foundry_sim::{SimConfig, Simulation};
use std::sync::{Arc, OnceLock};

const MATERIALS: &str = r##"[
    Material(id: "air", name: "Air", phase: Empty, colors: ["#00000000"]),
    Material(id: "bedrock", name: "Bedrock", phase: Solid, colors: ["#111111"], hardness: 255),
    Material(id: "test_ore", name: "Test ore", phase: Solid, colors: ["#557755"], density: 3000, hardness: 30,
        broken_into: Some("test_raw_ore")),
    Material(id: "test_raw_ore", name: "Test raw ore", phase: Powder, colors: ["#668866"], density: 2000),
    Material(id: "test_wood", name: "Test wood", phase: Solid, colors: ["#aa7744"], density: 700),
    Material(id: "test_water", name: "Test water", phase: Liquid, colors: ["#3355ff"], density: 1000, flow: 4),
    Material(id: "test_lava", name: "Test lava", phase: Liquid, colors: ["#ff5500"], density: 3000, flow: 1),
]"##;

const REACTIONS: &str = r#"[
    Reaction(a: "test_water", b: "test_lava", into_a: Some("air"), into_b: Some("test_ore")),
]"#;

const PARTS: &str = r#"[
    Part(id: "test_plate", name: "Test plate", category: "intermediate"),
    Part(id: "test_widget", name: "Test widget", category: "intermediate"),
    Part(id: "test_kit", name: "Test kit", category: "research"),
]"#;

const BUILDINGS: &str = r#"[
    Building(id: "test_lab", name: "Test lab", kind: "lab", size: (2, 2), tier: 0, body: "test_wood",
        ports: [(kind: PartIn, tile: (0, 0), side: Up)], params: {"kit_buffer": 10.0}),
    Building(id: "test_hub", name: "Test hub", kind: "hub", size: (2, 2), tier: 0, body: "test_wood",
        ports: [(kind: PartIn, tile: (0, 0), side: Left)]),
    Building(id: "test_crate", name: "Test crate", kind: "storage", size: (1, 1), tier: 0, body: "test_wood",
        params: {"slots": 8.0}),
    Building(id: "test_press", name: "Test press", kind: "crafter", size: (1, 1), tier: 0, body: "test_wood",
        crafts: ["test_pressing"]),
]"#;

const RECIPES: &str = r#"[
    Recipe(id: "test_make_plate", category: "hand", inputs: [("test_raw_ore", 4)], outputs: [("test_plate", 1)], time: 0.5),
    Recipe(id: "test_make_widget", category: "hand", inputs: [("test_plate", 2)], outputs: [("test_widget", 1)], time: 0.5),
    Recipe(id: "test_press_widget", category: "test_pressing", inputs: [("test_plate", 2)], outputs: [("test_widget", 1)],
        time: 1.0),
]"#;

const TECHS: &str = r#"[
    Tech(id: "test_widgets", name: "Widgets", tier: 0, kits: [("test_kit", 1)], units: 3, unit_time: 1.0,
        unlocks: ["test_make_widget", "test_press_widget"]),
    Tech(id: "test_tier1", name: "Tier 1 tech", tier: 1, kits: [("test_kit", 1)], units: 1, unit_time: 1.0),
    Tech(id: "test_smelting", name: "Smelting", tier: 0, discoveries: ["test_raw_ore", "reaction:test_water+test_lava"]),
]"#;

const MILESTONES: &str = r#"[
    Milestone(stage: 1, name: "First repair", deliver: [("test_plate", 5)], unlocks_tier: 1),
    Milestone(stage: 2, name: "Second repair", deliver: [("test_widget", 5)], unlocks_tier: 2),
]"#;

const GUIDE: &str = r#"[
    Goal(id: "build_lab", tier: 0, title: "Build a lab", text: "Place a lab.", condition: Build("test_lab", 1),
        reward_points: 2),
    Goal(id: "have_plates", tier: 0, title: "Make plates", text: "Have 4 plates.", condition: HaveItem("test_plate", 4),
        reward_points: 1),
    Goal(id: "repair_hub", tier: 1, title: "Repair the Hub", text: "Finish stage 2.", condition: Stage(2)),
]"#;

fn content() -> Arc<Content> {
    static C: OnceLock<Arc<Content>> = OnceLock::new();
    C.get_or_init(|| {
        let mut c = Content::from_ron(&[MATERIALS], &[REACTIONS]).expect("test materials load");
        let parts: Vec<PartDef> = ron::from_str(PARTS).expect("parts");
        let buildings: Vec<BuildingDef> = ron::from_str(BUILDINGS).expect("buildings");
        let recipes: Vec<RecipeDef> = ron::from_str(RECIPES).expect("recipes");
        let techs: Vec<TechDef> = ron::from_str(TECHS).expect("techs");
        let milestones: Vec<MilestoneDef> = ron::from_str(MILESTONES).expect("milestones");
        let mut errors = vec![];
        c.factory = FactoryContent::build(&c.materials, parts, buildings, recipes, techs, milestones, &mut errors);
        assert!(errors.is_empty(), "{errors:#?}");
        Arc::new(c)
    })
    .clone()
}

/// A 128 × 128 cell world (16 × 16 tiles) of air with a bedrock border.
fn world(c: &Arc<Content>) -> Simulation {
    let mut sim = Simulation::new(c.clone(), SimConfig { width_chunks: 2, height_chunks: 2, seed: 7, bedrock_border: true });
    sim.set_threads(1);
    sim
}

fn run(f: &mut Factory, sim: &mut Simulation, ticks: usize) {
    for _ in 0..ticks {
        f.tick(sim);
        sim.tick();
    }
}

fn kind(c: &Content, id: &str) -> BuildingKindId {
    c.factory.building(id).unwrap_or_else(|| panic!("no building {id}"))
}

fn item(c: &Content, id: &str) -> ItemRef {
    c.item(id).unwrap_or_else(|| panic!("no item {id}"))
}

fn recipe(c: &Content, id: &str) -> RecipeId {
    c.factory.recipe(id).unwrap_or_else(|| panic!("no recipe {id}"))
}

fn tech(c: &Content, id: &str) -> TechId {
    c.factory.tech(id).unwrap_or_else(|| panic!("no tech {id}"))
}

/// Whole kits in a lab (from the building window).
fn lab_kits(f: &Factory, lab: foundry_core::BuildingId) -> u32 {
    f.building_view(lab).unwrap().inputs.iter().map(|b| b.count).sum()
}

#[test]
fn a_lab_with_kits_finishes_a_tech_and_its_recipes_become_known() {
    let c = content();
    let mut sim = world(&c);
    let mut f = Factory::new(c.clone());
    let (plate, kit, widget) = (item(&c, "test_plate"), item(&c, "test_kit"), item(&c, "test_widget"));
    let (hand_recipe, press_recipe) = (recipe(&c, "test_make_widget"), recipe(&c, "test_press_widget"));
    let widgets = tech(&c, "test_widgets");

    // Before the research, the recipes that the technology unlocks are not known.
    f.player.insert(&c, plate, 4);
    assert!(f.is_recipe_known(recipe(&c, "test_make_plate")), "no technology unlocks it");
    assert!(!f.is_recipe_known(hand_recipe));
    assert_eq!(f.craft(hand_recipe, 1), Err(CraftError::NotKnown));
    let press = f.place(kind(&c, "test_press"), TilePos::new(12, 4), 0, false, &mut sim).unwrap();
    assert!(f.building_view(press).unwrap().recipes.is_empty());
    assert_eq!(f.set_recipe(press, Some(press_recipe)), Err(RecipeError::NotKnown { recipe: "Test widget".into() }));

    // A lab under a crate with 5 kits, and a lab with no kits.
    let lab = f.place(kind(&c, "test_lab"), TilePos::new(4, 4), 0, false, &mut sim).unwrap();
    let crate_id = f.place(kind(&c, "test_crate"), TilePos::new(4, 3), 0, false, &mut sim).unwrap();
    let empty_lab = f.place(kind(&c, "test_lab"), TilePos::new(8, 8), 0, false, &mut sim).unwrap();
    f.buildings.insert(&c, crate_id, kit, 5);
    run(&mut f, &mut sim, 60);
    let v = f.building_view(lab).unwrap();
    assert_eq!((v.status, v.reason.as_str()), (Status::Idle, "No research selected"));

    // 3 units of 1 s, one kit for each unit.
    f.progress.start_research(&c, widgets).unwrap();
    run(&mut f, &mut sim, 120);
    assert!(!f.progress.is_researched(widgets));
    let v = f.building_view(lab).unwrap();
    assert_eq!((v.status, v.reason.as_str()), (Status::Working, "Researching"));
    let v = f.building_view(empty_lab).unwrap();
    assert_eq!((v.status, v.reason.as_str()), (Status::NoInput, "Needs Test kit"));
    run(&mut f, &mut sim, 90);
    assert!(f.progress.is_researched(widgets));
    assert!(f.progress.events().contains(&ProgressEvent::TechDone(widgets)));
    assert_eq!(f.buildings.inventory(crate_id).unwrap().count(kit) + lab_kits(&f, lab), 2, "3 of 5 kits were used");
    let v = f.building_view(lab).unwrap();
    assert_eq!((v.status, v.reason.as_str()), (Status::Idle, "No research selected"));

    // Now hand crafting and the machine can use the recipes.
    assert!(f.is_recipe_known(hand_recipe));
    assert_eq!(f.can_craft(hand_recipe, 2), Ok(()));
    f.craft(hand_recipe, 2).unwrap();
    run(&mut f, &mut sim, 61);
    assert_eq!(f.player.count(widget), 2);
    assert_eq!(f.building_view(press).unwrap().recipes, vec![press_recipe]);
    assert_eq!(f.set_recipe(press, Some(press_recipe)), Ok(vec![]));
}

#[test]
fn the_hub_takes_a_milestone_delivery_and_opens_the_next_tier() {
    let c = content();
    let mut sim = world(&c);
    let mut f = Factory::new(c.clone());
    let (plate, widget, kit) = (item(&c, "test_plate"), item(&c, "test_widget"), item(&c, "test_kit"));
    let tier1 = tech(&c, "test_tier1");
    assert_eq!(f.progress.can_research(&c, tier1), Err(LockReason::NeedsTier { tier: 1, stage: Some(1) }));

    // A crate left of the Hub's part input. Stage 1 needs 5 plates; the crate has 8.
    let hub = f.place(kind(&c, "test_hub"), TilePos::new(9, 4), 0, false, &mut sim).unwrap();
    let crate_id = f.place(kind(&c, "test_crate"), TilePos::new(8, 4), 0, false, &mut sim).unwrap();
    f.buildings.insert(&c, crate_id, plate, 8);
    // Widgets are for stage 2. They wait in the Hub until stage 1 is done.
    f.player.insert(&c, widget, 3);
    assert_eq!(f.insert_from_player(hub, widget, 3), 3);
    assert_eq!(f.buildings.room_for(&c, hub, kit), 0, "no stage asks for kits");

    run(&mut f, &mut sim, 150);
    assert_eq!(f.progress.stage(), 1);
    assert_eq!(f.progress.unlocked_tier(), 1);
    assert!(f.progress.events().contains(&ProgressEvent::StageDone { stage: 1, tier: 1 }));
    assert_eq!(f.progress.can_research(&c, tier1), Ok(()));
    // No later stage asks for plates, so the Hub stops taking them.
    assert_eq!(f.buildings.inventory(crate_id).unwrap().count(plate), 3);
    assert_eq!(f.buildings.room_for(&c, hub, plate), 0);
    // The widgets went to stage 2.
    let m = f.progress.milestone_view(&c).unwrap();
    assert_eq!(m.stage, 2);
    assert_eq!((m.items[0].delivered, m.items[0].need), (3, 5));
    assert_eq!(f.buildings.inventory(hub).unwrap().count(widget), 0);
}

#[test]
fn the_guide_reports_goals_done() {
    let c = content();
    let mut sim = world(&c);
    let mut f = Factory::new(c.clone());
    f.guide = Arc::new(Guide::from_ron(GUIDE).unwrap());
    assert!(f.guide.check(&c).is_empty(), "{:?}", f.guide.check(&c));
    let goal = |f: &Factory, id: &str| f.guide_view().into_iter().find(|g| g.id == id);
    assert_eq!(goal(&f, "build_lab").unwrap().count, Some((0, 1)));
    assert!(goal(&f, "repair_hub").is_none(), "tier 1 goals do not show in tier 0");

    let lab = f.place(kind(&c, "test_lab"), TilePos::new(4, 4), 0, false, &mut sim).unwrap();
    // 3 plates in the inventory and 1 on the cursor.
    let ItemRef::Part(plate) = item(&c, "test_plate") else { panic!("a part") };
    f.player.insert(&c, ItemRef::Part(plate), 3);
    f.cursor = Some(PartStack::new(plate, 1));
    assert_eq!(goal(&f, "have_plates").unwrap().count, Some((4, 4)));

    // The goals are checked once every GUIDE_PERIOD ticks.
    run(&mut f, &mut sim, GUIDE_PERIOD as usize - 1);
    assert!(!f.progress.is_goal_done("build_lab"));
    run(&mut f, &mut sim, 1);
    assert!(f.progress.is_goal_done("build_lab"));
    assert!(f.progress.is_goal_done("have_plates"));
    assert!(f.progress.events().contains(&ProgressEvent::GoalDone("build_lab".into())));
    assert_eq!(f.progress.discovery_points(), 3, "the rewards of both goals");

    // A goal stays done.
    assert_eq!(f.building_count(kind(&c, "test_lab")), 1);
    f.remove(lab, &mut sim).unwrap();
    assert_eq!(f.building_count(kind(&c, "test_lab")), 0);
    run(&mut f, &mut sim, GUIDE_PERIOD as usize);
    let g = goal(&f, "build_lab").unwrap();
    assert!(g.done);
    assert_eq!(g.count, Some((1, 1)));
}

#[test]
fn a_scan_also_discovers_the_dug_powder_and_a_seen_reaction_is_discovered_once() {
    let c = content();
    let mut f = Factory::new(c.clone());
    let (ore, raw, wood) = (c.expect_material("test_ore"), c.expect_material("test_raw_ore"), c.expect_material("test_wood"));
    let smelting = tech(&c, "test_smelting");

    assert_eq!(f.scan(ore), vec![ore, raw]);
    assert_eq!(f.scan(ore), vec![]);
    assert_eq!(f.scan(raw), vec![], "already found by the ore scan");
    assert_eq!(f.scan(wood), vec![wood], "wood breaks into itself");
    assert_eq!(f.progress.discovery_points(), 3 * POINTS_PER_MATERIAL);
    assert_eq!(
        f.progress.lock_reasons(&c, smelting),
        vec![LockReason::NeedsDiscovery("reaction:test_water+test_lava".into())]
    );

    // The robot sees reactions near it only.
    f.player_pos = Some(CellPos::new(50, 50));
    assert!(!f.observe_reaction(0, CellPos::new(51 + REACTION_SEE_RANGE, 50)), "too far away");
    assert!(f.observe_reaction(0, CellPos::new(60, 40)));
    assert!(!f.observe_reaction(0, CellPos::new(60, 40)), "only the first time");
    assert!(!f.observe_reaction(9, CellPos::new(60, 40)), "no such reaction");
    assert!(f.progress.is_reaction_discovered("test_water+test_lava"));
    let found = ProgressEvent::Discovery {
        found: Discovered::Reaction("test_lava+test_water".into()),
        points: POINTS_PER_REACTION,
    };
    assert!(f.progress.events().contains(&found));
    assert_eq!(f.progress.can_research(&c, smelting), Ok(()));
}

#[test]
fn save_and_load_keep_the_progression() {
    let c = content();
    let mut sim = world(&c);
    let mut f = Factory::new(c.clone());
    f.guide = Arc::new(Guide::from_ron(GUIDE).unwrap());
    let widgets = tech(&c, "test_widgets");
    let lab = f.place(kind(&c, "test_lab"), TilePos::new(4, 4), 0, false, &mut sim).unwrap();
    f.buildings.insert(&c, lab, item(&c, "test_kit"), 3);
    f.scan(c.expect_material("test_ore"));
    f.progress.deliver(&c, Stack { item: item(&c, "test_plate"), count: 5 });
    f.progress.start_research(&c, widgets).unwrap();
    // 1.5 of 3 units.
    run(&mut f, &mut sim, 90);
    let before = f.progress.unit_progress(widgets).unwrap();
    assert_eq!(before.units_done, 1);
    assert!(f.progress.is_goal_done("build_lab"));

    let text = ron::to_string(&f.save()).expect("save");
    let mut g = Factory::new(c.clone());
    g.guide = f.guide.clone();
    g.load(ron::from_str(&text).expect("load"));
    // Events are notices for the game and are not saved.
    f.progress.drain_events().for_each(drop);
    assert_eq!(g.progress, f.progress);
    assert_eq!(g.progress.stage(), 1);
    assert_eq!(g.progress.current(), Some(widgets));
    assert!(g.progress.is_material_discovered(c.expect_material("test_raw_ore")));
    assert_eq!(g.building_count(kind(&c, "test_lab")), 1, "the counts for the guide are made again");

    // The loaded lab goes on with the research.
    drop(f);
    run(&mut g, &mut sim, 100);
    assert!(g.progress.is_researched(widgets));
    assert!(g.is_recipe_known(recipe(&c, "test_make_widget")));
}
