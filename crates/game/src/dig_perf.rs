//! A repeatable measure of the normal game while the robot digs and walks.
//!
//! Run it with:
//!
//! ```sh
//! cargo test --release -p deep_foundry dig_perf -- --ignored --nocapture
//! ```
//!
//! It builds the real demo world (no side limit, depth 128) and a normal game, with the camera of
//! the default window on a Retina display (3200 x 1800 pixels, zoom 6). Then it plays the dig
//! script of `perf.rs` for 10 seconds at 60 ticks per second: dig down, dig right while walking
//! right, dig left while walking left. The mouse moves each frame.
//!
//! Each loop is one tick and one frame, in the same order as the two threads of the game:
//! - simulation thread: commands, cell update, factory tick, snapshot, factory frame;
//! - main thread: take the frame, fill the UI model, run egui (UI and overlay), tessellate.
//!
//! It runs the script two times: first with the cell update called from a thread outside the
//! rayon pool (as the simulation thread did before `sim_pool.rs`), then on the pools of
//! `sim_pool.rs`. It prints the time of each part (average, median, 99th percentile, worst).
//! The waits for pool threads are long only when the CPU is busy, so run it also while other
//! programs use all cores (for example `yes > /dev/null` once per core). There is no GPU here, so
//! chunk uploads and drawing are not measured.

use crate::controls::CameraControl;
use crate::demo::{self, Shape};
use crate::factory_host::{FactoryHost, GameCommand};
use crate::normal::NormalMode;
use crate::overlay;
use crate::sim_pool::{self, Workers};
use crate::ui::SandboxUi;
use foundry_content::Content;
use foundry_core::{CellPos, Command, TICK_SECONDS};
use foundry_factory::Guide;
use foundry_ui::{GameMode, GameState};
use glam::{DVec2, UVec2};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Extra cells around the screen that the game asks for (as `app.rs`).
const VIEW_MARGIN: i32 = 64;
/// Screen size in pixels, and pixels per UI point (a Retina display).
const SCREEN: (u32, u32) = (3200, 1800);
const PPP: f32 = 2.0;
const ZOOM: f32 = 6.0;
/// Ticks (and frames) of the script.
const TICKS: u32 = 600;

/// Times of one part, in milliseconds.
struct Samples {
    name: &'static str,
    ms: Vec<f64>,
}

impl Samples {
    fn new(name: &'static str) -> Self {
        Self { name, ms: Vec::new() }
    }

    fn add(&mut self, since: Instant) -> Instant {
        let now = Instant::now();
        self.ms.push(now.duration_since(since).as_secs_f64() * 1000.0);
        now
    }

    fn print(&self) {
        let mut v = self.ms.clone();
        v.sort_by(f64::total_cmp);
        let pct = |p: f64| v.get(((v.len().saturating_sub(1)) as f64 * p).round() as usize).copied().unwrap_or(0.0);
        let late = v.iter().filter(|&&ms| ms > TICK_SECONDS * 1000.0).count();
        println!(
            "  {:<20} average {:>6.3} ms  median {:>6.3}  99% {:>6.3}  worst {:>7.3}  over 16.7 ms: {late}",
            self.name,
            v.iter().sum::<f64>() / v.len().max(1) as f64,
            pct(0.5),
            pct(0.99),
            pct(1.0)
        );
    }
}

