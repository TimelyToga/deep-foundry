//! Speed and memory tests of the infinite world.
//!
//! ```text
//! cargo run --release -p foundry_sim --example infinite -- loaded     # (a) tick time with 100,000 extra chunks
//! cargo run --release -p foundry_sim --example infinite -- walk [N]   # (b) move the view N chunks (default 10,000)
//! cargo run --release -p foundry_sim --example infinite -- generate   # how many chunks per second the source makes
//! ```
//!
//! `loaded` keeps 100,000 chunks live in memory (about 3.3 GB) for its strictest case.

use foundry_content::Content;
use foundry_core::{CHUNK_SIZE, CellPos, CellRect, Command, MaterialId, PaintMode};
use foundry_sim::{SimConfig, Simulation};
use std::sync::Arc;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let content = Arc::new(Content::load_default().unwrap());
    match args.first().map(String::as_str) {
        Some("loaded") => loaded(&content),
        Some("walk") => walk(&content, args.get(1).and_then(|a| a.parse().ok()).unwrap_or(10_000)),
        Some("generate") => generate(&content),
        _ => eprintln!("usage: infinite (loaded | walk [chunks] | generate)"),
    }
}

/// An infinite world: 16 chunks of sky over stone, 40 chunks in all.
fn world(content: &Arc<Content>) -> Simulation {
    Simulation::new(content.clone(), SimConfig { depth_chunks: 24, ..SimConfig::infinite(5, None) })
}

/// The work area: 40 × 25 chunks with falling sand and water in the top half (like the stress
/// example), so about 1,000 chunks are awake.
const WORK: CellRect = CellRect::new(0, 0, 40 * CHUNK_SIZE, 25 * CHUNK_SIZE);

fn add_work(s: &mut Simulation) {
    let c = s.content().clone();
    let (sand, water) = (c.expect_material("sand"), c.expect_material("water"));
    s.apply(Command::SetView { area: WORK });
    for y in (40..WORK.y1 / 2).step_by(3) {
        for x in (4..WORK.x1 - 4).step_by(2) {
            s.set_cell(CellPos::new(x, y), if (x / 64 + y / 64) % 2 == 0 { sand } else { water }, None);
        }
    }
}

/// Mean tick time (ms) and mean awake chunks over `ticks` ticks.
fn run(s: &mut Simulation, ticks: u32) -> (f64, f64) {
    let (mut ms, mut awake) = (0.0, 0.0);
    for _ in 0..ticks {
        let t = Instant::now();
        s.tick();
        ms += t.elapsed().as_secs_f64() * 1000.0;
        awake += s.stats().awake_chunks as f64;
    }
    (ms / ticks as f64, awake / ticks as f64)
}

/// (a) The same 1,000 awake chunks, with and without 100,000 more chunks in memory.
fn loaded(content: &Arc<Content>) {
    let ticks = 150;
    // Three worlds: only the work area; plus 100,000 live chunks (never unloaded); plus 100,000
    // packed chunks (an explored world that the player changed in many places).
    let small = || {
        let mut s = world(content);
        add_work(&mut s);
        s
    };
    let with_live = || {
        let mut s = world(content);
        s.settings_mut().unload_every_ticks = 0;
        // 100,000 stone chunks to the right of the work area: 2,500 columns × 40 rows (the 16 sky
        // rows are all air and are not stored as cells, so the stone rows are made wider).
        let start = Instant::now();
        let (x0, rows) = (100 * CHUNK_SIZE, 24);
        let columns = 100_000 / rows + 1;
        for i in 0..(columns / 200 + 1) {
            let x = x0 + i * 200 * CHUNK_SIZE;
            s.apply(Command::SetView { area: CellRect::new(x, 16 * CHUNK_SIZE, x + 200 * CHUNK_SIZE, (16 + rows) * CHUNK_SIZE) });
            s.tick();
        }
        println!("  made {} live chunks in {:.2} s", s.memory().live_chunks, start.elapsed().as_secs_f64());
        add_work(&mut s);
        s
    };
    let with_packed = || {
        let mut s = world(content);
        let stone = content.expect_material("stone");
        let start = Instant::now();
        // One stone cell in each of 100,000 sky chunks (a changed chunk each). In batches of 1,000
        // chunks: the view shows the batch, the chunks update once and sleep, and when the view
        // moves on they are packed.
        for batch in 0..100 {
            let x = (100 + batch * 100) * CHUNK_SIZE;
            s.apply(Command::SetView { area: CellRect::new(x, 2 * CHUNK_SIZE, x + 100 * CHUNK_SIZE, 12 * CHUNK_SIZE) });
            for i in 0..1000 {
                let (cx, cy) = (100 + batch * 100 + i / 10, 2 + i % 10);
                s.set_cell(CellPos::new(cx * CHUNK_SIZE + 5, cy * CHUNK_SIZE + 5), stone, None);
            }
            for _ in 0..10 {
                s.tick();
            }
        }
        add_work(&mut s);
        for _ in 0..10 {
            s.tick();
        }
        let m = s.memory();
        println!(
            "  made {} packed chunks ({:.1} MB) in {:.2} s",
            m.packed_chunks,
            m.packed_bytes as f64 / 1e6,
            start.elapsed().as_secs_f64()
        );
        s
    };
    println!("(a) {ticks} ticks of the same work, best of 3 rounds:");
    let mut best = [f64::MAX; 3];
    for round in 0..3 {
        for (k, make) in [&small as &dyn Fn() -> Simulation, &with_live, &with_packed].iter().enumerate() {
            let mut s = make();
            let (ms, awake) = run(&mut s, ticks);
            let m = s.memory();
            println!(
                "  round {round}, {:<28} mean {ms:6.3} ms per tick, awake {awake:6.1}, live {:6}, packed {:6}",
                ["1,000 awake only", "+ 100,000 live chunks", "+ 100,000 packed chunks"][k],
                m.live_chunks,
                m.packed_chunks
            );
            best[k] = best[k].min(ms);
        }
    }
    println!(
        "  best: work only {:.3} ms, with 100,000 live {:.3} ms ({:+.1}%), with 100,000 packed {:.3} ms ({:+.1}%)",
        best[0],
        best[1],
        (best[1] / best[0] - 1.0) * 100.0,
        best[2],
        (best[2] / best[0] - 1.0) * 100.0
    );
}

