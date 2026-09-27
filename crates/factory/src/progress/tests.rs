//! Tests for research, labs, Hub milestones, discovery, the tech tree layout and the guide.

use super::*;
use foundry_content::factory_defs::{BuildingDef, MilestoneDef, PartDef, RecipeDef, TechDef};
use foundry_content::{Content, FactoryContent, ItemRef, Stack};
use foundry_core::{BuildingKindId, MaterialId, PartId, TechId};
use std::collections::BTreeMap;

/// The real materials with a small factory made for these tests.
///
/// Techs: a (points only) -> b (red kits, 4 units) -> f; a -> c (discoveries); a -> g (red kits);
/// b + c -> d (tier 1, red and blue kits) -> e (tier 2).
fn test_content() -> Content {
    let mut c = Content::load_default().expect("assets load");
    let parts: Vec<PartDef> = ron::from_str(
        r#"[
            Part(id: "red_kit", name: "Red kit", category: "research"),
            Part(id: "blue_kit", name: "Blue kit", category: "research"),
            Part(id: "test_plate", name: "Test plate", category: "intermediate"),
        ]"#,
    )
    .unwrap();
    let buildings: Vec<BuildingDef> =
        ron::from_str(r#"[Building(id: "test_lab", name: "Test lab", kind: "lab", size: (2, 2), tier: 0, body: "stone")]"#)
            .unwrap();
    let recipes: Vec<RecipeDef> = ron::from_str(
        r#"[
            Recipe(id: "free_plate", category: "hand", inputs: [("sand", 4)], outputs: [("test_plate", 1)], time: 1.0),
            Recipe(id: "make_red_kit", category: "hand", inputs: [("test_plate", 1)], outputs: [("red_kit", 1)], time: 1.0),
            Recipe(id: "make_blue_kit", category: "hand", inputs: [("test_plate", 2)], outputs: [("blue_kit", 1)], time: 1.0),
        ]"#,
    )
    .unwrap();
    let techs: Vec<TechDef> = ron::from_str(
        r#"[
            Tech(id: "a", name: "A", tier: 0, discovery_points: 2),
            Tech(id: "b", name: "B", tier: 0, requires: ["a"], kits: [("red_kit", 1)], units: 4, unit_time: 1.0,
                 unlocks: ["make_red_kit"]),
            Tech(id: "c", name: "C", tier: 0, requires: ["a"], discoveries: ["sand", "reaction:water+lava"]),
            Tech(id: "d", name: "D", tier: 1, requires: ["b", "c"], kits: [("red_kit", 1), ("blue_kit", 2)], units: 2,
                 unit_time: 2.0, unlocks: ["make_blue_kit"], effects: [("belt_speed", 0.5)]),
            Tech(id: "e", name: "E", tier: 2, requires: ["d"]),
            Tech(id: "f", name: "F", tier: 0, requires: ["b"], kits: [("red_kit", 1)], units: 1, unit_time: 1.0,
                 effects: [("belt_speed", 0.25), ("dig_hardness", 1.0)]),
            Tech(id: "g", name: "G", tier: 0, requires: ["a"], kits: [("red_kit", 1)], units: 2, unit_time: 1.0),
        ]"#,
    )
    .unwrap();
    let milestones: Vec<MilestoneDef> = ron::from_str(
        r#"[
            Milestone(stage: 2, name: "Second", deliver: [("test_plate", 5)], unlocks_tier: 2),
            Milestone(stage: 1, name: "First", deliver: [("test_plate", 10), ("clay", 100)], unlocks_tier: 1),
        ]"#,
    )
    .unwrap();
    let mut errors = vec![];
    c.factory = FactoryContent::build(&c.materials, parts, buildings, recipes, techs, milestones, &mut errors);
    assert!(errors.is_empty(), "{errors:?}");
    c
}

fn tech(c: &Content, id: &str) -> TechId {
    c.factory.tech(id).unwrap_or_else(|| panic!("tech {id}"))
}

fn part(c: &Content, id: &str) -> PartId {
    c.factory.part(id).unwrap_or_else(|| panic!("part {id}"))
}

fn stack(c: &Content, id: &str, count: u32) -> Stack {
    Stack { item: c.item(id).unwrap_or_else(|| panic!("item {id}")), count }
}

