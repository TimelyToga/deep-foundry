//! Quick stress test of the movement pass: `cargo run --release -p foundry_sim --example stress [threads]`.
//! The full benchmark suite is in foundry_headless.

use foundry_content::Content;
use foundry_core::{CellPos, CellRect, PaintMode};
use foundry_sim::{SimConfig, Simulation};
use std::sync::Arc;
use std::time::Instant;

fn main() {
    let threads: usize = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(0);
    let content = Arc::new(Content::load_default().unwrap());
    let (sand, water, stone) = (content.expect_material("sand"), content.expect_material("water"), content.expect_material("stone"));

    // 1. Many awake chunks: a 48 × 32 chunk world (1536 chunks), half full of falling sand and water.
    let mut s = Simulation::new(content.clone(), SimConfig::finite(48, 32, 5));
    if threads > 0 {
        s.set_threads(threads);
    }
    let (w, h) = s.size_cells();
    for y in (40..h / 2).step_by(3) {
        for x in (4..w - 4).step_by(2) {
            s.set_cell(CellPos::new(x, y), if (x / 64 + y / 64) % 2 == 0 { sand } else { water }, None);
        }
    }
    for x in (100..w - 100).step_by(300) {
        s.paint(CellPos::new(x, h * 3 / 4), 30, stone, PaintMode::Replace, None);
    }
    let cells = s.count_material(CellRect::new(0, 0, w, h), sand) + s.count_material(CellRect::new(0, 0, w, h), water);
    println!("world {}x{} cells, {} moving cells, threads {}", w, h, cells, if threads == 0 { rayon::current_num_threads() } else { threads });
    let mut times = vec![];
    for t in 0..600 {
        let start = Instant::now();
        s.tick();
        times.push(start.elapsed().as_secs_f64() * 1000.0);
        if t % 100 == 0 {
            println!("tick {t:4}: {:6.2} ms, awake chunks {}", times[t], s.stats().awake_chunks);
        }
    }
    report("falling (600 ticks)", &mut times);

    // 2. Sand rain: 400 new sand cells per tick over the whole width.
    let mut times = vec![];
    for t in 0..600u32 {
        for i in 0..400u32 {
            let x = 4 + ((i * 7919 + t * 104_729) % (w as u32 - 8)) as i32;
            s.set_cell(CellPos::new(x, 3 + (i % 20) as i32), sand, None);
        }
        let start = Instant::now();
        s.tick();
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    println!("awake chunks at end: {}", s.stats().awake_chunks);
    report("sand rain (600 ticks)", &mut times);

    // 3. Flood: a block of water 1000 wide and 600 tall (600,000 cells) at the left of a
    //    48 × 16 chunk world, as if a dam was just removed.
    let mut s = Simulation::new(content.clone(), SimConfig::finite(48, 16, 5));
    if threads > 0 {
        s.set_threads(threads);
    }
    let (_, h) = s.size_cells();
    for y in h - 602..h - 2 {
        for x in 2..1002 {
            s.set_cell(CellPos::new(x, y), water, None);
        }
    }
    let mut times = vec![];
    let (mut awake, mut particles) = (0u64, 0usize);
    for _ in 0..600 {
        let start = Instant::now();
        s.tick();
        times.push(start.elapsed().as_secs_f64() * 1000.0);
        awake += s.stats().awake_chunks as u64;
        particles = particles.max(s.particles().len());
    }
    println!("flood: mean awake chunks {}, most particles {particles}", awake / 600);
    report("flood (600 ticks)", &mut times);
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
