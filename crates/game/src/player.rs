//! The player robot: a small body in the cell world (game design section 6.1).
//!
//! - The body is `ROBOT_W` × `ROBOT_H` cells. Its position is whole cells plus a remainder, so
//!   slow speeds still move it.
//! - Solid cells and powder stop it. Liquids, gases, fire and air do not.
//! - In a liquid it wades: it moves slower, it falls slower, and the jump key swims up.
//! - It steps up onto ledges of up to `STEP_UP` cells while it walks.
//! - It only checks the cells that it moves into. So when sand falls onto the robot, the robot
//!   can still walk out of it.
//!
//! Speeds are in cells per tick (60 ticks per second).

use foundry_content::{Content, Phase};
use foundry_core::{CellPos, CellRect};
use foundry_sim::Simulation;
use serde::{Deserialize, Serialize};

/// Width of the robot in cells.
pub const ROBOT_W: i32 = 8;
/// Height of the robot in cells.
pub const ROBOT_H: i32 = 16;
/// Highest ledge the robot walks up without a jump, in cells.
pub const STEP_UP: i32 = 3;

const WALK_SPEED: f32 = 1.0;
const GROUND_ACCEL: f32 = 0.25;
const AIR_ACCEL: f32 = 0.12;
const GRAVITY: f32 = 0.12;
const MAX_FALL: f32 = 4.0;
/// Start speed of a jump: about 20 cells high.
const JUMP_SPEED: f32 = 2.2;
/// In a liquid: speed factor, gravity factor and swim speed up.
const WADE_FACTOR: f32 = 0.5;
const WADE_GRAVITY: f32 = 0.35;
const SWIM_UP: f32 = 0.9;
/// Part of the body cells that must be liquid for the robot to count as "in a liquid".
const WADE_FRACTION: f32 = 0.25;

/// The keys that move the robot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MoveInput {
    /// -1 left, 0 none, 1 right.
    pub x: i8,
    /// The jump key is down.
    pub jump: bool,
}

/// The robot body.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Robot {
    /// Left column of the body.
    pub left: i32,
    /// Top row of the body.
    pub top: i32,
    /// Movement that is less than one cell, -1 to 1.
    pub rem: (f32, f32),
    /// Speed in cells per tick (x right, y down).
    pub vel: (f32, f32),
    pub on_ground: bool,
    pub in_liquid: bool,
    /// -1 looks left, 1 looks right.
    pub facing: i8,
}

impl Robot {
    /// A robot whose feet are at `feet` (the cell under the middle of the body).
    pub fn standing_at(feet: CellPos) -> Self {
        Self {
            left: feet.x - ROBOT_W / 2,
            top: feet.y - ROBOT_H,
            rem: (0.0, 0.0),
            vel: (0.0, 0.0),
            on_ground: false,
            in_liquid: false,
            facing: 1,
        }
    }

    /// The cells of the body.
    pub fn rect(&self) -> CellRect {
        CellRect::new(self.left, self.top, self.left + ROBOT_W, self.top + ROBOT_H)
    }

    /// The middle of the body, in cells.
    pub fn center(&self) -> (f32, f32) {
        (self.left as f32 + self.rem.0 + ROBOT_W as f32 * 0.5, self.top as f32 + self.rem.1 + ROBOT_H as f32 * 0.5)
    }

    /// The cell at the middle of the body.
    pub fn center_cell(&self) -> CellPos {
        CellPos::new(self.left + ROBOT_W / 2, self.top + ROBOT_H / 2)
    }

    /// Run one tick of movement.
    pub fn step(&mut self, input: MoveInput, sim: &Simulation) {
        let content = sim.content().clone();
        let c = &*content;
        self.in_liquid = liquid_fraction(sim, c, self.rect()) >= WADE_FRACTION;
        let factor = if self.in_liquid { WADE_FACTOR } else { 1.0 };

        // Horizontal speed moves toward the key direction.
        if input.x != 0 {
            self.facing = input.x.signum();
        }
        let target = input.x as f32 * WALK_SPEED * factor;
        let accel = if self.on_ground { GROUND_ACCEL } else { AIR_ACCEL };
        self.vel.0 = approach(self.vel.0, target, accel);

        // Vertical speed: gravity, jump and swim.
        if self.in_liquid {
            self.vel.1 = (self.vel.1 + GRAVITY * WADE_GRAVITY).min(MAX_FALL * WADE_FACTOR);
            self.vel.1 *= 0.92;
            if input.jump {
                self.vel.1 = (self.vel.1 - 0.3).max(-SWIM_UP);
            }
        } else {
            if input.jump && self.on_ground {
                self.vel.1 = -JUMP_SPEED;
            }
            self.vel.1 = (self.vel.1 + GRAVITY).min(MAX_FALL);
        }

        self.move_x(sim, c);
        self.move_y(sim, c);
        self.on_ground = !is_free(sim, c, shifted(self.rect(), 0, 1), self.rect());
        if self.on_ground && self.vel.1 > 0.0 {
            self.vel.1 = 0.0;
            self.rem.1 = 0.0;
        }
    }

    fn move_x(&mut self, sim: &Simulation, c: &Content) {
        self.rem.0 += self.vel.0;
        let steps = self.rem.0.trunc() as i32;
        self.rem.0 -= steps as f32;
        let dir = steps.signum();
        for _ in 0..steps.abs() {
            let body = self.rect();
            if is_free(sim, c, shifted(body, dir, 0), body) {
                self.left += dir;
                continue;
            }
            // A low ledge: step up onto it.
            let can_step = self.on_ground || self.in_liquid;
            let up = (1..=STEP_UP).find(|&up| {
                can_step
                    && is_free(sim, c, shifted(body, 0, -up), body)
                    && is_free(sim, c, shifted(body, dir, -up), shifted(body, 0, -up))
            });
            match up {
                Some(up) => {
                    self.left += dir;
                    self.top -= up;
                }
                None => {
                    self.vel.0 = 0.0;
                    self.rem.0 = 0.0;
                    break;
                }
            }
        }
    }

