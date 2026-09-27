//! The starting world: stone ground with gentle hills, a dirt layer, a sand dune, a water pool,
//! a small lava pocket and a few wooden posts. It is made with `Simulation::set_cell` before the
//! simulation thread starts.
//!
//! All sizes are fractions of the world size, so any `--world` size works.

use foundry_content::Content;
use foundry_core::{CellPos, MaterialId, Rng};
use foundry_sim::{SimConfig, Simulation};
use std::f64::consts::PI;
use std::sync::Arc;

/// The demo world and a good place for the camera.
pub struct Demo {
    pub sim: Simulation,
    /// World cell to show at the screen center at the start.
    pub start_center: (f64, f64),
}

struct Materials {
    stone: MaterialId,
    dirt: MaterialId,
    sand: MaterialId,
    gravel: MaterialId,
    water: MaterialId,
    lava: MaterialId,
    wood: MaterialId,
}

/// The shape of the ground. y goes down, so a larger y is lower.
struct Terrain {
    /// Top of the ground in each column, with the pool basin cut out.
    ground: Vec<i32>,
    /// Top of the ground in each column, without the basin.
    surface: Vec<i32>,
    /// Pool columns: x0..x1.
    pool: (i32, i32),
    /// Water level of the pool (the y of the top water row).
    water_top: i32,
}

impl Terrain {
    fn new(width: i32, height: i32, rng: &mut Rng) -> Self {
        let (w, h) = (width as f64, height as f64);
        let scale = (h / 1024.0).clamp(0.25, 4.0);
        let phases = [rng.unit() as f64 * 2.0 * PI, rng.unit() as f64 * 2.0 * PI, rng.unit() as f64 * 2.0 * PI];
        let surface: Vec<i32> = (0..width)
            .map(|x| {
                let x = x as f64;
                let hills = 30.0 * (x * 2.0 * PI / (w * 0.45) + phases[0]).sin()
                    + 12.0 * (x * 2.0 * PI / (w * 0.13) + phases[1]).sin()
                    + 4.0 * (x / 23.0 + phases[2]).sin();
                (h * 0.55 + hills * scale).round() as i32
            })
            .collect();

        // The pool: a smooth bowl cut into the ground, lined with stone.
        let pool = ((w * 0.29) as i32, (w * 0.41) as i32);
        let depth = 70.0 * scale;
        let mut ground = surface.clone();
        for x in pool.0..pool.1 {
            let u = (x - pool.0) as f64 / (pool.1 - pool.0) as f64;
            ground[x as usize] += (depth * (PI * u).sin().powf(0.6)).round() as i32;
        }
        // Fill up to 3 cells below the lower rim, so no water spills out.
        let water_top = surface[pool.0 as usize].max(surface[(pool.1 - 1) as usize]) + 3;
        Self { ground, surface, pool, water_top }
    }

    fn in_pool(&self, x: i32) -> bool {
        x >= self.pool.0 - 6 && x < self.pool.1 + 6
    }
}

