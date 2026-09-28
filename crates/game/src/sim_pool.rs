//! The worker threads of the simulation thread.
//!
//! The cell update splits its work into jobs and runs them with rayon (about 7 rounds of jobs per
//! tick while cells move, for example while the robot digs). A rayon call from a thread that is
//! not in the pool must wait until a pool thread wakes up, runs the jobs and reports back. When
//! other programs keep the CPU busy (for example a build), each of these waits can take several
//! milliseconds. Measured on an M1 Max under load: the work of a tick while digging is about
//! 0.1 ms, but ticks took 20 to 48 ms. The robot then stops, and the dig point does not follow
//! the mouse.
//!
//! So the simulation loop runs on a thread of its own one-thread pool (`solo`):
//! - A tick with few awake chunks runs all its jobs on that thread. There is no wait for another
//!   thread.
//! - A tick with many awake chunks (large falls and floods), or a tick that must make many new
//!   chunks (the view grew, for example after a zoom out), runs on a second pool with more
//!   threads (`crew`), because there the work is larger than the cost of the wait.
//! - On macOS all these threads ask for the "user interactive" service class, the class of the
//!   main thread of the window. The system then runs them before background work such as a build.

use foundry_core::{CellRect, Command};

/// From this number of awake chunks in the last tick, the next tick uses the `crew` pool.
pub const CREW_FROM: u32 = 64;
/// Below this number of awake chunks in the last tick, the next tick runs on the loop thread.
pub const SOLO_BELOW: u32 = 32;

/// From this number of chunks that a new view adds, the next tick uses the `crew` pool. One
/// thread makes a chunk in about 25 µs, so 256 chunks take about 6 ms. Smaller views stay on the
/// loop thread: with a busy CPU, the wait for the crew threads can be longer than that.
pub const CREW_FOR_NEW_VIEW: usize = 256;

/// The pools of the simulation thread. See the module documentation.
pub struct Workers {
    crew: Option<rayon::ThreadPool>,
    /// The awake chunks are many: ticks use the `crew` pool.
    parallel: bool,
    /// A command asked for a large tick (see `note`).
    large_next: bool,
    /// The last tick used the `crew` pool.
    on_crew: bool,
}

impl Workers {
    /// Look at a command before the simulation applies it. `view` is the view of the simulation
    /// now. A new view with many new chunks makes the next tick use the `crew` pool.
    pub fn note(&mut self, cmd: &Command, view: Option<CellRect>) {
        if let Command::SetView { area } = cmd {
            let old = view.unwrap_or_default();
            let new_chunks = area.chunks().filter(|c| c.cell_rect().intersect(&old).is_empty()).count();
            self.large_next |= new_chunks >= CREW_FOR_NEW_VIEW;
        }
    }

    /// Run one tick (`f`). `awake` is the number of awake chunks of the last tick.
    pub fn tick<R: Send>(&mut self, awake: u32, f: impl FnOnce() -> R + Send) -> R {
        self.parallel = if self.parallel { awake >= SOLO_BELOW } else { awake >= CREW_FROM };
        self.on_crew = self.parallel || std::mem::take(&mut self.large_next);
        self.big(self.on_crew, f)
    }

    /// Run `f` on the `crew` pool if `parallel` (for large jobs such as loading a world), else on
    /// this thread.
    pub fn big<R: Send>(&self, parallel: bool, f: impl FnOnce() -> R + Send) -> R {
        match &self.crew {
            Some(crew) if parallel => crew.install(f),
            _ => f(),
        }
    }

    /// True if the last tick used the `crew` pool.
    pub fn on_crew(&self) -> bool {
        self.on_crew
    }
}

/// Threads of the `crew` pool: the CPU cores less two (for the main thread and the loop thread),
/// at least 2 and at most 8.
fn crew_size() -> usize {
    let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
    cores.saturating_sub(2).clamp(2, 8)
}

/// Run the simulation loop `f` on the thread of a new one-thread pool, with a `crew` pool for
/// large ticks. Returns when `f` returns. If a pool cannot be made, `f` runs on this thread.
pub fn run<R: Send>(f: impl FnOnce(&mut Workers) -> R + Send) -> R {
    let builder = |n: usize, name: &'static str| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .thread_name(move |i| format!("{name}-{i}"))
            .start_handler(|_| set_interactive())
            .build()
            .map_err(|e| log::warn!("cannot make the {name} threads: {e}"))
            .ok()
    };
    let mut workers = Workers { crew: builder(crew_size(), "simulation-crew"), parallel: false, large_next: false, on_crew: false };
    match builder(1, "simulation-loop") {
        Some(solo) => solo.install(|| f(&mut workers)),
        None => f(&mut workers),
    }
}

/// Ask the system to run this thread before background work (macOS only).
pub fn set_interactive() {
    #[cfg(target_os = "macos")]
    {
        // From <pthread/qos.h>.
        const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;
        unsafe extern "C" {
            fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
        }
        // SAFETY: the function only changes the scheduling class of the calling thread.
        let r = unsafe { pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0) };
        if r != 0 {
            log::debug!("cannot set the thread service class: error {r}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_ticks_run_on_the_loop_thread_and_large_ticks_on_the_crew() {
        run(|w| {
            let here = std::thread::current().id();
            let on = |w: &mut Workers, awake: u32| w.tick(awake, || std::thread::current().id());
            assert_eq!(on(w, 3), here, "few awake chunks: the loop thread");
            assert_ne!(on(w, CREW_FROM), here, "many awake chunks: a crew thread");
            assert!(w.on_crew());
            assert_ne!(on(w, SOLO_BELOW), here, "stays on the crew until the work is small");
            assert_eq!(on(w, SOLO_BELOW - 1), here, "back on the loop thread");
            // A new view with many new chunks: the next tick only runs on the crew.
            let view = CellRect::new(0, 0, 640, 384);
            w.note(&Command::SetView { area: CellRect::new(64, 0, 704, 384) }, Some(view));
            assert_eq!(on(w, 3), here, "one new column of chunks: the loop thread");
            w.note(&Command::SetView { area: CellRect::new(0, 0, 2048, 1024) }, Some(view));
            assert_ne!(on(w, 3), here, "a zoom out: a crew thread");
            assert!(w.on_crew());
            assert_eq!(on(w, 3), here, "then the loop thread again");
            assert!(!w.on_crew());
            // A rayon call on the loop thread runs on the loop thread.
            use rayon::prelude::*;
            let ids: Vec<_> = (0..4).into_par_iter().map(|_| std::thread::current().id()).collect();
            assert!(ids.iter().all(|id| *id == here));
        });
    }
}
