//! The robot's pictures: the sprite sheet from `assets/sprites/robot.txt`, and which frame to
//! draw for the robot's state (standing, walking, jumping, falling, flying with the jetpack).
//!
//! The text file is pixel art as text (its top comment explains the format). The game reads it
//! at start, so the art can change without a new build. A copy is built into the program for
//! when the file is missing or broken.

use crate::player::{ROBOT_H, ROBOT_W, Robot};
use foundry_render::Sprite;
use glam::DVec2;
use std::collections::HashMap;

const BUILT_IN: &str = include_str!("../../../assets/sprites/robot.txt");

/// Size of one frame in art pixels.
pub const FRAME_W: u32 = 20;
pub const FRAME_H: u32 = 28;
/// Art pixels for each cell. The body box (8 x 16 cells) is 12 x 24 art pixels.
const ART_PER_CELL: f64 = 1.5;
/// The bottom middle of the body box in a frame (art pixels): the feet.
const PIVOT: [f32; 2] = [10.0, 28.0];
/// The tip of the drill in a frame (art pixels), for the tool beam.
const DRILL_TIP: [f32; 2] = [19.5, 20.5];

/// The frames of the robot, in the order of the sheet.
const FRAMES: [&str; 10] = ["idle0", "idle1", "walk0", "walk1", "walk2", "walk3", "jump", "fall", "jet0", "jet1"];

/// The sprite sheet: all frames side by side.
pub struct RobotSheet {
    pub width: u32,
    pub height: u32,
    /// RGBA8, sRGB, straight alpha.
    pub color: Vec<u8>,
    /// The parts that give light; the rest is transparent.
    pub emission: Vec<u8>,
}

impl RobotSheet {
    /// Read the sheet from the assets folder, or use the built-in copy.
    pub fn load() -> Self {
        let path = foundry_content::default_assets_dir().join("sprites").join("robot.txt");
        if let Ok(text) = std::fs::read_to_string(&path) {
            match parse(&text) {
                Ok(sheet) => return sheet,
                Err(e) => log::error!("{}: {e}; using the built-in robot", path.display()),
            }
        }
        parse(BUILT_IN).expect("the built-in robot sprite is valid")
    }
}

/// Parse the text format into a sheet with the frames of `FRAMES`, in that order.
pub fn parse(text: &str) -> Result<RobotSheet, String> {
    let mut colors: HashMap<char, [u8; 4]> = HashMap::new();
    let mut emissive: Vec<char> = Vec::new();
    let mut frames: HashMap<String, Vec<Vec<char>>> = HashMap::new();
    let mut current: Option<String> = None;
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace();
        match words.next() {
            Some("color") => {
                let key = words.next().and_then(|k| k.chars().next()).ok_or(format!("line {}: color needs a key", n + 1))?;
                let hex = words.next().ok_or(format!("line {}: color needs #rrggbb", n + 1))?;
                colors.insert(key, hex_color(hex).ok_or(format!("line {}: bad color {hex}", n + 1))?);
            }
            Some("emissive") => emissive.extend(words.filter_map(|w| w.chars().next())),
            Some("frame") => {
                let name = words.next().ok_or(format!("line {}: frame needs a name", n + 1))?.to_string();
                frames.insert(name.clone(), Vec::new());
                current = Some(name);
            }
            _ => {
                let name = current.as_ref().ok_or(format!("line {}: pixels before the first frame", n + 1))?;
                frames.get_mut(name).expect("inserted").push(line.chars().collect());
            }
        }
    }
    let (fw, fh) = (FRAME_W as usize, FRAME_H as usize);
    let width = fw * FRAMES.len();
    let mut color = vec![0u8; width * fh * 4];
    let mut emission = vec![0u8; width * fh * 4];
    for (i, name) in FRAMES.iter().enumerate() {
        let rows = frames.get(*name).ok_or(format!("frame {name} is missing"))?;
        if rows.len() != fh || rows.iter().any(|r| r.len() != fw) {
            return Err(format!("frame {name} must be {fw} x {fh} pixels"));
        }
        for (y, row) in rows.iter().enumerate() {
            for (x, ch) in row.iter().enumerate() {
                if *ch == '.' {
                    continue;
                }
                let c = *colors.get(ch).ok_or(format!("frame {name}: unknown color key {ch:?}"))?;
                let at = (y * width + i * fw + x) * 4;
                let target = if emissive.contains(ch) { &mut emission } else { &mut color };
                target[at..at + 4].copy_from_slice(&c);
            }
        }
    }
    Ok(RobotSheet { width: width as u32, height: fh as u32, color, emission })
}

