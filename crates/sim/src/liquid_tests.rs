//! Liquid tests: scenes with water, oil, lava and mud. Each test runs until everything rests
//! (no awake chunk, no particle), checks that no liquid is lost, and checks the result.
//!
//! `cargo test -p foundry_sim --release liquid_frames -- --ignored --nocapture` runs every scene,
//! prints numbers, and writes image frames to `out/liquids/` (particles are light dots).

use super::*;

const STONE: &str = "stone";

/// A scene: a world, and liquid that is added before some ticks (a pour).
struct Scene {
    name: &'static str,
    sim: Simulation,
    /// Materials whose total (cells + particles) must not change.
    kept: Vec<MaterialId>,
    /// Cells added by `pour` so far, per material in `kept`.
    added: Vec<usize>,
    /// (area, material, ticks): add one row of the material in the area (a stream) while tick < ticks.
    pour: Option<(CellRect, MaterialId, u64)>,
    /// Frames for the contact sheet: (tick, picture).
    sheet: Vec<(u64, image::RgbImage)>,
}

/// What a scene run measured.
#[derive(Debug)]
struct Outcome {
    /// Tick when everything rested (no awake chunk and no particle). 0 = never.
    rest: u64,
    max_particles: usize,
    /// Tick of the first splash particle.
    first_splash: u64,
}

fn content() -> Arc<Content> {
    Arc::new(Content::load_default().unwrap())
}

fn scene(name: &'static str, width_chunks: i32, height_chunks: i32, liquids: &[&str]) -> Scene {
    let content = content();
    let kept = liquids.iter().map(|n| content.expect_material(n)).collect::<Vec<_>>();
    let sim = Simulation::new(content, SimConfig { width_chunks, height_chunks, seed: 4, bedrock_border: true });
    Scene { name, sim, added: vec![0; kept.len()], kept, pour: None, sheet: vec![] }
}

impl Scene {
    fn mat(&self, name: &str) -> MaterialId {
        self.sim.content().expect_material(name)
    }

    /// Fill cells with x in `x0..x1` and y in `y0..y1`.
    fn fill(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, name: &str) {
        let m = self.mat(name);
        for y in y0..y1 {
            for x in x0..x1 {
                self.sim.set_cell(CellPos::new(x, y), m, None);
            }
        }
    }

    fn ball(&mut self, cx: i32, cy: i32, r: i32, name: &str) {
        let m = self.mat(name);
        self.sim.paint(CellPos::new(cx, cy), r, m, PaintMode::Replace, None);
    }

    /// A stone box with walls `t` thick: inside x in `x0..x1`, floor at `y1`, walls from `y0` down.
    fn open_box(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, t: i32) {
        self.fill(x0 - t, y0, x0, y1 + t, STONE);
        self.fill(x1, y0, x1 + t, y1 + t, STONE);
        self.fill(x0 - t, y1, x1 + t, y1 + t, STONE);
    }

    /// Total of a kept material: cells in the world plus material particles.
    fn total(&self, m: MaterialId) -> usize {
        let cells: usize = self.sim.world.chunks.iter().flatten().map(|c| c.mat.iter().filter(|&&v| v == m.0).count()).sum();
        cells + self.sim.particles().count_material(m)
    }

    /// Run until everything rests (plus `extra` ticks), or `max` ticks. Checks every tick that
    /// no kept material is lost. Saves frames at the given ticks when `frames` is set.
    fn run(&mut self, max: u64, frames: Option<(&[u64], CellRect, u32)>) -> Outcome {
        let start: Vec<usize> = self.kept.iter().map(|&m| self.total(m)).collect();
        let mut out = Outcome { rest: 0, max_particles: 0, first_splash: 0 };
        for t in 1..=max {
            if let Some((area, m, until)) = self.pour
                && t <= until
            {
                let k = self.kept.iter().position(|&q| q == m).unwrap();
                for x in area.x0..area.x1 {
                    let p = CellPos::new(x, area.y0);
                    if self.sim.cell(p).material.is_air() {
                        self.sim.set_cell(p, m, None);
                        self.added[k] += 1;
                    }
                }
            }
            self.sim.tick();
            let n = self.sim.particles().len();
            out.max_particles = out.max_particles.max(n);
            if n > 0 && out.first_splash == 0 {
                out.first_splash = t;
            }
            for (k, &m) in self.kept.iter().enumerate() {
                assert_eq!(self.total(m), start[k] + self.added[k], "{}: material {} is kept at tick {t}", self.name, m.0);
            }
            if let Some((ticks, area, scale)) = frames
                && ticks.contains(&t)
            {
                self.frame(t, area, scale);
            }
            let pouring = self.pour.is_some_and(|(_, _, until)| t <= until);
            if !pouring && self.sim.stats().awake_chunks == 0 && self.sim.particles().is_empty() {
                out.rest = t;
                if let Some((_, area, scale)) = frames {
                    self.frame(t, area, scale);
                }
                break;
            }
        }
        out
    }

