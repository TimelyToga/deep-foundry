//! Room machines in a real simulation: the kiln, the coke oven and the blast furnace.

mod common;

use common::*;
use foundry_content::Content;
use foundry_core::{BuildingId, CellRect, TilePos};
use foundry_factory::rooms::Problem;
use foundry_factory::{Factory, Status};
use foundry_sim::Simulation;

/// The ground starts at this cell row (tile row 12).
const GROUND: i32 = 96;

/// A room for a test: the inside is `w` × `h` tiles with its top-left inside tile at `at`. The
/// wall ring is one tile thick. `controller` and `hatches` are tiles of the ring; every other ring
/// tile gets a `wall` block, except the tiles in `gaps`.
struct RoomPlan<'a> {
    at: TilePos,
    w: i32,
    h: i32,
    wall: &'a str,
    controller: (&'a str, TilePos),
    hatches: Vec<(&'a str, TilePos)>,
    gaps: Vec<TilePos>,
}

impl RoomPlan<'_> {
    fn ring(&self) -> Vec<TilePos> {
        let (x0, y0, x1, y1) = (self.at.x - 1, self.at.y - 1, self.at.x + self.w, self.at.y + self.h);
        let mut v = vec![];
        for y in y0..=y1 {
            for x in x0..=x1 {
                if x == x0 || x == x1 || y == y0 || y == y1 {
                    v.push(TilePos::new(x, y));
                }
            }
        }
        v
    }

    /// Place the room. Returns the controller.
    fn build(&self, f: &mut Factory, sim: &mut Simulation) -> BuildingId {
        let c = f.content.clone();
        let ctrl = f.place(kind(&c, self.controller.0), self.controller.1, 0, false, sim).expect("place the controller");
        for (h, t) in &self.hatches {
            f.place(kind(&c, h), *t, 0, false, sim).expect("place a hatch");
        }
        for t in self.ring() {
            if t == self.controller.1 || self.hatches.iter().any(|(_, h)| *h == t) || self.gaps.contains(&t) {
                continue;
            }
            f.place(kind(&c, self.wall), t, 0, false, sim).expect("place a wall");
        }
        ctrl
    }
}

/// A kiln on the ground: 3 × 2 tiles inside, the controller in the left wall, a hatch in the roof.
fn kiln_plan() -> RoomPlan<'static> {
    RoomPlan {
        at: TilePos::new(5, 9),
        w: 3,
        h: 2,
        wall: "clay_brick_wall",
        controller: ("kiln_controller", TilePos::new(4, 10)),
        hatches: vec![("kiln_hatch", TilePos::new(6, 8))],
        gaps: vec![],
    }
}

fn setup() -> (std::sync::Arc<Content>, Simulation, Factory) {
    let c = content();
    let sim = world(&c, Some(GROUND));
    let mut f = Factory::new(c.clone());
    research_all(&mut f);
    (c, sim, f)
}

fn room_temp(f: &Factory, id: BuildingId) -> i16 {
    f.buildings.room(id).and_then(|r| r.temperature).unwrap_or(-1)
}

#[test]
fn a_kiln_fires_clay_bricks_with_charcoal() {
    let (c, mut sim, mut f) = setup();
    let id = kiln_plan().build(&mut f, &mut sim);
    run(&mut f, &mut sim, 2);
    assert!(f.buildings.room(id).unwrap().is_valid(), "{:?}", f.buildings.room(id).unwrap().problem);
    f.set_recipe(id, c.factory.recipe("clay_brick")).unwrap();
    assert_eq!(f.buildings.insert(&c, id, item(&c, "raw_clay_brick"), 4), 4);
    run(&mut f, &mut sim, 20);
    let v = f.building_view(id).unwrap();
    assert_eq!(v.status, Status::TooCold);
    assert!(v.reason.contains("Put fuel in the fuel slot"), "{}", v.reason);
    // The raw bricks are recipe inputs; charcoal goes into the fuel slot.
    assert_eq!(f.buildings.insert(&c, id, item(&c, "charcoal"), 400), 400);
    let mut hottest = 0;
    let mut ticks = 0;
    while ticks < 60 * 120 {
        run(&mut f, &mut sim, 60);
        ticks += 60;
        hottest = hottest.max(room_temp(&f, id));
        if f.building_view(id).unwrap().outputs[0].count >= 4 {
            break;
        }
    }
    let v = f.building_view(id).unwrap();
    println!("{ticks} ticks, hottest {hottest} °C, fuel left {:?}, room {:?}", v.fuel.map(|x| x.units), v.room);
    assert!(hottest >= 900, "the kiln reached only {hottest} °C");
    assert_eq!(v.outputs[0].count, 4, "status {:?}: {}", v.status, v.reason);
    let r = v.room.unwrap();
    assert!(r.valid && r.tiles == 6 && r.hatches == 1, "{r:?}");
    assert!(sim.count_material(CellRect::new(40, 72, 64, 88), mat(&c, "charcoal")) > 0, "the fuel burns as cells in the room");
}

