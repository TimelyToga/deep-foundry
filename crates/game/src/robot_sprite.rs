//! The picture of the robot: which animation frame it shows, and the sprites for the renderer.
//!
//! - The sprite sheet and its description are `assets/sprites/robot.png` and `robot.ron`. The
//!   script `tools/sprites/make_robot.py` makes them. The game reads the files at startup. If
//!   they are missing or wrong, it uses the copies built into the program.
//! - One sheet pixel is one world cell. The body is drawn at whole cells, so it lines up with
//!   the cells.
//! - The robot looks right in the sheet. When it looks left, every sprite is mirrored.
//! - The body, the arm and the arm outline are in `SpriteLayer::Body` (behind liquids and partly
//!   through them). The flame, the exhaust, the tool beam, the sparks and the dust are in
//!   `SpriteLayer::Front`.
//! - The front arm with the tool is a separate sprite in 16 directions. With a tool in use it
//!   points at the aim point, and the robot turns to the aim point.
//! - Particles have no state: their places come from the tick number. So the picture of any tick
//!   is the same every time (also in screenshots).

use crate::player::{ROBOT_H, ROBOT_W, Robot};
use anyhow::{Context, Result, bail};
use foundry_core::CellPos;
use foundry_render::{Sprite, SpriteLayer};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

const BUILT_IN_SHEET: &[u8] = include_bytes!("../../../assets/sprites/robot.png");
const BUILT_IN_DESC: &str = include_str!("../../../assets/sprites/robot.ron");

/// The robot shows the "land" animation this many ticks after a landing...
const LAND_TICKS: u16 = 8;
/// ...when it landed at least this fast (cells per tick).
const LAND_SPEED: f32 = 1.6;
/// It shows the "walk" animation above this speed (cells per tick).
const WALK_MIN: f32 = 0.15;

/// The description of the sheet (`robot.ron`). See the comments in that file.
#[derive(Debug, Clone, Deserialize)]
pub struct SheetDesc {
    pub image: String,
    pub frame: (i32, i32),
    pub body_at: (i32, i32),
    pub shoulder: (i32, i32),
    pub arm_pivot: (i32, i32),
    pub nozzle: (i32, i32),
    pub flame_at: (i32, i32),
    pub animations: HashMap<String, AnimDesc>,
    pub arm: Strip,
    pub arm_outline: Strip,
    pub arm_tips: Vec<(i32, i32)>,
    pub flame: Strip,
    pub pixel: Strip,
}

/// One animation: a row of frames.
#[derive(Debug, Clone, Deserialize)]
pub struct AnimDesc {
    pub row: u16,
    pub frames: u16,
    pub speed: Speed,
    pub looped: bool,
    pub poses: Vec<Pose>,
}

/// How fast an animation plays.
#[derive(Debug, Clone, Copy, Deserialize)]
pub enum Speed {
    /// Frames per second.
    Fps(f32),
    /// One frame per this many cells walked.
    Cells(f32),
}

/// The arm in one body frame (when no tool is used).
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Pose {
    /// Arm direction, 0 to 15 (0 forward, 4 down, 8 back, 12 up).
    pub arm: u8,
    /// Cells the shoulder moves down in this frame.
    pub shoulder: i32,
}

/// A row of frames that is not a body animation.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Strip {
    pub row: u16,
    pub frames: u16,
}

/// The body animations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anim {
    Idle,
    Walk,
    Jump,
    Fall,
    Land,
    Fly,
    Wade,
}

impl Anim {
    pub const ALL: [Anim; 7] = [Anim::Idle, Anim::Walk, Anim::Jump, Anim::Fall, Anim::Land, Anim::Fly, Anim::Wade];

