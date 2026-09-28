//! Tests of the robot movement. Each test builds a small world with a few cells and runs
//! `Robot::step` tick by tick.

use super::*;
use foundry_core::MaterialId;
use foundry_sim::SimConfig;
use std::sync::Arc;

/// The floor row: the robot stands with its feet on row 199.
const FLOOR: i32 = 200;

/// An empty world (4 × 4 chunks) with a stone floor at y = 200 and below.
fn world() -> Simulation {
    let content = Arc::new(Content::load_default().unwrap());
    let mut sim = Simulation::new(content.clone(), SimConfig::finite(4, 4, 1));
    fill(&mut sim, "stone", 2, FLOOR, 254, 254);
    sim
}

/// Fill the cells x0..x1 × y0..y1 with a material ("air" clears them).
fn fill(sim: &mut Simulation, material: &str, x0: i32, y0: i32, x1: i32, y1: i32) {
    let m = if material == "air" { MaterialId::AIR } else { sim.content().expect_material(material) };
    for y in y0..y1 {
        for x in x0..x1 {
            sim.set_cell(CellPos::new(x, y), m, None);
        }
    }
}

fn run(r: &mut Robot, sim: &Simulation, input: MoveInput, ticks: u32) {
    for _ in 0..ticks {
        r.step(input, sim);
    }
}

const RIGHT: MoveInput = MoveInput { x: 1, jump: false };
const LEFT: MoveInput = MoveInput { x: -1, jump: false };
const JUMP: MoveInput = MoveInput { x: 0, jump: true };

/// A robot standing on the floor with its left side at `left`.
fn standing(sim: &Simulation, left: i32) -> Robot {
    let mut r = Robot::standing_at(CellPos::new(left + ROBOT_W / 2, FLOOR));
    run(&mut r, sim, MoveInput::default(), 3);
    assert!(r.on_ground);
    r
}

/// Walk with `input` for `ticks` ticks. Returns the number of ticks in the air and the largest
/// change of the drawn height in one tick.
fn walk(r: &mut Robot, sim: &Simulation, input: MoveInput, ticks: u32) -> (u32, f32) {
    let (mut air, mut jump) = (0, 0.0f32);
    let mut drawn = r.draw_top_left().1;
    for _ in 0..ticks {
        r.step(input, sim);
        air += !r.on_ground as u32;
        let y = r.draw_top_left().1;
        jump = jump.max((y - drawn).abs());
        drawn = y;
    }
    (air, jump)
}

#[test]
fn falls_and_lands_on_the_floor() {
    let sim = world();
    let mut r = Robot::standing_at(CellPos::new(100, 120));
    run(&mut r, &sim, MoveInput::default(), 120);
    assert!(r.on_ground);
    assert_eq!(r.rect().y1, FLOOR, "the feet are on the floor");
    assert!(r.land_speed > 2.0, "it landed fast: {}", r.land_speed);
}

#[test]
fn walks_and_jumps() {
    let sim = world();
    let mut r = standing(&sim, 96);
    run(&mut r, &sim, RIGHT, 60);
    assert!(r.left > 140, "walked right: {}", r.left);
    assert_eq!(r.facing, 1);
    let ground = r.top;
    let mut highest = r.top;
    for _ in 0..40 {
        r.step(JUMP, &sim);
        highest = highest.min(r.top);
        if r.vel.1 > 0.0 {
            break;
        }
    }
    assert!(ground - highest >= 15, "jump height {}", ground - highest);
}

#[test]
fn speeds_up_and_stops_quickly() {
    let sim = world();
    let mut r = standing(&sim, 96);
    run(&mut r, &sim, RIGHT, 5);
    assert_eq!(r.vel.0, WALK_SPEED, "full speed after 5 ticks");
    run(&mut r, &sim, MoveInput::default(), 3);
    assert_eq!(r.vel.0, 0.0, "stopped after 3 ticks");
    // Turning around is fast too.
    run(&mut r, &sim, RIGHT, 5);
    run(&mut r, &sim, LEFT, 7);
    assert!((r.vel.0 + WALK_SPEED).abs() < 1e-4, "turned: {}", r.vel.0);
}

#[test]
fn jetpack_flies_higher_than_a_jump_and_runs_out() {
    let sim = world();
    let mut r = standing(&sim, 96);
    let ground = r.top;
    let mut highest = r.top;
    let mut least_fuel = JET_FUEL;
    for _ in 0..300 {
        r.step(JUMP, &sim);
        highest = highest.min(r.top);
        least_fuel = least_fuel.min(r.fuel);
        if r.on_ground && r.vel.1 == 0.0 && least_fuel < JET_FUEL {
            break;
        }
    }
    assert!(ground - highest >= 30, "flight height {}", ground - highest);
    assert!(least_fuel < 1.0, "the fuel ran out");
    assert!(r.on_ground, "it came down");
    run(&mut r, &sim, MoveInput::default(), 60);
    assert_eq!(r.fuel, JET_FUEL, "refilled on the ground");
}