#[test]
fn an_open_room_shows_the_hole() {
    let (_c, mut sim, mut f) = setup();
    let mut plan = kiln_plan();
    let gap = TilePos::new(8, 8);
    plan.gaps.push(gap);
    let id = plan.build(&mut f, &mut sim);
    run(&mut f, &mut sim, 2);
    let room = f.buildings.room(id).unwrap();
    assert_eq!(room.problem, Some(Problem::Hole { at: gap }));
    let v = f.building_view(id).unwrap();
    assert_eq!(v.status, Status::NoRecipe);
    let r = v.room.unwrap();
    assert!(!r.valid);
    assert_eq!(r.problem_tile, Some(gap));
    assert_eq!(r.problem.as_deref(), Some("The room has a hole 4 tiles right and 2 tiles up from the controller. Close it with a wall block."));
    // With a recipe the status is "Room not valid" and the reason names the hole.
    let c = f.content.clone();
    f.set_recipe(id, c.factory.recipe("clay_brick")).unwrap();
    run(&mut f, &mut sim, 2);
    let v = f.building_view(id).unwrap();
    assert_eq!(v.status, Status::NoRoom);
    assert!(v.reason.starts_with("The room has a hole"), "{}", v.reason);
    // Close the hole: the room is valid at once (a placement makes the room check again).
    f.place(kind(&c, "clay_brick_wall"), gap, 0, false, &mut sim).unwrap();
    run(&mut f, &mut sim, 1);
    assert!(f.buildings.room(id).unwrap().is_valid());
}

/// Run until `done` is true or `max` ticks passed. Returns the ticks run and the hottest room
/// temperature.
fn run_until(f: &mut Factory, sim: &mut Simulation, id: BuildingId, max: usize, done: impl Fn(&Factory, &Simulation) -> bool) -> (usize, i16) {
    let (mut ticks, mut hottest) = (0, i16::MIN);
    while ticks < max && !done(f, sim) {
        run(f, sim, 30);
        ticks += 30;
        hottest = hottest.max(room_temp(f, id));
    }
    (ticks, hottest)
}

fn output_count(f: &Factory, id: BuildingId, item_id: &str) -> u32 {
    let it = item(&f.content, item_id);
    f.buildings.outputs(&f.content, id).iter().filter(|s| s.item == it).map(|s| s.count).sum()
}

#[test]
fn a_kiln_makes_charcoal_from_wood() {
    let (c, mut sim, mut f) = setup();
    let id = kiln_plan().build(&mut f, &mut sim);
    f.set_recipe(id, c.factory.recipe("charcoal")).unwrap();
    // Wood is a recipe input and a fuel: the input buffer (16 crafts of 32) fills first, then the
    // fuel slot.
    let wood = item(&c, "wood");
    assert_eq!(f.buildings.insert(&c, id, wood, 512 + 300), 812);
    let v = f.building_view(id).unwrap();
    assert_eq!((v.inputs[0].count, v.fuel.unwrap().units), (512, 300));
    let (ticks, hottest) = run_until(&mut f, &mut sim, id, 60 * 60, |f, _| output_count(f, id, "charcoal") >= 48);
    println!("charcoal after {ticks} ticks, hottest {hottest} °C");
    assert!(output_count(&f, id, "charcoal") >= 48, "{:?}", f.building_view(id));
    assert!((300..900).contains(&hottest), "a wood fire is hot enough for charcoal, not for bricks: {hottest}");
}

