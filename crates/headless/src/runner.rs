//! Run scenes and their checks, and print the results as a table.

use crate::check::{Check, RunData};
use crate::paths;
use crate::scene::Scene;
use anyhow::Result;
use foundry_content::Content;
use foundry_core::CellPos;
use foundry_sim::Simulation;
use rayon::prelude::*;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

/// The result of one check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    Fail,
    /// A pending check that fails. This does not fail the test.
    PendingFail,
    /// A pending check that passes. The `pending` mark can probably go.
    PendingPass,
}

impl Outcome {
    pub fn label(self) -> &'static str {
        match self {
            Outcome::Pass => "pass",
            Outcome::Fail => "FAIL",
            Outcome::PendingFail => "pending",
            Outcome::PendingPass => "pending, passes",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CheckResult {
    /// What the check tests.
    pub what: String,
    pub outcome: Outcome,
    /// What was found and what was wanted.
    pub detail: String,
}

/// The results of one scene.
#[derive(Debug, Clone)]
pub struct SceneReport {
    pub name: String,
    pub ticks: u32,
    /// Mean time of one tick in milliseconds.
    pub ms_per_tick: f64,
    /// Set if the scene could not load or run. The scene then counts as failed.
    pub error: Option<String>,
    pub checks: Vec<CheckResult>,
}

impl SceneReport {
    /// True if the scene has an error or a non-pending check failed.
    pub fn failed(&self) -> bool {
        self.error.is_some() || self.checks.iter().any(|c| c.outcome == Outcome::Fail)
    }

    fn from_error(name: String, e: anyhow::Error) -> SceneReport {
        SceneReport { name, ticks: 0, ms_per_tick: 0.0, error: Some(format!("{e:#}")), checks: vec![] }
    }
}

/// Run a scene and its checks. `ticks: None` uses the scene's own tick count.
/// `after_tick(sim, tick)` is called after each tick (tick 1 is the first). Use it to save pictures.
/// Returns the simulation after the last tick and the report.
pub fn run_scene(
    scene: &Scene,
    content: &Arc<Content>,
    ticks: Option<u32>,
    after_tick: &mut dyn FnMut(&Simulation, u32),
) -> (Simulation, SceneReport) {
    let ticks = ticks.unwrap_or(scene.def.ticks);
    let checks = &scene.def.checks;
    let no_change_window =
        checks.iter().filter_map(|c| if let Check::NoChange { last_ticks, .. } = c { Some(*last_ticks) } else { None }).max();
    let every: Vec<u32> = checks
        .iter()
        .filter_map(|c| if let Check::Deterministic { every, .. } = c { Some(*every) } else { None })
        .collect();
    let record = |t: u32| {
        let for_no_change = no_change_window.is_some_and(|n| t + n >= ticks);
        let for_determinism = !every.is_empty() && (t == ticks || every.iter().any(|e| t.is_multiple_of(*e)));
        for_no_change || for_determinism
    };

    let mut sim = scene.build_sim(content.clone());
    let start_totals = material_totals(&sim);
    let mut hashes = vec![];
    let seconds = run_ticks(&mut sim, ticks, &record, &mut hashes, after_tick);

    // The second run for `Deterministic`: same scene, same seed, one thread.
    let mut second = vec![];
    let mut error = None;
    if !every.is_empty() {
        match hash_run(scene, content, ticks, &record, true) {
            Ok(h) => second = h,
            Err(e) => error = Some(format!("{e:#}")),
        }
    }

    let end_totals = material_totals(&sim);
    let data = RunData {
        sim: &sim,
        origin: scene.origin,
        ticks,
        start_totals: &start_totals,
        end_totals: &end_totals,
        last_hashes: &hashes,
        first_run: &hashes,
        second_run: &second,
    };
    let results = checks
        .iter()
        .map(|c| {
            let (ok, detail) = c.evaluate(&data, content);
            let outcome = match (c.pending(), ok) {
                (false, true) => Outcome::Pass,
                (false, false) => Outcome::Fail,
                (true, true) => Outcome::PendingPass,
                (true, false) => Outcome::PendingFail,
            };
            CheckResult { what: c.describe(), outcome, detail }
        })
        .collect();
    let report = SceneReport {
        name: scene.name.clone(),
        ticks,
        ms_per_tick: if ticks > 0 { seconds * 1000.0 / ticks as f64 } else { 0.0 },
        error,
        checks: results,
    };
    (sim, report)
}

/// Build the scene and run `ticks` ticks. Returns (tick, world hash) for each tick in `0..=ticks`
/// where `record(tick)` is true. Tick 0 is the state before the first tick. With `one_thread`, the run uses a rayon pool with a single thread,
/// so any parallel work in the simulation runs on one thread.
pub fn hash_run(
    scene: &Scene,
    content: &Arc<Content>,
    ticks: u32,
    record: &(dyn Fn(u32) -> bool + Sync),
    one_thread: bool,
) -> Result<Vec<(u32, u64)>> {
    let run = || {
        let mut sim = scene.build_sim(content.clone());
        let mut hashes = vec![];
        run_ticks(&mut sim, ticks, record, &mut hashes, &mut |_, _| {});
        hashes
    };
    if one_thread {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(1).build()?;
        Ok(pool.install(run))
    } else {
        Ok(run())
    }
}

/// Run `ticks` ticks. Push (tick, world hash) for each tick in `0..=ticks` where `record(tick)` is true.
/// Returns the time of the ticks in seconds (hashing and `after_tick` are not counted).
fn run_ticks(
    sim: &mut Simulation,
    ticks: u32,
    record: &(dyn Fn(u32) -> bool + Sync),
    hashes: &mut Vec<(u32, u64)>,
    after_tick: &mut dyn FnMut(&Simulation, u32),
) -> f64 {
    if record(0) {
        hashes.push((0, sim.world_hash()));
    }
    let mut seconds = 0.0;
    for t in 1..=ticks {
        let start = Instant::now();
        sim.tick();
        seconds += start.elapsed().as_secs_f64();
        if record(t) {
            hashes.push((t, sim.world_hash()));
        }
        after_tick(sim, t);
    }
    seconds
}

/// The number of cells of each material in the whole world. Index: material id.
pub fn material_totals(sim: &Simulation) -> Vec<u64> {
    let mut totals = vec![0u64; sim.content().materials.len()];
    let (w, h) = sim.size_cells();
    for y in 0..h {
        for x in 0..w {
            totals[sim.cell(CellPos::new(x, y)).material.index()] += 1;
        }
    }
    totals
}

/// Load and run one scene file. A load error becomes a failed report.
pub fn run_scene_file(path: &Path, content: &Arc<Content>) -> SceneReport {
    match Scene::load(path, content) {
        Ok(scene) => run_scene(&scene, content, None, &mut |_, _| {}).1,
        Err(e) => SceneReport::from_error(paths::stem(path), e),
    }
}

/// Run every scene in `assets/scenes/tests/` whose name contains `filter`. Scenes run in parallel.
/// The reports are in name order.
pub fn run_tests(content: &Arc<Content>, filter: Option<&str>) -> Result<Vec<SceneReport>> {
    let files: Vec<_> = paths::ron_files(&paths::test_scenes_dir())?
        .into_iter()
        .filter(|p| filter.is_none_or(|f| paths::stem(p).contains(f)))
        .collect();
    Ok(files.par_iter().map(|p| run_scene_file(p, content)).collect())
}

/// Totals over a list of reports.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Summary {
    pub scenes: usize,
    pub failed_scenes: usize,
    pub pass: usize,
    pub fail: usize,
    pub pending: usize,
    pub pending_passes: usize,
}

pub fn summarize(reports: &[SceneReport]) -> Summary {
    let mut s = Summary { scenes: reports.len(), ..Default::default() };
    for r in reports {
        if r.failed() {
            s.failed_scenes += 1;
        }
        for c in &r.checks {
            match c.outcome {
                Outcome::Pass => s.pass += 1,
                Outcome::Fail => s.fail += 1,
                Outcome::PendingFail => s.pending += 1,
                Outcome::PendingPass => {
                    s.pending += 1;
                    s.pending_passes += 1
                }
            }
        }
    }
    s
}

/// The results as a text table: one block for each scene, one row for each check.
pub fn format_table(reports: &[SceneReport]) -> String {
    let what_w = reports.iter().flat_map(|r| r.checks.iter().map(|c| c.what.len())).max().unwrap_or(0).max(5);
    let mut out = String::new();
    for r in reports {
        let status = if r.failed() { "FAIL" } else { "ok" };
        let _ = writeln!(out, "{status:<4}  {} ({} ticks, {:.3} ms per tick)", r.name, r.ticks, r.ms_per_tick);
        if let Some(e) = &r.error {
            let _ = writeln!(out, "      {:<15}  {e}", "FAIL");
        }
        for c in &r.checks {
            let _ = writeln!(out, "      {:<15}  {:<what_w$}  {}", c.outcome.label(), c.what, c.detail);
        }
    }
    let s = summarize(reports);
    let _ = write!(
        out,
        "\n{} scenes ({} failed), {} checks: {} pass, {} fail, {} pending",
        s.scenes,
        s.failed_scenes,
        s.pass + s.fail + s.pending,
        s.pass,
        s.fail,
        s.pending
    );
    if s.pending_passes > 0 {
        let _ = write!(out, " ({} pending checks pass now; remove their `pending: true`)", s.pending_passes);
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::{Image, color};
    use crate::scene::ron_options;

    /// A scene from code: one sand cell at (3, 0) above a stone floor (row 7), 8 x 8 pixels.
    fn small_scene(checks: &str, ticks: u32) -> (Scene, Arc<Content>) {
        let content = Arc::new(Content::load_default().unwrap());
        let mut img = Image::new(8, 8);
        img.fill_rect(0, 7, 8, 1, color("#6b6a70"));
        img.set(3, 0, color("#d9c38c"));
        let text = format!(
            r##"Scene(legend: {{ "#d9c38c": "sand", "#6b6a70": "stone" }}, ticks: {ticks}, checks: [{checks}])"##
        );
        let def = ron_options().from_str(&text).unwrap();
        let scene =
            Scene::from_parts("small".into(), "small.ron".into(), "small.png".into(), def, &img, &content).unwrap();
        (scene, content)
    }

    fn outcomes(checks: &str, ticks: u32) -> Vec<Outcome> {
        let (scene, content) = small_scene(checks, ticks);
        run_scene(&scene, &content, None, &mut |_, _| {}).1.checks.iter().map(|c| c.outcome).collect()
    }

    #[test]
    fn checks_pass_and_fail() {
        let got = outcomes(
            r#"
            Count(material: "sand", rect: (x: 0, y: 6, w: 8, h: 1), exact: 1),
            Count(material: "sand", rect: (x: 0, y: 0, w: 8, h: 1), min: 1),
            CellIs(at: (x: 3, y: 6), material: "sand"),
            CellIs(at: (x: 3, y: 0), material: "sand"),
            Total(material: "sand", min: 1, max: 1),
            TotalUnchanged(material: "sand"),
            Temperature(material: "stone", of: Each, min: 15, max: 25),
            Temperature(material: "sand", min: 100),
            Temperature(rect: (x: 0, y: 0, w: 8, h: 8), max: 25),
            NoChange(last_ticks: 10),
            Deterministic(every: 7),
            "#,
            60,
        );
        use Outcome::*;
        assert_eq!(got, vec![Pass, Fail, Pass, Fail, Pass, Pass, Pass, Fail, Pass, Pass, Pass]);
    }

    #[test]
    fn no_change_fails_while_sand_falls() {
        assert_eq!(outcomes("NoChange(last_ticks: 3)", 4), vec![Outcome::Fail]);
        assert_eq!(outcomes("NoChange(last_ticks: 30)", 10), vec![Outcome::Fail], "window longer than the run");
    }

    #[test]
    fn pending_checks_do_not_fail_the_scene() {
        let (scene, content) = small_scene(
            r#"Total(material: "sand", exact: 5, pending: true), Total(material: "sand", exact: 1, pending: true)"#,
            5,
        );
        let report = run_scene(&scene, &content, None, &mut |_, _| {}).1;
        assert_eq!(report.checks[0].outcome, Outcome::PendingFail);
        assert_eq!(report.checks[1].outcome, Outcome::PendingPass);
        assert!(!report.failed());
        let s = summarize(std::slice::from_ref(&report));
        assert_eq!((s.pass, s.fail, s.pending, s.pending_passes), (0, 0, 2, 1));
        assert!(format_table(&[report]).contains("pending, passes"));
    }

    #[test]
    fn after_tick_sees_every_tick() {
        let (scene, content) = small_scene("", 5);
        let mut seen = vec![];
        let (sim, report) = run_scene(&scene, &content, Some(7), &mut |_, t| seen.push(t));
        assert_eq!(seen, vec![1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(report.ticks, 7);
        assert_eq!(sim.tick_count(), 7);
    }
}
