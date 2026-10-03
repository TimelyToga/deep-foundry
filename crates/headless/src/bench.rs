//! Benchmarks. Each benchmark world is made in code, runs a fixed number of ticks, and is timed.
//!
//! Only `Simulation::tick` is timed. The work that adds cells before a tick (for example sand rain) is not timed.

use anyhow::{Context, Result, anyhow};
use foundry_content::Content;
use foundry_core::{CHUNK_SIZE, CellPos, CellRect, ChunkPos, Command, MaterialId, PaintMode, Rng};
use foundry_sim::{SimConfig, Simulation};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

/// `--compare` fails if a benchmark's mean tick time is more than this many times its baseline.
pub const SLOWER_LIMIT: f64 = 1.10;

/// One benchmark.
pub struct Bench {
    pub name: &'static str,
    pub about: &'static str,
    /// Ticks to run if the command does not give `--ticks`.
    pub ticks: u32,
    make: fn(&Arc<Content>) -> BenchWorld,
}

/// An action that runs before each tick, with the tick number (0 is the first). It is not timed.
pub type BeforeTick = Box<dyn FnMut(&mut Simulation, u32)>;

/// A benchmark world: the simulation, and an action that runs before each tick.
pub struct BenchWorld {
    pub sim: Simulation,
    pub before_tick: BeforeTick,
}

/// All benchmarks, in the order the `bench` command runs them.
pub const BENCHES: &[Bench] = &[
    Bench {
        name: "sand_rain",
        about: "32 x 8 chunks. 256 sand cells are added near the top every tick over a wide area.",
        ticks: 600,
        make: sand_rain,
    },
    Bench {
        name: "ocean",
        about: "32 x 16 chunks. A block of about 1 million water cells with air on its right side (as if a wall was just removed).",
        ticks: 600,
        make: ocean,
    },
    Bench {
        name: "lava_water",
        about: "16 x 8 chunks. A lava pool and a water pool flow toward each other and meet.",
        ticks: 600,
        make: lava_water,
    },
    Bench {
        name: "pile_collapse",
        about: "16 x 16 chunks. A sand column 200 cells wide and 958 cells tall falls into a pile.",
        ticks: 600,
        make: pile_collapse,
    },
    Bench {
        name: "settled_world",
        about: "64 x 32 chunks. Stone ground with a flat sand layer and closed water caves. Nothing moves except one sand cell per tick.",
        ticks: 300,
        make: settled_world,
    },
    Bench {
        name: "mixed",
        about: "64 x 32 chunks. Settled ground plus sand rain, an ocean, lava and water, and a sand column. About 1,500 chunks with material.",
        ticks: 300,
        make: mixed,
    },
    Bench {
        name: "infinite_work",
        about: "Infinite world. The view shows 40 x 25 chunks of falling sand and water (about 1,000 awake chunks at the start).",
        ticks: 150,
        make: infinite_work,
    },
    Bench {
        name: "infinite_explored",
        about: "As infinite_work, plus 100,000 changed chunks far away (packed). The tick time must be the same as infinite_work.",
        ticks: 150,
        make: infinite_explored,
    },
    Bench {
        name: "infinite_walk",
        about: "Infinite world. A view of 28 x 16 chunks moves one chunk to the right each tick; every 32 ticks a hole is dug and sand and water fall in. New chunks are made each tick.",
        ticks: 600,
        make: infinite_walk,
    },
];

/// Find a benchmark by name.
pub fn find(name: &str) -> Option<&'static Bench> {
    BENCHES.iter().find(|b| b.name == name)
}

/// The numbers from one benchmark run. Times are in milliseconds per tick.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchResult {
    pub ticks: u32,
    pub mean_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
    /// Mean and largest `SimStats::awake_chunks` over all ticks.
    pub awake_mean: f64,
    pub awake_max: u32,
    /// Chunks with at least one cell that is not air, after the last tick.
    pub chunks_with_material: u32,
}