/// Play the dig script once. `workers`: run the cell update on these pools (`None`: call it from
/// this thread, which is outside every rayon pool).
fn measure(mut workers: Option<&mut Workers>) {
    let content = Arc::new(Content::load_default().unwrap());
    let guide = Arc::new(Guide::load_default().unwrap());
    let d = demo::build(content.clone(), Shape::Infinite { depth_chunks: 128 }, 1);
    let mut sim = d.sim;
    let mut host = FactoryHost::new_game(content.clone(), guide, &mut sim, d.start_center.0 as i32).unwrap();
    let (w, h) = sim.size_cells();
    let (x, y) = host.robot.center();
    let mut controls = CameraControl::new(DVec2::new(x as f64, y as f64), ZOOM, UVec2::new(SCREEN.0, SCREEN.1), DVec2::new(w as f64, h as f64));

    let ctx = egui::Context::default();
    let mut ui = SandboxUi::new(&ctx, content.clone(), std::env::temp_dir().join("deep-foundry-dig-perf"));
    ui.set_mode(GameMode::Normal);
    ui.model.state = GameState::Playing;
    let mut n = NormalMode::new();

    let mut last_view = None;
    let mut commands: Vec<GameCommand> = Vec::new();
    let mut parts = [
        Samples::new("sim: commands"),
        Samples::new("sim: cell update"),
        Samples::new("sim: factory tick"),
        Samples::new("sim: snapshot"),
        Samples::new("sim: factory frame"),
        Samples::new("main: take frame"),
        Samples::new("main: fill model"),
        Samples::new("main: egui"),
        Samples::new("main: tessellate"),
    ];
    let mut sim_total = Samples::new("sim thread total");
    let mut main_total = Samples::new("main thread total");
    let mut chunks_sent = 0usize;
    let mut dug_ticks = 0u32;
    // Warm up: the robot lands, the first view is made.
    for _ in 0..30 {
        sim.tick();
        host.tick(&mut sim);
    }
    let tick_length = Duration::from_secs_f64(TICK_SECONDS);
    let mut next = Instant::now();
    for frame in 0..TICKS {
        // 60 ticks per second, as the game: between two ticks the pool threads go to sleep.
        next += tick_length;
        spin_sleep::sleep_until(next);
        // ---- simulation thread ----
        let t_sim = Instant::now();
        let mut t = t_sim;
        for c in commands.drain(..) {
            match c {
                GameCommand::Sim(c) => sim.apply(c),
                GameCommand::Factory(c) => host.apply(c, &mut sim),
            }
        }
        t = parts[0].add(t);
        match workers.as_mut() {
            Some(w) => w.tick(sim.stats().awake_chunks, || sim.advance()),
            None => sim.advance(),
        };
        t = parts[1].add(t);
        host.tick(&mut sim);
        t = parts[2].add(t);
        let snapshot = sim.take_snapshot();
        t = parts[3].add(t);
        let factory_frame = host.frame(&sim, snapshot.tick);
        parts[4].add(t);
        sim_total.add(t_sim);
        chunks_sent += snapshot.chunks.len();
        dug_ticks += factory_frame.digging as u32;

        // ---- main thread ----
        let t_main = Instant::now();
        let now = t_main;
        let mut t = t_main;
        n.take_frame(factory_frame, now);
        t = parts[5].add(t);
        // The camera follows the robot (as `follow_robot`).
        if let Some((x, y)) = n.robot_pos(now) {
            let target = DVec2::new(x as f64 + 4.0, y as f64);
            let c = &mut controls.camera.center;
            *c += (target - *c) * (1.0 - (-TICK_SECONDS * 12.0).exp());
        }
        n.fill_model(&mut ui.model);
        ui.model.hover_detail = n.hover_detail(&content, ui.model.hover.as_ref());
        t = parts[6].add(t);
        // The mouse and the keys of the script.
        let ((dx, dy), walk) = crate::perf::dig_script_at(frame as f32 * TICK_SECONDS as f32);
        let (cx, cy) = n.frame.robot.map(|r| r.center()).unwrap_or((0.0, 0.0));
        let mouse = CellPos::new((cx + dx).floor() as i32, (cy + dy).floor() as i32);
        n.held.dig = true;
        n.held.right = walk > 0;
        n.held.left = walk < 0;
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(SCREEN.0 as f32, SCREEN.1 as f32) / PPP)),
            ..Default::default()
        };
        raw.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(PPP);
        let camera = controls.camera;
        let build = n.build_view(&content, Some(mouse));
        let out = ctx.run_ui(raw, |root| {
            let _ = ui.ui.show(root.ctx(), &ui.model);
            let painter = root.ctx().layer_painter(egui::LayerId::background());
            let scene = overlay::Scene {
                camera: &camera,
                ppp: root.ctx().pixels_per_point(),
                robot_at: n.robot_pos(now),
                build: &build,
                content: &content,
                atlas: ui.ui.atlas(),
            };
            overlay::draw(&painter, &n.frame, &scene);
        });
        t = parts[7].add(t);
        let _jobs = ctx.tessellate(out.shapes, out.pixels_per_point);
        parts[8].add(t);
        // The input for the next tick, and the view (as `send_normal_input` and `frame`).
        commands.extend(n.update(mouse));
        if let Some(c) = n.input(&content, mouse, controls.camera.visible_rect()) {
            commands.push(c);
        }
        if let Some(c) = n.windows(false, false) {
            commands.push(c);
        }
        let view = controls.view_area(VIEW_MARGIN);
        if last_view != Some(view) {
            commands.push(Command::SetView { area: view }.into());
            last_view = Some(view);
        }
        main_total.add(t_main);
    }
    println!("  {TICKS} ticks and frames, {dug_ticks} ticks dug, {chunks_sent} chunk images sent");
    for p in &parts {
        p.print();
    }
    sim_total.print();
    main_total.print();
}