#[test]
fn jetpack_push_grows_smoothly() {
    let sim = world();
    let mut r = Robot::standing_at(CellPos::new(100, 120));
    run(&mut r, &sim, MoveInput::default(), 10);
    assert!(!r.on_ground && r.vel.1 > 0.5, "falling");
    // The jump key in the air: the jetpack starts soft and gets stronger.
    let mut last = r.vel.1;
    let mut changes = vec![];
    for _ in 0..12 {
        r.step(JUMP, &sim);
        assert!(r.jetting);
        changes.push(last - r.vel.1);
        last = r.vel.1;
    }
    assert!(changes[0] < changes[8], "the push grows: {changes:?}");
    assert!(changes.iter().all(|&d| d <= JET_THRUST * JET_BRAKE), "no sudden kick: {changes:?}");
    assert!(r.vel.1 < 0.0, "it rises after 12 ticks: {}", r.vel.1);
}

#[test]
fn fuel_refills_after_a_wait() {
    let sim = world();
    let mut r = standing(&sim, 96);
    run(&mut r, &sim, JUMP, 60);
    assert!(r.fuel < JET_FUEL * 0.5, "used fuel: {}", r.fuel);
    run(&mut r, &sim, MoveInput::default(), 200);
    assert!(r.on_ground);
    // Land again with an empty tank.
    r.fuel = 0.0;
    r.ground_ticks = 0;
    run(&mut r, &sim, MoveInput::default(), JET_REFILL_DELAY as u32);
    assert_eq!(r.fuel, 0.0, "no refill during the wait");
    assert!(!r.refilling());
    run(&mut r, &sim, MoveInput::default(), 1);
    assert!(r.fuel > 0.0 && r.refilling(), "refills after the wait");
    run(&mut r, &sim, MoveInput::default(), 40);
    assert_eq!(r.fuel_fraction(), 1.0);
    assert!(!r.refilling(), "full");
}

#[test]
fn stops_at_powder_and_steps_up_low_ledges() {
    let mut sim = world();
    // A 2-cell step at x = 120 and a sand wall at x = 160.
    fill(&mut sim, "stone", 120, 198, 260, 200);
    fill(&mut sim, "sand", 160, 150, 170, 198);
    let mut r = standing(&sim, 96);
    let (air, _) = walk(&mut r, &sim, RIGHT, 120);
    assert_eq!(r.rect().y1, 198, "stepped up onto the ledge");
    assert_eq!(r.rect().x1, 160, "stopped at the sand");
    assert_eq!(air, 0, "never in the air");
}

#[test]
fn steps_up_and_down_without_leaving_the_ground() {
    let mut sim = world();
    // Stairs up: steps of 1, 2 and 3 cells, then down again.
    fill(&mut sim, "stone", 110, 199, 190, 200);
    fill(&mut sim, "stone", 120, 197, 180, 199);
    fill(&mut sim, "stone", 130, 194, 170, 197);
    let mut r = standing(&sim, 90);
    let (air, jump) = walk(&mut r, &sim, RIGHT, 130);
    assert!(r.left > 200, "walked over the stairs: {}", r.left);
    assert_eq!(air, 0, "never in the air");
    assert!(jump <= 1.7, "the drawn body moves smoothly: {jump}");
    assert_eq!(r.draw_lift, 0.0);
    // And back.
    let (air, _) = walk(&mut r, &sim, LEFT, 130);
    assert!(r.left < 100);
    assert_eq!(air, 0);
}

#[test]
fn no_hops_at_a_wall() {
    let mut sim = world();
    // A wall 5 cells high (too high to step up) at x = 130.
    fill(&mut sim, "stone", 130, 195, 140, 200);
    let mut r = standing(&sim, 100);
    let top = r.top;
    for _ in 0..200 {
        r.step(RIGHT, &sim);
        assert_eq!(r.top, top, "the robot stays down");
        assert!(r.on_ground);
        assert_eq!(r.draw_lift, 0.0);
    }
    assert_eq!(r.rect().x1, 130);
    assert!(r.blocked > 0, "it knows it walks into a wall");
}

#[test]
fn no_hops_at_a_sand_pile() {
    let mut sim = world();
    // A block of sand. It slides into a pile while the robot walks into it, and sand flows into
    // the space of the robot.
    fill(&mut sim, "sand", 130, 160, 170, 200);
    let mut r = standing(&sim, 100);
    let mut air = 0;
    for _ in 0..300 {
        sim.tick();
        r.step(RIGHT, &sim);
        air += !r.on_ground as u32;
    }
    assert!(air <= 10, "in the air for {air} ticks with no jump");
    assert!(r.left > 175, "walked over the pile: {:?}", r.rect());
}