fn hex_color(s: &str) -> Option<[u8; 4]> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8, 255])
}

/// The frame for the robot's state. `x` is the drawn left edge (for the walk cycle: the legs
/// move with the distance walked, so the feet do not slide).
fn frame(robot: &Robot, x: f32, tick: u64) -> &'static str {
    if !robot.on_ground {
        if robot.jetting {
            return if (tick / 3).is_multiple_of(2) { "jet0" } else { "jet1" };
        }
        return if robot.vel.1 < -0.05 { "jump" } else { "fall" };
    }
    if robot.vel.0.abs() > 0.05 {
        const WALK: [&str; 4] = ["walk0", "walk1", "walk2", "walk3"];
        return WALK[((x / 3.0).floor() as i64).rem_euclid(4) as usize];
    }
    // Standing: the antenna light blinks now and then.
    if tick % 120 < 8 { "idle1" } else { "idle0" }
}

/// The robot's sprite. `at` is the drawn top-left corner of the body (cells).
pub fn robot_sprite(robot: &Robot, at: (f32, f32), tick: u64) -> Sprite {
    let name = frame(robot, at.0, tick);
    let index = FRAMES.iter().position(|f| *f == name).unwrap_or(0) as u32;
    Sprite {
        pos: feet(at),
        src: [index * FRAME_W, 0, FRAME_W, FRAME_H],
        pivot: PIVOT,
        scale: (1.0 / ART_PER_CELL) as f32,
        angle: 0.0,
        flip_x: robot.facing < 0,
    }
}

/// The bottom middle of the body (cells), for a drawn top-left corner `at`.
fn feet(at: (f32, f32)) -> DVec2 {
    DVec2::new(at.0 as f64 + ROBOT_W as f64 * 0.5, at.1 as f64 + ROBOT_H as f64)
}

