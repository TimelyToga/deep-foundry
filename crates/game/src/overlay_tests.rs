//! Tests of the aim point that the overlay draws (the dig circle and the end of the tool beam):
//! it follows the mouse in every frame, also while the dig button is held, while the robot walks,
//! and while no new tick arrives.

use super::*;
use crate::construct::Mods;
use crate::demo::{self, Shape};
use crate::factory_host::{FactoryCommand, FactoryHost, GameCommand};
use crate::normal::NormalMode;
use foundry_factory::Guide;
use foundry_sim::Simulation;
use std::sync::Arc;
use std::time::Instant;

fn setup() -> (FactoryHost, Simulation, NormalMode, Arc<Content>) {
    let content = Arc::new(Content::load_default().unwrap());
    let d = demo::build(content.clone(), Shape::Box { width_chunks: 32, height_chunks: 16 }, 3);
    let mut sim = d.sim;
    let mut host = FactoryHost::new_game(content.clone(), Arc::new(Guide::default()), &mut sim, d.start_center.0 as i32).unwrap();
    // The robot lands.
    for _ in 0..30 {
        sim.tick();
        host.tick(&mut sim);
    }
    let mut n = NormalMode::new();
    n.take_frame(host.frame(&sim, sim.tick_count()), Instant::now());
    (host, sim, n, content)
}

/// The input command of this frame, if the input changed.
fn input(n: &mut NormalMode, content: &Content, mouse: CellPos) -> Option<crate::factory_host::PlayerInput> {
    match n.input(content, mouse, CellRect::new(0, 0, 10, 10)) {
        Some(GameCommand::Factory(FactoryCommand::Input(i))) => Some(i),
        _ => None,
    }
}

#[test]
fn aim_point_follows_the_mouse_while_no_tick_arrives() {
    let (_host, _sim, mut n, content) = setup();
    let (cx, cy) = n.frame.robot.unwrap().center();
    let start = CellPos::new(cx as i32 + 10, cy as i32 + 12);
    // The dig button goes down in the world.
    assert!(n.press(&content, true, start, Mods::default()).is_empty());
    assert!(n.held.dig);
    // The simulation is late: the same factory frame for 20 frames while the mouse moves.
    for i in 0..20 {
        let mouse = CellPos::new(start.x + i, start.y - i / 2);
        assert_eq!(aim_point(&n.frame, Some(mouse)), mouse, "frame {i}: the drawn aim point is at the mouse");
        let i_input = input(&mut n, &content, mouse).unwrap_or_else(|| panic!("frame {i}: no new input"));
        assert_eq!(i_input.aim, mouse, "frame {i}: the next tick digs at the mouse");
        assert!(i_input.dig);
    }
    // Far from the robot: the aim point stays in reach.
    let far = CellPos::new(cx as i32 + 500, cy as i32);
    let p = aim_point(&n.frame, Some(far));
    let d = ((p.x as f32 + 0.5 - cx).powi(2) + (p.y as f32 + 0.5 - cy).powi(2)).sqrt();
    assert!(d <= tools::REACH + 1.0, "{d}");
    // No mouse over the world (the mouse is over the UI): the aim point of the last tick.
    assert_eq!(aim_point(&n.frame, None), n.frame.aim);
}

#[test]
fn aim_point_follows_the_mouse_while_the_robot_walks_and_digs() {
    let (mut host, mut sim, mut n, content) = setup();
    let x0 = n.frame.robot.unwrap().left;
    let (cx, cy) = n.frame.robot.unwrap().center();
    assert!(n.press(&content, true, CellPos::new(cx as i32 + 12, cy as i32 + 8), Mods::default()).is_empty());
    n.held.right = true;
    let mut dug = false;
    for i in 0..40 {
        let (cx, cy) = n.frame.robot.unwrap().center();
        // The mouse moves with the robot (the camera follows it) and a little up and down.
        let mouse = CellPos::new(cx as i32 + 12, cy as i32 + 4 + i % 5);
        assert_eq!(aim_point(&n.frame, Some(mouse)), mouse, "frame {i}: the drawn aim point is at the mouse");
        if let Some(i_input) = input(&mut n, &content, mouse) {
            host.apply(FactoryCommand::Input(i_input), &mut sim);
        }
        sim.tick();
        host.tick(&mut sim);
        n.take_frame(host.frame(&sim, sim.tick_count()), Instant::now());
        dug |= n.frame.digging;
        // The tick used the aim of this frame.
        assert_eq!(n.frame.aim, tools::clamp_aim(n.frame.robot.as_ref().unwrap(), mouse), "tick {i}");
    }
    assert!(n.frame.robot.unwrap().left > x0, "the robot walked");
    assert!(dug, "the robot dug");
}
