//! Crafters in a real simulation: timing, ports, overclocking, power, heat, exhaust, parts.

mod common;

use common::*;
use foundry_content::{Content, ItemRef, Stack};
use foundry_core::{CellPos, CellRect, TilePos};
use foundry_factory::buildings::Buildings;
use foundry_factory::progress_link::{KitBuffer, LabStatus, ProgressLink};
use foundry_factory::{Factory, Status};

const GROUND: i32 = 96;

#[test]
fn a_crafter_turns_inputs_into_outputs_in_the_recipe_time() {
    let c = content();
    let mut sim = world(&c, Some(GROUND));
    let mut f = Factory::new(c.clone());
    let id = f.place(kind(&c, "test_press"), TilePos::new(4, 10), 0, false, &mut sim).unwrap();
    assert_eq!(f.building_view(id).unwrap().reason, "Choose a recipe");
    f.set_recipe(id, c.factory.recipe("test_press_sand")).unwrap(); // 8 sand -> 4 gravel, 1 s
    let (sand, gravel) = (item(&c, "sand"), item(&c, "gravel"));
    run(&mut f, &mut sim, 5);
    let v = f.building_view(id).unwrap();
    assert_eq!(v.status, Status::NoInput);
    assert_eq!(v.reason, "Needs 8 more Sand");
    assert_eq!(f.buildings.insert(&c, id, sand, 100), 16, "the input buffer holds 2 crafts");
    run(&mut f, &mut sim, 59);
    let v = f.building_view(id).unwrap();
    assert_eq!(v.status, Status::Working);
    assert_eq!(v.outputs[0].count, 0);
    assert!(v.progress > 0.9);
    run(&mut f, &mut sim, 1);
    let v = f.building_view(id).unwrap();
    assert_eq!(v.outputs[0], foundry_factory::BufferView::new(&c, gravel, 4, 8, 4));
    assert_eq!(v.inputs[0].count, 0, "the second craft started at once");
    run(&mut f, &mut sim, 60);
    // The output port faces the stone ground, so the gravel stays inside: 8 = full.
    let v = f.building_view(id).unwrap();
    assert_eq!(v.outputs[0].count, 8);
    assert_eq!(f.take_outputs_to_player(id), vec![Stack { item: gravel, count: 8 }]);
    assert_eq!(f.player.count(gravel), 8);
}

#[test]
fn ports_take_sand_from_above_and_put_gravel_below() {
    let c = content();
    let mut sim = world(&c, Some(GROUND));
    let mut f = Factory::new(c.clone());
    let stone = mat(&c, "stone");
    // The press stands on a stone pillar under its left tile, so its output (right tile, down)
    // has air below.
    fill(&mut sim, CellRect::new(32, 80, 40, GROUND), stone, None);
    let id = f.place(kind(&c, "test_press"), TilePos::new(4, 8), 0, false, &mut sim).unwrap();
    f.set_recipe(id, c.factory.recipe("test_press_sand")).unwrap();
    let (sand, gravel) = (mat(&c, "sand"), mat(&c, "gravel"));
    // 64 sand cells on top of the input tile, between two stone walls so none slides away.
    fill(&mut sim, CellRect::new(30, 40, 32, 64), stone, None);
    fill(&mut sim, CellRect::new(40, 40, 42, 64), stone, None);
    fill(&mut sim, CellRect::new(32, 56, 40, 64), sand, None);
    run(&mut f, &mut sim, 600);
    assert_eq!(count_all(&sim, sand), 0, "all sand went in");
    let v = f.building_view(id).unwrap();
    let in_machine = v.outputs[0].count as usize;
    assert_eq!(count_all(&sim, gravel) + in_machine, 32, "64 sand make 32 gravel");
    assert!(sim.count_material(CellRect::new(40, 80, 48, GROUND), gravel) > 0, "gravel fell below the output");
}