fn kits(c: &Content, list: &[(&str, u32)]) -> KitBuffer {
    let mut k = KitBuffer::new();
    for (id, n) in list {
        k.add(part(c, id), *n);
    }
    k
}

/// Run labs until one reports `Done` or the tick limit is reached. Returns the ticks used.
fn run_labs(p: &mut Progress, c: &Content, labs: &mut [(f32, KitBuffer)], max_ticks: u32) -> u32 {
    for tick in 1..=max_ticks {
        for (speed, buffer) in labs.iter_mut() {
            if let LabStatus::Done(_) = p.lab_tick(c, *speed, buffer) {
                return tick;
            }
        }
    }
    panic!("research did not finish in {max_ticks} ticks");
}

#[test]
fn research_needs_requirements_points_and_discoveries() {
    let c = test_content();
    let mut p = Progress::new(&c);
    let (a, b, cc) = (tech(&c, "a"), tech(&c, "b"), tech(&c, "c"));

    assert_eq!(p.can_research(&c, b), Err(LockReason::NeedsTech(a)));
    assert_eq!(p.can_research(&c, a), Err(LockReason::NeedsPoints { need: 2, have: 0 }));
    assert!(p.start_research(&c, a).is_err());
    assert_eq!(p.current(), None);

    assert!(p.discover_material(c.expect_material("sand")));
    assert!(p.discover_material(c.expect_material("clay")));
    assert_eq!(p.discovery_points(), 2);

    // A has no kits: it is done as soon as it starts, and it spends its points.
    assert_eq!(p.start_research(&c, a), Ok(()));
    assert!(p.is_researched(a));
    assert_eq!(p.current(), None);
    assert_eq!(p.discovery_points(), 0);
    assert!(p.events().contains(&ProgressEvent::TechDone(a)));
    assert_eq!(p.can_research(&c, a), Err(LockReason::AlreadyDone));

    // C needs a scan of sand (done) and the water + lava reaction.
    assert_eq!(p.lock_reasons(&c, cc), vec![LockReason::NeedsDiscovery("reaction:water+lava".into())]);
    assert_eq!(
        LockReason::NeedsDiscovery("reaction:water+lava".into()).text(&c),
        "Needs discovery: reaction Water + Lava"
    );
    assert!(p.discover_reaction("lava+water"));
    assert_eq!(p.can_research(&c, cc), Ok(()));
    assert_eq!(p.can_research(&c, b), Ok(()));
}

#[test]
fn recipes_are_known_from_the_start_or_after_their_tech() {
    let c = test_content();
    let mut p = Progress::new(&c);
    let free = c.factory.recipe("free_plate").unwrap();
    let red = c.factory.recipe("make_red_kit").unwrap();
    assert!(p.is_recipe_known(&c, free));
    assert!(!p.is_recipe_known(&c, red));
    assert_eq!(p.known_recipes(&c), vec![free]);

    p.debug_complete(&c, tech(&c, "b"));
    assert!(p.is_researched(tech(&c, "a")), "debug_complete also finishes the requirements");
    assert!(p.is_recipe_known(&c, red));
    assert_eq!(p.known_recipes(&c), vec![free, red]);
}

#[test]
fn milestones_open_research_tiers() {
    let c = test_content();
    let mut p = Progress::new(&c);
    let (d, e) = (tech(&c, "d"), tech(&c, "e"));
    p.debug_complete(&c, tech(&c, "b"));
    p.debug_complete(&c, tech(&c, "c"));

    assert_eq!(p.unlocked_tier(), 0);
    assert_eq!(p.can_research(&c, d), Err(LockReason::NeedsTier { tier: 1, stage: Some(1) }));
    assert_eq!(p.can_research(&c, d).unwrap_err().text(&c), "Needs tier 1 (repair stage 1 of the Hub)");
    assert_eq!(p.queue_research(&c, d), Err(LockReason::NeedsTier { tier: 1, stage: Some(1) }));
    assert!(p.queue().is_empty(), "a failed queue call changes nothing");

    assert_eq!(p.deliver(&c, stack(&c, "test_plate", 10)), 10);
    assert_eq!(p.deliver(&c, stack(&c, "clay", 100)), 100);
    assert_eq!(p.stage(), 1);
    assert_eq!(p.unlocked_tier(), 1);
    assert_eq!(p.can_research(&c, d), Ok(()));
    assert_eq!(p.lock_reasons(&c, e), vec![LockReason::NeedsTier { tier: 2, stage: Some(2) }, LockReason::NeedsTech(d)]);
}