#[test]
fn a_wood_fire_is_too_cold_for_bricks() {
    let (c, mut sim, mut f) = setup();
    let id = kiln_plan().build(&mut f, &mut sim);
    f.set_recipe(id, c.factory.recipe("clay_brick")).unwrap();
    f.buildings.insert(&c, id, item(&c, "raw_clay_brick"), 4);
    f.buildings.insert(&c, id, item(&c, "wood"), 400);
    run(&mut f, &mut sim, 60 * 20);
    let v = f.building_view(id).unwrap();
    assert_eq!(v.status, Status::TooCold);
    assert!(v.reason.contains("The fire heats it"), "{}", v.reason);
    assert!(room_temp(&f, id) > 500, "the wood burns: {}", room_temp(&f, id));
}

#[test]
fn hatches_take_items_from_a_crate_on_the_roof_and_give_bricks_to_a_crate_below() {
    let (c, mut sim, mut f) = setup();
    // A kiln in the air: 2 × 1 tiles inside. Roof hatch at (5, 4) with a crate on it, floor hatch
    // at (6, 6) with a crate under it.
    let plan = RoomPlan {
        at: TilePos::new(5, 5),
        w: 2,
        h: 1,
        wall: "clay_brick_wall",
        controller: ("kiln_controller", TilePos::new(4, 5)),
        hatches: vec![("kiln_hatch", TilePos::new(5, 4)), ("kiln_hatch", TilePos::new(6, 6))],
        gaps: vec![],
    };
    let id = plan.build(&mut f, &mut sim);
    let top = f.place(kind(&c, "crate"), TilePos::new(5, 3), 0, false, &mut sim).unwrap();
    let below = f.place(kind(&c, "crate"), TilePos::new(6, 7), 0, false, &mut sim).unwrap();
    f.set_recipe(id, c.factory.recipe("clay_brick")).unwrap();
    let (raw, brick, charcoal) = (item(&c, "raw_clay_brick"), item(&c, "clay_brick"), item(&c, "charcoal"));
    assert_eq!(f.buildings.insert(&c, top, raw, 8), 8);
    assert_eq!(f.buildings.insert(&c, top, charcoal, 400), 400);
    let (ticks, _) = run_until(&mut f, &mut sim, id, 60 * 60, |f, _| f.buildings.inventory(below).unwrap().count(brick) >= 8);
    let r = f.buildings.room(id).unwrap();
    let shape = r.shape.as_ref().unwrap();
    assert_eq!(shape.hatches.len(), 2);
    assert!(shape.hatches[0].takes_in() && !shape.hatches[0].gives_out(), "the roof hatch only takes in");
    assert!(!shape.hatches[1].takes_in() && shape.hatches[1].gives_out(), "the floor hatch only gives out");
    assert_eq!(f.buildings.inventory(below).unwrap().count(brick), 8, "after {ticks} ticks: {:?}", f.building_view(id));
    assert_eq!(f.buildings.inventory(top).unwrap().count(raw), 0);
    // The crate below also got the ash of the fire; the crate on top keeps no bricks.
    assert_eq!(f.buildings.inventory(top).unwrap().count(brick), 0);
    assert!(f.buildings.inventory(below).unwrap().count(item(&c, "ash")) > 0);
}