#[test]
fn overclocking_is_faster_and_uses_more_power() {
    let c = content();
    let mut sim = world(&c, Some(GROUND));
    let mut f = Factory::new(c.clone());
    let id = f.place(kind(&c, "test_press_mk2"), TilePos::new(4, 10), 0, false, &mut sim).unwrap();
    // A generator and cables to the power port (tile 4, 11) of the press.
    let generator = f.place(kind(&c, "test_generator"), TilePos::new(2, 11), 0, false, &mut sim).unwrap();
    for x in 2..5 {
        f.place(kind(&c, "test_cable"), TilePos::new(x, 11), 0, false, &mut sim).unwrap();
    }
    f.set_recipe(id, c.factory.recipe("test_press_sand")).unwrap();
    f.buildings.insert(&c, id, item(&c, "sand"), 8);
    run(&mut f, &mut sim, 29);
    let v = f.building_view(id).unwrap();
    assert_eq!(v.outputs[0].count, 0);
    assert_eq!(v.power_w, 400.0, "tier 1 machine, tier 0 recipe: 4 × 100 W");
    run(&mut f, &mut sim, 1);
    assert_eq!(f.building_view(id).unwrap().outputs[0].count, 4, "2 × speed: 30 ticks");
    // Without power it stops: the generator is gone.
    f.buildings.insert(&c, id, item(&c, "sand"), 8);
    f.remove(generator, &mut sim).unwrap();
    run(&mut f, &mut sim, 5);
    let v = f.building_view(id).unwrap();
    assert_eq!(v.status, Status::NoPower);
    assert!(v.reason.starts_with("No power: put a copper cable"), "{}", v.reason);
    assert_eq!(v.power_w, 5.0);
}

#[test]
fn min_temp_is_read_from_the_heat_port() {
    let c = content();
    let mut sim = world(&c, Some(GROUND));
    let mut f = Factory::new(c.clone());
    // The oven stands on the ground; its heat port reads the stone row below it.
    let id = f.place(kind(&c, "test_oven"), TilePos::new(5, 11), 0, false, &mut sim).unwrap();
    f.set_recipe(id, c.factory.recipe("test_bake_sand")).unwrap();
    f.buildings.insert(&c, id, item(&c, "sand"), 4);
    run(&mut f, &mut sim, 3);
    let v = f.building_view(id).unwrap();
    assert_eq!(v.status, Status::TooCold);
    assert_eq!(v.reason, "Too cold: 20 °C, needs 500 °C");
    // Hot stone under the oven. Heat flows away from it into the ground and the oven, so it is
    // made hot again in each tick, like a heat source.
    for _ in 0..90 {
        fill(&mut sim, CellRect::new(40, GROUND, 48, GROUND + 1), mat(&c, "stone"), Some(900));
        run(&mut f, &mut sim, 1);
    }
    let v = f.building_view(id).unwrap();
    assert_eq!(v.outputs[0].count, 4, "status {:?}", v.status);
}

#[test]
fn a_blocked_exhaust_stops_the_machine() {
    let c = content();
    let mut sim = world(&c, Some(GROUND));
    let mut f = Factory::new(c.clone());
    let id = f.place(kind(&c, "test_vent"), TilePos::new(5, 11), 0, false, &mut sim).unwrap();
    f.set_recipe(id, c.factory.recipe("test_vent_gas")).unwrap(); // 1 sand -> 4 CO2, 0.1 s
    let co2 = mat(&c, "carbon_dioxide");
    f.buildings.insert(&c, id, item(&c, "sand"), 2);
    run(&mut f, &mut sim, 20);
    assert!(count_all(&sim, co2) > 0, "gas went out of the exhaust");
    // Cover the exhaust with stone.
    fill(&mut sim, CellRect::new(40, 80, 48, 88), mat(&c, "stone"), None);
    let before = count_all(&sim, co2);
    f.buildings.insert(&c, id, item(&c, "sand"), 2);
    run(&mut f, &mut sim, 30);
    let v = f.building_view(id).unwrap();
    assert_eq!(v.status, Status::OutputBlocked, "{v:?}");
    assert!(v.reason.starts_with("Output blocked"));
    assert!(count_all(&sim, co2) <= before, "no gas went out");
    assert!(v.outputs[0].count > 0);
}