impl Bench {
    /// Make the world. Public so that other tools (for example a profiler run) can use the same worlds.
    pub fn make(&self, content: &Arc<Content>) -> BenchWorld {
        (self.make)(content)
    }

    /// Make the world and run `ticks` ticks. Returns the numbers and the simulation after the last tick.
    pub fn run(&self, content: &Arc<Content>, ticks: u32) -> (BenchResult, Simulation) {
        let mut world = self.make(content);
        let mut times = Vec::with_capacity(ticks as usize);
        let (mut awake_sum, mut awake_max) = (0u64, 0u32);
        for t in 0..ticks {
            (world.before_tick)(&mut world.sim, t);
            let start = Instant::now();
            world.sim.tick();
            times.push(start.elapsed().as_secs_f64() * 1000.0);
            let awake = world.sim.stats().awake_chunks;
            awake_sum += awake as u64;
            awake_max = awake_max.max(awake);
        }
        let n = times.len().max(1);
        let mean = times.iter().sum::<f64>() / n as f64;
        times.sort_by(f64::total_cmp);
        let pick = |p: usize| times.get((times.len().saturating_sub(1)) * p / 100).copied().unwrap_or(0.0);
        let result = BenchResult {
            ticks,
            mean_ms: round3(mean),
            p50_ms: round3(pick(50)),
            p95_ms: round3(pick(95)),
            max_ms: round3(pick(100)),
            awake_mean: (awake_sum as f64 / n as f64 * 10.0).round() / 10.0,
            awake_max,
            chunks_with_material: chunks_with_material(&world.sim),
        };
        (result, world.sim)
    }
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// Chunks with at least one cell that is not air (bedrock counts). For a world with no side
/// limit: the chunks with cells in memory.
pub fn chunks_with_material(sim: &Simulation) -> u32 {
    let (w, h) = sim.size_cells();
    if w == 0 {
        return sim.world().live_count() as u32;
    }
    let mut n = 0;
    for cy in 0..h / CHUNK_SIZE {
        for cx in 0..w / CHUNK_SIZE {
            // One read for each chunk. A read for each cell makes a chunk that is not in memory again for every cell.
            let found = sim
                .world()
                .with_cells(ChunkPos::new(cx, cy), |c| c.is_some_and(|c| c.mat.iter().any(|&m| m != MaterialId::AIR.0)));
            n += found as u32;
        }
    }
    n
}

/// Saved benchmark numbers, by benchmark name. The file is `bench/baseline.ron`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Baseline {
    /// The machine that made the numbers (see `machine`). Numbers from another machine do not compare well.
    #[serde(default)]
    pub machine: String,
    pub benches: BTreeMap<String, BenchResult>,
}

/// A short text about this machine: operating system, CPU architecture, and thread count.
pub fn machine() -> String {
    let threads = std::thread::available_parallelism().map_or(0, |n| n.get());
    format!("{} {}, {threads} threads", std::env::consts::OS, std::env::consts::ARCH)
}

impl Baseline {
    /// Read the baseline file. `Ok(None)` if the file does not exist.
    pub fn load(path: &Path) -> Result<Option<Baseline>> {
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
        let b = crate::scene::ron_options().from_str(&text).map_err(|e| anyhow!("{}: {e}", path.display()))?;
        Ok(Some(b))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let pretty = ron::ser::PrettyConfig::new().depth_limit(3);
        let body = ron::ser::to_string_pretty(self, pretty)?;
        let text = format!(
            "// Benchmark baseline: milliseconds per tick.\n\
             // Made by `cargo run -p foundry_headless --release -- bench --save-baseline`.\n\
             // `bench --compare` fails if a mean is more than {:.0}% above the number here.\n{body}\n",
            (SLOWER_LIMIT - 1.0) * 100.0
        );
        std::fs::write(path, text).with_context(|| format!("cannot write {}", path.display()))
    }
}

/// One benchmark compared with its baseline.
#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    pub name: String,
    pub baseline_ms: Option<f64>,
    pub mean_ms: f64,
    /// Why there is no comparison, if there is none.
    pub skipped: Option<String>,
    /// True if the mean is more than `SLOWER_LIMIT` times the baseline mean.
    pub slower: bool,
}