    fn frame(&mut self, t: u64, area: CellRect, scale: u32) {
        let path = format!("{}/../../out/liquids/{}_{t:05}.png", env!("CARGO_MANIFEST_DIR"), self.name);
        dump_png(&self.sim, area, scale, &path);
        self.sheet.push((t, render(&self.sim, area, 1)));
    }

    /// Write all frames of the run into one picture, `cols` frames per row, with the tick
    /// written as short bars under each frame (one bar per 100 ticks, a thin one per 10).
    fn save_sheet(&self, cols: usize, scale: u32) {
        if self.sheet.is_empty() {
            return;
        }
        let (w, h) = self.sheet[0].1.dimensions();
        let (fw, fh) = (w * scale + 6, h * scale + 14);
        let rows = self.sheet.len().div_ceil(cols) as u32;
        let mut img = image::RgbImage::from_pixel(fw * cols as u32, fh * rows, image::Rgb([60, 60, 70]));
        for (n, (t, frame)) in self.sheet.iter().enumerate() {
            let (ox, oy) = ((n % cols) as u32 * fw + 3, (n / cols) as u32 * fh + 3);
            for y in 0..h * scale {
                for x in 0..w * scale {
                    img.put_pixel(ox + x, oy + y, *frame.get_pixel(x / scale, y / scale));
                }
            }
            // Tick marks: a tall bar per 100 ticks, a short bar per 10 ticks.
            let (hundreds, tens) = ((t / 100) as u32, ((t % 100) / 10) as u32);
            let mut x = ox;
            for (count, tall) in [(hundreds, 7), (tens, 3)] {
                for _ in 0..count {
                    for dy in 0..tall {
                        for dx in 0..2 {
                            if x + dx < img.width() {
                                img.put_pixel(x + dx, oy + h * scale + 9 - dy, image::Rgb([230, 230, 90]));
                            }
                        }
                    }
                    x += 4;
                }
                x += 4;
            }
        }
        let path = format!("{}/../../out/liquids/{}_sheet.png", env!("CARGO_MANIFEST_DIR"), self.name);
        img.save(path).unwrap();
    }

    /// Height of the liquid surface in each column `x0..x1`: the y of the first cell (from the top,
    /// starting at `y0`) that is one of `liquids`. `None` if the column has none.
    fn surface(&self, x0: i32, x1: i32, y0: i32, liquids: &[MaterialId]) -> Vec<Option<i32>> {
        let h = self.sim.size_cells().1;
        (x0..x1).map(|x| (y0..h).find(|&y| liquids.contains(&self.sim.cell(CellPos::new(x, y)).material))).collect()
    }

    /// Lowest and highest surface row over the columns `x0..x1`.
    fn surface_range(&self, x0: i32, x1: i32, y0: i32, liquids: &[MaterialId]) -> (i32, i32) {
        let s: Vec<i32> = self.surface(x0, x1, y0, liquids).into_iter().map(|v| v.unwrap_or(i32::MAX)).collect();
        (*s.iter().min().unwrap(), *s.iter().max().unwrap())
    }

    /// The rightmost column in `x0..x1` with the material in row `y`.
    fn front(&self, y: i32, x0: i32, x1: i32, m: MaterialId) -> i32 {
        (x0..x1).rev().find(|&x| self.sim.cell(CellPos::new(x, y)).material == m).unwrap_or(x0)
    }
}

// ---- Scenes ----

/// A ball of water (radius 30) falls into a 384 x 256 box.
fn ball_scene(cx: i32) -> Scene {
    let mut s = scene("ball", 6, 4, &["water"]);
    s.ball(cx, 50, 30, "water");
    s
}

/// A block of water 298 wide and 232 tall stands at the left of a 1024 x 384 basin, as if a
/// dam was just removed.
fn dam_scene() -> Scene {
    let mut s = scene("dam", 16, 6, &["water"]);
    s.fill(2, 150, 300, 382, "water");
    s
}