/// The tip of the drill (cells), where the tool beam starts.
pub fn drill_tip(at: (f32, f32), facing: i8) -> DVec2 {
    let dx = (DRILL_TIP[0] - PIVOT[0]) as f64 / ART_PER_CELL;
    let dy = (DRILL_TIP[1] - PIVOT[1]) as f64 / ART_PER_CELL;
    feet(at) + DVec2::new(dx * facing.signum() as f64, dy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundry_core::CellPos;

    #[test]
    fn the_robot_file_has_all_frames() {
        let sheet = parse(BUILT_IN).unwrap();
        assert_eq!((sheet.width, sheet.height), (FRAME_W * FRAMES.len() as u32, FRAME_H));
        // The visor glows: it is in the emission picture, not in the color picture.
        let lit = sheet.emission.chunks(4).filter(|p| p[3] > 0).count();
        let solid = sheet.color.chunks(4).filter(|p| p[3] > 0).count();
        assert!(lit > 50 && solid > 2000, "{lit} glowing and {solid} colored pixels");
    }

    /// Writes out/robot_look.png: every frame of the robot on the lit surface (top row) and in a
    /// dark cave with its lamp (bottom row). To check the art by eye.
    /// Run with `cargo test -p deep_foundry -- --ignored robot_look_image`.
    #[test]
    #[ignore]
    fn robot_look_image() {
        use foundry_core::{CHUNK_SIZE, ChunkImage, ChunkPos, Snapshot, local_index, pack_texel};
        use foundry_render::headless::{CAPTURE_FORMAT, capture, create_device};
        use foundry_render::{Camera, Renderer};
        let Some((device, queue)) = create_device() else { return };
        let content = foundry_content::Content::load_default().unwrap();
        let mut renderer = Renderer::new(&device, &queue, CAPTURE_FORMAT, &content);
        crate::render_setup::load_robot_sheet(&mut renderer);
        let (stone, dirt) = (content.expect_material("stone").0, content.expect_material("dirt").0);
        // Sky above y = 60, dirt ground from y = 60, a closed cave from y = 110 to 150.
        let mut snap = Snapshot { world_cells: (0, 256), ..Default::default() };
        for cx in 0..4 {
            for cy in 0..4 {
                let pos = ChunkPos::new(cx, cy);
                let mut c = ChunkImage::new_air(pos);
                for y in 0..CHUNK_SIZE {
                    for x in 0..CHUNK_SIZE {
                        let (wx, wy) = (cx * CHUNK_SIZE + x, cy * CHUNK_SIZE + y);
                        let cave = (8..248).contains(&wx) && (110..150).contains(&wy);
                        let m = if wy < 60 || cave { 0 } else if wy < 76 { dirt } else { stone };
                        c.texels[local_index(x, y)] = pack_texel(m, 20, ((wx * 7 + wy * 3) % 8) as u8, 0, 0);
                    }
                }
                snap.chunks.push(c);
            }
        }
        renderer.set_surface_level(60);
        let camera = Camera::new(DVec2::new(128.0, 105.0), 6.0, glam::UVec2::new(1536, 700));
        renderer.apply_snapshot(&snap, &camera);
        let mut sprites = vec![];
        let mut lights = vec![];
        for (i, _) in FRAMES.iter().enumerate() {
            for (row, feet_y) in [(0, 60.0), (1, 150.0)] {
                let at = (10.0 + i as f32 * 24.0, feet_y - ROBOT_H as f32);
                let mut s = robot_sprite(&Robot::standing_at(CellPos::new(0, 0)), at, 0);
                s.src[0] = i as u32 * FRAME_W;
                s.flip_x = i % 2 == 1 && row == 1;
                sprites.push(s);
                if row == 1 {
                    lights.push(crate::render_setup::robot_lamp((at.0 + 4.0, at.1 + 8.0), 1));
                }
            }
        }
        renderer.set_sprites(&sprites);
        renderer.set_lights(&lights[..2]);
        let img = capture(&device, &queue, &mut renderer, &camera);
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/robot_look.png");
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        image::RgbaImage::from_raw(1536, 700, img).unwrap().save(&out).unwrap();
    }

    #[test]
    fn bad_files_give_errors() {
        assert!(parse("frame idle0\n...\n").is_err());
        assert!(parse("color k #12345\n").is_err());
    }

    #[test]
    fn frames_follow_the_state() {
        let mut r = Robot::standing_at(CellPos::new(100, 100));
        r.on_ground = true;
        assert_eq!(frame(&r, 0.0, 50), "idle0");
        r.vel.0 = 0.5;
        let walk: Vec<&str> = (0..4).map(|i| frame(&r, i as f32 * 3.0, 0)).collect();
        assert_eq!(walk, vec!["walk0", "walk1", "walk2", "walk3"]);
        r.on_ground = false;
        r.vel.1 = -1.0;
        assert_eq!(frame(&r, 0.0, 0), "jump");
        r.vel.1 = 1.0;
        assert_eq!(frame(&r, 0.0, 0), "fall");
        r.jetting = true;
        assert!(frame(&r, 0.0, 0).starts_with("jet"));
        // Mirrored when the robot looks left; the feet are at the bottom middle of the body.
        r.facing = -1;
        let s = robot_sprite(&r, (10.0, 20.0), 0);
        assert!(s.flip_x);
        assert_eq!(s.pos, DVec2::new(14.0, 36.0));
        assert!(drill_tip((10.0, 20.0), -1).x < 14.0);
    }
}
