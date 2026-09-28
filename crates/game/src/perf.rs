//! Timing logs for the window, and a fixed dig script to measure with.
//!
//! - `DEEP_FOUNDRY_PERF=1`: once per second, print one line for the main thread and one line for
//!   the simulation thread. Each part shows the average and the worst time of that second.
//! - `DEEP_FOUNDRY_DIG_SCRIPT=1`: in the normal mode, the game plays a fixed script: the mouse
//!   moves around the robot, the dig button is down, and the robot walks right, then left.
//!
//! Example:
//!
//! ```sh
//! DEEP_FOUNDRY_PERF=1 DEEP_FOUNDRY_DIG_SCRIPT=1 cargo run -p deep_foundry --release -- \
//!     --ui-state playing --mode normal --exit-after 14
//! ```

use std::sync::OnceLock;
use std::time::Instant;

fn env_on(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| !v.is_empty() && v != "0")
}

/// True if the timing logs are on.
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| env_on("DEEP_FOUNDRY_PERF"))
}

/// True if the dig script is on.
pub fn dig_script() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| env_on("DEEP_FOUNDRY_DIG_SCRIPT"))
}

/// The sum and the worst time of each part, over one second.
#[derive(Debug, Clone)]
pub struct Parts {
    names: &'static [&'static str],
    sum: Vec<f64>,
    max: Vec<f64>,
    /// Number of rounds (frames or ticks).
    pub count: u32,
    /// Sum and worst of the whole round.
    total_sum: f64,
    total_max: f64,
    start: Instant,
    part: usize,
    last: Instant,
}

impl Parts {
    pub fn new(names: &'static [&'static str]) -> Self {
        let now = Instant::now();
        Self {
            names,
            sum: vec![0.0; names.len()],
            max: vec![0.0; names.len()],
            count: 0,
            total_sum: 0.0,
            total_max: 0.0,
            start: now,
            part: 0,
            last: now,
        }
    }

    /// Start a round.
    pub fn begin(&mut self) {
        self.start = Instant::now();
        self.last = self.start;
        self.part = 0;
    }

    /// The next part ends now.
    pub fn mark(&mut self) {
        let now = Instant::now();
        if let Some(i) = (self.part < self.names.len()).then_some(self.part) {
            let ms = now.duration_since(self.last).as_secs_f64() * 1000.0;
            self.sum[i] += ms;
            self.max[i] = self.max[i].max(ms);
        }
        self.part += 1;
        self.last = now;
    }

    /// The round ends now.
    pub fn end(&mut self) {
        let ms = self.start.elapsed().as_secs_f64() * 1000.0;
        self.total_sum += ms;
        self.total_max = self.total_max.max(ms);
        self.count += 1;
    }

    /// One line: "name avg/max" for each part, then the whole round.
    pub fn line(&self) -> String {
        let n = self.count.max(1) as f64;
        let mut s = String::new();
        for (i, name) in self.names.iter().enumerate() {
            s.push_str(&format!("{name} {:.2}/{:.2}  ", self.sum[i] / n, self.max[i]));
        }
        s.push_str(&format!("| all {:.2}/{:.2} ms", self.total_sum / n, self.total_max));
        s
    }

    /// Forget the sums (a new second starts).
    pub fn reset(&mut self) {
        self.sum.iter_mut().for_each(|v| *v = 0.0);
        self.max.iter_mut().for_each(|v| *v = 0.0);
        self.count = 0;
        self.total_sum = 0.0;
        self.total_max = 0.0;
    }
}

/// Parts of one loop of the simulation thread.
pub const SIM_PARTS: &[&str] = &["commands", "cells", "factory", "snapshot", "frame", "publish"];

/// Timing logs of the simulation thread. All methods do nothing when the logs are off.
pub struct SimPerf {
    parts: Option<Parts>,
    since: Instant,
    /// Ticks that ran right after another one, because the thread was late.
    late: u32,
    /// Time that the thread could not catch up and forgot.
    lost_ms: f64,
    /// Chunk images sent.
    chunks: usize,
    /// The parts of the cell update (`SimStats::sections`): name, sum and worst (ms).
    sections: Vec<(&'static str, f64, f64)>,
    /// Chunks that the chunk source made.
    generated: u32,
    /// Ticks on the `crew` pool (`sim_pool.rs`).
    parallel: u32,
}

impl SimPerf {
    pub fn new() -> Self {
        Self {
            parts: enabled().then(|| Parts::new(SIM_PARTS)),
            since: Instant::now(),
            late: 0,
            lost_ms: 0.0,
            chunks: 0,
            sections: Vec::new(),
            generated: 0,
            parallel: 0,
        }
    }

    /// A loop starts. `late`: it runs right after another loop.
    pub fn begin(&mut self, late: bool) {
        if let Some(p) = self.parts.as_mut() {
            p.begin();
            self.late += late as u32;
        }
    }

    /// The next part of the loop ends now.
    pub fn mark(&mut self) {
        if let Some(p) = self.parts.as_mut() {
            p.mark();
        }
    }

