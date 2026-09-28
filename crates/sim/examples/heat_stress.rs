//! Speed test of the heat pass:
//! `cargo run --release -p foundry_sim --example heat_stress [threads] [ticks] [case]`.
//! `threads` 0 (default) uses all cores. `case` is one of the names below (default: all).
//!
//! 1. "random": a 48 × 32 chunk world (1,536 chunks) full of stone and copper with random
//!    temperatures from 20 to 820 °C, so heat flows in almost every cell of every chunk in every
//!    tick. This is the worst case.
//! 2. "blocks": the same world full of stone at 20 °C, with a copper block at 600 °C (16 × 16
//!    cells) in each chunk. Every chunk stays heat-active for a long time, but many of its cells
//!    are at rest.
//! 3. "lava": lava, water, ice and hot metal in a 32 × 16 chunk world with air, so movement,
//!    boiling, melting and cooling happen at once.
//!
//! Prints the time of the heat pass (the "heat" section of `SimStats`) and of the whole tick.

use foundry_content::Content;
use foundry_core::{CHUNK_AREA, CellPos, CellRect, ChunkPos, PaintMode, Rng, local_index};
use foundry_sim::{SimConfig, Simulation};
use std::sync::Arc;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let threads: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(0);
    let ticks: usize = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(300);
    let case = args.get(3).cloned().unwrap_or_default();
    let want = |name: &str| case.is_empty() || case == name;
    let content = Arc::new(Content::load_default().unwrap());
    let (stone, copper) = (content.expect_material("stone").0, content.expect_material("copper_block").0);
    let new_sim = |w: i32, h: i32, seed: u64| {
        let mut s = Simulation::new(content.clone(), SimConfig::finite(w, h, seed));
        if threads > 0 {
            s.set_threads(threads);
        }
        s
    };

    if want("random") {
        let mut s = new_sim(48, 32, 5);
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
        run(&mut s, "random", ticks);
    }

    if want("blocks") {
        let mut s = new_sim(48, 32, 5);
        for cy in 0..32 {
            for cx in 0..48 {
                let mut mats = vec![stone; CHUNK_AREA];
                let mut temps = vec![20i16; CHUNK_AREA];
                for y in 24..40 {
                    for x in 24..40 {
                        mats[local_index(x, y)] = copper;
                        temps[local_index(x, y)] = 600;
                    }
                }
                s.fill_chunk(ChunkPos::new(cx, cy), &mats, Some(&temps));
            }
        }
        s.tick();
        run(&mut s, "blocks", ticks);
    }

    if want("lava") {
        let mut s = new_sim(32, 16, 6);
        let m = |n: &str| content.expect_material(n);
        let (w, h) = s.size_cells();
        for x in (200..w - 200).step_by(400) {
            s.paint(CellPos::new(x, h - 150), 60, m("lava"), PaintMode::Replace, None);
            s.paint(CellPos::new(x + 200, h - 150), 60, m("water"), PaintMode::Replace, None);
            s.paint(CellPos::new(x + 100, h - 300), 30, m("ice"), PaintMode::Replace, Some(-20));
            s.paint(CellPos::new(x + 100, 200), 40, m("copper_block"), PaintMode::Replace, Some(1000));
        }
        run(&mut s, "lava", ticks * 2);
        let cells = |n: &str| s.count_material(CellRect::new(0, 0, w, h), m(n));
        println!("  steam {}, stone {}, water {}", cells("steam"), cells("stone"), cells("water"));
    }
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