    /// The name in `robot.ron`.
    pub fn name(self) -> &'static str {
        match self {
            Anim::Idle => "idle",
            Anim::Walk => "walk",
            Anim::Jump => "jump",
            Anim::Fall => "fall",
            Anim::Land => "land",
            Anim::Fly => "fly",
            Anim::Wade => "wade",
        }
    }

    pub fn from_name(name: &str) -> Option<Anim> {
        Anim::ALL.into_iter().find(|a| a.name() == name)
    }
}

/// The tools that point the arm at the aim point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Dig,
    Spray,
    Scan,
}

/// A tool in use this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToolUse {
    pub kind: ToolKind,
    /// The aim point (moved into reach).
    pub aim: CellPos,
    /// Cells were dug or sprayed in the last tick.
    pub working: bool,
    /// The color of the dug or sprayed material (RGBA).
    pub color: Option<[u8; 4]>,
}

/// A fixed animation frame (for screenshots).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Forced {
    pub anim: Anim,
    /// `None`: the frame from the time.
    pub frame: Option<usize>,
}

/// The sheet and its description.
pub struct RobotLook {
    pub desc: SheetDesc,
    /// RGBA8 texels, row by row from the top.
    pub rgba: Vec<u8>,
    pub size: (u32, u32),
}

impl RobotLook {
    /// Read `assets/sprites/robot.ron` and its image. If that fails, use the built-in copies.
    pub fn load() -> Self {
        let dir = foundry_content::default_assets_dir().join("sprites");
        match Self::from_dir(&dir) {
            Ok(look) => look,
            Err(e) => {
                log::warn!("robot sprites in {} cannot be used ({e:#}); using the built-in copy", dir.display());
                Self::built_in()
            }
        }
    }

    /// The sheet built into the program.
    pub fn built_in() -> Self {
        Self::parse(BUILT_IN_DESC, |_| Ok(BUILT_IN_SHEET.to_vec())).expect("the built-in robot sprites are valid")
    }

    fn from_dir(dir: &Path) -> Result<Self> {
        let desc = std::fs::read_to_string(dir.join("robot.ron")).context("cannot read robot.ron")?;
        Self::parse(&desc, |name| std::fs::read(dir.join(name)).with_context(|| format!("cannot read {name}")))
    }