impl Comparison {
    /// Change of the mean against the baseline, in percent. Positive is slower.
    pub fn change_percent(&self) -> Option<f64> {
        self.baseline_ms.filter(|b| *b > 0.0).map(|b| (self.mean_ms / b - 1.0) * 100.0)
    }
}

/// Compare results with a baseline. A benchmark is only compared if the tick count is the same.
pub fn compare(results: &[(&str, BenchResult)], baseline: &Baseline) -> Vec<Comparison> {
    results
        .iter()
        .map(|(name, r)| {
            let base = baseline.benches.get(*name);
            let skipped = match base {
                None => Some("no baseline".to_string()),
                Some(b) if b.ticks != r.ticks => Some(format!("baseline ran {} ticks", b.ticks)),
                Some(_) => None,
            };
            let baseline_ms = base.map(|b| b.mean_ms);
            let slower = skipped.is_none() && baseline_ms.is_some_and(|b| r.mean_ms > b * SLOWER_LIMIT);
            Comparison { name: name.to_string(), baseline_ms, mean_ms: r.mean_ms, skipped, slower }
        })
        .collect()
}

// ---- Benchmark worlds ----

fn new_sim(content: &Arc<Content>, width_chunks: i32, height_chunks: i32) -> Simulation {
    Simulation::new(content.clone(), SimConfig::finite(width_chunks, height_chunks, 1))
}

/// Fill the cells with x in `x0..x1` and y in `y0..y1`.
fn fill(sim: &mut Simulation, x0: i32, y0: i32, x1: i32, y1: i32, m: MaterialId) {
    for y in y0..y1 {
        for x in x0..x1 {
            sim.set_cell(CellPos::new(x, y), m, None);
        }
    }
}

/// Put `count` cells of `m` at random air cells with x in `x0..x1` and y in `y0..y1`.
fn scatter(sim: &mut Simulation, rng: &mut Rng, m: MaterialId, (x0, y0, x1, y1): (i32, i32, i32, i32), count: u32) {
    for _ in 0..count {
        let p = CellPos::new(x0 + rng.below((x1 - x0) as u32) as i32, y0 + rng.below((y1 - y0) as u32) as i32);
        if sim.cell(p).material.is_air() {
            sim.set_cell(p, m, None);
        }
    }
}

fn no_action() -> BeforeTick {
    Box::new(|_, _| {})
}

fn sand_rain(content: &Arc<Content>) -> BenchWorld {
    let sim = new_sim(content, 32, 8);
    let sand = content.expect_material("sand");
    let w = sim.size_cells().0;
    let mut rng = Rng::new(7);
    BenchWorld { sim, before_tick: Box::new(move |sim, _| scatter(sim, &mut rng, sand, (64, 4, w - 64, 12), 256)) }
}

fn ocean(content: &Arc<Content>) -> BenchWorld {
    let mut sim = new_sim(content, 32, 16);
    let h = sim.size_cells().1;
    fill(&mut sim, 2, 320, 1400, h - 2, content.expect_material("water"));
    BenchWorld { sim, before_tick: no_action() }
}

fn lava_water(content: &Arc<Content>) -> BenchWorld {
    let mut sim = new_sim(content, 16, 8);
    let (w, h) = sim.size_cells();
    fill(&mut sim, 2, 300, 400, h - 2, content.expect_material("lava"));
    fill(&mut sim, w - 400, 300, w - 2, h - 2, content.expect_material("water"));
    BenchWorld { sim, before_tick: no_action() }
}

fn pile_collapse(content: &Arc<Content>) -> BenchWorld {
    let mut sim = new_sim(content, 16, 16);
    let h = sim.size_cells().1;
    fill(&mut sim, 412, 64, 612, h - 2, content.expect_material("sand"));
    BenchWorld { sim, before_tick: no_action() }
}