#[test]
fn deliveries_across_a_stage_boundary() {
    let c = test_content();
    let mut p = Progress::new(&c);
    let plate = c.item("test_plate").unwrap();
    let clay = c.item("clay").unwrap();

    assert_eq!(p.next_milestone(&c).unwrap().stage, 1);
    assert_eq!(p.deliver(&c, stack(&c, "sand", 50)), 0, "the stage does not need sand");
    assert_eq!(p.deliver(&c, stack(&c, "test_plate", 15)), 10, "the Hub takes only what the stage needs");
    assert_eq!(p.hub_wants(&c, plate), 0);
    assert_eq!(p.hub_wants(&c, clay), 100);
    assert_eq!(p.deliver(&c, stack(&c, "clay", 30)), 30);
    let view = p.milestone_view(&c).unwrap();
    assert_eq!(view.stage, 1);
    assert_eq!(view.items[0], DeliveryView { item: plate, need: 10, delivered: 10 });
    assert_eq!(view.items[1], DeliveryView { item: clay, need: 100, delivered: 30 });
    assert_eq!(p.stage(), 0);

    // This delivery completes stage 1. The rest of the stack is not taken.
    assert_eq!(p.deliver(&c, stack(&c, "clay", 80)), 70);
    assert_eq!(p.stage(), 1);
    assert_eq!(p.unlocked_tier(), 1);
    assert!(p.events().contains(&ProgressEvent::StageDone { stage: 1, tier: 1 }));

    // Stage 2 starts empty.
    let view = p.milestone_view(&c).unwrap();
    assert_eq!(view.stage, 2);
    assert_eq!(view.items, vec![DeliveryView { item: plate, need: 5, delivered: 0 }]);
    assert_eq!(p.deliver(&c, stack(&c, "test_plate", 15)), 5);
    assert_eq!(p.stage(), 2);
    assert_eq!(p.unlocked_tier(), 2);

    // All stages are done.
    assert!(p.milestone_view(&c).is_none());
    assert_eq!(p.deliver(&c, stack(&c, "test_plate", 15)), 0);
}

#[test]
fn two_labs_use_one_kit_per_unit() {
    let c = test_content();
    let mut p = Progress::new(&c);
    let (a, b) = (tech(&c, "a"), tech(&c, "b"));
    let red = part(&c, "red_kit");
    p.debug_complete(&c, a);
    p.start_research(&c, b).unwrap();
    assert_eq!(p.current(), Some(b));

    // B: 4 units of 1 second. Two labs at speed 1 need 4 × 60 / 2 = 120 ticks.
    let mut labs = [(1.0, kits(&c, &[("red_kit", 10)])), (1.0, kits(&c, &[("red_kit", 10)]))];
    assert_eq!(run_labs(&mut p, &c, &mut labs, 1000), 120);
    assert!(p.is_researched(b));
    assert_eq!(labs[0].1.count(red), 8);
    assert_eq!(labs[1].1.count(red), 8);
    assert_eq!(labs[0].1.partly_used(red), 0.0);
    assert_eq!(labs[1].1.partly_used(red), 0.0);
    assert_eq!(p.lab_tick(&c, 1.0, &mut labs[0].1), LabStatus::NoResearch);
}

#[test]
fn labs_with_different_speeds_share_the_work() {
    let c = test_content();
    let mut p = Progress::new(&c);
    let b = tech(&c, "b");
    let red = part(&c, "red_kit");
    p.debug_complete(&c, tech(&c, "a"));
    p.start_research(&c, b).unwrap();

    // 4 units at 1.5 units per 60 ticks: 160 ticks. The fast lab does 2 2/3 units, the slow lab 1 1/3.
    let mut labs = [(1.0, kits(&c, &[("red_kit", 10)])), (0.5, kits(&c, &[("red_kit", 10)]))];
    assert_eq!(run_labs(&mut p, &c, &mut labs, 1000), 160);
    assert_eq!(labs[0].1.count(red), 7, "the fast lab started 3 kits");
    assert_eq!(labs[1].1.count(red), 8, "the slow lab started 2 kits");
    // The used kits are exactly 4: the started kits minus their unused rest.
    let used: f64 = labs.iter().map(|(_, k)| (10 - k.count(red)) as f64 - k.partly_used(red)).sum();
    assert!((used - 4.0).abs() < 1e-6, "{used}");
    assert!((labs[0].1.partly_used(red) - 1.0 / 3.0).abs() < 1e-6);
}