    fn move_y(&mut self, sim: &Simulation, c: &Content) {
        self.rem.1 += self.vel.1;
        let steps = self.rem.1.trunc() as i32;
        self.rem.1 -= steps as f32;
        let dir = steps.signum();
        for _ in 0..steps.abs() {
            let body = self.rect();
            if is_free(sim, c, shifted(body, 0, dir), body) {
                self.top += dir;
            } else {
                self.vel.1 = 0.0;
                self.rem.1 = 0.0;
                break;
            }
        }
    }
}

/// True if a cell of this material stops the robot.
pub fn blocks(content: &Content, material: foundry_core::MaterialId) -> bool {
    matches!(content.materials.phase[material.index()], Phase::Solid | Phase::Powder)
}

fn approach(v: f32, target: f32, step: f32) -> f32 {
    if v < target { (v + step).min(target) } else { (v - step).max(target) }
}

fn shifted(r: CellRect, dx: i32, dy: i32) -> CellRect {
    CellRect::new(r.x0 + dx, r.y0 + dy, r.x1 + dx, r.y1 + dy)
}

/// True if no cell of `to` that is not also in `from` stops the robot.
fn is_free(sim: &Simulation, content: &Content, to: CellRect, from: CellRect) -> bool {
    for y in to.y0..to.y1 {
        for x in to.x0..to.x1 {
            let p = CellPos::new(x, y);
            if from.contains(p) {
                continue;
            }
            if blocks(content, sim.cell(p).material) {
                return false;
            }
        }
    }
    true
}

/// The part of the cells in `r` that are liquid, 0 to 1.
fn liquid_fraction(sim: &Simulation, content: &Content, r: CellRect) -> f32 {
    let mut n = 0;
    for y in r.y0..r.y1 {
        for x in r.x0..r.x1 {
            if content.materials.phase[sim.cell(CellPos::new(x, y)).material.index()] == Phase::Liquid {
                n += 1;
            }
        }
    }
    n as f32 / (r.width() * r.height()).max(1) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundry_sim::SimConfig;
    use std::sync::Arc;

    /// An empty world (4 × 4 chunks) with a stone floor at y = 200 and above.
    fn world() -> Simulation {
        let content = Arc::new(Content::load_default().unwrap());
        let mut sim = Simulation::new(content.clone(), SimConfig { width_chunks: 4, height_chunks: 4, seed: 1, bedrock_border: true });
        let stone = content.expect_material("stone");
        for y in 200..254 {
            for x in 2..254 {
                sim.set_cell(CellPos::new(x, y), stone, None);
            }
        }
        sim
    }

    fn run(r: &mut Robot, sim: &Simulation, input: MoveInput, ticks: u32) {
        for _ in 0..ticks {
            r.step(input, sim);
        }
    }

    #[test]
    fn falls_and_lands_on_the_floor() {
        let sim = world();
        let mut r = Robot::standing_at(CellPos::new(100, 120));
        run(&mut r, &sim, MoveInput::default(), 120);
        assert!(r.on_ground);
        assert_eq!(r.rect().y1, 200, "the feet are on the floor");
    }

    #[test]
    fn walks_and_jumps() {
        let sim = world();
        let mut r = Robot::standing_at(CellPos::new(100, 200));
        run(&mut r, &sim, MoveInput::default(), 5);
        run(&mut r, &sim, MoveInput { x: 1, jump: false }, 60);
        assert!(r.left > 140, "walked right: {}", r.left);
        assert_eq!(r.facing, 1);
        let ground = r.top;
        let mut highest = r.top;
        for _ in 0..40 {
            r.step(MoveInput { x: 0, jump: true }, &sim);
            highest = highest.min(r.top);
            if r.vel.1 > 0.0 {
                break;
            }
        }
        assert!(ground - highest >= 15, "jump height {}", ground - highest);
    }

    #[test]
    fn stops_at_powder_and_steps_up_low_ledges() {
        let mut sim = world();
        let c = sim.content().clone();
        let sand = c.expect_material("sand");
        // A 2-cell step at x = 120 and a sand wall at x = 160.
        for x in 120..260 {
            for y in 198..200 {
                sim.set_cell(CellPos::new(x, y), c.expect_material("stone"), None);
            }
        }
        for y in 150..198 {
            for x in 160..170 {
                sim.set_cell(CellPos::new(x, y), sand, None);
            }
        }
        let mut r = Robot::standing_at(CellPos::new(100, 200));
        run(&mut r, &sim, MoveInput { x: 1, jump: false }, 120);
        assert_eq!(r.rect().y1, 198, "stepped up onto the ledge");
        assert_eq!(r.rect().x1, 160, "stopped at the sand");
    }

    #[test]
    fn wades_through_water() {
        let mut sim = world();
        let water = sim.content().expect_material("water");
        for y in 170..200 {
            for x in 110..200 {
                sim.set_cell(CellPos::new(x, y), water, None);
            }
        }
        let mut r = Robot::standing_at(CellPos::new(100, 200));
        run(&mut r, &sim, MoveInput { x: 1, jump: false }, 150);
        assert!(r.left > 150, "walked into the water: {}", r.left);
        assert!(r.in_liquid);
        // Swim up.
        let top = r.top;
        run(&mut r, &sim, MoveInput { x: 0, jump: true }, 20);
        assert!(r.top < top, "swam up");
    }
}