#[test]
fn part_ports_move_parts_between_crates_and_a_machine() {
    let c = content();
    let mut sim = world(&c, Some(GROUND));
    let mut f = Factory::new(c.clone());
    let row = 11;
    let crate_kind = kind(&c, "test_crate");
    let left = f.place(crate_kind, TilePos::new(4, row), 0, false, &mut sim).unwrap();
    let asm = f.place(kind(&c, "test_assembler"), TilePos::new(5, row), 0, false, &mut sim).unwrap();
    let right = f.place(crate_kind, TilePos::new(6, row), 0, false, &mut sim).unwrap();
    f.set_recipe(asm, c.factory.recipe("test_make_gear")).unwrap(); // 3 plates -> 1 gear, 0.5 s
    let (plate, gear, tin) = (item(&c, "bronze_plate"), item(&c, "bronze_gear"), item(&c, "tin_plate"));
    f.buildings.insert(&c, left, plate, 30);
    run(&mut f, &mut sim, 900);
    let out = f.buildings.inventory(right).unwrap();
    assert_eq!(f.buildings.inventory(left).unwrap().count(plate), 0);
    assert_eq!(out.count(gear), 10, "30 plates make 10 gears");
    // Byproducts (50 % chance) also go out through the part output.
    let tins = out.count(tin);
    assert!((1..10).contains(&tins), "{tins} tin plates");
    // The same setup gives the same byproducts (fixed random seeds).
    let mut sim2 = world(&c, Some(GROUND));
    let mut g = Factory::new(c.clone());
    let l2 = g.place(crate_kind, TilePos::new(4, row), 0, false, &mut sim2).unwrap();
    let a2 = g.place(kind(&c, "test_assembler"), TilePos::new(5, row), 0, false, &mut sim2).unwrap();
    let r2 = g.place(crate_kind, TilePos::new(6, row), 0, false, &mut sim2).unwrap();
    g.set_recipe(a2, c.factory.recipe("test_make_gear")).unwrap();
    g.buildings.insert(&c, l2, plate, 30);
    run(&mut g, &mut sim2, 900);
    assert_eq!(g.buildings.inventory(r2).unwrap().count(tin), tins);
    // Idle buildings sleep: the crates always, the assembler after it runs out of plates.
    assert_eq!(f.buildings.awake_count(), 0);
}

#[test]
fn a_lab_takes_kits_from_a_crate() {
    let c = content();
    let mut sim = world(&c, Some(GROUND));
    let mut f = Factory::new(c.clone());
    // The basic lab (2 × 2) has a part input on top of its top-left tile.
    let lab = f.place(kind(&c, "basic_lab"), TilePos::new(4, 10), 0, false, &mut sim).unwrap();
    // The crate floats above that tile. (Buildings do not need support.)
    let crate_id = f.place(kind(&c, "test_crate"), TilePos::new(4, 9), 0, false, &mut sim).unwrap();
    let kit = item(&c, "bronze_kit");
    f.buildings.insert(&c, crate_id, kit, 25);
    f.buildings.wake(lab);
    run(&mut f, &mut sim, 400);
    let v = f.building_view(lab).unwrap();
    assert_eq!(v.inputs.len(), 1);
    assert_eq!((v.inputs[0].item, v.inputs[0].count, v.inputs[0].capacity), (kit, 10, 10));
    assert_eq!(f.buildings.inventory(crate_id).unwrap().count(kit), 15);
    // The player can put kits in too, up to the limit.
    assert_eq!(f.buildings.room_for(&c, lab, kit), 0);
    assert_eq!(f.buildings.room_for(&c, lab, item(&c, "bronze_gear")), 0, "only kits");
}

/// A progress for tests: takes up to 5 bronze plates per delivery call and counts them; a lab
/// uses one kit of any type per call.
#[derive(Default)]
struct TestProgress {
    delivered: u32,
    kits_used: u32,
}