    /// Count the chunks and the parts of the cell update of a snapshot.
    pub fn snapshot(&mut self, s: &foundry_core::Snapshot, ticked: bool) {
        if self.parts.is_none() {
            return;
        }
        self.chunks += s.chunks.len();
        if !ticked {
            return;
        }
        for (i, (name, ms)) in s.stats.sections.iter().enumerate() {
            if self.sections.len() <= i {
                self.sections.push((name, 0.0, 0.0));
            }
            let x = &mut self.sections[i];
            x.1 += *ms as f64;
            x.2 = x.2.max(*ms as f64);
        }
        self.generated += s.stats.generated_chunks;
    }

    /// The loop ends now (after the publish).
    pub fn end(&mut self) {
        if let Some(p) = self.parts.as_mut() {
            p.mark();
            p.end();
        }
    }

    /// The thread forgot this lost time.
    pub fn lost(&mut self, d: std::time::Duration) {
        self.lost_ms += d.as_secs_f64() * 1000.0;
    }

    /// Print two lines once per second. `parallel`: the last tick used the `crew` pool.
    pub fn print_each_second(&mut self, parallel: bool) {
        let Some(p) = self.parts.as_mut() else { return };
        self.parallel += parallel as u32;
        if self.since.elapsed().as_secs_f64() < 1.0 {
            return;
        }
        println!("perf sim:  {} | ticks {} late {} lost {:.1} ms chunks {} crew {}", p.line(), p.count, self.late, self.lost_ms, self.chunks, self.parallel);
        let n = p.count.max(1) as f64;
        let sections: Vec<String> = self.sections.iter().map(|(name, sum, max)| format!("{name} {:.2}/{:.2}", sum / n, max)).collect();
        println!("perf cells: {} | generated chunks {}", sections.join("  "), self.generated);
        p.reset();
        *self = Self { parts: self.parts.take(), ..Self::new() };
    }
}

/// Parts of one frame of the main thread.
pub const MAIN_PARTS: &[&str] = &["snapshot", "model", "egui", "actions", "acquire", "draw", "present"];

/// Timing logs of the main thread for one second.
pub struct MainPerf {
    pub parts: Parts,
    since: Instant,
    /// Frames that got no new tick, one tick, and more than one tick.
    pub ticks_per_frame: [u32; 3],
    /// The longest time between two frames.
    pub max_interval_ms: f64,
    /// Chunk images uploaded.
    pub uploads: u32,
    /// When the dig script started.
    pub script_start: Option<Instant>,
    /// Frames in which the drawn robot moved much more or much less than its speed says, while
    /// it walked. Each one is a visible jerk.
    pub jerks: u32,
    /// The drawn robot x of the last frame.
    last_x: Option<f32>,
}

impl MainPerf {
    pub fn new() -> Self {
        Self {
            parts: Parts::new(MAIN_PARTS),
            since: Instant::now(),
            ticks_per_frame: [0; 3],
            max_interval_ms: 0.0,
            uploads: 0,
            script_start: None,
            jerks: 0,
            last_x: None,
        }
    }

    /// Numbers of a frame: the new ticks, the drawn robot x and its speed (cells per tick), and
    /// the time since the last frame (seconds).
    pub fn frame_data(&mut self, new_ticks: u64, robot: Option<(f32, f32)>, dt: f32) {
        self.ticks_per_frame[(new_ticks as usize).min(2)] += 1;
        if let (Some((x, vel)), Some(last)) = (robot, self.last_x) {
            let expected = vel * dt / foundry_core::TICK_SECONDS as f32;
            if vel.abs() > 0.5 && ((x - last) - expected).abs() > 0.5 * expected.abs() {
                self.jerks += 1;
            }
        }
        self.last_x = robot.map(|r| r.0);
    }

    /// A frame ends. Print the line once per second.
    pub fn end_frame(&mut self, interval_ms: f64) {
        self.parts.end();
        self.max_interval_ms = self.max_interval_ms.max(interval_ms);
        if self.since.elapsed().as_secs_f64() >= 1.0 {
            let [none, one, more] = self.ticks_per_frame;
            println!(
                "perf main: {} | frames {} (0 ticks {none}, 1 tick {one}, 2+ ticks {more}) longest gap {:.1} ms uploads {} robot jerks {}",
                self.parts.line(),
                self.parts.count,
                self.max_interval_ms,
                self.uploads,
                self.jerks
            );
            self.parts.reset();
            self.ticks_per_frame = [0; 3];
            self.max_interval_ms = 0.0;
            self.uploads = 0;
            self.jerks = 0;
            self.since = Instant::now();
        }
    }
}

/// The mouse offset from the middle of the robot (in cells) and the walk direction at `t`
/// seconds after the script started. It digs down, then right while walking right, then left
/// while walking left, and the mouse moves in small circles all the time.
pub fn dig_script_at(t: f32) -> ((f32, f32), i8) {
    let t = t % 12.0;
    let (wx, wy) = ((t * 5.0).cos() * 5.0, (t * 5.0).sin() * 5.0);
    match t {
        t if t < 3.0 => ((wx, 14.0 + wy), 0),
        t if t < 7.0 => ((16.0 + wx, wy), 1),
        _ => ((-16.0 + wx, wy), -1),
    }
}
