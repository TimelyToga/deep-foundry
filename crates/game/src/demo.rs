//! The demo world, made by a chunk source: stone ground with gentle hills, a dirt layer, a sand
//! dune, a water pool, a small lava pocket, gravel patches and a few wooden posts.
//!
//! The world has no limit to the left and right. The hills go on without end. The pool, dune,
//! lava pocket, posts and gravel repeat in sections of `SECTION` cells. Each section except the
//! start section moves its features a little, from the seed. Section 0 (x = 0 to 2048) is the
//! start area. A ball of water falls into each pool, and the steep side of each dune slides, when
//! the section is first made.

use foundry_content::Content;
use foundry_core::{CHUNK_SIZE, CellRect, ChunkPos, Rng, local_index};
use foundry_sim::{ChunkCells, ChunkSource, SimConfig, Simulation};
use std::f64::consts::PI;
use std::sync::Arc;

/// Width of one section of the demo world in cells (32 chunks).
pub const SECTION: i32 = 2048;

/// The demo world and a good place for the camera.
pub struct Demo {
    pub sim: Simulation,
    /// World cell to show at the screen center at the start.
    pub start_center: (f64, f64),
}

/// The shape of the world to make.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    /// No limit to the left and right. `depth_chunks` below the surface level.
    Infinite { depth_chunks: i32 },
    /// A finite box of this many chunks, with bedrock walls.
    Box { width_chunks: i32, height_chunks: i32 },
}

#[derive(Debug, Clone, Copy)]
struct Materials {
    stone: u16,
    dirt: u16,
    sand: u16,
    gravel: u16,
    water: u16,
    lava: u16,
    wood: u16,
    bedrock: u16,
}

/// An ellipse: center and radii.
#[derive(Debug, Clone, Copy)]
struct Ellipse {
    cx: i32,
    cy: i32,
    rx: i32,
    ry: i32,
}

impl Ellipse {
    fn contains(&self, x: i32, y: i32) -> bool {
        let (dx, dy) = ((x - self.cx) as f64 / self.rx.max(1) as f64, (y - self.cy) as f64 / self.ry.max(1) as f64);
        dx * dx + dy * dy <= 1.0
    }

    fn bounds(&self) -> CellRect {
        CellRect::new(self.cx - self.rx, self.cy - self.ry, self.cx + self.rx + 1, self.cy + self.ry + 1)
    }
}

/// The features of one section. All positions are world cells.
struct Section {
    /// Pool columns: pool.0..pool.1.
    pool: (i32, i32),
    /// Water level of the pool (the y of the top water row).
    water_top: i32,
    ball: Ellipse,
    /// Dune columns: dune.0..dune.1.
    dune: (i32, i32),
    /// The first column of each post (3 columns wide).
    posts: [i32; 3],
    lava: Ellipse,
    gravel: [Ellipse; 6],
}

/// Sizes of the features in cells.
const POOL_DEPTH: f64 = 70.0;
const DUNE_HEIGHT: f64 = 110.0;
/// The dune rises to its top at this fraction of its width, then falls steeply.
const DUNE_PEAK: f64 = 0.86;
const POST_HEIGHT: i32 = 36;
/// Depth of the lava pocket center below the surface level.
const LAVA_DEPTH: i32 = 174;

/// The demo world as a chunk source.
pub struct DemoSource {
    m: Materials,
    /// The base level of the ground. The hills go about 46 cells above and below it.
    surface_y: i32,
    /// The row below the last row of the world. The two rows above it are bedrock.
    bottom_y: i32,
}

impl DemoSource {
    pub fn new(content: &Content, surface_y: i32, bottom_y: i32) -> Self {
        let id = |name: &str| content.expect_material(name).0;
        let m = Materials {
            stone: id("stone"),
            dirt: id("dirt"),
            sand: id("sand"),
            gravel: id("gravel"),
            water: id("water"),
            lava: id("lava"),
            wood: id("wood"),
            bedrock: id("bedrock"),
        };
        Self { m, surface_y, bottom_y }
    }

    /// Random phases of the hill waves, from the seed.
    fn phases(seed: u64) -> [f64; 3] {
        let mut rng = Rng::new(seed ^ 0xde70);
        [0; 3].map(|_| rng.unit() as f64 * 2.0 * PI)
    }

    /// Top of the ground in column `x`, without the pool basin.
    fn surface(&self, x: i32, phases: &[f64; 3]) -> i32 {
        let (x, w) = (x as f64, SECTION as f64);
        let hills = 30.0 * (x * 2.0 * PI / (w * 0.45) + phases[0]).sin()
            + 12.0 * (x * 2.0 * PI / (w * 0.13) + phases[1]).sin()
            + 4.0 * (x / 23.0 + phases[2]).sin();
        (self.surface_y as f64 + hills).round() as i32
    }

    /// Top of the ground in column `x`, with the pool basin cut out.
    fn ground(&self, x: i32, surface: i32, sec: &Section) -> i32 {
        if x < sec.pool.0 || x >= sec.pool.1 {
            return surface;
        }
        let u = (x - sec.pool.0) as f64 / (sec.pool.1 - sec.pool.0) as f64;
        surface + (POOL_DEPTH * (PI * u).sin().powf(0.6)).round() as i32
    }