/// (b) Move a view of 28 × 16 chunks to the right, one chunk per tick (about the fastest camera
/// pan at zoom 1). Every 32 ticks, dig a hole and drop sand and water into it, so changed
/// chunks are left behind. Prints tick times, chunks made per second and memory.
fn walk(content: &Arc<Content>, chunks: i32) {
    let mut s = world(content);
    let c = s.content().clone();
    let (sand, water) = (c.expect_material("sand"), c.expect_material("water"));
    let (w, h) = (28 * CHUNK_SIZE, 16 * CHUNK_SIZE);
    let y0 = 8 * CHUNK_SIZE;
    let mut times = Vec::with_capacity(chunks as usize);
    let mut generated_time = 0.0;
    let start = Instant::now();
    for t in 0..chunks {
        let x = t * CHUNK_SIZE;
        s.apply(Command::SetView { area: CellRect::new(x, y0, x + w, y0 + h) });
        if t % 32 == 0 {
            let p = CellPos::new(x + w - 200, 1060);
            s.paint(p, 30, MaterialId::AIR, PaintMode::Replace, None);
            s.paint(p.offset(-20, -120), 12, sand, PaintMode::Replace, None);
            s.paint(p.offset(20, -100), 14, water, PaintMode::Replace, None);
        }
        let t0 = Instant::now();
        s.tick();
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        times.push(ms);
        generated_time += s.stats().sections[0].1 as f64;
        s.take_snapshot();
        if t % 2000 == 0 || t == chunks - 1 {
            let m = s.memory();
            println!(
                "  chunk {t:6}: tick {ms:6.3} ms, live {:5}, air {:4}, packed {:5} ({:6.2} MB), paused {:4}, made so far {:7}, memory {:6.1} MB",
                m.live_chunks,
                m.air_chunks,
                m.packed_chunks,
                m.packed_bytes as f64 / 1e6,
                m.paused_chunks,
                m.generated_total,
                m.bytes as f64 / 1e6
            );
        }
    }
    let total = start.elapsed().as_secs_f64();
    let m = s.memory();
    times.sort_by(f64::total_cmp);
    let mean = times.iter().sum::<f64>() / times.len() as f64;
    println!(
        "(b) walked {chunks} chunks in {total:.1} s ({:.0} ticks per second): tick mean {mean:.3} ms, p50 {:.3}, p95 {:.3}, max {:.3} ms",
        chunks as f64 / total,
        times[times.len() / 2],
        times[times.len() * 95 / 100],
        times[times.len() - 1]
    );
    println!(
        "    chunks made: {} ({:.0} per second of walking; {:.1} ms of tick time was spent making the view's chunks)",
        m.generated_total,
        m.generated_total as f64 / total,
        generated_time
    );
    let mut bytes = vec![];
    let save = Instant::now();
    s.save(&mut bytes).unwrap();
    println!(
        "    memory at the end: {:.1} MB of chunk data (live {}, air {}, packed {} = {:.1} MB, paused {}); process now {:.0} MB",
        m.bytes as f64 / 1e6,
        m.live_chunks,
        m.air_chunks,
        m.packed_chunks,
        m.packed_bytes as f64 / 1e6,
        m.paused_chunks,
        rss_mb()
    );
    println!(
        "    save: {} chunks, {:.2} MB in {:.0} ms",
        s.world().saved_positions().len(),
        bytes.len() as f64 / 1e6,
        save.elapsed().as_secs_f64() * 1000.0
    );
}

/// Chunks made per second: a large view makes 50,000 stone chunks at once, with all threads and with one.
fn generate(content: &Arc<Content>) {
    for threads in [0, 1] {
        let mut s = world(content);
        if threads > 0 {
            s.set_threads(threads);
        }
        s.settings_mut().unload_every_ticks = 0;
        let start = Instant::now();
        // 2,000 columns × 24 stone rows = 48,000 chunks (the sky rows are all air).
        s.apply(Command::SetView { area: CellRect::new(0, 0, 2000 * CHUNK_SIZE, 40 * CHUNK_SIZE) });
        s.take_snapshot();
        let secs = start.elapsed().as_secs_f64();
        let m = s.memory();
        println!(
            "{} threads: made {} chunks ({} with cells, {} all air) in {:.2} s: {:.0} chunks per second",
            if threads == 0 { rayon::current_num_threads() } else { threads },
            m.generated_total,
            m.live_chunks,
            m.air_chunks,
            secs,
            m.generated_total as f64 / secs
        );
    }
}

/// The resident memory of this process now, in MB (from `ps`; 0 if that fails).
fn rss_mb() -> f64 {
    let pid = std::process::id().to_string();
    std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<f64>().ok())
        .map_or(0.0, |kb| kb / 1024.0)
}