impl ProgressLink for TestProgress {
    fn lab_tick(&mut self, _content: &Content, _speed: f32, kits: &mut KitBuffer) -> LabStatus {
        let first = kits.iter().next();
        match first {
            Some((p, _)) => {
                self.kits_used += kits.take(p, 1);
                LabStatus::Working
            }
            None => LabStatus::MissingKits(vec![]),
        }
    }

    fn deliver(&mut self, content: &Content, stack: Stack) -> u32 {
        if stack.item != content.item("bronze_plate").unwrap() {
            return 0;
        }
        let n = stack.count.min(5);
        self.delivered += n;
        n
    }
}

#[test]
fn the_hub_delivers_to_the_progression_and_the_lab_researches() {
    let c = content();
    let mut sim = world(&c, Some(GROUND));
    let mut b = Buildings::new(&c);
    let t = foundry_factory::Transform::IDENTITY;
    let hub = b.place(&c, kind(&c, "test_hub"), TilePos::new(6, 10), t, &mut sim).unwrap();
    // A crate left of the hub's part input (top-left tile, left side).
    let crate_id = b.place(&c, kind(&c, "test_crate"), TilePos::new(5, 10), t, &mut sim).unwrap();
    let (plate, gear) = (item(&c, "bronze_plate"), item(&c, "bronze_gear"));
    b.insert(&c, crate_id, plate, 12);
    b.insert(&c, crate_id, gear, 3);
    // Items the milestones do not ask for are refused.
    assert_eq!(b.insert(&c, hub, item(&c, "tin_plate"), 1), 0);
    let lab = b.place(&c, kind(&c, "basic_lab"), TilePos::new(1, 10), t, &mut sim).unwrap();
    b.insert(&c, lab, item(&c, "bronze_kit"), 4);
    let mut p = TestProgress::default();
    for _ in 0..600 {
        b.tick(&c, &mut sim, &mut p);
        sim.tick();
    }
    assert_eq!(p.delivered, 12, "all plates went through the hub");
    assert_eq!(b.inventory(crate_id).unwrap().count(plate), 0);
    // Gears are also asked for by milestone 1, so the hub took them; the test progress refuses
    // them, so they wait in the hub.
    assert_eq!(b.inventory(hub).unwrap().count(gear), 3);
    assert_eq!(p.kits_used, 4);
    let v = b.view(&c, lab).unwrap();
    assert_eq!(v.status, Status::NoInput);
    assert_eq!(v.reason, "Needs research kits");
}

#[test]
fn a_recipe_of_another_category_or_a_higher_tier_is_refused() {
    let c = content();
    let mut sim = world(&c, Some(GROUND));
    let mut f = Factory::new(c.clone());
    let id = f.place(kind(&c, "test_press"), TilePos::new(4, 10), 0, false, &mut sim).unwrap();
    let err = f.set_recipe(id, c.factory.recipe("test_make_gear")).unwrap_err();
    assert_eq!(err.to_string(), "Test press cannot make Bronze gear");
    let v = f.building_view(id).unwrap();
    assert_eq!(v.recipes, vec![c.factory.recipe("test_press_sand").unwrap()]);
    let crate_id = f.place(kind(&c, "test_crate"), TilePos::new(8, 11), 0, false, &mut sim).unwrap();
    assert!(f.set_recipe(crate_id, None).is_err());
    assert_eq!(f.buildings.insert(&c, id, ItemRef::Material(mat(&c, "clay")), 5), 0, "not a recipe input");
}