/// Settled ground: stone from `top` down, a flat sand layer 20 cells thick on it,
/// and closed caves in the stone (water caves full to the top, and air caves).
fn settled_ground(sim: &mut Simulation, content: &Content, top: i32) {
    let (w, h) = sim.size_cells();
    let (stone, sand, water) =
        (content.expect_material("stone"), content.expect_material("sand"), content.expect_material("water"));
    fill(sim, 2, top, w - 2, h - 2, stone);
    fill(sim, 2, top - 20, w - 2, top, sand);
    let mut x = 100;
    while x + 400 < w - 2 {
        fill(sim, x, top + 200, x + 300, top + 320, water);
        fill(sim, x + 100, top + 500, x + 300, top + 580, MaterialId::AIR);
        x += 512;
    }
}

fn settled_world(content: &Arc<Content>) -> BenchWorld {
    let mut sim = new_sim(content, 64, 32);
    settled_ground(&mut sim, content, 1024);
    let sand = content.expect_material("sand");
    let drop = CellPos::new(sim.size_cells().0 / 2, 64);
    let before_tick = Box::new(move |sim: &mut Simulation, _| {
        if sim.cell(drop).material.is_air() {
            sim.set_cell(drop, sand, None);
        }
    });
    BenchWorld { sim, before_tick }
}

fn mixed(content: &Arc<Content>) -> BenchWorld {
    let mut sim = new_sim(content, 64, 32);
    let ground = 1024;
    let surface = ground - 20;
    settled_ground(&mut sim, content, ground);
    let (sand, water, lava) =
        (content.expect_material("sand"), content.expect_material("water"), content.expect_material("lava"));
    // x 0..1024: sand rain. The air already has some falling sand at the start.
    let mut rng = Rng::new(11);
    scatter(&mut sim, &mut rng, sand, (64, 16, 960, surface), 9000);
    // x 1024..2048: an ocean block that flows right.
    fill(&mut sim, 1024, 256, 1700, surface, water);
    // x 2048..3072: lava and water meet.
    fill(&mut sim, 2048, 800, 2400, surface, lava);
    fill(&mut sim, 2720, 800, 3072, surface, water);
    // x 3072..4096: a sand column collapses.
    fill(&mut sim, 3400, 100, 3700, surface, sand);
    BenchWorld { sim, before_tick: Box::new(move |sim, _| scatter(sim, &mut rng, sand, (64, 4, 960, 12), 128)) }
}

/// An infinite world 40 chunks tall: 32 chunks of sky over stone.
fn infinite_sim(content: &Arc<Content>) -> Simulation {
    Simulation::new(content.clone(), SimConfig { sky_chunks: 32, depth_chunks: 8, ..SimConfig::infinite(1, None) })
}

/// The view shows 40 x 25 chunks of sky with falling sand and water (every 2nd column, every
/// 3rd row), so about 1,000 chunks are awake at the start. The work starts at tick 1100 in every
/// world (ticks with no work run first), so the worlds of different benchmarks run the same ticks.
fn add_falling_work(sim: &mut Simulation, content: &Content) {
    let (sand, water) = (content.expect_material("sand"), content.expect_material("water"));
    let area = CellRect::new(0, 0, 40 * CHUNK_SIZE, 25 * CHUNK_SIZE);
    sim.apply(Command::SetView { area });
    while sim.tick_count() < 1100 {
        sim.tick();
    }
    for y in (40..area.y1 - 40).step_by(3) {
        for x in (4..area.x1 - 4).step_by(2) {
            sim.set_cell(CellPos::new(x, y), if (x / 64 + y / 64) % 2 == 0 { sand } else { water }, None);
        }
    }
}

fn infinite_work(content: &Arc<Content>) -> BenchWorld {
    let mut sim = infinite_sim(content);
    add_falling_work(&mut sim, content);
    BenchWorld { sim, before_tick: no_action() }
}