/// A stream of water falls about 250 cells from a hole into a basin (512 x 384).
fn waterfall_scene() -> Scene {
    let mut s = scene("waterfall", 8, 6, &["water"]);
    let area = CellRect::new(100, 30, 106, 31);
    s.pour = Some((area, s.mat("water"), 900));
    s
}

#[test]
fn dropped_ball_splashes_spreads_and_rests_flat() {
    let mut s = ball_scene(192);
    let water = s.mat("water");
    let out = s.run(3000, None);
    let (lo, hi) = s.surface_range(2, 382, 0, &[water]);
    println!("ball: {out:?}, surface rows {lo}..{hi}");
    assert!(out.max_particles > 20, "the landing splashes: {} particles", out.max_particles);
    assert!(out.rest > 0 && out.rest < 800, "water comes to rest fast: tick {}", out.rest);
    assert!(hi - lo <= 1, "the surface is flat: rows {lo} to {hi}");
}

#[test]
fn dam_break_floods_the_basin_and_rests_flat() {
    let mut s = dam_scene();
    let water = s.mat("water");
    let mut front_at_60 = 0;
    for t in 1..=60 {
        s.sim.tick();
        if t == 60 {
            front_at_60 = s.front(381, 2, 1022, water);
        }
    }
    let out = s.run(4000, None);
    let (lo, hi) = s.surface_range(2, 1022, 0, &[water]);
    println!("dam: front at tick 60: x {front_at_60}; {out:?}; surface rows {lo}..{hi}");
    assert!(front_at_60 > 700, "the flood wave runs fast: front at x {front_at_60} after 60 ticks");
    assert!(out.rest > 0, "the flood comes to rest");
    assert!(hi - lo <= 1, "the surface is flat: rows {lo} to {hi}");
}

#[test]
fn waterfall_splashes_and_the_pool_rises_flat() {
    let mut s = waterfall_scene();
    let water = s.mat("water");
    let out = s.run(4000, None);
    let (lo, hi) = s.surface_range(2, 510, 100, &[water]);
    println!("waterfall: {out:?}; surface rows {lo}..{hi}");
    assert!(out.max_particles > 20, "the stream splashes: {} particles", out.max_particles);
    assert!(out.rest > 0, "the pool comes to rest");
    assert!(hi - lo <= 1, "the pool is flat: rows {lo} to {hi}");
}

/// Writes frames of all scenes to `out/liquids/` and prints numbers.
#[test]
#[ignore]
fn liquid_frames() {
    let only = std::env::var("SCENE").ok();
    let want = |n: &str| only.as_deref().is_none_or(|o| o == n);
    if want("ball") {
        let mut s = ball_scene(192);
        let water = s.mat("water");
        let ticks = [10, 20, 30, 40, 50, 60, 80, 100, 130, 160, 200, 300, 400, 600];
        let out = s.run(3000, Some((&ticks, CellRect::new(0, 0, 384, 256), 3)));
        s.save_sheet(3, 1);
        println!("ball: {out:?}, surface {:?}", s.surface_range(2, 382, 0, &[water]));
    }
    if want("dam") {
        let mut s = dam_scene();
        let water = s.mat("water");
        let ticks = [20, 40, 60, 90, 120, 160, 200, 300, 400, 600, 900, 1200];
        let out = s.run(4000, Some((&ticks, CellRect::new(0, 0, 1024, 384), 1)));
        s.save_sheet(2, 1);
        println!("dam: {out:?}, surface {:?}", s.surface_range(2, 1022, 0, &[water]));
    }
    if want("waterfall") {
        let mut s = waterfall_scene();
        let water = s.mat("water");
        let ticks = [40, 80, 150, 300, 600, 900, 1000, 1200];
        let out = s.run(4000, Some((&ticks, CellRect::new(0, 0, 512, 384), 2)));
        s.save_sheet(3, 1);
        println!("waterfall: {out:?}, surface {:?}", s.surface_range(2, 510, 100, &[water]));
    }
}

#[test]
#[ignore]
fn debug_fall_start() {
    let mut s = scene("dbg", 6, 4, &["water"]);
    s.fill(170, 40, 214, 90, "water");
    for t in 0..3 {
        for x in [179, 180, 181] {
            let mut line = String::new();
            for y in 38..70 {
                let p = CellPos::new(x, y);
                let ch = s.sim.world.chunk(p.chunk()).unwrap();
                let i = p.local_index();
                if ch.mat[i] == 0 { line += " .  "; } else { line += &format!("{:3} ", ch.shade[i]); }
            }
            println!("t{t} x{x} {line}");
        }
        s.sim.tick();
    }
}