#[test]
fn a_lab_without_kits_waits_and_uses_nothing() {
    let c = test_content();
    let mut p = Progress::new(&c);
    let d = tech(&c, "d");
    let (red, blue) = (part(&c, "red_kit"), part(&c, "blue_kit"));
    p.debug_complete(&c, tech(&c, "b"));
    p.debug_complete(&c, tech(&c, "c"));
    p.debug_unlock_tier(1);
    p.start_research(&c, d).unwrap();

    let mut empty = KitBuffer::new();
    assert_eq!(
        p.lab_tick(&c, 1.0, &mut empty),
        LabStatus::MissingKits(vec![stack(&c, "red_kit", 1), stack(&c, "blue_kit", 2)])
    );
    // Red kits only: the lab still waits, and it does not use the red kit.
    let mut only_red = kits(&c, &[("red_kit", 5)]);
    assert_eq!(p.lab_tick(&c, 1.0, &mut only_red), LabStatus::MissingKits(vec![stack(&c, "blue_kit", 2)]));
    assert_eq!(only_red.count(red), 5);
    assert_eq!(p.tech_progress(&c, d), 0.0);

    // With both kits the lab works. The first tick starts one kit of each type. D needs 2 blue
    // kits per unit, so one blue kit is enough for half a unit.
    let mut both = kits(&c, &[("red_kit", 5), ("blue_kit", 5)]);
    assert_eq!(p.lab_tick(&c, 1.0, &mut both), LabStatus::Working);
    assert_eq!(both.count(red), 4);
    assert_eq!(both.count(blue), 4);
    assert!(p.tech_progress(&c, d) > 0.0);
    // D: 2 units of 2 seconds = 240 ticks at speed 1. One tick is done.
    let mut labs = [(1.0, both)];
    assert_eq!(run_labs(&mut p, &c, &mut labs, 1000), 239);
    assert_eq!(labs[0].1.count(red), 3);
    assert_eq!(labs[0].1.count(blue), 1);
}

#[test]
fn queue_adds_requirements_and_runs_in_order() {
    let c = test_content();
    let mut p = Progress::new(&c);
    let (a, b, f) = (tech(&c, "a"), tech(&c, "b"), tech(&c, "f"));

    // F needs B, and B needs A. A needs 2 points, so nothing can start yet.
    assert_eq!(p.queue_research(&c, f), Ok(()));
    assert_eq!(p.queue(), &[a, b, f]);
    assert_eq!(p.current(), None);
    p.tick(&c);
    assert_eq!(p.current(), None);

    // With points, A starts and is done at once. Then B starts.
    p.add_discovery_points(2);
    p.tick(&c);
    assert!(p.is_researched(a));
    assert_eq!(p.current(), Some(b));
    assert_eq!(p.queue(), &[f]);

    // Queueing a technology again changes nothing.
    assert_eq!(p.queue_research(&c, f), Ok(()));
    assert_eq!(p.queue(), &[f]);

    // When B is done, F starts in the same tick.
    let mut labs = [(1.0, kits(&c, &[("red_kit", 10)]))];
    run_labs(&mut p, &c, &mut labs, 1000);
    assert!(p.is_researched(b));
    assert_eq!(p.current(), Some(f));
    assert!(p.queue().is_empty());
    run_labs(&mut p, &c, &mut labs, 1000);
    assert!(p.is_researched(f));
    assert_eq!(p.current(), None);
    assert_eq!(p.queue_research(&c, f), Err(LockReason::AlreadyDone));
}

