//! The headless program: scene tests, benchmarks, and PNG pictures of the cell world.
//! Run `cargo run -p foundry_headless --release -- help`.

use anyhow::{Context, Result, bail};
use foundry_content::Content;
use foundry_core::CellRect;
use foundry_headless::bench::{self, BENCHES, Baseline};
use foundry_headless::image::{render_cells, render_heat};
use foundry_headless::scene::Scene;
use foundry_headless::{paths, runner};
use foundry_sim::Simulation;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

const HELP: &str = "\
foundry_headless: scene tests, benchmarks and pictures, with no window.

Usage: cargo run -p foundry_headless --release -- <command>

Commands:
  scene <name> [--ticks N] [--png out.png] [--scale S] [--every K] [--heat]
      Run one scene and its checks. Write a PNG of the final cells
      (default: out/<name>.png). --every K also writes a PNG every K ticks.
      --heat draws temperatures in place of materials.
  test [filter]
      Run every scene in assets/scenes/tests/ whose name contains the filter.
      Exit code 1 if a check that is not pending fails.
  bench [name] [--ticks N] [--repeat N] [--save-baseline] [--compare] [--png out.png]
      Run the benchmarks (all, or one by name). Print ms per tick.
      --png writes a picture of the world after the last tick (one benchmark only).
      --repeat N runs each benchmark N times and keeps the run with the
      lowest mean (default: 3 with --save-baseline or --compare, else 1).
      --save-baseline writes bench/baseline.ron.
      --compare fails if a mean is more than 10% above the baseline.
  determinism <scene> [--ticks N] [--every K]
      Run a scene twice (the second time on one thread). Compare world
      hashes every K ticks (default 100).
  help
      Show this text.

<name> is a scene name in assets/scenes/ or assets/scenes/tests/, or a path to a .ron file.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
    }
}

/// Returns Ok(false) if a test or comparison failed.
fn run(args: &[String]) -> Result<bool> {
    let Some((command, rest)) = args.split_first() else {
        print!("{HELP}");
        return Ok(true);
    };
    match command.as_str() {
        "scene" => cmd_scene(&Args::parse(rest, &["--ticks", "--png", "--scale", "--every"], &["--heat"])?),
        "test" => cmd_test(&Args::parse(rest, &[], &[])?),
        "bench" => cmd_bench(&Args::parse(rest, &["--ticks", "--repeat", "--png"], &["--save-baseline", "--compare"])?),
        "determinism" => cmd_determinism(&Args::parse(rest, &["--ticks", "--every"], &[])?),
        "help" | "--help" | "-h" => {
            print!("{HELP}");
            Ok(true)
        }
        other => bail!("unknown command `{other}`. Run `help` for the list."),
    }
}

/// Command line arguments after the command name.
struct Args {
    positional: Vec<String>,
    values: Vec<(String, String)>,
    switches: Vec<String>,
}

impl Args {
    fn parse(args: &[String], with_value: &[&str], switches: &[&str]) -> Result<Args> {
        let mut a = Args { positional: vec![], values: vec![], switches: vec![] };
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            if with_value.contains(&arg.as_str()) {
                let v = it.next().with_context(|| format!("{arg} needs a value"))?;
                a.values.push((arg.clone(), v.clone()));
            } else if switches.contains(&arg.as_str()) {
                a.switches.push(arg.clone());
            } else if arg.starts_with("--") {
                bail!("unknown option `{arg}`. Run `help` for the list.");
            } else {
                a.positional.push(arg.clone());
            }
        }
        Ok(a)
    }

    fn value(&self, name: &str) -> Option<&str> {
        self.values.iter().rev().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    fn number(&self, name: &str) -> Result<Option<u32>> {
        self.value(name).map(|v| v.parse().with_context(|| format!("{name}: `{v}` is not a whole number"))).transpose()
    }

    fn switch(&self, name: &str) -> bool {
        self.switches.iter().any(|s| s == name)
    }
}

fn load_content() -> Result<Arc<Content>> {
    Ok(Arc::new(Content::load_default().context("cannot load the data files")?))
}