#[test]
fn walks_over_single_loose_cells() {
    let mut sim = world();
    for x in [115, 121, 128, 140, 141, 150] {
        fill(&mut sim, "sand", x, FLOOR - 1, x + 1, FLOOR);
    }
    fill(&mut sim, "gravel", 160, FLOOR - 2, 161, FLOOR);
    let mut r = standing(&sim, 96);
    let (air, jump) = walk(&mut r, &sim, RIGHT, 100);
    assert!(r.left > 170, "walked past the cells: {}", r.left);
    assert_eq!(air, 0);
    assert!(jump <= 1.7, "{jump}");
}

#[test]
fn walks_over_a_gap_and_a_small_pit() {
    let mut sim = world();
    // A 1-cell gap in the floor and a pit 9 cells wide and 2 deep.
    fill(&mut sim, "air", 120, FLOOR, 121, FLOOR + 5);
    fill(&mut sim, "air", 140, FLOOR, 149, FLOOR + 2);
    let mut r = standing(&sim, 96);
    let (air, _) = walk(&mut r, &sim, RIGHT, 100);
    assert!(r.left > 170, "walked over: {}", r.left);
    assert_eq!(air, 0);
    let (air, _) = walk(&mut r, &sim, LEFT, 100);
    assert!(r.left < 110, "and back: {}", r.left);
    assert_eq!(air, 0);
}

#[test]
fn walks_under_a_low_ceiling() {
    let mut sim = world();
    // A tunnel 17 cells high with a 1-cell bump on the floor, then a tunnel exactly as high as
    // the robot.
    fill(&mut sim, "stone", 120, FLOOR - 30, 160, FLOOR - 17);
    fill(&mut sim, "stone", 140, FLOOR - 1, 145, FLOOR);
    fill(&mut sim, "stone", 160, FLOOR - 30, 200, FLOOR - ROBOT_H);
    let mut r = standing(&sim, 96);
    let (air, _) = walk(&mut r, &sim, RIGHT, 140);
    assert!(r.left > 205, "walked through: {}", r.left);
    assert_eq!(air, 0);
}

#[test]
fn wades_through_water() {
    let mut sim = world();
    fill(&mut sim, "water", 110, 170, 200, FLOOR);
    let mut r = standing(&sim, 96);
    run(&mut r, &sim, RIGHT, 150);
    assert!(r.left > 150, "walked into the water: {}", r.left);
    assert!(r.in_liquid);
    // Swim up.
    let top = r.top;
    run(&mut r, &sim, MoveInput { x: 0, jump: true }, 20);
    assert!(r.top < top, "swam up");
}

#[test]
fn climbs_out_of_deep_water_onto_a_high_bank() {
    let mut sim = world();
    // A pool 30 deep. The left bank is 5 cells above the water; the robot has no fuel.
    fill(&mut sim, "stone", 2, 165, 100, FLOOR);
    fill(&mut sim, "water", 100, 170, 200, FLOOR);
    fill(&mut sim, "stone", 200, 150, 254, FLOOR);
    let mut r = Robot::standing_at(CellPos::new(150, FLOOR));
    r.fuel = 0.0;
    run(&mut r, &sim, MoveInput::default(), 60);
    assert!(r.in_liquid);
    let mut out = false;
    for _ in 0..400 {
        r.step(MoveInput { x: -1, jump: true }, &sim);
        r.fuel = 0.0;
        if r.on_ground && r.rect().y1 <= 165 {
            out = true;
            break;
        }
    }
    assert!(out, "climbed onto the bank: {:?}", r.rect());
}

#[test]
fn jumps_out_of_a_dug_hole() {
    let mut sim = world();
    // A shaft the robot dug below itself: 11 cells wide, 24 deep, with a round bottom.
    fill(&mut sim, "dirt", 2, 150, 254, FLOOR);
    fill(&mut sim, "air", 95, 150, 106, 170);
    for y in 170..176 {
        let half = 5 - (y - 170);
        fill(&mut sim, "air", 100 - half, y, 101 + half, y + 1);
    }
    let mut r = Robot::standing_at(CellPos::new(100, 170));
    run(&mut r, &sim, MoveInput::default(), 30);
    assert!(r.on_ground && r.rect().y0 > 150, "in the hole: {:?}", r.rect());
    run(&mut r, &sim, MoveInput { x: 1, jump: true }, 90);
    run(&mut r, &sim, RIGHT, 60);
    assert!(r.on_ground && r.rect().y1 == 150 && r.left > 106, "out of the hole: {:?}", r.rect());
}