#[test]
fn cancel_removes_dependents_and_keeps_progress() {
    let c = test_content();
    let mut p = Progress::new(&c);
    let (b, f) = (tech(&c, "b"), tech(&c, "f"));
    p.debug_complete(&c, tech(&c, "a"));
    p.queue_research(&c, f).unwrap();
    assert_eq!(p.current(), Some(b));
    assert_eq!(p.queue(), &[f]);

    // One unit of B (60 ticks).
    let mut buffer = kits(&c, &[("red_kit", 10)]);
    for _ in 0..60 {
        assert_eq!(p.lab_tick(&c, 1.0, &mut buffer), LabStatus::Working);
    }
    assert_eq!(p.unit_progress(b).unwrap().units_done, 1);

    // Stopping B also removes F, which needs B.
    assert!(p.cancel_research(&c, b));
    assert_eq!(p.current(), None);
    assert!(p.queue().is_empty());
    assert!(!p.cancel_research(&c, b), "B is not running now");
    assert!((p.tech_progress(&c, b) - 0.25).abs() < 1e-6, "B keeps its progress");

    // B continues from where it stopped: 3 more units.
    p.start_research(&c, b).unwrap();
    assert_eq!(run_labs(&mut p, &c, &mut [(1.0, buffer)], 1000), 180);
}

#[test]
fn start_research_switches_and_keeps_progress() {
    let c = test_content();
    let mut p = Progress::new(&c);
    let (b, g) = (tech(&c, "b"), tech(&c, "g"));
    p.debug_complete(&c, tech(&c, "a"));
    p.start_research(&c, b).unwrap();
    let mut buffer = kits(&c, &[("red_kit", 20)]);
    for _ in 0..90 {
        p.lab_tick(&c, 1.0, &mut buffer);
    }

    // G takes over. B waits at the front of the queue with 1.5 units done.
    p.start_research(&c, g).unwrap();
    assert_eq!(p.current(), Some(g));
    assert_eq!(p.queue(), &[b]);
    assert!((p.tech_progress(&c, b) - 1.5 / 4.0).abs() < 1e-6);
    let views = p.tech_views(&c);
    assert_eq!(views[b.0 as usize].state, TechState::Available);
    assert_eq!(views[b.0 as usize].queue_position, Some(0));

    // When G is done, B runs again and needs 2.5 more units (150 ticks). The half-used kit
    // from B's first run is used for G.
    let mut labs = [(1.0, buffer)];
    assert_eq!(run_labs(&mut p, &c, &mut labs, 1000), 120);
    assert_eq!(p.current(), Some(b));
    assert_eq!(run_labs(&mut p, &c, &mut labs, 1000), 150);
    // 4 units of B + 2 units of G = 6 kits.
    let red = part(&c, "red_kit");
    assert_eq!(labs[0].1.count(red), 14);
    assert_eq!(labs[0].1.partly_used(red), 0.0);
}

#[test]
fn discovery_gives_points_once() {
    let c = test_content();
    let mut p = Progress::new(&c);
    let sand = c.expect_material("sand");

    assert!(!p.discover_material(MaterialId::AIR), "air gives nothing");
    assert!(p.discover_material(sand));
    assert!(!p.discover_material(sand));
    assert_eq!(p.discovery_points(), POINTS_PER_MATERIAL);
    assert!(p.is_material_discovered(sand));

    assert!(p.discover_reaction("reaction:water+lava"));
    assert!(!p.discover_reaction("lava + water"), "the order of the two sides does not matter");
    assert!(p.is_reaction_discovered("water+lava"));
    assert_eq!(p.discovery_points(), POINTS_PER_MATERIAL + POINTS_PER_REACTION);

    let events: Vec<ProgressEvent> = p.drain_events().collect();
    assert_eq!(
        events,
        vec![
            ProgressEvent::Discovery { found: Discovered::Material(sand), points: POINTS_PER_MATERIAL },
            ProgressEvent::Discovery { found: Discovered::Reaction("lava+water".into()), points: POINTS_PER_REACTION },
        ]
    );
    assert!(p.events().is_empty());

    // The key of the real water + lava reaction is the same key.
    let r = c
        .reactions
        .iter()
        .find(|r| {
            r.a == foundry_content::Matcher::Material(c.expect_material("water"))
                && r.b == foundry_content::Matcher::Material(c.expect_material("lava"))
        })
        .expect("water + lava reaction");
    assert_eq!(reaction_key(&c, r), "lava+water");
}