#[test]
fn powder_that_falls_on_a_roof_hatch_goes_in() {
    let (c, mut sim, mut f) = setup();
    let id = kiln_plan().build(&mut f, &mut sim);
    f.set_recipe(id, c.factory.recipe("clay_brick")).unwrap();
    // 64 charcoal cells on top of the roof hatch at tile (6, 8), between two stone posts.
    let stone = mat(&c, "stone");
    fill(&mut sim, CellRect::new(46, 40, 48, 64), stone, None);
    fill(&mut sim, CellRect::new(56, 40, 58, 64), stone, None);
    fill(&mut sim, CellRect::new(48, 48, 56, 56), mat(&c, "charcoal"), None);
    run(&mut f, &mut sim, 120);
    assert_eq!(sim.count_material(CellRect::new(46, 40, 58, 64), mat(&c, "charcoal")), 0);
    let fuel = f.building_view(id).unwrap().fuel.unwrap();
    assert_eq!(fuel.material, Some(mat(&c, "charcoal")));
    // The fire does not start before there is work: all 64 units are still in the slot.
    assert_eq!(fuel.units, 64);
}

#[test]
fn room_problems_too_big_wrong_wall_no_hatch() {
    // Too big: 6 × 5 = 30 tiles inside a kiln (limit 24).
    let (c, mut sim, mut f) = setup();
    let mut plan = kiln_plan();
    plan.at = TilePos::new(3, 6);
    (plan.w, plan.h) = (6, 5);
    plan.controller.1 = TilePos::new(2, 8);
    plan.hatches[0].1 = TilePos::new(4, 5);
    let id = plan.build(&mut f, &mut sim);
    run(&mut f, &mut sim, 1);
    assert_eq!(f.buildings.room(id).unwrap().problem, Some(Problem::TooBig { tiles: 30, limit: 24 }));

    // A blast furnace with clay brick walls: wrong wall block.
    let (_, mut sim, mut f) = setup();
    let mut plan = kiln_plan();
    plan.controller.0 = "blast_furnace_controller";
    plan.hatches[0].0 = "blast_furnace_hatch";
    let id = plan.build(&mut f, &mut sim);
    run(&mut f, &mut sim, 1);
    let p = f.buildings.room(id).unwrap().problem.clone();
    assert!(matches!(p, Some(Problem::WrongWall { ref name, .. }) if name == "Clay brick wall"), "{p:?}");
    let text = f.building_view(id).unwrap().room.unwrap().problem.unwrap();
    assert!(text.ends_with("This room needs Firebrick wall."), "{text}");

    // No hatch.
    let (_, mut sim, mut f) = setup();
    let mut plan = kiln_plan();
    plan.hatches.clear();
    let id = plan.build(&mut f, &mut sim);
    run(&mut f, &mut sim, 1);
    assert_eq!(f.buildings.room(id).unwrap().problem, Some(Problem::NoHatch));

    // A controller that stands alone.
    let (_, mut sim, mut f) = setup();
    let id = f.place(kind(&c, "kiln_controller"), TilePos::new(4, 10), 0, false, &mut sim).unwrap();
    run(&mut f, &mut sim, 1);
    assert_eq!(f.buildings.room(id).unwrap().problem, Some(Problem::NotInWall));
}

#[test]
fn removing_a_wall_opens_the_room() {
    let (c, mut sim, mut f) = setup();
    let id = kiln_plan().build(&mut f, &mut sim);
    run(&mut f, &mut sim, 1);
    assert!(f.buildings.room(id).unwrap().is_valid());
    let wall = f.buildings.at_tile(TilePos::new(8, 11), foundry_content::Layer::Front).unwrap();
    f.remove(wall, &mut sim).unwrap();
    run(&mut f, &mut sim, 1);
    assert_eq!(f.buildings.room(id).unwrap().problem, Some(Problem::Hole { at: TilePos::new(8, 11) }));
    let _ = c;
}

/// A firebrick room in the air, 2 × 2 tiles inside: the controller in the left wall, a roof
/// hatch, and two taps in the right wall (the lower one at tile row 7).
fn furnace_plan<'a>(controller: &'a str, hatch: &'a str) -> RoomPlan<'a> {
    RoomPlan {
        at: TilePos::new(5, 6),
        w: 2,
        h: 2,
        wall: "firebrick_wall",
        controller: (controller, TilePos::new(4, 6)),
        hatches: vec![(hatch, TilePos::new(5, 5)), (hatch, TilePos::new(7, 7)), (hatch, TilePos::new(7, 6))],
        gaps: vec![],
    }
}