fn infinite_explored(content: &Arc<Content>) -> BenchWorld {
    let mut sim = infinite_sim(content);
    let stone = content.expect_material("stone");
    // One stone cell in each of 100,000 sky chunks, in batches of 1,000: the view shows a batch,
    // the chunks update once and sleep, and when the view moves on they are packed.
    for batch in 0..100 {
        let x0 = 100 + batch * 100;
        let area = CellRect::new(x0 * CHUNK_SIZE, 2 * CHUNK_SIZE, (x0 + 100) * CHUNK_SIZE, 12 * CHUNK_SIZE);
        sim.apply(Command::SetView { area });
        for i in 0..1000 {
            let (cx, cy) = (x0 + i / 10, 2 + i % 10);
            sim.set_cell(CellPos::new(cx * CHUNK_SIZE + 5, cy * CHUNK_SIZE + 5), stone, None);
        }
        for _ in 0..10 {
            sim.tick();
        }
    }
    // Move the view to the work area, so the last batch is packed too.
    sim.apply(Command::SetView { area: CellRect::new(0, 0, 40 * CHUNK_SIZE, 25 * CHUNK_SIZE) });
    for _ in 0..10 {
        sim.tick();
    }
    add_falling_work(&mut sim, content);
    BenchWorld { sim, before_tick: no_action() }
}

fn infinite_walk(content: &Arc<Content>) -> BenchWorld {
    let sim = Simulation::new(content.clone(), SimConfig { depth_chunks: 24, ..SimConfig::infinite(1, None) });
    let (sand, water) = (content.expect_material("sand"), content.expect_material("water"));
    let (w, h, y0) = (28 * CHUNK_SIZE, 16 * CHUNK_SIZE, 8 * CHUNK_SIZE);
    let before_tick = Box::new(move |sim: &mut Simulation, t: u32| {
        let x = t as i32 * CHUNK_SIZE;
        sim.apply(Command::SetView { area: CellRect::new(x, y0, x + w, y0 + h) });
        if t.is_multiple_of(32) {
            let p = CellPos::new(x + w - 200, 1060);
            sim.paint(p, 30, MaterialId::AIR, PaintMode::Replace, None);
            sim.paint(p.offset(-20, -120), 12, sand, PaintMode::Replace, None);
            sim.paint(p.offset(20, -100), 14, water, PaintMode::Replace, None);
        }
    });
    BenchWorld { sim, before_tick }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(mean_ms: f64, ticks: u32) -> BenchResult {
        BenchResult {
            ticks,
            mean_ms,
            p50_ms: mean_ms,
            p95_ms: mean_ms,
            max_ms: mean_ms,
            awake_mean: 1.0,
            awake_max: 1,
            chunks_with_material: 1,
        }
    }

    #[test]
    fn compare_flags_only_more_than_ten_percent() {
        let mut base = Baseline::default();
        base.benches.insert("a".into(), result(10.0, 100));
        base.benches.insert("b".into(), result(10.0, 100));
        base.benches.insert("c".into(), result(10.0, 50));
        let rows = compare(&[("a", result(10.9, 100)), ("b", result(11.2, 100)), ("c", result(99.0, 100)), ("d", result(1.0, 100))], &base);
        assert!(!rows[0].slower);
        assert!(rows[1].slower);
        assert!(!rows[2].slower && rows[2].skipped.is_some(), "different tick count is not compared");
        assert!(rows[3].skipped.is_some());
    }

    #[test]
    fn baseline_round_trip() {
        let mut base = Baseline { machine: machine(), ..Default::default() };
        base.benches.insert("sand_rain".into(), result(1.25, 600));
        let path = std::env::temp_dir().join(format!("foundry_baseline_{}.ron", std::process::id()));
        base.save(&path).unwrap();
        let back = Baseline::load(&path).unwrap().unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(base, back);
    }

    #[test]
    fn every_bench_runs() {
        let content = Arc::new(Content::load_default().unwrap());
        for b in BENCHES {
            let (r, sim) = b.run(&content, 1);
            // Some worlds run ticks while they are made (infinite_explored packs its far chunks).
            assert!(sim.tick_count() >= 1);
            assert_eq!(r.ticks, 1);
            assert!(r.chunks_with_material > 0, "{}", b.name);
        }
    }
}
