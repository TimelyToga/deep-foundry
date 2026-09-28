//! The player robot: a small body in the cell world (game design section 6.1).
//!
//! - The body is `ROBOT_W` × `ROBOT_H` cells. Its position is whole cells plus a remainder, so
//!   slow speeds still move it. The remainder never points into a wall, a ceiling or the ground,
//!   so the drawn body does not shake when the robot pushes against them.
//! - Solid cells and powder stop it. Liquids, gases, fire and air do not.
//! - It only checks the cells that it moves into. So when sand falls onto the robot, the robot
//!   can still walk out of it. When much of the body is in powder (the robot is buried), powder
//!   does not stop it when it moves up, so it can jump out.
//! - Walking: it steps up onto ledges of up to `STEP_UP` cells and it follows the ground down
//!   steps of up to `STEP_UP` cells, so it stays on uneven ground (also under a liquid). It never leaves the ground
//!   unless the jump key is down or the ground ends. The picture of the robot moves to the new
//!   height over a few ticks (`draw_lift`), so a step does not look like a jump.
//! - Jump: a press of the jump key is kept for `JUMP_BUFFER` ticks, so a press just before the
//!   robot lands still jumps. A jump also works for `COYOTE` ticks after the robot walked off a
//!   ledge. When the key is released while the robot rises, the jump is lower.
//! - In the air: the robot climbs a ledge at its feet (up to `AIR_STEP_UP` cells) when it is not
//!   rising fast. When its head hits a corner, it moves up to `CORNER_NUDGE` cells to the side.
//! - Jetpack: the jump key held in the air (game design 6.1). The push grows over `JET_SPOOL`
//!   ticks. It has fuel for `JET_FUEL` ticks. On the ground the fuel fills again after a wait of
//!   `JET_REFILL_DELAY` ticks. (No exhaust gas yet.)
//! - In a liquid it wades: it moves slower, it falls slower, and the jump key swims up. When the
//!   head is out of the liquid, or the feet are on the ground, the jump key jumps out.
//!
//! All movement numbers are in the "Tuning" block below. Speeds are in cells per tick (60 ticks
//! per second). Times are in ticks.

use foundry_content::{Content, Phase};
use foundry_core::{CellPos, CellRect};
use foundry_sim::Simulation;
use serde::{Deserialize, Serialize};

/// Width of the robot in cells.
pub const ROBOT_W: i32 = 8;
/// Height of the robot in cells.
pub const ROBOT_H: i32 = 16;

// ------------------------------------------------------------ Tuning

