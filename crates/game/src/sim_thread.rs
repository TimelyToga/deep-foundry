//! Runs the simulation on its own thread at a fixed 60 ticks per second.
//!
//! Each tick: apply all queued commands, call `advance`, then publish a snapshot to the mailbox.
//! The timing uses deadlines: tick N starts at `start + N × tick length`. If the thread falls behind,
//! it runs at most `MAX_CATCH_UP` ticks at once and then forgets the rest of the lost time.

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use foundry_core::{Command, Snapshot, SnapshotMailbox, TICK_SECONDS};
use foundry_sim::Simulation;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The most ticks in a row when the thread is late.
const MAX_CATCH_UP: u32 = 3;

pub struct SimThread {
    commands: Sender<Command>,
    mailbox: Arc<SnapshotMailbox>,
    stop: Arc<AtomicBool>,
    /// Measured ticks per second, as the bits of an f32.
    tps: Arc<AtomicU32>,
    handle: Option<JoinHandle<()>>,
}

impl SimThread {
    /// Start the thread. It owns the simulation from now on.
    pub fn start(sim: Simulation) -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        let mailbox = Arc::new(SnapshotMailbox::new());
        let stop = Arc::new(AtomicBool::new(false));
        let tps = Arc::new(AtomicU32::new(0));
        let handle = {
            let (mailbox, stop, tps) = (mailbox.clone(), stop.clone(), tps.clone());
            std::thread::Builder::new()
                .name("simulation".into())
                .spawn(move || run(sim, rx, &mailbox, &stop, &tps))
                .expect("cannot start the simulation thread")
        };
        Self { commands: tx, mailbox, stop, tps, handle: Some(handle) }
    }

    /// Queue a command. The simulation applies it at the start of the next tick.
    pub fn send(&self, cmd: Command) {
        // An error means the thread has stopped. Nothing to do then.
        let _ = self.commands.send(cmd);
    }

    /// The newest snapshot, if there is a new one.
    pub fn take_snapshot(&self) -> Option<Snapshot> {
        self.mailbox.take()
    }

    /// Ticks per second over the last second.
    pub fn ticks_per_second(&self) -> f32 {
        f32::from_bits(self.tps.load(Ordering::Relaxed))
    }

    /// Stop the thread and wait for it.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take()
            && h.join().is_err()
        {
            log::error!("the simulation thread panicked");
        }
    }
}

impl Drop for SimThread {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run(
    mut sim: Simulation,
    commands: Receiver<Command>,
    mailbox: &SnapshotMailbox,
    stop: &AtomicBool,
    tps: &AtomicU32,
) {
    let tick = Duration::from_secs_f64(TICK_SECONDS);
    let mut next = Instant::now();
    let mut count_start = Instant::now();
    let mut count = 0u32;
    while !stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        if now < next {
            spin_sleep::sleep_until(next);
            continue;
        }
        let mut ran = 0;
        while ran < MAX_CATCH_UP && Instant::now() >= next {
            loop {
                match commands.try_recv() {
                    Ok(cmd) => sim.apply(cmd),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            }
            if sim.advance() {
                count += 1;
            }
            mailbox.publish(sim.take_snapshot());
            next += tick;
            ran += 1;
        }
        // Still late after the catch-up ticks: forget the lost time.
        let now = Instant::now();
        if now > next + tick {
            next = now;
        }
        let since = now.duration_since(count_start);
        if since >= Duration::from_secs(1) {
            tps.store((count as f32 / since.as_secs_f32()).to_bits(), Ordering::Relaxed);
            count = 0;
            count_start = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundry_content::Content;
    use foundry_core::{CellPos, CellRect, PaintMode};
    use foundry_sim::SimConfig;

    fn small_sim() -> Simulation {
        let content = Arc::new(Content::load_default().unwrap());
        Simulation::new(content, SimConfig::finite(2, 2, 1))
    }

    /// Wait until a snapshot matches, or fail after 2 seconds.
    fn wait_for(t: &SimThread, mut f: impl FnMut(&Snapshot) -> bool) -> Snapshot {
        let end = Instant::now() + Duration::from_secs(2);
        while Instant::now() < end {
            if let Some(s) = t.take_snapshot()
                && f(&s)
            {
                return s;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("no matching snapshot in time");
    }

    #[test]
    fn runs_ticks_and_applies_commands() {
        let sim = small_sim();
        let sand = sim.content().expect_material("sand");
        let mut t = SimThread::start(sim);
        t.send(Command::SetView { area: CellRect::new(0, 0, 128, 128) });
        t.send(Command::Paint {
            center: CellPos::new(64, 20),
            radius: 3,
            material: sand,
            mode: PaintMode::Replace,
            temperature: None,
        });
        let s = wait_for(&t, |s| s.tick >= 5);
        assert!(!s.paused);
        // All four chunks arrived at some point (the first snapshot has them all).
        t.send(Command::SetPaused(true));
        let paused = wait_for(&t, |s| s.paused);
        std::thread::sleep(Duration::from_millis(100));
        let later = wait_for(&t, |_| true);
        assert_eq!(later.tick, paused.tick, "no ticks while paused");
        t.send(Command::Step);
        wait_for(&t, |s| s.tick == paused.tick + 1);
        t.stop();
    }

    #[test]
    fn rate_is_about_60_per_second() {
        let t = SimThread::start(small_sim());
        let first = wait_for(&t, |_| true).tick;
        std::thread::sleep(Duration::from_millis(500));
        let last = wait_for(&t, |_| true).tick;
        let n = last - first;
        assert!((25..=35).contains(&n), "{n} ticks in 0.5 s");
    }
}
