//! Time the generator on one thread: the mean time per chunk for 1,000 chunks in a few parts of
//! the world. The time is the CPU time of the thread, and each area runs 5 times and the lowest
//! mean is kept, because other programs on the machine make some runs slow. Run it with
//! `cargo run --release -p foundry_worldgen --example speed`.
//! `-- loop` keeps making chunks for 20 seconds (for a profiler).

use foundry_content::Content;
use foundry_core::{CHUNK_AREA, ChunkPos};
use foundry_sim::{ChunkCells, ChunkSource};
use foundry_worldgen::{WorldGen, WorldGenSettings};
use std::time::Instant;

/// CPU time of this thread in seconds.
fn thread_time() -> f64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: `ts` is a valid timespec for the call to write.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    ts.tv_sec as f64 + ts.tv_nsec as f64 * 1e-9
}

fn main() {
    let content = Content::load_default().expect("data files");
    let wg = WorldGen::new(&content, WorldGenSettings::default());
    let mut mat = Box::new([0u16; CHUNK_AREA]);
    let mut temp = Box::new([0i16; CHUNK_AREA]);
    let mut run = |name: &str, cx0: i32, cy0: i32, w: i32, h: i32| {
        let mut best = f64::MAX;
        let n = w * h;
        for _ in 0..5 {
            let start = thread_time();
            for cy in cy0..cy0 + h {
                for cx in cx0..cx0 + w {
                    mat.fill(0);
                    temp.fill(ChunkCells::MATERIAL_DEFAULT);
                    let mut cells =
                        ChunkCells { pos: ChunkPos::new(cx, cy), seed: 1, mat: &mut mat, temp: &mut temp, awake: false };
                    wg.generate(&mut cells);
                }
            }
            best = best.min((thread_time() - start) * 1e6 / n as f64);
        }
        println!("{name:<28} {n:>5} chunks  {best:>6.1} microseconds per chunk");
        best
    };
    if std::env::args().any(|a| a == "loop") {
        let start = Instant::now();
        while start.elapsed().as_secs() < 20 {
            run("upper stone", -20, 30, 40, 25);
        }
        return;
    }
    // A view of 28 x 16 chunks is about 450 chunks; these areas are 1,000 chunks each.
    let a = run("surface band (start area)", -20, 11, 40, 25);
    let b = run("upper stone", -20, 30, 40, 25);
    let c = run("deep rock", -20, 60, 40, 25);
    let d = run("surface band (far out)", 3000, 11, 40, 25);
    println!("mean of the four areas: {:.1} microseconds per chunk", (a + b + c + d) / 4.0);
}