#[test]
fn effects_are_the_sum_of_done_techs() {
    let c = test_content();
    let mut p = Progress::new(&c);
    assert_eq!(p.effect("belt_speed"), 0.0);
    p.debug_complete(&c, tech(&c, "f"));
    assert_eq!(p.effect("belt_speed"), 0.25);
    p.debug_complete(&c, tech(&c, "d"));
    assert_eq!(p.effect("belt_speed"), 0.75);
    assert_eq!(p.effect("dig_hardness"), 1.0);
    assert_eq!(p.effect("no_such_effect"), 0.0);
}

#[test]
fn tech_views_show_state_cost_and_reasons() {
    let c = test_content();
    let mut p = Progress::new(&c);
    let (a, b, cc, d) = (tech(&c, "a"), tech(&c, "b"), tech(&c, "c"), tech(&c, "d"));
    p.debug_complete(&c, a);
    p.start_research(&c, b).unwrap();
    let mut buffer = kits(&c, &[("red_kit", 10)]);
    for _ in 0..120 {
        p.lab_tick(&c, 1.0, &mut buffer);
    }

    let views = p.tech_views(&c);
    assert_eq!(views.len(), c.factory.techs.len());
    assert_eq!(views[a.0 as usize].state, TechState::Done);
    match views[b.0 as usize].state {
        TechState::Researching { percent } => assert!((percent - 50.0).abs() < 1e-3, "{percent}"),
        ref s => panic!("{s:?}"),
    }
    let vc = &views[cc.0 as usize];
    assert!(matches!(vc.state, TechState::Locked(_)));
    assert_eq!(vc.reasons, vec!["Needs discovery: Sand", "Needs discovery: reaction Water + Lava"]);
    assert_eq!(vc.cost.discoveries.len(), 2);
    assert!(!vc.cost.discoveries[0].done);
    let vd = &views[d.0 as usize];
    assert_eq!(vd.cost.kits, vec![stack(&c, "red_kit", 1), stack(&c, "blue_kit", 2)]);
    assert_eq!(vd.cost.units, 2);
    assert_eq!(vd.unlocks, vec![c.factory.recipe("make_blue_kit").unwrap()]);
    assert_eq!(vd.requires, vec![b, cc]);
    assert_eq!(
        vd.reasons,
        vec!["Needs tier 1 (repair stage 1 of the Hub)".to_string(), "Needs B".to_string(), "Needs C".to_string()]
    );

    let status = p.research_status(&c).unwrap();
    assert_eq!(status.tech, b);
    assert_eq!(status.units_done, 2);
    assert_eq!(status.units, 4);
    assert!((status.progress - 0.5).abs() < 1e-6);
    assert_eq!(status.kits, vec![stack(&c, "red_kit", 1)]);
    assert_eq!(p.current_kits(&c), &[stack(&c, "red_kit", 1)]);
}