#[test]
#[ignore = "a measure, not a check: run it with --release --ignored --nocapture"]
fn dig_perf() {
    println!("--- dig_perf: cell update called from outside the pool (as before sim_pool.rs) ---");
    measure(None);
    println!("--- dig_perf: cell update on the pools of sim_pool.rs ---");
    sim_pool::run(|w| measure(Some(w)));
}

/// A large flood and a falling sand column (as the `ocean` and `pile_collapse` benchmarks of
/// `foundry_headless`), so that many chunks are awake.
fn big_scene(content: &Arc<Content>) -> foundry_sim::Simulation {
    let mut sim = foundry_sim::Simulation::new(content.clone(), foundry_sim::SimConfig::finite(32, 16, 1));
    let h = sim.size_cells().1;
    let (water, sand) = (content.expect_material("water"), content.expect_material("sand"));
    for y in 320..h - 2 {
        for x in 2..1400 {
            sim.set_cell(CellPos::new(x, y), water, None);
        }
    }
    for y in 64..h - 2 {
        for x in 1612..1812 {
            sim.set_cell(CellPos::new(x, y), sand, None);
        }
    }
    sim
}

/// Run 300 ticks of the big scene and print the tick times. `workers`: as in `measure`.
fn measure_big(mut workers: Option<&mut Workers>) {
    let content = Arc::new(Content::load_default().unwrap());
    let mut sim = big_scene(&content);
    let mut ticks = Samples::new("cell update");
    let (mut awake, mut crew) = (0u64, 0u32);
    for _ in 0..300 {
        let t = Instant::now();
        match workers.as_mut() {
            Some(w) => {
                w.tick(sim.stats().awake_chunks, || sim.advance());
                crew += w.parallel() as u32;
            }
            None => {
                sim.advance();
            }
        }
        ticks.add(t);
        awake += sim.stats().awake_chunks as u64;
    }
    println!("  300 ticks, {} awake chunks per tick on average, {crew} ticks on the crew pool", awake / 300);
    ticks.print();
}

/// Large scenes (the sandbox) must still use many threads and must not be slower.
#[test]
#[ignore = "a measure, not a check: run it with --release --ignored --nocapture"]
fn big_scene_perf() {
    println!("--- big_scene_perf: cell update called from outside the pool (as before sim_pool.rs) ---");
    measure_big(None);
    println!("--- big_scene_perf: cell update on the pools of sim_pool.rs ---");
    sim_pool::run(|w| measure_big(Some(w)));
}