/// Top walk speed.
const WALK_SPEED: f32 = 1.0;
/// Speed change per tick on the ground: with a walk key down, with no key down, and with the
/// key against the movement.
const GROUND_ACCEL: f32 = 0.2;
const GROUND_STOP: f32 = 0.34;
const GROUND_TURN: f32 = 0.4;
/// Speed change per tick in the air: with a walk key down, and with no key down.
const AIR_ACCEL: f32 = 0.12;
const AIR_STOP: f32 = 0.04;
const GRAVITY: f32 = 0.12;
const MAX_FALL: f32 = 4.0;
/// Start speed of a jump: about 20 cells high.
const JUMP_SPEED: f32 = 2.2;
/// When the jump key is released during the rise, the speed up drops to this (a lower jump).
const JUMP_CUT_SPEED: f32 = 0.8;
/// Ticks a jump press is kept. A press this long before the robot lands still jumps.
const JUMP_BUFFER: u8 = 7;
/// Ticks after the robot walked off a ledge in which a jump still works.
const COYOTE: u16 = 6;
/// Highest ledge the robot walks up without a jump, in cells. It also follows the ground down
/// steps this high.
pub const STEP_UP: i32 = 3;
/// Highest ledge the robot climbs in the air (when it falls or is near the top of a jump).
const AIR_STEP_UP: i32 = 3;
/// In the air, the robot climbs ledges only when its speed up is less than this.
const AIR_STEP_MAX_RISE: f32 = 0.6;
/// When the head hits a corner, the robot moves up to this many cells to the side.
const CORNER_NUDGE: i32 = 3;
/// The drawn body catches up with a step: the part of the distance that is left after each tick.
const LIFT_KEEP: f32 = 0.55;
/// Ticks the robot counts as "walking into a wall" after a sideways move failed.
const BLOCKED_TICKS: u8 = 8;
/// In a liquid: speed factor, gravity factor and swim speed up.
const WADE_FACTOR: f32 = 0.5;
const WADE_GRAVITY: f32 = 0.35;
const SWIM_UP: f32 = 0.9;
/// Start speed of a jump out of a liquid (about 15 cells high).
const WATER_JUMP: f32 = 1.9;
/// Part of the body cells that must be liquid for the robot to count as "in a liquid".
const WADE_FRACTION: f32 = 0.25;
/// Part of the body cells that must be powder for the robot to count as buried.
const BURIED_FRACTION: f32 = 0.3;
/// Highest speed up through powder, when buried.
const BURIED_RISE: f32 = 0.7;
/// Powder that slid into the bottom row of the body lifts the robot by up to this many cells per
/// tick.
const POWDER_LIFT: i32 = 2;
/// Jetpack fuel: ticks of flight.
pub const JET_FUEL: f32 = 90.0;
/// Upward push per tick at full power. Gravity pulls down at the same time, so the robot rises
/// when this is more than `GRAVITY`.
const JET_THRUST: f32 = 0.24;
/// The push is this many times stronger while the robot falls (the jetpack stops a fall fast).
const JET_BRAKE: f32 = 1.5;
/// Ticks until the jetpack gives its full push, and the power of the first tick (0 to 1).
const JET_SPOOL: f32 = 8.0;
const JET_START_POWER: f32 = 0.3;
/// Top speed up with the jetpack.
const JET_MAX_UP: f32 = 1.4;
/// The jetpack starts only when the speed up is less than this: near the top of a jump, or
/// when the robot falls.
const JET_START: f32 = 0.8;
/// Ticks on the ground before the fuel fills again.
pub const JET_REFILL_DELAY: u16 = 20;
/// Fuel that comes back per tick on the ground, after the wait.
const JET_REFILL: f32 = 2.5;