    fn parse(desc: &str, read: impl Fn(&str) -> Result<Vec<u8>>) -> Result<Self> {
        let desc: SheetDesc = ron::from_str(desc).context("robot.ron")?;
        let png = read(&desc.image)?;
        let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png).context("robot sheet image")?.to_rgba8();
        let (w, h) = image.dimensions();
        let rows_needed = |s: u16| (s as u32 + 1) * desc.frame.1 as u32;
        for a in Anim::ALL {
            let Some(d) = desc.animations.get(a.name()) else { bail!("robot.ron has no animation {}", a.name()) };
            if d.frames == 0 || d.poses.len() != d.frames as usize {
                bail!("animation {}: {} poses for {} frames", a.name(), d.poses.len(), d.frames);
            }
            if rows_needed(d.row) > h || d.frames as u32 * desc.frame.0 as u32 > w {
                bail!("animation {} is outside the image", a.name());
            }
        }
        if desc.arm_tips.len() != desc.arm.frames as usize || desc.arm.frames == 0 || desc.arm_outline.frames != desc.arm.frames {
            bail!("robot.ron: the arm needs one tip and one outline per frame");
        }
        for s in [desc.arm, desc.arm_outline, desc.flame, desc.pixel] {
            if rows_needed(s.row) > h || s.frames == 0 {
                bail!("robot.ron: a strip is outside the image");
            }
        }
        Ok(Self { desc, rgba: image.into_raw(), size: (w, h) })
    }

    fn anim(&self, a: Anim) -> &AnimDesc {
        &self.desc.animations[a.name()]
    }

    /// Which animation and frame the robot shows. `tick` is the simulation tick (60 per second).
    pub fn choose(&self, r: &Robot, tick: u64) -> (Anim, usize) {
        let anim = if r.in_liquid {
            Anim::Wade
        } else if r.jetting {
            Anim::Fly
        } else if !r.on_ground {
            if r.vel.1 < -0.3 { Anim::Jump } else { Anim::Fall }
        } else if r.ground_ticks <= LAND_TICKS && r.land_speed >= LAND_SPEED {
            Anim::Land
        } else if r.vel.0.abs() > WALK_MIN && r.blocked == 0 {
            Anim::Walk
        } else {
            Anim::Idle
        };
        let d = self.anim(anim);
        let n = d.frames as usize;
        let frame = match anim {
            Anim::Jump => r.air_ticks as usize / 4,
            Anim::Land => (r.ground_ticks.saturating_sub(1) as usize * n) / LAND_TICKS as usize,
            _ => {
                // In a liquid with no movement, the legs move slower.
                let slow = if anim == Anim::Wade && r.vel.0.abs() < WALK_MIN { 0.4 } else { 1.0 };
                self.frame_at(d, r.walk_dist, tick, slow)
            }
        };
        (anim, if d.looped { frame % n } else { frame.min(n - 1) })
    }

    fn frame_at(&self, d: &AnimDesc, walked: f32, tick: u64, slow: f64) -> usize {
        match d.speed {
            Speed::Cells(c) => (walked / c.max(0.1)) as usize,
            Speed::Fps(fps) => (tick as f64 * foundry_core::TICK_SECONDS * fps as f64 * slow) as usize,
        }
    }

    /// The sprites of the robot for one frame, added to `out`.
    /// - `at`: the top-left corner of the body to draw, in cells (between two ticks).
    /// - `tool`: the tool in use, if any.
    /// - `tick`: the simulation tick (for the animations and the particles).
    /// - `forced`: show this animation frame (for screenshots).
    pub fn sprites(&self, r: &Robot, at: (f32, f32), tool: Option<ToolUse>, tick: u64, forced: Option<Forced>, out: &mut Vec<Sprite>) {
        let d = &self.desc;
        let (fw, fh) = d.frame;
        let (bx, by) = (at.0.round() as i32, at.1.round() as i32);
        let center_x = bx as f32 + ROBOT_W as f32 * 0.5;
        let facing = match tool {
            Some(t) if (t.aim.x as f32 + 0.5 - center_x).abs() >= 1.0 => (t.aim.x as f32 + 0.5 - center_x).signum() as i8,
            _ => r.facing,
        };
        let left = facing < 0;
        // A pixel column of a frame, mirrored when the robot looks left.
        let mx = |x: i32| if left { fw - 1 - x } else { x };
        let body_at_x = if left { fw - d.body_at.0 - ROBOT_W } else { d.body_at.0 };
        let (fx, fy) = (bx - body_at_x, by - d.body_at.1);

        let (anim, frame) = match forced {
            Some(f) => {
                let n = self.anim(f.anim).frames as usize;
                (f.anim, f.frame.unwrap_or_else(|| self.frame_at(self.anim(f.anim), r.walk_dist, tick, 1.0)) % n)
            }
            None => self.choose(r, tick),
        };
        let ad = self.anim(anim);
        let pose = ad.poses[frame];
        let rect = |row: u16, col: usize| [(col as i32 * fw) as u16, (row as i32 * fh) as u16];
        let sprite = |cell: [i32; 2], src: [u16; 2], layer| Sprite { cell, src, size: [fw as u16, fh as u16], tint: [255; 4], flip_x: left, layer };

        // The arm: its direction, and where the shoulder and the tool tip are.
        let (sx, sy) = (fx + mx(d.shoulder.0), fy + d.shoulder.1 + pose.shoulder);
        let dirs = d.arm.frames as usize;
        let dir = match tool {
            Some(t) => {
                let dx = (t.aim.x - sx) as f32 * facing as f32;
                let dy = (t.aim.y - sy) as f32;
                let turn = dy.atan2(dx).rem_euclid(std::f32::consts::TAU);
                (turn / std::f32::consts::TAU * dirs as f32).round() as usize % dirs
            }
            None => pose.arm as usize % dirs,
        };
        let (ax, ay) = (sx - mx(d.arm_pivot.0), sy - d.arm_pivot.1);
        let tip = d.arm_tips[dir];
        let tip = CellPos::new(ax + mx(tip.0), ay + tip.1);

        let nozzle = (fx + mx(d.nozzle.0), fy + d.nozzle.1);
        if r.jetting || forced.is_some_and(|f| f.anim == Anim::Fly) {
            let power = if r.jetting { r.jet_power } else { 1.0 };
            let n = d.flame.frames as usize;
            let flicker = (tick / 2 % 2) as usize;
            let i = if power < 0.6 { flicker } else { n.saturating_sub(2) + flicker }.min(n - 1);
            out.push(sprite([nozzle.0 - mx(d.flame_at.0), nozzle.1 - d.flame_at.1], rect(d.flame.row, i), SpriteLayer::Front));
            self.exhaust(nozzle, facing, tick, out);
        }
        out.push(sprite([ax, ay], rect(d.arm_outline.row, dir), SpriteLayer::Body));
        out.push(sprite([fx, fy], rect(ad.row, frame), SpriteLayer::Body));
        out.push(sprite([ax, ay], rect(d.arm.row, dir), SpriteLayer::Body));
        if anim == Anim::Land && r.land_speed >= 2.0 && forced.is_none() {
            self.dust(bx, by + ROBOT_H - 1, r.ground_ticks as i32, out);
        }
        if let Some(t) = tool {
            self.beam(tip, t, tick, out);
        }
    }

    /// A one-cell particle.
    fn pixel(&self, x: i32, y: i32, color: [u8; 4], out: &mut Vec<Sprite>) {
        let p = &self.desc.pixel;
        let src = [0, (p.row as i32 * self.desc.frame.1) as u16];
        out.push(Sprite { cell: [x, y], src, size: [1, 1], tint: color, flip_x: false, layer: SpriteLayer::Front });
    }

    /// Hot gas and sparks below the jetpack flame, moving down and back.
    fn exhaust(&self, nozzle: (i32, i32), facing: i8, tick: u64, out: &mut Vec<Sprite>) {
        for i in 0..7u64 {
            let life = 16;
            let age = (tick + i * 5) % life;
            let t = age as f32 / life as f32;
            let seed = hash(i + (tick + i * 5) / life * 31);
            let jitter = (seed % 3) as i32 - 1;
            let x = nozzle.0 - (facing as f32 * t * 4.0).round() as i32 + jitter;
            let y = nozzle.1 + 5 + (t * 12.0) as i32;
            let color = if t < 0.3 {
                [255, 214, 90, 255]
            } else if t < 0.6 {
                [236, 96, 30, 220]
            } else {
                [90, 84, 92, (200.0 * (1.0 - t)) as u8 + 40]
            };
            self.pixel(x, y, color, out);
        }
    }

    /// Dust at the feet after a hard landing.
    fn dust(&self, cx: i32, feet_y: i32, age: i32, out: &mut Vec<Sprite>) {
        let alpha = (220 - age * 25).clamp(40, 220) as u8;
        for k in 0..3 {
            let spread = 2 + age / 2 + k;
            let y = feet_y - (k + age / 3).min(2);
            self.pixel(cx - 1 - spread, y, [196, 184, 160, alpha], out);
            self.pixel(cx + ROBOT_W + spread, y, [196, 184, 160, alpha], out);
        }
    }

    /// The tool beam from the tool tip to the aim point, with sparks and flying cells.
    fn beam(&self, tip: CellPos, t: ToolUse, tick: u64, out: &mut Vec<Sprite>) {
        let line = cells_between(tip, t.aim);
        let n = line.len().max(1);
        let material = t.color.unwrap_or([150, 130, 110, 255]);
        match t.kind {
            ToolKind::Dig => {
                for (i, p) in line.iter().enumerate() {
                    let bright = (i as u64 + tick).is_multiple_of(3);
                    let color = match (t.working, bright) {
                        (true, true) => [255, 255, 236, 255],
                        (true, false) => [255, 216, 110, 235],
                        (false, true) => [255, 170, 80, 200],
                        (false, false) => [0, 0, 0, 0],
                    };
                    if color[3] > 0 {
                        self.pixel(p.x, p.y, color, out);
                    }
                }
                if t.working {
                    // Sparks fly out of the dig point.
                    for i in 0..8u64 {
                        let life = 6;
                        let age = (tick + i * 2) % life;
                        let seed = hash(i * 977 + (tick + i * 2) / life);
                        let a = (seed % 360) as f32 * std::f32::consts::PI / 180.0;
                        let d = 1.0 + age as f32 * 1.2;
                        let color = [[255, 255, 230, 255], [255, 226, 110, 255], [255, 150, 50, 230]][(seed % 3) as usize];
                        self.pixel(t.aim.x + (a.cos() * d).round() as i32, t.aim.y + (a.sin() * d).round() as i32, color, out);
                    }
                    // Dug cells fly to the tool.
                    for i in 0..6u64 {
                        let k = ((tick * 2 + i * 7) % 20) as f32 / 20.0;
                        let p = line[((1.0 - k) * (n - 1) as f32) as usize];
                        let wobble = (hash(i + tick / 4) % 3) as i32 - 1;
                        self.pixel(p.x + wobble, p.y - wobble, material, out);
                    }
                }
            }
            ToolKind::Spray => {
                for (i, p) in line.iter().enumerate() {
                    if (i as u64 + tick * 2).is_multiple_of(4) {
                        self.pixel(p.x, p.y, if t.working { material } else { [150, 190, 230, 150] }, out);
                    }
                }
            }
            ToolKind::Scan => {
                for (i, p) in line.iter().enumerate() {
                    if (i as u64 + tick).is_multiple_of(2) {
                        self.pixel(p.x, p.y, [120, 246, 255, 170], out);
                    }
                }
                // Four corners around the scanned cell, blinking.
                if (tick / 8).is_multiple_of(2) {
                    for (dx, dy) in [(-2, -2), (2, -2), (-2, 2), (2, 2)] {
                        self.pixel(t.aim.x + dx, t.aim.y + dy, [120, 246, 255, 255], out);
                    }
                }
            }
        }
    }
}

