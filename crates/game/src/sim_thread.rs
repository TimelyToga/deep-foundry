//! Runs the simulation on its own thread at a fixed 60 ticks per second.
//!
//! Each tick: apply all queued commands, call `advance`, then publish a snapshot to the mailbox.
//! The loop runs on its own worker threads (`sim_pool.rs`): a tick with little work runs on one
//! thread, so it does not wait for other threads.
//! The timing uses deadlines: tick N starts at `start + N × tick length`. If the thread falls behind,
//! it runs at most `MAX_CATCH_UP` ticks at once and then forgets the rest of the lost time.

use crate::factory_host::{self, FactoryFrame, FactoryHost, FactoryMailbox, GameCommand};
use crate::perf::SimPerf;
use crate::sim_pool::{self, Workers};
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
    commands: Sender<GameCommand>,
    mailbox: Arc<SnapshotMailbox>,
    /// The factory views (normal mode only).
    factory: Arc<FactoryMailbox>,
    stop: Arc<AtomicBool>,
    /// Measured ticks per second, as the bits of an f32.
    tps: Arc<AtomicU32>,
    handle: Option<JoinHandle<()>>,
}

impl SimThread {
    /// Start the thread. It owns the simulation from now on.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn start(sim: Simulation) -> Self {
        Self::start_with(sim, None)
    }

    /// Start the thread with the factory and the robot (the normal game mode). The factory ticks
    /// after each cell update, and its views go to `take_factory`.
    pub fn start_with(sim: Simulation, host: Option<FactoryHost>) -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        let mailbox = Arc::new(SnapshotMailbox::new());
        let factory = Arc::new(FactoryMailbox::default());
        let stop = Arc::new(AtomicBool::new(false));
        let tps = Arc::new(AtomicU32::new(0));
        let handle = {
            let (mailbox, factory, stop, tps) = (mailbox.clone(), factory.clone(), stop.clone(), tps.clone());
            std::thread::Builder::new()
                .name("simulation".into())
                .spawn(move || sim_pool::run(|w| run(w, sim, host, rx, &mailbox, &factory, &stop, &tps)))
                .expect("cannot start the simulation thread")
        };
        Self { commands: tx, mailbox, factory, stop, tps, handle: Some(handle) }
    }

    /// Queue a command. The simulation applies it at the start of the next tick.
    pub fn send(&self, cmd: impl Into<GameCommand>) {
        // An error means the thread has stopped. Nothing to do then.
        let _ = self.commands.send(cmd.into());
    }

    /// The newest factory views, if there are new ones (normal mode only).
    pub fn take_factory(&self) -> Option<FactoryFrame> {
        self.factory.take()
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

#[allow(clippy::too_many_arguments)]
fn run(
    workers: &mut Workers,
    mut sim: Simulation,
    mut host: Option<FactoryHost>,
    commands: Receiver<GameCommand>,
    mailbox: &SnapshotMailbox,
    factory: &FactoryMailbox,
    stop: &AtomicBool,
    tps: &AtomicU32,
) {
    let tick = Duration::from_secs_f64(TICK_SECONDS);
    let mut next = Instant::now();
    let mut count_start = Instant::now();
    let mut count = 0u32;
    // Messages for the player, sent with the next snapshot.
    let mut notices: Vec<String> = Vec::new();
    // Timing logs (`DEEP_FOUNDRY_PERF`).
    let mut perf = SimPerf::new();
    while !stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        if now < next {
            spin_sleep::sleep_until(next);
            continue;
        }
        let mut ran = 0;
        while ran < MAX_CATCH_UP && Instant::now() >= next {
            perf.begin(ran > 0);
            loop {
                match commands.try_recv() {
                    Ok(GameCommand::Sim(Command::SaveWorld { path })) => {
                        // `&mut`: a shared `Simulation` cannot go to another thread (it is not `Sync`).
                        let (sim, at) = (&mut sim, &path);
                        notices.push(workers.big(true, move || save_world(sim, at)));
                        factory_host::save_side(host.as_ref(), &path, &mut notices);
                    }
                    Ok(GameCommand::Sim(Command::LoadWorld { path })) => notices.push(workers.big(true, || load_world(&mut sim, &path))),
                    Ok(GameCommand::Sim(cmd)) => {
                        workers.note(&cmd, sim.view());
                        sim.apply(cmd);
                    }
                    Ok(GameCommand::Factory(cmd)) => {
                        if let Some(h) = host.as_mut() {
                            h.apply(cmd, &mut sim);
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            }
            perf.mark();
            let ticked = workers.tick(sim.stats().awake_chunks, || sim.advance());
            perf.mark();
            if ticked {
                count += 1;
                if let Some(h) = host.as_mut() {
                    h.tick(&mut sim);
                }
            }
            perf.mark();
            let mut snapshot = sim.take_snapshot();
            perf.mark();
            let frame = host.as_mut().map(|h| h.frame(&sim, snapshot.tick));
            perf.mark();
            perf.snapshot(&snapshot, ticked, workers.on_crew());
            if let Some(f) = frame {
                factory.publish(f);
            }
            snapshot.notices.append(&mut notices);
            mailbox.publish(snapshot);
            perf.end();
            next += tick;
            ran += 1;
        }
        // Still late after the catch-up ticks: forget the lost time.
        let now = Instant::now();
        if now > next + tick {
            perf.lost(now.duration_since(next));
            next = now;
        }
        perf.print_each_second();
        let since = now.duration_since(count_start);
        if since >= Duration::from_secs(1) {
            tps.store((count as f32 / since.as_secs_f32()).to_bits(), Ordering::Relaxed);
            count = 0;
            count_start = now;
        }
    }
}

fn file_name(path: &std::path::Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| path.display().to_string())
}

/// `Command::SaveWorld`. Returns the message for the player.
fn save_world(sim: &Simulation, path: &std::path::Path) -> String {
    if let Some(dir) = path.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        return format!("Save failed: cannot make the folder {}: {e}", dir.display());
    }
    match sim.save_file(path) {
        Ok(()) => format!("Game saved: {}", file_name(path)),
        Err(e) => format!("Save failed: {e}"),
    }
}

/// `Command::LoadWorld`. The new world replaces the old one only if the file loads.
/// Returns the message for the player.
fn load_world(sim: &mut Simulation, path: &std::path::Path) -> String {
    match Simulation::load_file_with_resolver(sim.content().clone(), &crate::demo::resolve_source, path) {
        Ok((loaded, report)) => {
            // The settings are the player's, not part of the world: keep them.
            let settings = sim.settings().clone();
            *sim = loaded;
            *sim.settings_mut() = settings;
            if report.unknown_materials.is_empty() {
                format!("Game loaded: {}", file_name(path))
            } else {
                format!("Game loaded: {}. Unknown materials became air: {}", file_name(path), report.unknown_materials.join(", "))
            }
        }
        Err(e) => format!("Load failed: {e}"),
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
    fn save_and_load_report_notices() {
        let dir = std::env::temp_dir().join(format!("deep-foundry-simthread-{}", std::process::id()));
        let path = dir.join("test.dfworld");
        let t = SimThread::start(small_sim());
        t.send(Command::SaveWorld { path: path.clone() });
        let s = wait_for(&t, |s| !s.notices.is_empty());
        assert_eq!(s.notices, vec!["Game saved: test".to_string()]);
        t.send(Command::LoadWorld { path: path.clone() });
        let s = wait_for(&t, |s| !s.notices.is_empty());
        assert_eq!(s.notices, vec!["Game loaded: test".to_string()]);
        t.send(Command::LoadWorld { path: dir.join("missing.dfworld") });
        let s = wait_for(&t, |s| !s.notices.is_empty());
        assert!(s.notices[0].starts_with("Load failed"), "{:?}", s.notices);
        let _ = std::fs::remove_dir_all(&dir);
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