// ------------------------------------------------------------ Robot

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
    /// Jetpack fuel in ticks, 0 to `JET_FUEL`.
    #[serde(default)]
    pub fuel: f32,
    /// The jetpack pushed in the last tick.
    #[serde(default)]
    pub jetting: bool,
    /// Jetpack power, 0 to 1. It grows while the jetpack runs.
    #[serde(default)]
    pub jet_power: f32,
    /// Ticks on the ground since the robot landed (up to 1000).
    #[serde(default)]
    pub ground_ticks: u16,
    /// Ticks in the air since the robot left the ground or a liquid (up to 1000).
    #[serde(default)]
    pub air_ticks: u16,
    /// The fall speed when the robot landed the last time, in cells per tick.
    #[serde(default)]
    pub land_speed: f32,
    /// The jump key was down in the last tick.
    #[serde(default)]
    pub jump_held: bool,
    /// Ticks left in which a press of the jump key still starts a jump.
    #[serde(default)]
    pub jump_buffer: u8,
    /// The robot left the ground with a jump (false again when it lands).
    #[serde(default)]
    pub jumping: bool,
    /// Ticks left in which the robot counts as walking into a wall.
    #[serde(default)]
    pub blocked: u8,
    /// Cells walked on the ground (for the walk animation).
    #[serde(default)]
    pub walk_dist: f32,
    /// Cells between the drawn body and the real body after a step (positive: drawn lower).
    /// It goes to 0 over a few ticks.
    #[serde(default)]
    pub draw_lift: f32,
    /// Much of the body is in powder: powder does not stop the robot when it moves up.
    #[serde(default)]
    pub buried: bool,
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
            fuel: JET_FUEL,
            jetting: false,
            jet_power: 0.0,
            ground_ticks: 0,
            air_ticks: 0,
            land_speed: 0.0,
            jump_held: false,
            jump_buffer: 0,
            jumping: false,
            blocked: 0,
            walk_dist: 0.0,
            draw_lift: 0.0,
            buried: false,
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

    /// The top-left corner of the body as it is drawn, in cells (with the step smoothing).
    pub fn draw_top_left(&self) -> (f32, f32) {
        (self.left as f32 + self.rem.0, self.top as f32 + self.rem.1 + self.draw_lift)
    }

    /// Jetpack fuel, 0 to 1.
    pub fn fuel_fraction(&self) -> f32 {
        (self.fuel / JET_FUEL).clamp(0.0, 1.0)
    }

    /// The fuel fills now (on the ground, after the wait).
    pub fn refilling(&self) -> bool {
        self.on_ground && self.ground_ticks > JET_REFILL_DELAY && self.fuel < JET_FUEL
    }

    /// Run one tick of movement.
    pub fn step(&mut self, input: MoveInput, sim: &Simulation) {
        let content = sim.content().clone();
        let c = &*content;
        let body = self.rect();
        let (liquid, powder) = fractions(sim, c, body);
        self.in_liquid = liquid >= WADE_FRACTION;
        self.buried = powder >= BURIED_FRACTION;
        let pressed = input.jump && !self.jump_held;
        self.jump_held = input.jump;
        self.jump_buffer = if pressed { JUMP_BUFFER } else { self.jump_buffer.saturating_sub(1) };
        self.blocked = self.blocked.saturating_sub(1);
        self.draw_lift *= LIFT_KEEP;
        if self.draw_lift.abs() < 0.1 {
            self.draw_lift = 0.0;
        }

        // Horizontal speed moves toward the key direction.
        if input.x != 0 {
            self.facing = input.x.signum();
        }
        let factor = if self.in_liquid { WADE_FACTOR } else { 1.0 };
        let target = input.x as f32 * WALK_SPEED * factor;
        let grip = self.on_ground || self.in_liquid;
        let rate = match (input.x != 0, self.vel.0 * target < 0.0, grip) {
            (false, _, true) => GROUND_STOP,
            (false, _, false) => AIR_STOP,
            (true, true, true) => GROUND_TURN,
            (true, false, true) => GROUND_ACCEL,
            (true, _, false) => AIR_ACCEL,
        };
        self.vel.0 = approach(self.vel.0, target, rate);

        // Vertical speed: gravity, jump, jetpack and swim.
        self.jetting = false;
        if self.in_liquid {
            self.vel.1 = (self.vel.1 + GRAVITY * WADE_GRAVITY).min(MAX_FALL * WADE_FACTOR);
            self.vel.1 *= 0.92;
            if input.jump {
                if self.on_ground || head_out(sim, c, body) {
                    // Jump out of the liquid (onto a bank, or from the bottom of shallow water).
                    self.vel.1 = self.vel.1.min(-WATER_JUMP);
                    self.jumping = true;
                } else {
                    self.vel.1 = (self.vel.1 - 0.3).max(-SWIM_UP);
                }
            }
        } else if self.buried && input.jump {
            // Climb up through the powder.
            self.vel.1 = -BURIED_RISE;
        } else {
            let coyote = !self.on_ground && self.air_ticks <= COYOTE && !self.jumping;
            if self.jump_buffer > 0 && (self.on_ground || coyote) {
                self.vel.1 = -JUMP_SPEED;
                self.jumping = true;
                self.jump_buffer = 0;
            } else if input.jump && !self.on_ground && self.fuel > 0.0 && self.vel.1 > -JET_START {
                // Near the top of the jump (or falling): the jetpack pushes up.
                self.jet_power = (self.jet_power.max(JET_START_POWER) + 1.0 / JET_SPOOL).min(1.0);
                let brake = if self.vel.1 > 0.0 { JET_BRAKE } else { 1.0 };
                self.vel.1 = (self.vel.1 - JET_THRUST * self.jet_power * brake).max(-JET_MAX_UP - GRAVITY);
                self.fuel = (self.fuel - 1.0).max(0.0);
                self.jetting = true;
            }
            if self.jumping && !input.jump && self.vel.1 < -JUMP_CUT_SPEED {
                self.vel.1 = -JUMP_CUT_SPEED;
            }
            self.vel.1 = (self.vel.1 + GRAVITY).min(MAX_FALL);
        }
        if !self.jetting {
            self.jet_power = 0.0;
        }
        if self.buried {
            self.vel.1 = self.vel.1.max(-BURIED_RISE);
        }

        let was_on_ground = self.on_ground;
        let fall_speed = self.vel.1;
        self.rise_out_of_powder(sim, c);
        self.move_x(sim, c);
        self.move_y(sim, c);
        self.keep_rem_out_of_walls(sim, c);
        self.on_ground = !self.free(sim, c, self.rect(), 0, 1);
        // Walking down a step: follow the ground, do not fall.
        if !self.on_ground && was_on_ground && !self.jumping && self.vel.1 >= 0.0 {
            self.follow_ground(sim, c);
        }

        if self.on_ground {
            if self.ground_ticks == 0 {
                self.land_speed = fall_speed.max(0.0);
                self.ground_ticks = 0;
            }
            self.ground_ticks = (self.ground_ticks + 1).min(1000);
            self.jumping = false;
            if self.ground_ticks > JET_REFILL_DELAY {
                self.fuel = (self.fuel + JET_REFILL).min(JET_FUEL);
            }
            if self.vel.1 > 0.0 {
                self.vel.1 = 0.0;
                self.rem.1 = 0.0;
            }
        } else {
            self.ground_ticks = 0;
        }
        self.air_ticks = if self.on_ground || self.in_liquid { 0 } else { (self.air_ticks + 1).min(1000) };
    }

    /// True if the body can move by (dx, dy) from `from`: no cell that it moves into stops it.
    fn free(&self, sim: &Simulation, c: &Content, from: CellRect, dx: i32, dy: i32) -> bool {
        is_free(sim, c, shifted(from, dx, dy), from, self.buried && dy < 0)
    }

    fn move_x(&mut self, sim: &Simulation, c: &Content) {
        self.rem.0 += self.vel.0;
        let steps = self.rem.0.trunc() as i32;
        self.rem.0 -= steps as f32;
        let dir = steps.signum();
        let max_up = self.max_step_up();
        for _ in 0..steps.abs() {
            let body = self.rect();
            if self.free(sim, c, body, dir, 0) {
                self.left += dir;
                if self.on_ground {
                    self.walk_dist += 1.0;
                }
                continue;
            }
            match self.step_up(sim, c, body, dir, max_up) {
                Some(up) => {
                    self.left += dir;
                    self.top -= up;
                    self.draw_lift += up as f32;
                    self.walk_dist += 1.0;
                }
                None => {
                    self.stop_x();
                    return;
                }
            }
        }
    }

    /// Stop against a wall.
    fn stop_x(&mut self) {
        self.vel.0 = 0.0;
        self.rem.0 = 0.0;
        self.blocked = BLOCKED_TICKS;
    }

    /// A low ledge beside the body in `dir`: the smallest step up (up to `max_up` cells) onto it.
    /// The cells above the body and the cells beside the raised body must be free.
    fn step_up(&self, sim: &Simulation, c: &Content, body: CellRect, dir: i32, max_up: i32) -> Option<i32> {
        (1..=max_up).find(|&up| {
            let raised = shifted(body, 0, -up);
            self.free(sim, c, body, 0, -up) && is_free(sim, c, shifted(raised, dir, 0), raised, false)
        })
    }

    fn move_y(&mut self, sim: &Simulation, c: &Content) {
        self.rem.1 += self.vel.1;
        let steps = self.rem.1.trunc() as i32;
        self.rem.1 -= steps as f32;
        let dir = steps.signum();
        for _ in 0..steps.abs() {
            let body = self.rect();
            if self.free(sim, c, body, 0, dir) {
                self.top += dir;
                continue;
            }
            // The head hits a corner: move a little to the side and go on.
            if dir < 0
                && let Some(dx) = self.corner_nudge(sim, c, body)
            {
                self.left += dx;
                self.top -= 1;
                continue;
            }
            self.vel.1 = 0.0;
            self.rem.1 = 0.0;
            return;
        }
    }

    /// The part of a cell that is left (`rem`) must not point into a wall, a ceiling or the
    /// ground. If it did, the drawn body would move into the wall and jump back each few ticks
    /// (a shake). Run it after both moves, because the move up or down changes which cells are
    /// beside the body.
    fn keep_rem_out_of_walls(&mut self, sim: &Simulation, c: &Content) {
        let body = self.rect();
        let dx = self.rem.0.signum() as i32;
        if self.rem.0 != 0.0 && !self.free(sim, c, body, dx, 0) && self.step_up(sim, c, body, dx, self.max_step_up()).is_none() {
            self.stop_x();
        }
        let dy = self.rem.1.signum() as i32;
        if self.rem.1 != 0.0 && !self.free(sim, c, body, 0, dy) && (dy > 0 || self.corner_nudge(sim, c, body).is_none()) {
            self.vel.1 = 0.0;
            self.rem.1 = 0.0;
        }
    }

    /// The highest ledge the robot can step up onto now.
    fn max_step_up(&self) -> i32 {
        if self.on_ground || self.in_liquid {
            STEP_UP
        } else if self.vel.1 > -AIR_STEP_MAX_RISE {
            AIR_STEP_UP
        } else {
            0
        }
    }

    /// When the head hits a corner on the way up: the smallest sideways move (up to
    /// `CORNER_NUDGE` cells) after which the body can move up. The side of the movement first.
    fn corner_nudge(&self, sim: &Simulation, c: &Content, body: CellRect) -> Option<i32> {
        let first = if self.vel.0 < 0.0 { -1 } else { 1 };
        for k in 1..=CORNER_NUDGE {
            for side in [first, -first] {
                let dx = side * k;
                let moved = shifted(body, dx, 0);
                if self.free(sim, c, body, dx, 0) && self.free(sim, c, moved, 0, -1) {
                    return Some(dx);
                }
            }
        }
        None
    }

    /// Powder that flowed into the bottom row of the body (the robot is not a cell, so sand can
    /// slide into its space) lifts the robot, up to `POWDER_LIFT` cells per tick, when the cells
    /// above the head are free. So sand that slides onto the feet does not trap the robot.
    fn rise_out_of_powder(&mut self, sim: &Simulation, c: &Content) {
        for _ in 0..POWDER_LIFT {
            let body = self.rect();
            let feet = CellRect::new(body.x0, body.y1 - 1, body.x1, body.y1);
            if is_free(sim, c, feet, CellRect::EMPTY, false) || !is_free(sim, c, shifted(body, 0, -1), body, false) {
                return;
            }
            self.top -= 1;
            self.draw_lift += 1.0;
            self.vel.1 = self.vel.1.min(0.0);
        }
    }

    /// The robot walked off a step: move it down onto the ground below, if the ground is at most
    /// `STEP_UP` cells down.
    fn follow_ground(&mut self, sim: &Simulation, c: &Content) {
        let body = self.rect();
        for d in 1..=STEP_UP {
            if !self.free(sim, c, body, 0, d) {
                return;
            }
            if !self.free(sim, c, shifted(body, 0, d), 0, 1) {
                self.top += d;
                self.draw_lift -= d as f32;
                self.on_ground = true;
                self.vel.1 = 0.0;
                self.rem.1 = 0.0;
                return;
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

/// True if no cell of `to` that is not also in `from` stops the robot. With `pass_powder`,
/// powder does not stop it.
fn is_free(sim: &Simulation, content: &Content, to: CellRect, from: CellRect, pass_powder: bool) -> bool {
    for y in to.y0..to.y1 {
        for x in to.x0..to.x1 {
            let p = CellPos::new(x, y);
            if from.contains(p) {
                continue;
            }
            match content.materials.phase[sim.cell(p).material.index()] {
                Phase::Solid => return false,
                Phase::Powder if !pass_powder => return false,
                _ => {}
            }
        }
    }
    true
}

/// The part of the cells in `r` that are liquid, and the part that are powder (0 to 1 each).
fn fractions(sim: &Simulation, content: &Content, r: CellRect) -> (f32, f32) {
    let (mut liquid, mut powder) = (0, 0);
    for y in r.y0..r.y1 {
        for x in r.x0..r.x1 {
            match content.materials.phase[sim.cell(CellPos::new(x, y)).material.index()] {
                Phase::Liquid => liquid += 1,
                Phase::Powder => powder += 1,
                _ => {}
            }
        }
    }
    let n = (r.width() * r.height()).max(1) as f32;
    (liquid as f32 / n, powder as f32 / n)
}

/// True if the top quarter of the body has no liquid (the head is above the surface).
fn head_out(sim: &Simulation, content: &Content, body: CellRect) -> bool {
    let head = CellRect::new(body.x0, body.y0, body.x1, body.y0 + ROBOT_H / 4);
    fractions(sim, content, head).0 == 0.0
}

#[cfg(test)]
#[path = "player_tests.rs"]
mod tests;