    fn section(&self, s: i32, seed: u64, phases: &[f64; 3]) -> Section {
        let mut rng = Rng::for_chunk(seed, 0, ChunkPos::new(s, 0), 0x5ec7_1011);
        let base = s * SECTION;
        // Section 0 is the start area: no shift.
        let shift = if s == 0 { 0 } else { rng.below(200) as i32 - 100 };
        let at = |fraction: f64| base + shift + (SECTION as f64 * fraction) as i32;
        let pool = (at(0.29), at(0.41));
        // Fill up to 3 cells below the lower rim, so no water spills out.
        let water_top = self.surface(pool.0, phases).max(self.surface(pool.1 - 1, phases)) + 3;
        let pool_mid = (pool.0 + pool.1) / 2;
        let ball = Ellipse { cx: pool_mid, cy: water_top - 110, rx: 20, ry: 20 };
        let dune = (at(0.52), at(0.72));
        let posts = [0, 1, 2].map(|i| pool.1 + 14 + i * 22);
        let lava = Ellipse { cx: at(0.475), cy: self.surface_y + LAVA_DEPTH, rx: 56, ry: 22 };
        let gravel = [0; 6].map(|_| {
            let cx = base + 40 + rng.below((SECTION - 80) as u32) as i32;
            let cy = self.surface(cx, phases) + 60 + rng.below(256) as i32;
            Ellipse { cx, cy, rx: 12 + rng.below(20) as i32, ry: 6 + rng.below(8) as i32 }
        });
        Section { pool, water_top, ball, dune, posts, lava, gravel }
    }
}

impl ChunkSource for DemoSource {
    fn generate(&self, cells: &mut ChunkCells) {
        let m = self.m;
        let phases = Self::phases(cells.seed);
        let (x0, y0) = (cells.left(), cells.top());
        let chunk = cells.pos.cell_rect();
        // SECTION is a multiple of the chunk size, so a chunk is inside one section.
        let sec = self.section(x0.div_euclid(SECTION), cells.seed, &phases);
        let touches = |r: CellRect| !r.intersect(&chunk).is_empty();
        let ball = touches(sec.ball.bounds());
        let lava = touches(sec.lava.bounds());
        let gravel: Vec<&Ellipse> = sec.gravel.iter().filter(|g| touches(g.bounds())).collect();

        for lx in 0..CHUNK_SIZE {
            let x = x0 + lx;
            let surface = self.surface(x, &phases);
            let ground = self.ground(x, surface, &sec);
            let in_pool = x >= sec.pool.0 - 6 && x < sec.pool.1 + 6;
            let dirt_depth = if in_pool { 0 } else { (16.0 + 5.0 * (x as f64 / 41.0).sin()) as i32 };
            let dune_top = if x >= sec.dune.0 && x < sec.dune.1 {
                let u = (x - sec.dune.0) as f64 / (sec.dune.1 - sec.dune.0) as f64;
                let rise =
                    if u < DUNE_PEAK { (0.5 * PI * u / DUNE_PEAK).sin().powi(2) } else { 1.0 - (u - DUNE_PEAK) / (1.0 - DUNE_PEAK) };
                ground - (DUNE_HEIGHT * rise).round() as i32
            } else {
                ground
            };
            let post = sec.posts.iter().any(|&p| x >= p && x < p + 3);
            for ly in 0..CHUNK_SIZE {
                let y = y0 + ly;
                let mut mat = if y >= self.bottom_y - 2 {
                    m.bedrock
                } else if y < ground {
                    0
                } else if y < ground + dirt_depth {
                    m.dirt
                } else {
                    m.stone
                };
                if mat == m.stone && gravel.iter().any(|g| g.contains(x, y)) {
                    mat = m.gravel;
                }
                if x >= sec.pool.0 && x < sec.pool.1 && y >= sec.water_top && y < ground {
                    mat = m.water;
                }
                if ball && sec.ball.contains(x, y) {
                    mat = m.water;
                }
                if post && y >= ground - POST_HEIGHT && y < ground + 4 {
                    mat = m.wood;
                }
                if lava && sec.lava.contains(x, y) {
                    mat = if y > sec.lava.cy - sec.lava.ry / 5 { m.lava } else { 0 };
                }
                if y >= dune_top && y < ground {
                    mat = m.sand;
                }
                if y >= self.bottom_y - 2 {
                    mat = m.bedrock;
                }
                cells.mat[local_index(lx, ly)] = mat;
            }
        }

        // The water ball falls, and the steep side of the dune slides, as soon as they are made.
        let steep = CellRect::new(
            sec.dune.0 + ((sec.dune.1 - sec.dune.0) as f64 * DUNE_PEAK) as i32 - 8,
            self.surface_y - 200,
            sec.dune.1 + 8,
            self.surface_y + 60,
        );
        cells.awake = ball || touches(steep);
    }