pub fn build(content: Arc<Content>, world_chunks: (i32, i32), seed: u64) -> Demo {
    let m = Materials {
        stone: content.expect_material("stone"),
        dirt: content.expect_material("dirt"),
        sand: content.expect_material("sand"),
        gravel: content.expect_material("gravel"),
        water: content.expect_material("water"),
        lava: content.expect_material("lava"),
        wood: content.expect_material("wood"),
    };
    let config = SimConfig { width_chunks: world_chunks.0, height_chunks: world_chunks.1, seed, bedrock_border: true };
    let mut sim = Simulation::new(content, config);
    let (width, height) = sim.size_cells();
    let mut rng = Rng::new(seed ^ 0xde70);
    let t = Terrain::new(width, height, &mut rng);
    let scale = (height as f64 / 1024.0).clamp(0.25, 4.0);

    // Ground: dirt on top (not around the pool), stone below. Leave the 2-cell bedrock border alone.
    let inner = |x: i32, y: i32| x >= 2 && x < width - 2 && y >= 0 && y < height - 2;
    for x in 2..width - 2 {
        let top = t.ground[x as usize];
        let dirt_depth = if t.in_pool(x) { 0 } else { (16.0 + 5.0 * (x as f64 / 41.0).sin()) as i32 };
        for y in top.max(0)..height - 2 {
            let mat = if y < top + dirt_depth { m.dirt } else { m.stone };
            sim.set_cell(CellPos::new(x, y), mat, None);
        }
    }

    // Gravel patches in the stone.
    for _ in 0..(width / 300).max(2) {
        let cx = 40 + rng.below((width - 80).max(1) as u32) as i32;
        let cy = t.surface[cx.clamp(0, width - 1) as usize] + 60 + rng.below((height / 4).max(1) as u32) as i32;
        let (rx, ry) = (12 + rng.below(20) as i32, 6 + rng.below(8) as i32);
        fill_ellipse(cx, cy, rx, ry, |x, y| {
            if inner(x, y) && sim.cell(CellPos::new(x, y)).material == m.stone {
                sim.set_cell(CellPos::new(x, y), m.gravel, None);
            }
        });
    }

    // Water in the pool.
    for x in t.pool.0..t.pool.1 {
        for y in t.water_top..t.ground[x as usize] {
            if inner(x, y) {
                sim.set_cell(CellPos::new(x, y), m.water, None);
            }
        }
    }

    // A ball of water above the pool. It falls in at the start.
    let pool_mid = (t.pool.0 + t.pool.1) / 2;
    let ball_r = (20.0 * scale) as i32;
    fill_ellipse(pool_mid, t.water_top - (110.0 * scale) as i32, ball_r, ball_r, |x, y| {
        if inner(x, y) {
            sim.set_cell(CellPos::new(x, y), m.water, None);
        }
    });

    // Wooden posts on the right bank of the pool, set a few cells into the ground.
    let post_h = (36.0 * scale) as i32;
    for i in 0..3 {
        let px = t.pool.1 + 14 + i * 22;
        for x in px..px + 3 {
            if x >= width - 2 {
                continue;
            }
            let top = t.ground[x as usize];
            for y in top - post_h..top + 4 {
                if inner(x, y) {
                    sim.set_cell(CellPos::new(x, y), m.wood, None);
                }
            }
        }
    }

    // A small lava pocket in a cave deep in the stone.
    let (lx, ly) = ((width as f64 * 0.475) as i32, (height as f64 * 0.72) as i32);
    let (lrx, lry) = ((56.0 * scale) as i32, (22.0 * scale) as i32);
    fill_ellipse(lx, ly, lrx, lry, |x, y| {
        if inner(x, y) {
            let mat = if y > ly - lry / 5 { m.lava } else { MaterialId::AIR };
            sim.set_cell(CellPos::new(x, y), mat, None);
        }
    });

    // A sand dune: a gentle slope up, then a steep side that slides down at the start.
    let (d0, d1) = ((width as f64 * 0.52) as i32, (width as f64 * 0.72) as i32);
    let dune_h = 110.0 * scale;
    let peak = 0.86;
    for x in d0..d1 {
        let u = (x - d0) as f64 / (d1 - d0) as f64;
        let rise = if u < peak { (0.5 * PI * u / peak).sin().powi(2) } else { 1.0 - (u - peak) / (1.0 - peak) };
        let top = t.ground[x as usize] - (dune_h * rise).round() as i32;
        for y in top..t.ground[x as usize] {
            if inner(x, y) {
                sim.set_cell(CellPos::new(x, y), m.sand, None);
            }
        }
    }

    // Start between the pool and the dune, a little above the ground, so the lava pocket also shows.
    let cx = width as f64 * 0.48;
    let cy = t.surface[cx as usize] as f64 + 30.0 * scale;
    Demo { sim, start_center: (cx, cy) }
}

/// Call `f` for each cell inside an ellipse.
fn fill_ellipse(cx: i32, cy: i32, rx: i32, ry: i32, mut f: impl FnMut(i32, i32)) {
    let (rx, ry) = (rx.max(1), ry.max(1));
    for y in cy - ry..=cy + ry {
        for x in cx - rx..=cx + rx {
            let (dx, dy) = ((x - cx) as f64 / rx as f64, (y - cy) as f64 / ry as f64);
            if dx * dx + dy * dy <= 1.0 {
                f(x, y);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundry_core::CellRect;

    #[test]
    fn demo_has_all_parts() {
        let content = Arc::new(Content::load_default().unwrap());
        let demo = build(content.clone(), (8, 4), 5);
        let (w, h) = demo.sim.size_cells();
        let all = CellRect::new(0, 0, w, h);
        for id in ["stone", "dirt", "sand", "water", "lava", "wood", "bedrock"] {
            let n = demo.sim.count_material(all, content.expect_material(id));
            assert!(n > 0, "no {id} in the demo world");
        }
        // The top row is sky.
        assert_eq!(demo.sim.count_material(CellRect::new(2, 0, w - 2, 1), MaterialId::AIR), (w - 4) as usize);
    }

    #[test]
    fn same_seed_same_demo() {
        let content = Arc::new(Content::load_default().unwrap());
        let a = build(content.clone(), (6, 4), 9).sim.world_hash();
        let b = build(content.clone(), (6, 4), 9).sim.world_hash();
        let c = build(content, (6, 4), 10).sim.world_hash();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