fn cmd_scene(a: &Args) -> Result<bool> {
    let name = a.positional.first().context("usage: scene <name> [--ticks N] [--png out.png] [--scale S]")?;
    let content = load_content()?;
    let scene = Scene::find(name, &content)?;
    let heat = a.switch("--heat");
    let png = match a.value("--png") {
        Some(p) => PathBuf::from(p),
        None => paths::out_dir().join(format!("{}{}.png", scene.name, if heat { "_heat" } else { "" })),
    };
    let area = CellRect::new(0, 0, scene.world_chunks.0 * 64, scene.world_chunks.1 * 64);
    // Default scale: the largest whole number that keeps the picture at or below 1024 pixels (1 to 8).
    let scale = a.number("--scale")?.unwrap_or((1024 / area.width().max(area.height()).max(1) as u32).clamp(1, 8));
    let every = a.number("--every")?.filter(|k| *k > 0);
    let draw = |sim: &Simulation| if heat { render_heat(sim, area, scale) } else { render_cells(sim, area, scale) };

    println!(
        "scene {}: {} x {} image at ({}, {}) in a {} x {} chunk world, {} threads",
        scene.name,
        scene.width,
        scene.height,
        scene.origin.x,
        scene.origin.y,
        scene.world_chunks.0,
        scene.world_chunks.1,
        rayon::current_num_threads()
    );
    let mut frame_error = None;
    let mut after_tick = |sim: &Simulation, t: u32| {
        if let Some(k) = every
            && t.is_multiple_of(k)
            && let Err(e) = draw(sim).save_png(&frame_path(&png, t))
        {
            frame_error.get_or_insert(e);
        }
    };
    let (sim, report) = runner::run_scene(&scene, &content, a.number("--ticks")?, &mut after_tick);
    if let Some(e) = frame_error {
        return Err(e);
    }
    draw(&sim).save_png(&png)?;
    print!("{}", runner::format_table(std::slice::from_ref(&report)));
    println!("picture: {} (scale {scale})", png.display());
    if let Some(k) = every {
        println!("frames: {} every {k} ticks", frame_path(&png, 0).display().to_string().replace("000000", "<tick>"));
    }
    Ok(!report.failed())
}

/// `out/x.png` -> `out/x_t000100.png`.
fn frame_path(png: &Path, tick: u32) -> PathBuf {
    let stem = png.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    png.with_file_name(format!("{stem}_t{tick:06}.png"))
}

fn cmd_test(a: &Args) -> Result<bool> {
    let content = load_content()?;
    let filter = a.positional.first().map(String::as_str);
    let reports = runner::run_tests(&content, filter)?;
    if reports.is_empty() {
        bail!("no scene in {} matches `{}`", paths::test_scenes_dir().display(), filter.unwrap_or(""));
    }
    print!("{}", runner::format_table(&reports));
    Ok(!reports.iter().any(|r| r.failed()))
}