/// The cells on a line from `a` to `b` (Bresenham), without `a`, with `b`.
fn cells_between(a: CellPos, b: CellPos) -> Vec<CellPos> {
    let (dx, dy) = ((b.x - a.x).abs(), -(b.y - a.y).abs());
    let (sx, sy) = ((b.x - a.x).signum(), (b.y - a.y).signum());
    let (mut x, mut y, mut err) = (a.x, a.y, dx + dy);
    let mut out = vec![];
    while (x, y) != (b.x, b.y) && out.len() < 400 {
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
        out.push(CellPos::new(x, y));
    }
    out
}

/// A number that looks random, from a number.
fn hash(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn robot() -> Robot {
        let mut r = Robot::standing_at(CellPos::new(100, 200));
        r.on_ground = true;
        r.ground_ticks = 100;
        r
    }

    #[test]
    fn built_in_sheet_and_files_are_valid() {
        let look = RobotLook::built_in();
        assert_eq!(look.rgba.len() as u32, look.size.0 * look.size.1 * 4);
        let dir = foundry_content::default_assets_dir().join("sprites");
        RobotLook::from_dir(&dir).unwrap();
    }

    #[test]
    fn picks_the_animation_from_the_movement() {
        let look = RobotLook::built_in();
        let mut r = robot();
        assert_eq!(look.choose(&r, 0).0, Anim::Idle);
        r.vel.0 = 1.0;
        assert_eq!(look.choose(&r, 0).0, Anim::Walk);
        r.blocked = 3;
        assert_eq!(look.choose(&r, 0).0, Anim::Idle, "walking into a wall shows idle");
        r.on_ground = false;
        r.vel.1 = -2.0;
        assert_eq!(look.choose(&r, 0).0, Anim::Jump);
        r.vel.1 = 1.0;
        assert_eq!(look.choose(&r, 0).0, Anim::Fall);
        r.jetting = true;
        assert_eq!(look.choose(&r, 0).0, Anim::Fly);
        r.in_liquid = true;
        assert_eq!(look.choose(&r, 0).0, Anim::Wade);
        let mut r = robot();
        r.ground_ticks = 2;
        r.land_speed = 3.0;
        assert_eq!(look.choose(&r, 0), (Anim::Land, 0));
        r.ground_ticks = LAND_TICKS;
        assert_eq!(look.choose(&r, 0).0, Anim::Land);
        r.ground_ticks = LAND_TICKS + 1;
        assert_eq!(look.choose(&r, 0).0, Anim::Idle);
    }

    #[test]
    fn walk_frames_follow_the_distance() {
        let look = RobotLook::built_in();
        let mut r = robot();
        r.vel.0 = 1.0;
        let frames: Vec<usize> = (0..12).map(|i| {
            r.walk_dist = i as f32 * 3.0;
            look.choose(&r, 0).1
        }).collect();
        assert_eq!(frames, vec![0, 1, 2, 3, 4, 5, 0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn sprites_are_on_the_grid_and_mirror_to_the_left() {
        let look = RobotLook::built_in();
        let d = &look.desc;
        let mut r = robot();
        let mut out = vec![];
        look.sprites(&r, (96.4, 184.0), None, 0, None, &mut out);
        // Arm outline, body, arm.
        assert_eq!(out.len(), 3);
        let body = out[1];
        assert_eq!(body.cell, [96 - d.body_at.0, 184 - d.body_at.1]);
        assert!(!body.flip_x);
        r.facing = -1;
        out.clear();
        look.sprites(&r, (96.4, 184.0), None, 0, None, &mut out);
        assert!(out.iter().all(|s| s.flip_x));
        // The body frame is centered on the body, so it does not move when mirrored.
        assert_eq!(out[1].cell, body.cell);
    }

    #[test]
    fn the_arm_points_at_the_aim_and_the_robot_turns() {
        let look = RobotLook::built_in();
        let r = robot();
        let arm_row = look.desc.arm.row as i32 * look.desc.frame.1;
        let dir_of = |out: &[Sprite]| out.iter().find(|s| s.src[1] as i32 == arm_row).map(|s| s.src[0] as i32 / look.desc.frame.0).unwrap();
        for (aim, dir, flip) in [((160, 196), 0, false), ((40, 196), 0, true), ((105, 260), 4, false), ((105, 120), 12, false)] {
            let tool = ToolUse { kind: ToolKind::Dig, aim: CellPos::new(aim.0, aim.1), working: true, color: None };
            let mut out = vec![];
            look.sprites(&r, (96.0, 184.0), Some(tool), 0, None, &mut out);
            assert_eq!(dir_of(&out), dir, "aim {aim:?}");
            assert_eq!(out[1].flip_x, flip, "aim {aim:?}");
            assert!(out.iter().any(|s| s.layer == SpriteLayer::Front), "beam and sparks");
        }
    }

    #[test]
    fn beam_reaches_the_aim() {
        let line = cells_between(CellPos::new(0, 0), CellPos::new(5, -3));
        assert_eq!(line.last(), Some(&CellPos::new(5, -3)));
        assert_eq!(line.len(), 5);
        assert!(cells_between(CellPos::new(2, 2), CellPos::new(2, 2)).is_empty());
    }
}