#[test]
#[ignore]
fn debug_cliff() {
    let mut s = scene("cliff", 6, 3, &["water"]);
    s.fill(2, 60, 128, 190, "water");
    let ticks: Vec<u64> = (1..=12).map(|k| k * 5).collect();
    s.run(60, Some((&ticks, CellRect::new(0, 0, 384, 192), 2)));
    s.save_sheet(3, 1);
    for y in (60..190).step_by(8) {
        let mut line = String::new();
        for x in 118..140 {
            let p = CellPos::new(x, y);
            let ch = s.sim.world.chunk(p.chunk()).unwrap();
            let i = p.local_index();
            line += &if ch.mat[i] == 0 { "  .".to_string() } else { format!(" {:02x}", ch.motion[i]) };
        }
        println!("y{y:3}:{line}");
    }
}

#[test]
#[ignore]
fn debug_block() {
    let mut s = scene("block", 6, 4, &["water"]);
    s.fill(130, 214, 250, 254, "water");
    let ticks: Vec<u64> = vec![1, 3, 8, 16, 30, 60, 100, 150, 200, 300, 400, 600];
    let out = s.run(2000, Some((&ticks, CellRect::new(0, 128, 384, 256), 2)));
    s.save_sheet(3, 1);
    println!("{out:?}");
}

#[test]
#[ignore]
fn debug_block_rows() {
    let mut s = scene("block", 6, 4, &["water"]);
    s.fill(130, 214, 250, 254, "water");
    for t in 0..6 {
        println!("tick {t}");
        for y in [213, 214, 215, 230, 240, 250, 252, 253] {
            let mut line = String::new();
            for x in 236..262 {
                let p = CellPos::new(x, y);
                let ch = s.sim.world.chunk(p.chunk()).unwrap();
                let i = p.local_index();
                line += &if ch.mat[i] == 0 { "  .".to_string() } else { format!(" {:02x}", ch.motion[i]) };
            }
            println!("y{y:3}:{line}");
        }
        s.sim.tick();
    }
}

#[test]
#[ignore]
fn debug_slope() {
    let mut s = scene("slope", 6, 4, &["water"]);
    s.fill(130, 214, 250, 254, "water");
    for _ in 0..60 { s.sim.tick(); }
    for t in 0..3 {
        println!("tick {}", s.sim.tick_count());
        for y in 222..254 {
            let mut line = String::new();
            for x in 244..290 {
                let p = CellPos::new(x, y);
                let ch = s.sim.world.chunk(p.chunk()).unwrap();
                let i = p.local_index();
                line += &if ch.mat[i] == 0 { " .".to_string() } else { format!("{:2x}", ch.motion[i] >> 4) };
            }
            println!("y{y:3}:{line}");
        }
        s.sim.tick();
    }
    let d: Vec<_> = s.sim.world.loaded_chunks().filter_map(|p| { let c = s.sim.world.chunk(p).unwrap(); (!c.dirty.is_empty()).then_some((p, c.dirty)) }).collect();
    println!("{d:?}");
}