fn cmd_bench(a: &Args) -> Result<bool> {
    let list: Vec<&bench::Bench> = match a.positional.first() {
        Some(n) => {
            let names: Vec<&str> = BENCHES.iter().map(|b| b.name).collect();
            vec![bench::find(n).with_context(|| format!("no benchmark `{n}`. Benchmarks: {}", names.join(", ")))?]
        }
        None => BENCHES.iter().collect(),
    };
    let (save, compare) = (a.switch("--save-baseline"), a.switch("--compare"));
    if cfg!(debug_assertions) {
        if save || compare {
            bail!("baselines need a release build: add --release after `cargo run`");
        }
        eprintln!("warning: this is a debug build. Use --release for real numbers.");
    }
    let content = load_content()?;
    let ticks = a.number("--ticks")?;
    let repeat = a.number("--repeat")?.unwrap_or(if save || compare { 3 } else { 1 }).max(1);
    println!("{} threads, best of {repeat} run(s) for each benchmark", rayon::current_num_threads());
    println!(
        "{:<14} {:>6} {:>9} {:>8} {:>8} {:>8} {:>13} {:>9}",
        "benchmark", "ticks", "mean ms", "p50 ms", "p95 ms", "max ms", "awake (mean)", "with mat"
    );
    if a.value("--png").is_some() && list.len() != 1 {
        bail!("--png needs one benchmark name");
    }
    let mut results = vec![];
    let mut picture = None;
    for b in list {
        // Other programs on the machine make some runs slow. The fastest run is the most exact.
        let (r, sim) = (0..repeat)
            .map(|_| b.run(&content, ticks.unwrap_or(b.ticks)))
            .min_by(|x, y| x.0.mean_ms.total_cmp(&y.0.mean_ms))
            .expect("repeat is at least 1");
        if let Some(png) = a.value("--png") {
            let (w, h) = sim.size_cells();
            let scale = (1024 / w.max(h).max(1) as u32).clamp(1, 8);
            render_cells(&sim, CellRect::new(0, 0, w, h), scale).save_png(Path::new(png))?;
            picture = Some(png);
        }
        println!(
            "{:<14} {:>6} {:>9.3} {:>8.3} {:>8.3} {:>8.3} {:>13.1} {:>9}",
            b.name, r.ticks, r.mean_ms, r.p50_ms, r.p95_ms, r.max_ms, r.awake_mean, r.chunks_with_material
        );
        results.push((b.name, r));
    }
    println!("(awake = SimStats::awake_chunks. with mat = chunks with any cell that is not air, after the last tick.)");
    if let Some(png) = picture {
        println!("picture: {png}");
    }

    let path = paths::baseline_path();
    let mut ok = true;
    if compare {
        let base = Baseline::load(&path)?.with_context(|| format!("no baseline at {}", path.display()))?;
        println!("\ncompared with {} (made on: {}):", path.display(), base.machine);
        if base.machine != bench::machine() {
            println!("warning: the baseline is from another machine ({}). Save a new baseline here first.", base.machine);
        }
        for c in bench::compare(&results, &base) {
            let verdict = match (&c.skipped, c.slower) {
                (Some(why), _) => format!("skipped: {why}"),
                (None, true) => "SLOWER".to_string(),
                (None, false) => "ok".to_string(),
            };
            let change = c.change_percent().map_or(String::new(), |p| format!("{p:+.1}%"));
            let base_ms = c.baseline_ms.map_or("-".to_string(), |b| format!("{b:.3}"));
            println!("{:<14} {:>9} -> {:>9.3} ms {:>8}  {verdict}", c.name, base_ms, c.mean_ms, change);
            ok &= !c.slower;
        }
    }
    if save {
        let mut base = Baseline::load(&path)?.unwrap_or_default();
        if base.machine != bench::machine() {
            // Numbers from two machines must not mix in one file.
            base.benches.clear();
        }
        base.machine = bench::machine();
        for (name, r) in results {
            base.benches.insert(name.to_string(), r);
        }
        base.save(&path)?;
        println!("saved {}", path.display());
    }
    Ok(ok)
}

fn cmd_determinism(a: &Args) -> Result<bool> {
    let name = a.positional.first().context("usage: determinism <scene> [--ticks N] [--every K]")?;
    let content = load_content()?;
    let scene = Scene::find(name, &content)?;
    let ticks = a.number("--ticks")?.unwrap_or(scene.def.ticks);
    let every = a.number("--every")?.unwrap_or(100).max(1);
    let record = |t: u32| t.is_multiple_of(every) || t == ticks;
    println!("scene {}: {ticks} ticks, first run on {} threads, second run on 1 thread", scene.name, rayon::current_num_threads());
    let first = runner::hash_run(&scene, &content, ticks, &record, false)?;
    let second = runner::hash_run(&scene, &content, ticks, &record, true)?;
    let mut same = true;
    for ((t, a), (_, b)) in first.iter().zip(&second) {
        let verdict = if a == b { "same" } else { "DIFFERENT" };
        same &= a == b;
        println!("tick {t:>6}  {a:016x}  {b:016x}  {verdict}");
    }
    println!("{}", if same { "deterministic: yes" } else { "deterministic: NO" });
    Ok(same)
}
