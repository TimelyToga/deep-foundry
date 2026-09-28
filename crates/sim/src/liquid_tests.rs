//! Liquid tests: scenes with water, oil, lava and mud. Each test runs until everything rests
//! (no awake chunk, no particle), checks every tick that no liquid is lost, and checks the result.
//!
//! `cargo test -p foundry_sim --release liquid_frames -- --ignored --nocapture` runs every scene,
//! prints numbers, and writes image frames and one contact sheet per scene to `out/liquids/`
//! (particles are light dots). `SCENE=<name>` runs one scene.

use super::*;

/// A scene: a world, and the liquids whose totals must not change.
struct Scene {
    name: &'static str,
    sim: Simulation,
    /// Materials whose total (cells + particles) must not change.
    kept: Vec<MaterialId>,
    /// Frames for the contact sheet: (tick, picture).
    sheet: Vec<(u64, image::RgbImage)>,
}

/// What a scene run measured.
#[derive(Debug)]
struct Outcome {
    /// Tick when everything rested (no awake chunk and no particle). 0 = never.
    rest: u64,
    /// Most particles in the air at once.
    max_particles: usize,
    /// Particles that were in the air, summed over all ticks (a measure of the splash).
    particle_ticks: usize,
}

fn scene(name: &'static str, width_chunks: i32, height_chunks: i32, liquids: &[&str]) -> Scene {
    let mut content = Content::load_default().unwrap();
    let kept = liquids.iter().map(|n| content.expect_material(n)).collect::<Vec<_>>();
    // These tests check movement only. The kept liquids start at 20 °C like the rest of the world
    // and get no phase changes here, so heat has no work (hot lava on the cold floor would cool
    // for a long time and freeze into stone).
    for m in &kept {
        let (t, i) = (&mut content.materials, m.index());
        t.temperature[i] = foundry_core::DEFAULT_TEMPERATURE;
        (t.melt[i], t.freeze[i], t.boil[i], t.condense[i]) = (None, None, None, None);
    }
    let content = Arc::new(content);
    let sim = Simulation::new(content, SimConfig::finite(width_chunks, height_chunks, 4));
    Scene { name, sim, kept, sheet: vec![] }
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

    /// A stone tank with walls 3 cells thick: inside x in `x0..x1`, y in `y0..y1`, open at the
    /// top, filled with `liquid`, with a hole in the floor at x in `hole0..hole1`.
    #[allow(clippy::too_many_arguments)]
    fn tank(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, hole0: i32, hole1: i32, liquid: &str) {
        self.fill(x0 - 3, y0, x0, y1 + 3, "stone");
        self.fill(x1, y0, x1 + 3, y1 + 3, "stone");
        self.fill(x0, y1, hole0, y1 + 3, "stone");
        self.fill(hole1, y1, x1, y1 + 3, "stone");
        self.fill(x0, y0, x1, y1, liquid);
    }

    /// Total of a material: cells in the world plus material particles.
    fn total(&self, m: MaterialId) -> usize {
        let world = &self.sim.world;
        let cells: usize = world.loaded_chunks().filter_map(|p| world.chunk(p)).map(|c| c.mat.iter().filter(|&&v| v == m.0).count()).sum();
        cells + self.sim.particles().count_material(m)
    }

    /// Run until everything rests, or `max` ticks. Checks every tick that no kept material is
    /// lost. With `frames`, saves pictures at the given ticks and at the end.
    fn run(&mut self, max: u64, frames: Option<(&[u64], CellRect, u32)>) -> Outcome {
        let start: Vec<usize> = self.kept.iter().map(|&m| self.total(m)).collect();
        let mut out = Outcome { rest: 0, max_particles: 0, particle_ticks: 0 };
        for t in 1..=max {
            self.sim.tick();
            let n = self.sim.particles().len();
            out.max_particles = out.max_particles.max(n);
            out.particle_ticks += n;
            for (k, &m) in self.kept.iter().enumerate() {
                assert_eq!(self.total(m), start[k], "{}: material {} is kept at tick {t}", self.name, m.0);
            }
            if let Some((ticks, area, scale)) = frames
                && ticks.contains(&t)
            {
                self.frame(t, area, scale);
            }
            if self.sim.stats().awake_chunks == 0 && self.sim.particles().is_empty() {
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

    /// Write all frames into one picture, `cols` frames per row. Under each frame, the tick is
    /// shown as bars: a tall bar per 100 ticks, a short bar per 10 ticks.
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

    /// The y of the first cell (from `y0` down) in column `x` that is one of `liquids`.
    fn surface_at(&self, x: i32, y0: i32, liquids: &[MaterialId]) -> Option<i32> {
        let h = self.sim.size_cells().1;
        (y0..h).find(|&y| liquids.contains(&self.sim.cell(CellPos::new(x, y)).material))
    }

    /// Highest and lowest surface row over the columns `x0..x1` that have liquid.
    fn surface_range(&self, x0: i32, x1: i32, y0: i32, liquids: &[MaterialId]) -> (i32, i32) {
        let s: Vec<i32> = (x0..x1).filter_map(|x| self.surface_at(x, y0, liquids)).collect();
        (*s.iter().min().unwrap_or(&0), *s.iter().max().unwrap_or(&0))
    }

    /// The rightmost column in `x0..x1` with the material in row `y`.
    fn front(&self, y: i32, x0: i32, x1: i32, m: MaterialId) -> i32 {
        (x0..x1).rev().find(|&x| self.sim.cell(CellPos::new(x, y)).material == m).unwrap_or(x0)
    }

    fn count(&self, r: CellRect, m: MaterialId) -> usize {
        self.sim.count_material(r, m)
    }
}

// ---- Scenes ----

/// A ball of water (radius 30) falls about 170 cells into a 384 x 256 box. Its center is on a
/// chunk border (x = 192) when `cx` is 192.
fn ball_scene(cx: i32) -> Scene {
    let mut s = scene("ball", 6, 4, &["water"]);
    s.ball(cx, 50, 30, "water");
    s
}

/// A block of water 238 wide and 170 tall stands at the left of a 1024 x 384 basin, as if a
/// dam was just removed.
fn dam_scene() -> Scene {
    let mut s = scene("dam", 16, 6, &["water"]);
    s.fill(2, 212, 240, 382, "water");
    s
}

/// A tank (120 x 90 water) empties through a 5-cell hole; the stream falls about 250 cells
/// into a basin 508 wide (512 x 384 world).
fn waterfall_scene() -> Scene {
    let mut s = scene("waterfall", 8, 6, &["water"]);
    s.tank(40, 30, 160, 120, 140, 145, "water");
    s
}

/// A U-shaped stone bowl (inner radius 84) is filled from a tank above it.
fn bowl_scene() -> Scene {
    let mut s = scene("bowl", 6, 4, &["water"]);
    let stone = s.mat("stone");
    for y in 150..254 {
        for x in 90..295 {
            let (dx, dy) = ((x - 192) as f32, (y - 150) as f32);
            let r = (dx * dx + dy * dy).sqrt();
            if (84.0..90.0).contains(&r) {
                s.sim.set_cell(CellPos::new(x, y), stone, None);
            }
        }
    }
    s.tank(160, 10, 224, 60, 190, 194, "water");
    s
}

/// A narrow slot (3 cells wide, 100 tall) is filled from a tank above it.
fn channel_scene() -> Scene {
    let mut s = scene("channel", 4, 4, &["water"]);
    s.fill(120, 140, 125, 254, "stone");
    s.fill(128, 140, 133, 254, "stone");
    s.tank(100, 10, 150, 40, 125, 128, "water");
    s
}

/// Five steps (40 wide, 16 high) go down to the right into a basin. A tank pours onto the top step.
fn stairs_scene() -> Scene {
    let mut s = scene("stairs", 8, 5, &["water"]);
    for k in 0..5 {
        let (x0, top) = (2 + 40 * k, 150 + 16 * k);
        s.fill(x0, top, x0 + 40, 318, "stone");
    }
    s.tank(10, 40, 50, 120, 25, 30, "water");
    s
}

/// A pool of water in a box; a tank of oil empties onto it.
fn oil_scene() -> Scene {
    let mut s = scene("oil", 6, 4, &["water", "oil"]);
    s.fill(60, 150, 64, 254, "stone");
    s.fill(320, 150, 324, 254, "stone");
    s.fill(64, 214, 320, 254, "water");
    s.tank(160, 20, 224, 60, 190, 195, "oil");
    s
}

/// A block of a liquid (60 x 40) on a flat floor.
fn block_scene(name: &'static str, liquid: &'static str) -> Scene {
    let mut s = scene(name, 6, 4, &[liquid]);
    s.fill(162, 214, 222, 254, liquid);
    s
}

#[test]
fn dropped_ball_splashes_spreads_and_rests_flat() {
    for cx in [192, 200] {
        let mut s = ball_scene(cx);
        let water = s.mat("water");
        let out = s.run(3000, None);
        let (lo, hi) = s.surface_range(2, 382, 0, &[water]);
        println!("ball at x {cx}: {out:?}, surface rows {lo}..{hi}");
        assert!(out.max_particles > 50, "the landing splashes: {} particles", out.max_particles);
        assert!(out.rest > 0 && out.rest < 600, "water comes to rest fast: tick {}", out.rest);
        assert!(hi - lo <= 1, "the surface is flat: rows {lo} to {hi}");
    }
}

#[test]
fn dam_break_floods_the_basin_and_rests_flat() {
    let mut s = dam_scene();
    let water = s.mat("water");
    for _ in 0..60 {
        s.sim.tick();
    }
    let front = s.front(381, 2, 1022, water);
    let out = s.run(5000, None);
    let (lo, hi) = s.surface_range(2, 1022, 0, &[water]);
    println!("dam: front at tick 60: x {front}; {out:?}; surface rows {lo}..{hi}");
    assert!(front > 500, "the flood runs out fast: front at x {front} after 60 ticks");
    assert!(out.rest > 0, "the flood comes to rest");
    assert!(hi - lo <= 1, "the surface is flat: rows {lo} to {hi}");
}

#[test]
fn waterfall_splashes_and_the_pool_rises_flat() {
    let mut s = waterfall_scene();
    let water = s.mat("water");
    let out = s.run(5000, None);
    let (lo, hi) = s.surface_range(2, 510, 125, &[water]);
    let left = s.count(CellRect::new(40, 30, 160, 120), water);
    println!("waterfall: {out:?}; pool surface rows {lo}..{hi}; left in the tank {left}");
    assert!(out.max_particles > 10, "the stream splashes: {} particles", out.max_particles);
    assert!(out.rest > 0, "the pool comes to rest");
    assert!(left == 0, "the tank is empty: {left} cells left");
    assert!(hi - lo <= 1, "the pool is flat: rows {lo} to {hi}");
}

#[test]
fn a_bowl_fills_from_the_bottom_and_rests_flat() {
    let mut s = bowl_scene();
    let water = s.mat("water");
    let out = s.run(4000, None);
    let (lo, hi) = s.surface_range(110, 275, 70, &[water]);
    let outside = s.count(CellRect::new(0, 70, 384, 256), water) - s.count(CellRect::new(106, 150, 279, 240), water);
    println!("bowl: {out:?}; surface rows {lo}..{hi}; water outside the bowl {outside}");
    assert!(out.rest > 0, "the water comes to rest");
    assert!(hi - lo <= 1, "the surface is flat: rows {lo} to {hi}");
    assert!(outside < 10, "the water is in the bowl: {outside} cells outside");
}

#[test]
fn a_narrow_channel_fills_without_gaps() {
    let mut s = channel_scene();
    let water = s.mat("water");
    let out = s.run(4000, None);
    // The slot is 3 wide (x 125..128) and ends at the floor (y 254). All water is below the top
    // water cell, with no air between.
    let top = (125..128).filter_map(|x| s.surface_at(x, 41, &[water])).min().unwrap();
    let gaps = s.count(CellRect::new(125, top + 1, 128, 254), MaterialId::AIR);
    println!("channel: {out:?}; water top in the slot at y {top}, air gaps below it {gaps}");
    assert!(out.rest > 0, "the water comes to rest");
    assert_eq!(gaps, 0, "no air gaps in the channel");
}

#[test]
fn water_runs_down_stairs_into_the_basin() {
    let mut s = stairs_scene();
    let water = s.mat("water");
    let total = s.count(CellRect::new(0, 0, 512, 320), water);
    let out = s.run(5000, None);
    let on_steps = s.count(CellRect::new(0, 0, 202, 320), water);
    let (lo, hi) = s.surface_range(202, 510, 150, &[water]);
    println!("stairs: {out:?}; on the steps {on_steps} of {total}; basin surface rows {lo}..{hi}");
    assert!(out.rest > 0, "the water comes to rest");
    assert!(on_steps * 10 < total, "most water runs down: {on_steps} of {total} stay on the steps");
    assert!(hi - lo <= 1, "the basin is flat: rows {lo} to {hi}");
}

#[test]
fn oil_poured_on_water_floats_flat() {
    let mut s = oil_scene();
    let (water, oil) = (s.mat("water"), s.mat("oil"));
    let out = s.run(6000, None);
    // In every column: no water above oil.
    let mut mixed = 0;
    for x in 64..320 {
        let col: Vec<MaterialId> = (100..254).map(|y| s.sim.cell(CellPos::new(x, y)).material).collect();
        if let Some(last_oil) = col.iter().rposition(|&m| m == oil) {
            mixed += col[..last_oil].iter().filter(|&&m| m == water).count();
        }
    }
    let (lo, hi) = s.surface_range(64, 320, 100, &[water, oil]);
    let (ilo, ihi) = s.surface_range(64, 320, 100, &[water]);
    println!("oil: {out:?}; water cells above oil {mixed}; top rows {lo}..{hi}; water top rows {ilo}..{ihi}");
    assert!(out.rest > 0, "the liquids come to rest");
    assert!(mixed <= 5, "oil is on top: {mixed} water cells above oil");
    assert!(hi - lo <= 1, "the top is flat: rows {lo} to {hi}");
    assert!(ihi - ilo <= 2, "the border between oil and water is flat: rows {ilo} to {ihi}");
}

/// Width of the liquid on the bottom row after 60 ticks, the finished run, and the surface rows.
fn block_run(liquid: &'static str) -> (i32, Outcome, (i32, i32)) {
    let mut s = block_scene("block", liquid);
    let m = s.mat(liquid);
    for _ in 0..60 {
        s.sim.tick();
    }
    let width = (2..382).filter(|&x| s.sim.cell(CellPos::new(x, 253)).material == m).count() as i32;
    let out = s.run(8000, None);
    let range = s.surface_range(2, 382, 100, &[m]);
    (width, out, range)
}

#[test]
fn lava_and_mud_flow_slowly_and_rest_with_a_slope() {
    let (water_width, _, _) = block_run("water");
    for liquid in ["lava", "mud"] {
        let (width, out, (lo, hi)) = block_run(liquid);
        println!("{liquid}: width after 60 ticks {width} (water {water_width}); {out:?}; surface rows {lo}..{hi}");
        assert!(width < water_width, "{liquid} spreads slower than water");
        assert!(out.rest > 0, "{liquid} comes to rest");
        assert!(hi - lo >= 2, "{liquid} rests with a slope, not flat: rows {lo} to {hi}");
    }
}

/// A block in the middle of a symmetric box: the water in the left and right halves stays about
/// the same (no left or right bias).
#[test]
fn liquids_have_no_left_right_bias() {
    let mut s = scene("symmetry", 6, 4, &["water"]);
    s.fill(160, 150, 224, 254, "water");
    let water = s.mat("water");
    let mut worst = 0.0f64;
    for t in 1..=400 {
        s.sim.tick();
        if t % 20 == 0 {
            let l = s.count(CellRect::new(0, 0, 192, 256), water) as f64;
            let r = s.count(CellRect::new(192, 0, 384, 256), water) as f64;
            worst = worst.max((l - r).abs() / (l + r));
        }
    }
    println!("symmetry: largest left/right difference {:.1}%", worst * 100.0);
    assert!(worst < 0.08, "left and right halves stay close: {:.1}%", worst * 100.0);
}

/// Writes frames of all scenes to `out/liquids/` and prints numbers.
#[test]
#[ignore]
fn liquid_frames() {
    let only = std::env::var("SCENE").ok();
    let want = |n: &str| only.as_deref().is_none_or(|o| o == n);
    type Make = fn() -> Scene;
    /// (name, make, ticks, area, scale, cols): see the loop below.
    type SceneRow = (&'static str, Make, &'static [u64], CellRect, u32, usize);
    let scenes: [SceneRow; 9] = [
        ("ball", || ball_scene(192), &[10, 20, 30, 36, 40, 44, 48, 52, 60, 70, 80, 100, 130, 160, 200], CellRect::new(0, 0, 384, 256), 3, 4),
        ("dam", dam_scene, &[10, 20, 30, 40, 50, 60, 80, 100, 130, 160, 200, 300, 400, 600, 900, 1200], CellRect::new(0, 0, 1024, 384), 1, 2),
        ("waterfall", waterfall_scene, &[40, 80, 120, 200, 300, 450, 600, 800, 1000], CellRect::new(0, 0, 512, 384), 2, 3),
        ("bowl", bowl_scene, &[40, 80, 120, 200, 300, 450, 600], CellRect::new(0, 0, 384, 256), 3, 4),
        ("channel", channel_scene, &[40, 80, 120, 200, 300, 450], CellRect::new(64, 0, 192, 256), 3, 4),
        ("stairs", stairs_scene, &[40, 80, 120, 200, 300, 450, 600, 900], CellRect::new(0, 0, 512, 320), 2, 3),
        ("oil", oil_scene, &[40, 80, 120, 200, 300, 450, 600, 900, 1200], CellRect::new(0, 0, 384, 256), 3, 4),
        ("lava", || block_scene("lava", "lava"), &[20, 60, 120, 200, 300, 450, 600, 900], CellRect::new(0, 128, 384, 256), 3, 4),
        ("mud", || block_scene("mud", "mud"), &[20, 60, 120, 200, 300, 450, 600, 900], CellRect::new(0, 128, 384, 256), 3, 4),
    ];
    for (name, make, ticks, area, scale, cols) in scenes {
        if !want(name) {
            continue;
        }
        let mut s = make();
        let kept = s.kept.clone();
        let started = std::time::Instant::now();
        let out = s.run(8000, Some((ticks, area, scale)));
        let ms = started.elapsed().as_secs_f64() * 1000.0 / out.rest.max(1) as f64;
        s.save_sheet(cols, 1);
        let range = s.surface_range(area.x0 + 2, area.x1 - 2, area.y0, &kept);
        println!("{name}: {out:?}, surface rows {range:?}, {ms:.3} ms per tick");
    }
}