#[test]
#[ignore]
fn debug_hill() {
    let mut s = scene("hill", 6, 4, &["water"]);
    s.fill(130, 214, 250, 254, "water");
    let water = s.mat("water");
    for _ in 0..100 { s.sim.tick(); }
    for t in 0..4 {
        let prof: Vec<String> = s.surface(2, 382, 150, &[water]).iter().step_by(8).map(|v| format!("{}", 254 - v.unwrap_or(254))).collect();
        println!("tick {} awake {} heights {}", s.sim.tick_count(), s.sim.stats().awake_chunks, prof.join(" "));
        let before: Vec<u16> = (2..382).flat_map(|x| (150..254).map(move |y| (x, y))).map(|(x, y)| s.sim.cell(CellPos::new(x, y)).material.0).collect();
        let mo: Vec<u8> = (2..382).flat_map(|x| (150..254).map(move |y| (x, y))).map(|(x, y)| { let p = CellPos::new(x, y); s.sim.world.chunk(p.chunk()).map_or(0, |c| c.motion[p.local_index()]) }).collect();
        s.sim.tick();
        let after: Vec<u16> = (2..382).flat_map(|x| (150..254).map(move |y| (x, y))).map(|(x, y)| s.sim.cell(CellPos::new(x, y)).material.0).collect();
        let changed = before.iter().zip(&after).filter(|(a, b)| a != b).count();
        let energetic = mo.iter().zip(&before).filter(|(m, b)| **b != 0 && **m & 0xc0 != 0).count();
        println!("  changed cells {changed}, cells with momentum {energetic}");
    }
    for y in 218..254 {
        let mut line = String::new();
        for x in 100..300 {
            let p = CellPos::new(x, y);
            let c = s.sim.world.chunk(p.chunk()).unwrap();
            let i = p.local_index();
            line.push(if c.mat[i] == 0 { '.' } else if c.motion[i] & 0x1f != 0 { 'F' } else if c.motion[i] & 0xc0 != 0 { if c.motion[i] & 0x20 != 0 { '>' } else { '<' } } else { 'w' });
        }
        println!("{y}: {line}");
    }
}

#[test]
#[ignore]
fn debug_dam_face() {
    let mut s = dam_scene();
    let water = s.mat("water");
    let t: u64 = std::env::var("T").ok().and_then(|v| v.parse().ok()).unwrap_or(300);
    for _ in 0..t { s.sim.tick(); }
    let prof: Vec<String> = s.surface(2, 1022, 100, &[water]).iter().step_by(32).map(|v| format!("{}", 382 - v.unwrap_or(382))).collect();
    println!("tick {} awake {} particles {} heights {}", s.sim.tick_count(), s.sim.stats().awake_chunks, s.sim.particles().len(), prof.join(" "));
    let (mut runners, mut falling, mut top) = (0, 0, 0);
    for x in 2..1022 {
        for y in 100..382 {
            let p = CellPos::new(x, y);
            let Some(c) = s.sim.world.chunk(p.chunk()) else { continue };
            let i = p.local_index();
            if c.mat[i] == 0 { continue; }
            if c.motion[i] & 0x1f != 0 { falling += 1; } else if c.motion[i] & 0xc0 != 0 { runners += 1; }
            let up = CellPos::new(x, y - 1);
            if s.sim.cell(up).material.is_air() { top += 1; }
        }
    }
    println!("runners {runners} falling {falling} top cells {top}");
    let hash = s.sim.world_hash();
    let dirty: Vec<_> = s.sim.world.loaded_chunks().filter(|p| !s.sim.world.chunk(*p).unwrap().dirty.is_empty()).collect();
    println!("dirty chunks {:?}", dirty);
    let mut w = dam_scene();
    let _ = &mut w;
    s.sim.tick();
    println!("normal tick: world changed {}", hash != s.sim.world_hash());
    let before = s.sim.world_hash();
    for c in s.sim.world.chunks.iter_mut().flatten() { c.dirty = chunk::LocalRect::FULL; }
    s.sim.tick();
    println!("after waking all: world changed {} awake {}", before != s.sim.world_hash(), s.sim.stats().awake_chunks);
    let x0: i32 = std::env::var("X").ok().and_then(|v| v.parse().ok()).unwrap_or(400);
    let ys = s.surface(x0, x0 + 1, 100, &[water])[0].unwrap_or(300);
    for y in ys - 4..ys + 6 {
        let mut line = String::new();
        for x in x0..x0 + 150 {
            let p = CellPos::new(x, y);
            let c = s.sim.world.chunk(p.chunk()).unwrap();
            let i = p.local_index();
            line.push(if c.mat[i] == 0 { '.' } else if c.motion[i] & 0x1f != 0 { 'F' } else if c.motion[i] & 0xc0 != 0 { if c.motion[i] & 0x20 != 0 { '>' } else { '<' } } else if c.motion[i] & 0x20 != 0 { 'r' } else { 'l' });
        }
        println!("{y}: {line}");
    }
}