fn assert_layout_ok(factory: &FactoryContent) {
    let pos = layout_techs(factory);
    assert_eq!(pos.len(), factory.techs.len());
    for (i, t) in factory.techs.iter().enumerate() {
        for r in &t.requires {
            assert!(
                pos[i].column > pos[r.0 as usize].column,
                "`{}` must be right of its requirement `{}`",
                t.id,
                factory.tech_def(*r).id
            );
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    for p in &pos {
        assert!(seen.insert((p.column, p.row)), "two techs at {p:?}");
    }
}

#[test]
fn layout_places_every_tech_after_its_requirements() {
    assert_layout_ok(&test_content().factory);
    assert_layout_ok(&Content::load_default().unwrap().factory);

    let c = test_content();
    let pos = layout_techs(&c.factory);
    assert_eq!(pos[tech(&c, "a").0 as usize].column, 0);
    assert_eq!(pos[tech(&c, "b").0 as usize].column, 1);
    assert_eq!(pos[tech(&c, "d").0 as usize].column, 2);
    assert_eq!(pos[tech(&c, "e").0 as usize].column, 3);
}

#[test]
fn layout_removes_a_simple_crossing() {
    // In data order the lines r1 -> y and r2 -> x cross. The layout must uncross them.
    let base = Content::load_default().unwrap();
    let techs: Vec<TechDef> = ron::from_str(
        r#"[
            Tech(id: "r1", name: "R1", tier: 0),
            Tech(id: "r2", name: "R2", tier: 0),
            Tech(id: "x", name: "X", tier: 0, requires: ["r2"]),
            Tech(id: "y", name: "Y", tier: 0, requires: ["r1"]),
        ]"#,
    )
    .unwrap();
    let mut errors = vec![];
    let f = FactoryContent::build(&base.materials, vec![], vec![], vec![], techs, vec![], &mut errors);
    assert!(errors.is_empty(), "{errors:?}");
    assert_layout_ok(&f);
    let pos = layout_techs(&f);
    let row = |id: &str| pos[f.tech(id).unwrap().0 as usize].row;
    assert_eq!(row("r1") < row("r2"), row("y") < row("x"), "lines cross: {pos:?}");
}

/// A player with fixed items and buildings.
#[derive(Default)]
struct TestState {
    items: BTreeMap<ItemRef, u32>,
    buildings: BTreeMap<BuildingKindId, u32>,
}

impl GuideState for TestState {
    fn item_count(&self, item: ItemRef) -> u32 {
        self.items.get(&item).copied().unwrap_or(0)
    }
    fn building_count(&self, kind: BuildingKindId) -> u32 {
        self.buildings.get(&kind).copied().unwrap_or(0)
    }
}

const TEST_GUIDE: &str = r#"[
    Goal(id: "g_sand", tier: 0, title: "Sand", text: "Dig sand.", condition: HaveItem("sand", 10), reward_points: 3),
    Goal(id: "g_lab", tier: 0, title: "Labs", text: "Build 2 labs.", condition: Build("test_lab", 2)),
    Goal(id: "g_a", tier: 0, title: "A", text: "Research A.", condition: Research("a")),
    Goal(id: "g_scan", tier: 0, title: "Scan", text: "Scan clay or sand.", condition: Any([Discover("clay"), Discover("sand")])),
    Goal(id: "g_both", tier: 0, title: "Both", text: "Stage 1 and a plate.", condition: All([Stage(1), HaveItem("test_plate", 1)])),
    Goal(id: "g_t1", tier: 1, title: "Tier 1", text: "Later.", condition: HaveItem("sand", 1)),
    Goal(id: "g_unknown", tier: 0, title: "Unknown", text: "Never.", condition: Build("no_such_building", 1)),
]"#;

#[test]
fn guide_conditions() {
    let c = test_content();
    let guide = Guide::from_ron(TEST_GUIDE).unwrap();
    assert_eq!(guide.check(&c), vec!["guide goal `g_unknown`: `no_such_building` is not a building".to_string()]);

    let mut p = Progress::new(&c);
    let mut state = TestState::default();
    let sand = c.item("sand").unwrap();
    let lab = c.factory.building("test_lab").unwrap();

    state.items.insert(sand, 5);
    p.update_guide(&guide, &c, &state);
    assert!(!p.is_goal_done("g_sand"));
    let view = p.guide_view(&guide, &c, &state);
    assert_eq!(view.len(), 6, "the tier 1 goal is hidden");
    assert_eq!(view[0].count, Some((5, 10)));

    state.items.insert(sand, 10);
    state.buildings.insert(lab, 1);
    p.update_guide(&guide, &c, &state);
    assert!(p.is_goal_done("g_sand"));
    assert!(!p.is_goal_done("g_lab"));
    assert!(!p.is_goal_done("g_t1"), "tier 1 goals wait for tier 1");
    assert_eq!(p.discovery_points(), 3, "the goal gives its reward points");
    assert_eq!(p.events(), &[ProgressEvent::GoalDone("g_sand".into())]);

    // A done goal stays done when the items are gone.
    state.items.clear();
    state.buildings.insert(lab, 2);
    p.discover_material(c.expect_material("clay"));
    p.debug_complete(&c, tech(&c, "a"));
    p.update_guide(&guide, &c, &state);
    for id in ["g_sand", "g_lab", "g_a", "g_scan"] {
        assert!(p.is_goal_done(id), "{id}");
    }
    assert!(!p.is_goal_done("g_both"));
    assert_eq!(p.guide_view(&guide, &c, &state)[0].count, Some((10, 10)));

    // Stage 1 opens tier 1 and completes the stage goals.
    p.deliver(&c, stack(&c, "test_plate", 10));
    p.deliver(&c, stack(&c, "clay", 100));
    state.items.insert(sand, 1);
    state.items.insert(c.item("test_plate").unwrap(), 1);
    p.update_guide(&guide, &c, &state);
    assert!(p.is_goal_done("g_both"));
    assert!(p.is_goal_done("g_t1"));
    assert!(!p.is_goal_done("g_unknown"));
    assert_eq!(p.guide_view(&guide, &c, &state).len(), 7);
}