#[test]
fn fluid_ports_take_water_and_give_steam() {
    // This test is about the fluid ports. The steam of the boiler (110 °C) would cool in the 20 °C
    // air and condense into water, so steam does not condense in this test.
    let mut changed = (*content()).clone();
    let steam_id = changed.expect_material("steam");
    changed.materials.condense[steam_id.index()] = None;
    let c = std::sync::Arc::new(changed);
    let mut sim = world(&c, Some(GROUND));
    let mut f = Factory::new(c.clone());
    let (water, steam, stone) = (mat(&c, "water"), mat(&c, "steam"), mat(&c, "stone"));
    // A boiler on the ground with a water pool on its left (fluid input on the left side).
    let id = f.place(kind(&c, "test_boiler"), TilePos::new(6, 11), 0, false, &mut sim).unwrap();
    f.set_recipe(id, c.factory.recipe("test_boil")).unwrap(); // 4 water -> 4 steam, 0.2 s
    fill(&mut sim, CellRect::new(36, 80, 40, GROUND), stone, None);
    fill(&mut sim, CellRect::new(40, 88, 48, GROUND), water, None);
    // Water in the buffers, steam in the buffers, and the 4 water of a running craft.
    let buffers = |f: &Factory| {
        let v = f.building_view(id).unwrap();
        let running = if v.progress > 0.0 { 4 } else { 0 };
        v.inputs.iter().chain(&v.outputs).map(|b| b.count as usize).sum::<usize>() + running
    };
    for _ in 0..600 {
        run(&mut f, &mut sim, 1);
        assert_eq!(count_all(&sim, water) + count_all(&sim, steam) + buffers(&f), 64, "no fluid is lost or made");
    }
    assert!(count_all(&sim, steam) >= 32, "steam went out of the top port");
    assert!(sim.count_material(CellRect::new(40, 88, 48, GROUND), water) < 32, "the pool got smaller");
}

#[test]
fn a_burner_heats_the_oven_above_it() {
    let c = content();
    let mut sim = world(&c, Some(GROUND));
    let mut f = Factory::new(c.clone());
    let fire = f.place(kind(&c, "test_campfire"), TilePos::new(5, 11), 0, false, &mut sim).unwrap();
    let oven = f.place(kind(&c, "test_oven"), TilePos::new(5, 10), 0, false, &mut sim).unwrap();
    f.set_recipe(fire, c.factory.recipe("test_burn_charcoal")).unwrap();
    f.set_recipe(oven, c.factory.recipe("test_bake_sand")).unwrap(); // needs 500 °C
    f.buildings.insert(&c, oven, item(&c, "sand"), 4);
    run(&mut f, &mut sim, 10);
    assert_eq!(f.building_view(oven).unwrap().status, Status::TooCold);
    f.buildings.insert(&c, fire, item(&c, "charcoal"), 5);
    run(&mut f, &mut sim, 200);
    assert_eq!(f.building_view(fire).unwrap().status, Status::Working);
    let v = f.building_view(oven).unwrap();
    assert_eq!(v.outputs[0].count, 4, "{v:?}");
    // The burner heated the cells on both sides of its heat port to its heat temperature. (Then
    // the heat pass of the same tick moved a little of that heat away.)
    for p in [CellPos::new(44, 87), CellPos::new(44, 88)] {
        let t = sim.cell(p).temperature;
        assert!((780..=800).contains(&t), "{p:?}: {t} °C");
    }
}

#[test]
fn items_can_be_taken_back_out_of_the_input_buffers() {
    let c = content();
    let mut sim = world(&c, Some(GROUND));
    let mut f = Factory::new(c.clone());
    let id = f.place(kind(&c, "test_press"), TilePos::new(4, 10), 0, false, &mut sim).unwrap();
    let sand = item(&c, "sand");
    assert_eq!(f.buildings.take_input(&c, id, sand, 5), 0, "no recipe: no buffers");
    f.set_recipe(id, c.factory.recipe("test_press_sand")).unwrap();
    assert_eq!(f.buildings.insert(&c, id, sand, 7), 7);
    assert_eq!(f.buildings.take_input(&c, id, sand, 5), 5);
    assert_eq!(f.building_view(id).unwrap().inputs[0].count, 2);
    assert_eq!(f.buildings.take_input(&c, id, sand, 5), 2);
    assert_eq!(f.buildings.take_input(&c, id, item(&c, "gravel"), 5), 0, "not an input of the recipe");
}