    fn name(&self) -> &str {
        "demo"
    }

    fn settings(&self) -> String {
        format!("surface={} bottom={} section={SECTION}", self.surface_y, self.bottom_y)
    }
}

/// Make the demo world.
pub fn build(content: Arc<Content>, shape: Shape, seed: u64) -> Demo {
    let (mut config, surface_y) = match shape {
        Shape::Infinite { depth_chunks } => {
            let config = SimConfig { depth_chunks, ..SimConfig::infinite(seed, None) };
            let surface_y = config.surface_y();
            (config, surface_y)
        }
        // As in the first demo: the ground at about 55% of the height.
        Shape::Box { width_chunks, height_chunks } => {
            (SimConfig::finite(width_chunks, height_chunks, seed), (height_chunks * CHUNK_SIZE) * 55 / 100)
        }
    };
    let source = Arc::new(DemoSource::new(&content, surface_y, config.height_chunks() * CHUNK_SIZE));
    config.source = Some(source.clone() as Arc<dyn ChunkSource>);
    let sim = Simulation::new(content, config);
    // Start between the pool and the dune of section 0, a little above the ground, so the lava
    // pocket also shows.
    let cx = SECTION as f64 * 0.48;
    let cy = source.surface(cx as i32, &DemoSource::phases(seed)) as f64 + 30.0;
    Demo { sim, start_center: (cx, cy) }
}

/// The part of the world that section `s` covers, from the top of the world to `depth` cells.
#[cfg(test)]
fn section_rect(s: i32, depth: i32) -> CellRect {
    CellRect::new(s * SECTION, 0, (s + 1) * SECTION, depth)
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundry_core::{CellPos, Command, MaterialId};

    fn content() -> Arc<Content> {
        Arc::new(Content::load_default().unwrap())
    }

    #[test]
    fn demo_has_all_parts_in_every_section() {
        let content = content();
        let demo = build(content.clone(), Shape::Infinite { depth_chunks: 16 }, 5);
        for s in [0, 1, -3, 700] {
            let r = section_rect(s, demo.sim.size_cells().1);
            for id in ["stone", "dirt", "sand", "water", "lava", "wood", "bedrock", "gravel"] {
                let n = demo.sim.count_material(r, content.expect_material(id));
                assert!(n > 0, "no {id} in section {s}");
            }
            // The top row is sky.
            assert_eq!(demo.sim.count_material(CellRect::new(r.x0, 0, r.x1, 1), MaterialId::AIR), SECTION as usize);
        }
        // The start is in section 0, near the ground.
        let (cx, cy) = demo.start_center;
        assert!(cx > 0.0 && cx < SECTION as f64);
        let below = CellPos::new(cx as i32, cy as i32 + 60);
        assert!(!demo.sim.cell(below).material.is_air(), "ground below the start");
    }

    #[test]
    fn same_seed_same_demo() {
        let content = content();
        let hash = |seed: u64| {
            let mut d = build(content.clone(), Shape::Infinite { depth_chunks: 16 }, seed);
            let (cx, cy) = d.start_center;
            d.sim.apply(Command::SetView { area: CellRect::around(CellPos::new(cx as i32, cy as i32), 600) });
            for _ in 0..30 {
                d.sim.tick();
            }
            d.sim.world_hash()
        };
        assert_eq!(hash(9), hash(9));
        assert_ne!(hash(9), hash(10));
    }

    #[test]
    fn a_box_world_has_walls_and_the_start_area() {
        let content = content();
        let demo = build(content.clone(), Shape::Box { width_chunks: 32, height_chunks: 16 }, 5);
        assert_eq!(demo.sim.size_cells(), (2048, 1024));
        for id in ["stone", "dirt", "sand", "water", "lava", "wood"] {
            assert!(demo.sim.count_material(CellRect::new(0, 0, 2048, 1024), content.expect_material(id)) > 0, "{id}");
        }
        let bedrock = content.expect_material("bedrock");
        assert_eq!(demo.sim.cell(CellPos::new(0, 100)).material, bedrock);
        assert_eq!(demo.sim.cell(CellPos::new(2047, 100)).material, bedrock);
    }

    /// Chunks are made quickly enough for a camera that moves fast.
    #[test]
    fn making_a_chunk_is_fast() {
        let content = content();
        let source = DemoSource::new(&content, 1024, 9216);
        let mut mat = [0u16; foundry_core::CHUNK_AREA];
        let mut temp = [0i16; foundry_core::CHUNK_AREA];
        let start = std::time::Instant::now();
        let mut n = 0;
        for cx in 0..64 {
            for cy in 12..24 {
                let mut cells = ChunkCells { pos: ChunkPos::new(cx, cy), seed: 1, mat: &mut mat, temp: &mut temp, awake: false };
                source.generate(&mut cells);
                n += 1;
            }
        }
        let per_chunk = start.elapsed().as_secs_f64() / n as f64 * 1e6;
        println!("demo source: {per_chunk:.1} microseconds per chunk");
        assert!(per_chunk < 2000.0, "{per_chunk} microseconds per chunk");
    }
}