#[test]
fn guide_ids_must_be_unique() {
    let text = r#"[
        Goal(id: "same", tier: 0, title: "A", text: "A", condition: Stage(1)),
        Goal(id: "same", tier: 0, title: "B", text: "B", condition: Stage(1)),
    ]"#;
    let err = Guide::from_ron(text).unwrap_err().to_string();
    assert!(err.contains("used twice"), "{err}");
}

#[test]
fn guide_files_load() {
    let c = Content::load_default().unwrap();
    let guide = Guide::load_default().expect("guide files load");
    assert!(guide.goals.iter().filter(|g| g.tier == 0).count() >= 10);
    assert!(guide.goals.iter().filter(|g| g.tier == 1).count() >= 10);
    assert!(guide.goals.iter().any(|g| g.condition == Condition::Stage(1)));
    // The materials exist now. Many parts, buildings and techs of the full Tier 0-1 data may not
    // exist yet, so unknown names are only printed.
    for problem in guide.check(&c) {
        println!("{problem}");
    }
    for g in &guide.goals {
        let mut stack = vec![&g.condition];
        while let Some(cond) = stack.pop() {
            match cond {
                Condition::Discover(id) => assert!(c.material(id).is_some(), "goal {}: `{id}` is not a material", g.id),
                Condition::All(list) | Condition::Any(list) => stack.extend(list),
                _ => {}
            }
        }
    }
}

#[test]
fn starter_techs_in_the_real_data() {
    let c = Content::load_default().unwrap();
    let p = Progress::new(&c);
    let bronze = tech(&c, "bronze");
    let research = tech(&c, "research");
    assert!(p.lock_reasons(&c, research).contains(&LockReason::NeedsTech(bronze)));
    let alloy = c.factory.recipe("bronze_alloy").unwrap();
    assert!(!p.is_recipe_known(&c, alloy));
    let mut p = p;
    p.debug_complete(&c, bronze);
    assert!(p.is_recipe_known(&c, alloy));
}

#[test]
fn save_and_load_keeps_all_state() {
    let c = test_content();
    let mut p = Progress::new(&c);
    let (b, f) = (tech(&c, "b"), tech(&c, "f"));
    p.discover_material(c.expect_material("sand"));
    p.discover_reaction("water+lava");
    p.add_discovery_points(5);
    p.debug_complete(&c, tech(&c, "a"));
    p.queue_research(&c, f).unwrap();
    let mut buffer = kits(&c, &[("red_kit", 10), ("blue_kit", 3)]);
    for _ in 0..70 {
        p.lab_tick(&c, 1.0, &mut buffer);
    }
    p.deliver(&c, stack(&c, "test_plate", 4));
    let guide = Guide::from_ron(TEST_GUIDE).unwrap();
    let mut state = TestState::default();
    state.items.insert(c.item("sand").unwrap(), 10);
    p.update_guide(&guide, &c, &state);
    assert_eq!(p.current(), Some(b));
    p.drain_events().for_each(drop);

    let text = ron::to_string(&p).unwrap();
    let loaded: Progress = ron::from_str(&text).unwrap();
    assert_eq!(loaded, p);
    assert_eq!(loaded.unit_progress(b), p.unit_progress(b));
    assert_eq!(loaded.milestone_view(&c), p.milestone_view(&c));
    assert!(loaded.is_goal_done("g_sand"));

    let text = ron::to_string(&buffer).unwrap();
    let loaded_kits: KitBuffer = ron::from_str(&text).unwrap();
    assert_eq!(loaded_kits, buffer);
    assert!(loaded_kits.partly_used(part(&c, "red_kit")) > 0.0);
}