#[test]
#[ignore]
fn debug_dam_flux() {
    let mut s = dam_scene();
    let water = s.mat("water");
    let t: u64 = std::env::var("T").ok().and_then(|v| v.parse().ok()).unwrap_or(300);
    for _ in 0..t { s.sim.tick(); }
    let left = |s: &Scene| s.sim.count_material(CellRect::new(0, 0, 400, 384), water);
    for _ in 0..5 {
        let before: Vec<u16> = (2..1022).flat_map(|x| (100..382).map(move |y| (x, y))).map(|(x, y)| s.sim.cell(CellPos::new(x, y)).material.0).collect();
        let l0 = left(&s);
        s.sim.tick();
        let after: Vec<u16> = (2..1022).flat_map(|x| (100..382).map(move |y| (x, y))).map(|(x, y)| s.sim.cell(CellPos::new(x, y)).material.0).collect();
        let changed = before.iter().zip(&after).filter(|(a, b)| a != b).count();
        println!("tick {} changed {changed} water in x<400: {} -> {}", s.sim.tick_count(), l0, left(&s));
    }
}

#[test]
#[ignore]
fn debug_ball_end() {
    let mut s = ball_scene(192);
    let out = s.run(3000, None);
    println!("{out:?}");
    for c in s.sim.world.chunks.iter_mut().flatten() { c.dirty = chunk::LocalRect::FULL; }
    let hsh = s.sim.world_hash();
    let snap: Vec<u16> = (0..384).flat_map(|x| (240..254).map(move |y| (x, y))).map(|(x, y)| s.sim.cell(CellPos::new(x, y)).material.0).collect();
    for _ in 0..4 { s.sim.tick(); println!("awake {}", s.sim.stats().awake_chunks); }
    let snap2: Vec<u16> = (0..384).flat_map(|x| (240..254).map(move |y| (x, y))).map(|(x, y)| s.sim.cell(CellPos::new(x, y)).material.0).collect();
    let diffs: Vec<(i32, i32)> = (0..384).flat_map(|x| (240..254).map(move |y| (x, y))).zip(snap.iter().zip(&snap2)).filter(|(_, (a, b))| a != b).map(|(p, _)| p).collect();
    println!("woken: changed {} cells {:?}", hsh != s.sim.world_hash(), diffs);
    for y in 242..254 {
        let mut line = String::new();
        for x in 0..384 {
            let p = CellPos::new(x, y);
            let c = s.sim.world.chunk(p.chunk()).unwrap();
            let i = p.local_index();
            line.push(if c.mat[i] == 0 { '.' } else if c.mat[i] != s.mat("water").0 { '#' } else if c.motion[i] & 0x1f != 0 { 'F' } else if c.motion[i] & 0xc0 != 0 { 'e' } else { 'w' });
        }
        println!("{y}: {line}");
    }
}

#[test]
#[ignore]
fn debug_pool() {
    let mut s = waterfall_scene();
    let water = s.mat("water");
    for t in 1..=3400u64 {
        if let Some((area, m, until)) = s.pour && t <= until {
            for x in area.x0..area.x1 { let p = CellPos::new(x, area.y0); if s.sim.cell(p).material.is_air() { s.sim.set_cell(p, m, None); } }
        }
        s.sim.tick();
        if t == 1200 {
            // Row ends (top cells with air on liquid next to them) and whether a place to fall exists.
            let mat = |x: i32, y: i32| s.sim.cell(CellPos::new(x, y)).material;
            for y in 350..382 {
                for x in 3..509 {
                    if mat(x, y) != water || !mat(x, y - 1).is_air() { continue; }
                    for dir in [1, -1] {
                        if !(mat(x + dir, y).is_air() && mat(x + dir, y + 1) == water) { continue; }
                        let mut d = 1; let mut found = None;
                        while d < 600 { let tx = x + dir * d; if !mat(tx, y).is_air() { break; } if mat(tx, y + 1).is_air() { found = Some(d); break; } if mat(tx, y + 1) != water { break; } d += 1; }
                        let p = CellPos::new(x, y);
                        let c = s.sim.world.chunk(p.chunk()).unwrap();
                        let (lx, ly) = (x & 63, y & 63);
                        let awake = lx >= c.dirty.x0 && lx < c.dirty.x1 && ly >= c.dirty.y0 && ly < c.dirty.y1;
                        println!("row end ({x},{y}) dir {dir} motion {:02x} drop {:?} awake {awake}", c.motion[p.local_index()], found);
                    }
                }
            }
        }
        if t % 200 == 0 {
            let prof: Vec<String> = s.surface(2, 510, 100, &[water]).iter().step_by(16).map(|v| format!("{}", 382 - v.unwrap_or(382))).collect();
            println!("tick {t} awake {} p {} heights {}", s.sim.stats().awake_chunks, s.sim.particles().len(), prof.join(" "));
        }
    }
}