/// Cells of a material outside the furnace (its ring is tiles 4 to 7: cells 32 to 64).
fn outside_furnace(sim: &Simulation, m: foundry_core::MaterialId) -> usize {
    count_all(sim, m) - sim.count_material(CellRect::new(32, 32, 64, 72), m)
}

#[test]
fn a_coke_oven_makes_coke_and_creosote() {
    let (c, mut sim, mut f) = setup();
    let id = furnace_plan("coke_oven_controller", "coke_oven_hatch").build(&mut f, &mut sim);
    run(&mut f, &mut sim, 1);
    assert!(f.buildings.room(id).unwrap().is_valid(), "{:?}", f.buildings.room(id).unwrap().problem);
    f.set_recipe(id, c.factory.recipe("coke")).unwrap();
    let coal = item(&c, "raw_coal");
    // Coal is the recipe input and also a fuel: the input buffer (4 crafts of 32) fills first.
    assert_eq!(f.buildings.insert(&c, id, coal, 128 + 400), 528);
    let creosote = mat(&c, "creosote");
    let (ticks, hottest) = run_until(&mut f, &mut sim, id, 60 * 90, |f, sim| {
        let m = |id: &str| mat(&f.content, id);
        outside_furnace(sim, m("coke")) >= 48 && outside_furnace(sim, m("creosote")) >= 16
    });
    let coke = outside_furnace(&sim, mat(&c, "coke"));
    println!("coke oven: {coke} coke after {ticks} ticks, hottest {hottest} °C");
    assert!(coke >= 48, "{:?}", f.building_view(id));
    assert!(outside_furnace(&sim, creosote) >= 16, "creosote comes out of the taps");
    assert!(hottest >= 600);
}

#[test]
fn a_blast_furnace_makes_pig_iron_and_slag_with_a_blast() {
    let (c, mut sim, mut f) = setup();
    let id = furnace_plan("blast_furnace_controller", "blast_furnace_hatch").build(&mut f, &mut sim);
    // A barrel at each tap: the lower tap (tile row 7) and the upper tap (tile row 6).
    let low = f.place(kind(&c, "barrel"), TilePos::new(8, 7), 0, false, &mut sim).unwrap();
    let high = f.place(kind(&c, "barrel"), TilePos::new(8, 6), 0, false, &mut sim).unwrap();
    f.set_recipe(id, c.factory.recipe("pig_iron_smelting")).unwrap();
    for (it, n) in [("crushed_magnetite", 64), ("coke", 32 + 800), ("crushed_limestone", 16)] {
        f.buildings.insert(&c, id, item(&c, it), n);
    }
    // Without a blast, a coke fire is too cold for iron.
    run(&mut f, &mut sim, 60 * 15);
    let t = room_temp(&f, id);
    assert!((1000..1400).contains(&t), "coke alone: {t} °C");
    assert_eq!(f.building_view(id).unwrap().status, Status::TooCold);
    // Bellows or a blower make the fire 300 °C hotter.
    let (iron, slag) = (item(&c, "molten_pig_iron"), item(&c, "molten_slag"));
    let (mut ticks, mut hottest) = (0, 0);
    while ticks < 60 * 60 && f.buildings.inventory(low).unwrap().count(iron) < 56 {
        f.buildings.set_blast(id, 300);
        run(&mut f, &mut sim, 30);
        ticks += 30;
        hottest = hottest.max(room_temp(&f, id));
    }
    let (lo, hi) = (f.buildings.inventory(low).unwrap(), f.buildings.inventory(high).unwrap());
    println!("blast furnace: {} iron, {} slag after {ticks} ticks, hottest {hottest} °C", lo.count(iron), hi.count(slag));
    // 4 crafts: 56 iron in the lower barrel, 16 slag in the upper one. The heavy iron takes the
    // low tap, the light slag the tap above it.
    assert_eq!(lo.count(iron), 56, "{:?}", f.building_view(id));
    assert_eq!(lo.count(slag), 0);
    run(&mut f, &mut sim, 60);
    assert_eq!(f.buildings.inventory(high).unwrap().count(slag), 16);
    assert!(hottest >= 1400);
}