#[test]
fn climbs_out_when_buried_in_sand() {
    let mut sim = world();
    let mut r = standing(&sim, 100);
    // Sand falls onto the robot and buries it 10 cells deep.
    fill(&mut sim, "sand", 90, FLOOR - ROBOT_H - 10, 118, FLOOR);
    run(&mut r, &sim, RIGHT, 30);
    assert!(r.buried && r.left <= 110, "stuck in the sand: {:?}", r.rect());
    run(&mut r, &sim, JUMP, 100);
    assert!(!r.buried, "out of the sand");
    assert!(r.rect().y1 <= FLOOR - ROBOT_H - 10 + 3, "at the top of the sand: {:?}", r.rect());
    // Now it walks away on the sand.
    let left = r.left;
    run(&mut r, &sim, RIGHT, 10);
    assert!(r.left > left + 4 && r.rect().y1 <= FLOOR - ROBOT_H - 10, "walked on: {:?}", r.rect());
}

#[test]
fn lands_on_an_edge_and_climbs_a_ledge_in_the_air() {
    let mut sim = world();
    // A ledge from x = 130 with its top at y = 180.
    fill(&mut sim, "stone", 130, 180, 254, FLOOR);
    // Falling onto the edge with only two columns over it: it stands on the edge.
    let mut r = Robot::standing_at(CellPos::new(128, 100));
    run(&mut r, &sim, MoveInput::default(), 80);
    assert!(r.on_ground && r.rect().y1 == 180, "on the edge: {:?}", r.rect());
    run(&mut r, &sim, RIGHT, 20);
    assert!(r.on_ground && r.left > 135, "walks on: {:?}", r.rect());
    // Falling past the ledge with the feet 2 cells below its top, moving toward it: it climbs
    // onto the ledge and does not slide down the wall.
    let mut r = Robot::standing_at(CellPos::new(125, 182));
    r.vel = (WALK_SPEED, 0.5);
    run(&mut r, &sim, RIGHT, 20);
    assert!(r.on_ground && r.rect().y1 == 180 && r.left > 130, "climbed the ledge: {:?}", r.rect());
}

#[test]
fn head_is_moved_past_a_corner() {
    let mut sim = world();
    // A block over the left part of the robot's head.
    let mut r = standing(&sim, 100);
    fill(&mut sim, "stone", 80, FLOOR - ROBOT_H - 8, 102, FLOOR - ROBOT_H - 5);
    let mut highest = r.top;
    for _ in 0..25 {
        r.step(MoveInput { x: 0, jump: true }, &sim);
        highest = highest.min(r.top);
    }
    assert!(r.left >= 102, "moved to the side: {}", r.left);
    assert!(highest < FLOOR - ROBOT_H - 10, "went past the corner: {highest}");
}

#[test]
fn a_jump_press_just_before_landing_still_jumps() {
    let sim = world();
    let mut r = Robot::standing_at(CellPos::new(100, 150));
    // Fall until the feet are 5 cells above the floor, then press jump.
    while r.rect().y1 < FLOOR - 5 {
        r.step(MoveInput::default(), &sim);
    }
    r.fuel = 0.0;
    let mut highest = FLOOR;
    let mut landed = false;
    for _ in 0..40 {
        r.step(JUMP, &sim);
        landed |= r.on_ground;
        if landed {
            highest = highest.min(r.rect().y1);
        }
    }
    assert!(FLOOR - highest >= 15, "jumped after landing: {}", FLOOR - highest);
}

#[test]
fn a_jump_just_after_walking_off_a_ledge_still_works() {
    for (wait, jumps) in [(3, true), (20, false)] {
        let mut sim = world();
        fill(&mut sim, "stone", 2, 150, 120, FLOOR);
        let mut r = Robot::standing_at(CellPos::new(110, 150));
        run(&mut r, &sim, MoveInput::default(), 3);
        r.fuel = 0.0;
        while r.on_ground {
            r.step(RIGHT, &sim);
        }
        run(&mut r, &sim, RIGHT, wait - 1);
        r.step(MoveInput { x: 1, jump: true }, &sim);
        assert_eq!(r.vel.1 < 0.0, jumps, "wait {wait}: vel {:?}", r.vel);
    }
}

#[test]
fn a_short_press_makes_a_lower_jump() {
    let sim = world();
    let height = |hold: u32| {
        let mut r = standing(&sim, 100);
        r.fuel = 0.0;
        let ground = r.top;
        let mut highest = r.top;
        for t in 0..60 {
            r.step(MoveInput { x: 0, jump: t < hold }, &sim);
            highest = highest.min(r.top);
        }
        ground - highest
    };
    let (short, long) = (height(3), height(40));
    assert!(long >= 18, "full jump {long}");
    assert!(short * 2 < long, "short {short}, long {long}");
    assert!(short >= 4, "a tap still jumps: {short}");
}
