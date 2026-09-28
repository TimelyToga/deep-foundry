//! Speed test of the heat pass: `cargo run --release -p foundry_sim --example heat_stress [threads]`.
//!
//! 1. "all active": a 48 × 32 chunk world (1,536 chunks) full of stone and copper with random
//!    temperatures, so heat flows in every chunk in every tick.
//! 2. "lava and water": lava, water, ice and hot metal in a 32 × 16 chunk world with air, so
//!    movement, boiling, melting and cooling happen at once.
//!
//! Prints the time of the heat pass (the "heat" section of `SimStats`) and of the whole tick.

use foundry_content::Content;
use foundry_core::{CHUNK_AREA, CellPos, CellRect, ChunkPos, PaintMode, Rng};
use foundry_sim::{SimConfig, Simulation};
use std::sync::Arc;

fn main() {
    let arg = |i: usize, default: usize| std::env::args().nth(i).and_then(|a| a.parse().ok()).unwrap_or(default);
    // Arguments: threads (0: all), ticks of case 1, ticks of case 2.
    let (threads, ticks1, ticks2) = (arg(1, 0), arg(2, 300), arg(3, 600));
    let content = Arc::new(Content::load_default().unwrap());
    let (stone, copper) = (content.expect_material("stone").0, content.expect_material("copper_block").0);

    // 1. Every chunk is heat-active.
    let mut s = Simulation::new(content.clone(), SimConfig::finite(48, 32, 5));
    if threads > 0 {
        s.set_threads(threads);
    }
    let mut rng = Rng::new(7);
    for cy in 0..32 {
        for cx in 0..48 {
            let mut mats = vec![stone; CHUNK_AREA];
            let mut temps = vec![20i16; CHUNK_AREA];
            for i in 0..CHUNK_AREA {
                if (i / 64) % 16 < 4 {
                    mats[i] = copper;
                }
                temps[i] = 20 + rng.below(800) as i16;
            }
            s.fill_chunk(ChunkPos::new(cx, cy), &mats, Some(&temps));
        }
    }
    // The first tick moves nothing but visits every cell (fill_chunk marks the whole chunk).
    s.tick();
    run(&mut s, "all active", ticks1);
    if ticks2 == 0 {
        return;
    }

    // 2. Lava, water, ice and hot metal with air.
    let mut s = Simulation::new(content.clone(), SimConfig::finite(32, 16, 6));
    if threads > 0 {
        s.set_threads(threads);
    }
    let m = |n: &str| content.expect_material(n);
    let (w, h) = s.size_cells();
    for x in (200..w - 200).step_by(400) {
        s.paint(CellPos::new(x, h - 150), 60, m("lava"), PaintMode::Replace, None);
        s.paint(CellPos::new(x + 200, h - 150), 60, m("water"), PaintMode::Replace, None);
        s.paint(CellPos::new(x + 100, h - 300), 30, m("ice"), PaintMode::Replace, Some(-20));
        s.paint(CellPos::new(x + 100, 200), 40, m("copper_block"), PaintMode::Replace, Some(1000));
    }
    let cells = |s: &Simulation, n: &str| s.count_material(CellRect::new(0, 0, w, h), m(n));
    run(&mut s, "lava and water", ticks2);
    println!("  steam {}, stone {}, water {}", cells(&s, "steam"), cells(&s, "stone"), cells(&s, "water"));
}

fn run(s: &mut Simulation, name: &str, n: usize) {
    let (mut heat, mut total, mut chunks) = (vec![], vec![], 0u64);
    for _ in 0..n {
        s.tick();
        let st = s.stats();
        heat.push(st.sections.iter().find(|x| x.0 == "heat").map_or(0.0, |x| x.1) as f64);
        total.push(st.tick_ms as f64);
        chunks += st.awake_chunks as u64;
    }
    println!("{name}: {n} ticks, mean chunks worked {}", chunks / n as u64);
    report("  heat pass", &mut heat);
    report("  whole tick", &mut total);
}

fn report(name: &str, times: &mut [f64]) {
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mean = times.iter().sum::<f64>() / times.len() as f64;
    println!(
        "{name}: mean {:.2} ms, p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms",
        mean,
        times[times.len() / 2],
        times[times.len() * 95 / 100],
        times[times.len() - 1]
    );
}
