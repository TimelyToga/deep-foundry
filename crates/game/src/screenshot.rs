//! `--screenshot`: build the demo world, run some ticks with no thread and no window,
//! render one frame with the real renderer and the UI, and save it as a PNG file.
//!
//! `--ui-state` picks the screen (main menu, pause menu, game, materials window, ...).
//! `--no-ui` draws only the world. With `--mode normal` (or a normal-mode screen such as
//! `building`) the world has the Hub and the robot, and the factory ticks with the cells. Some
//! screens put items into the inventory first, so that the picture shows something.

use crate::args::{Args, UiState};
use crate::debug_panel::{self, StatsView};
use crate::demo;
use crate::construct::{BuildView, Mods};
use crate::factory_host::{FactoryCommand, FactoryFrame, FactoryHost, GameCommand, Placement, PlayerInput};
use crate::normal::NormalMode;
use crate::overlay;
use crate::player::MoveInput;
use crate::ui::{self, SandboxUi};
use anyhow::{Context, Result};
use foundry_content::{Content, ItemRef};
use foundry_core::{CellPos, Command, TILE_SIZE};
use foundry_factory::Guide;
use foundry_render::headless::{CAPTURE_FORMAT, capture, capture_with, create_device};
use foundry_render::wgpu;
use foundry_render::{Camera, Renderer};
use foundry_ui::{GameMode, GameState, HoverView, MenuPage, PerfView, WindowKind};
use glam::{DVec2, UVec2};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

pub fn run(args: &Args, out: &Path, content: Arc<Content>) -> Result<()> {
    let start = Instant::now();
    let state = if args.no_ui { UiState::Playing } else { args.ui_state.unwrap_or(UiState::Playing) };
    let demo = demo::build(content.clone(), args.shape(), args.seed);
    let mut sim = demo.sim;
    let normal = args.start_mode() == GameMode::Normal;
    let mut host = if normal {
        let guide = Arc::new(Guide::load_default().unwrap_or_default());
        let h = FactoryHost::new_game(content.clone(), guide, &mut sim, demo.start_center.0 as i32).map_err(anyhow::Error::msg)?;
        Some(h)
    } else {
        None
    };
    let built = start.elapsed();

    let size = args.image_size();
    let zoom = args.zoom.unwrap_or(if normal { 4.0 } else { 2.0 });
    let robot_center = |h: &FactoryHost| {
        let (x, y) = h.robot.center();
        DVec2::new(x as f64, y as f64 - 8.0)
    };
    let mut center = match (&args.center, &host) {
        (Some(c), _) => DVec2::from(*c),
        (None, Some(h)) => robot_center(h),
        (None, None) => DVec2::from(demo.start_center),
    };
    let view_of = |c: DVec2| Camera::new(c, zoom, UVec2::new(size.0, size.1)).visible_rect().expand(1);

    // Ask for the chunks on the screen, like the game does. The view is also the anchor: only
    // chunks near it are made and updated.
    sim.apply(Command::SetView { area: view_of(center) });
    let tick_start = Instant::now();
    if let Some(h) = host.as_mut()
        && args.walk != 0
    {
        let walk = PlayerInput { movement: MoveInput { x: args.walk.signum() as i8, jump: false }, ..Default::default() };
        h.apply(FactoryCommand::Input(walk), &mut sim);
        for _ in 0..args.walk.unsigned_abs() {
            sim.tick();
            h.tick(&mut sim);
            if args.center.is_none() {
                center = robot_center(h);
                sim.apply(Command::SetView { area: view_of(center) });
            }
        }
        h.apply(FactoryCommand::Input(PlayerInput::default()), &mut sim);
    }
    for _ in 0..args.ticks {
        sim.tick();
        if let Some(h) = host.as_mut() {
            h.tick(&mut sim);
        }
    }
    let ticks_time = tick_start.elapsed();
    let camera = Camera::new(center, zoom, UVec2::new(size.0, size.1));
    // The normal-mode screens: items, buildings and windows for the picture.
    let mut normal_view = None;
    if let Some(h) = host.as_mut() {
        let (frame, build) = setup_normal_screen(h, &mut sim, state, camera.visible_rect());
        normal_view = Some(NormalView { frame, build });
    }
    let snapshot = sim.take_snapshot();

    let (device, queue) = create_device().context("no GPU adapter found")?;
    let mut renderer = Renderer::new(&device, &queue, CAPTURE_FORMAT, &content);
    renderer.set_time(args.ticks as f64 * foundry_core::TICK_SECONDS);
    let upload_start = Instant::now();
    if state.has_world() {
        let evicted = renderer.apply_snapshot(&snapshot, &camera);
        if !evicted.is_empty() {
            log::warn!("{} chunks did not fit on the GPU; use a larger zoom", evicted.len());
        }
    }
    let upload_time = upload_start.elapsed();

    let pixels = if args.no_ui {
        capture(&device, &queue, &mut renderer, &camera)
    } else {
        let ctx = egui::Context::default();
        let saves_dir = args.saves.clone().unwrap_or_else(crate::saves::default_dir);
        let mut ui = SandboxUi::new(&ctx, content.clone(), saves_dir);
        ui.model.settings.ui_scale = args.ui_scale;
        // A picture has no frame rate, so the HUD shows "-" for the FPS.
        ui.model.perf = Some(PerfView {
            fps: 0.0,
            tick_ms: snapshot.stats.tick_ms,
            ticks_per_second: 0.0,
            awake_chunks: snapshot.stats.awake_chunks,
            loaded_chunks: snapshot.stats.loaded_chunks,
        });
        let stats = StatsView {
            tick: snapshot.stats.tick,
            tick_ms: snapshot.stats.tick_ms,
            awake_chunks: snapshot.stats.awake_chunks,
            loaded_chunks: snapshot.stats.loaded_chunks,
            packed_chunks: snapshot.stats.packed_chunks,
            gpu_chunks: renderer.stats().resident_chunks,
            gpu_capacity: renderer.stats().chunk_capacity,
            zoom: camera.zoom,
            ..Default::default()
        };
        let show_debug = state == UiState::Debug;
        if let Some(nv) = &normal_view {
            ui.set_mode(GameMode::Normal);
            let mut n = NormalMode::new();
            n.take_frame(nv.frame.clone(), Instant::now());
            n.fill_model(&mut ui.model);
            ui.model.perf = None;
            ui.model.settings.show_fps = false;
            // The HUD shows the building or the cell at the mouse, else at the image center.
            let pos = nv.build.mouse.unwrap_or(CellPos::new(center.x.floor() as i32, center.y.floor() as i32));
            let cell = sim.cell(pos);
            ui.model.hover =
                n.hover(pos).or(Some(HoverView::Cell { pos, material: cell.material, temperature: cell.temperature as f32 }));
        }
        match state {
            UiState::Menu => {}
            UiState::NewGame => ui.ui.open_menu(MenuPage::NewGame),
            UiState::Load => ui.ui.open_menu(MenuPage::Load),
            UiState::Settings => ui.ui.open_menu(MenuPage::Settings),
            UiState::Pause | UiState::Save => {
                ui.model.state = GameState::Paused;
                if state == UiState::Save {
                    ui.ui.open_menu(MenuPage::Save);
                }
            }
            UiState::Playing
            | UiState::Inventory
            | UiState::Debug
            | UiState::Building
            | UiState::Ghost
            | UiState::Hub
            | UiState::GhostRed
            | UiState::Drag
            | UiState::Alt
            | UiState::Remove => {
                ui.model.state = GameState::Playing;
                if state == UiState::Inventory {
                    ui.ui.open_window(WindowKind::Character);
                }
            }
            UiState::Research | UiState::Guide => {
                ui.model.state = GameState::Playing;
                ui.ui.open_window(if state == UiState::Research { WindowKind::Research } else { WindowKind::Guide });
            }
        }
        if state.has_world() && normal_view.is_none() {
            // The HUD shows the cell at the image center (there is no mouse).
            let pos = CellPos::new(center.x.floor() as i32, center.y.floor() as i32);
            let cell = sim.cell(pos);
            ui.model.hover = Some(HoverView::Cell { pos, material: cell.material, temperature: cell.temperature as f32 });
        }
        let mut panel = OffscreenUi::new(&device, ctx, ui, size);
        panel.overlay = normal_view.map(|nv| (camera, nv));
        panel.layout(&device, &queue, &stats, show_debug);
        let draw_world = state.has_world();
        let clear = (!draw_world).then_some(wgpu::Color::BLACK);
        capture_with(&device, &queue, &mut renderer, &camera, |encoder, view| panel.draw(&device, &queue, encoder, view, clear))
    };

    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("cannot make folder {}", dir.display()))?;
    }
    let image = image::RgbaImage::from_raw(size.0, size.1, pixels).context("image size does not match")?;
    image.save(out).with_context(|| format!("cannot write {}", out.display()))?;

    let view = camera.visible_rect();
    println!(
        "saved {} ({}x{}, zoom {}, center {:.1},{:.1}, cells {}..{} x {}..{}, screen {:?}); world built in {:.0} ms, {} ticks in {:.0} ms, {} chunks uploaded in {:.2} ms, {} chunks drawn",
        out.display(),
        size.0,
        size.1,
        camera.zoom,
        center.x,
        center.y,
        view.x0,
        view.x1,
        view.y0,
        view.y1,
        state,
        built.as_secs_f64() * 1000.0,
        args.ticks,
        ticks_time.as_secs_f64() * 1000.0,
        renderer.stats().uploaded_chunks,
        upload_time.as_secs_f64() * 1000.0,
        renderer.stats().drawn_chunks,
    );
    Ok(())
}

/// The factory views of a normal-mode picture.
struct NormalView {
    frame: FactoryFrame,
    build: BuildView,
}

/// Make the normal-mode screen: put the items for the picture into the inventory, place and open
/// buildings, and drive the main-thread side (`NormalMode`) like the mouse would. Returns the
/// factory views and the construction shapes.
fn setup_normal_screen(
    h: &mut FactoryHost,
    sim: &mut foundry_sim::Simulation,
    state: UiState,
    view: foundry_core::CellRect,
) -> (FactoryFrame, BuildView) {
    let content = h.factory.content.clone();
    let part = |id: &str| content.factory.part(id).expect("part in the data");
    let kind = |id: &str| content.factory.building(id).expect("building in the data");
    let mat = |id: &str| ItemRef::Material(content.expect_material(id));
    let give = |h: &mut FactoryHost, item: ItemRef, n: u32| {
        h.factory.player.insert(&content, item, n);
    };
    let run = |h: &mut FactoryHost, sim: &mut foundry_sim::Simulation, cmds: Vec<GameCommand>| {
        for c in cmds {
            if let GameCommand::Factory(c) = c {
                h.apply(c, sim);
            }
        }
    };
    // The middle cell of a footprint.
    let middle = |at: foundry_core::TilePos, k: foundry_core::BuildingKindId| {
        let size = content.factory.building_def(k).size;
        CellPos::new(at.x * TILE_SIZE + size.0 as i32 * 4, at.y * TILE_SIZE + size.1 as i32 * 4)
    };
    let mut n = NormalMode::new();
    let mut mouse = None;
    match state {
        UiState::Inventory => {
            // The robot dug some clay and sand, and makes clay bricks.
            give(h, mat("clay"), 180);
            give(h, mat("sand"), 120);
            h.apply(FactoryCommand::Craft { recipe: content.factory.recipe("raw_clay_brick").expect("recipe"), count: 5 }, sim);
            for _ in 0..40 {
                h.tick(sim);
            }
        }
        UiState::Building => {
            // A crate right of the robot, with some items in it.
            // The start inventory has one crate.
            h.apply(FactoryCommand::PickToCursor(part("crate")), sim);
            if let Some(at) = h.free_place(kind("crate"), sim) {
                h.apply(FactoryCommand::Place(Placement::new(kind("crate"), at, 0)), sim);
                h.apply(FactoryCommand::ClearCursor, sim);
                let cell = at.origin();
                h.apply(FactoryCommand::OpenAt(cell), sim);
                if let Some(id) = h.building_at(cell)
                    && let Some(inv) = h.factory.buildings.inventory_mut(id)
                {
                    inv.insert(&content, ItemRef::Part(part("raw_clay_brick")), 12);
                    inv.insert(&content, ItemRef::Part(part("wood_belt")), 20);
                }
            }
        }
        UiState::Hub => {
            // Some deliveries are done already.
            let brick = ItemRef::Part(part("clay_brick"));
            h.factory.progress.deliver(&content, foundry_content::Stack { item: brick, count: 36 });
            give(h, brick, 20);
            let hub = h.factory.buildings.iter().find(|(_, b)| content.factory.building_def(b.kind).kind == "hub").map(|(_, b)| b.at.origin());
            if let Some(cell) = hub {
                h.apply(FactoryCommand::OpenAt(cell), sim);
            }
        }
        UiState::Ghost | UiState::GhostRed => {
            // A steam crusher (three ports) in the hand, at a free place or down in the ground.
            give(h, ItemRef::Part(part("steam_crusher")), 2);
            h.apply(FactoryCommand::PickToCursor(part("steam_crusher")), sim);
            let k = kind("steam_crusher");
            if let Some(at) = h.free_place(k, sim) {
                let at = if state == UiState::GhostRed { foundry_core::TilePos::new(at.x + 1, at.y + 2) } else { at };
                mouse = Some(middle(at, k));
            }
        }
        UiState::Drag => {
            // A line of belts dragged to the right, starting next to the robot.
            give(h, ItemRef::Part(part("wood_belt")), 40);
            h.apply(FactoryCommand::PickToCursor(part("wood_belt")), sim);
            n.take_frame(h.frame(sim, 0), Instant::now());
            if let Some(at) = h.free_place(kind("wood_belt"), sim) {
                let start = middle(at, kind("wood_belt"));
                let cmds = n.press(&content, true, start, Mods::default());
                run(h, sim, cmds);
                for step in 1..=6 {
                    let m = start.offset(step * TILE_SIZE, 0);
                    let cmds = n.update(m);
                    run(h, sim, cmds);
                    n.take_frame(h.frame(sim, 0), Instant::now());
                    mouse = Some(m);
                }
            }
        }
        UiState::Alt => {
            // Machines with recipes, a crate and belts, seen in the alt mode.
            let spots = [("steam_press", "bronze_plate"), ("steam_crusher", "")];
            let mut x = None;
            for (id, _) in spots {
                give(h, ItemRef::Part(part(id)), 1);
                h.apply(FactoryCommand::PickToCursor(part(id)), sim);
                if let Some(at) = h.free_place(kind(id), sim) {
                    h.apply(FactoryCommand::Place(Placement::new(kind(id), at, 0)), sim);
                    x = Some(at);
                }
            }
            // Recipes: the first recipe each machine can run (the alt icon is its product).
            let ids: Vec<_> = h.factory.buildings.iter().map(|(id, b)| (id, b.kind)).collect();
            for (id, k) in ids {
                if let Some(r) = foundry_factory::Buildings::recipes_for(&content, k).first() {
                    let _ = h.factory.buildings.set_recipe(&content, id, Some(*r));
                }
            }
            give(h, ItemRef::Part(part("wood_belt")), 8);
            h.apply(FactoryCommand::PickToCursor(part("wood_belt")), sim);
            if let Some(at) = h.free_place(kind("wood_belt"), sim) {
                for i in 0..3 {
                    h.apply(FactoryCommand::Place(Placement::new(kind("wood_belt"), foundry_core::TilePos::new(at.x + i, at.y), 2)), sim);
                }
            }
            h.apply(FactoryCommand::ClearCursor, sim);
            for _ in 0..30 {
                h.tick(sim);
            }
            n.build.alt = true;
            mouse = x.map(|at| middle(at, kind("steam_crusher")));
        }
        UiState::Remove => {
            // Four belts; the remove button went down on the first and moved over the others.
            give(h, ItemRef::Part(part("wood_belt")), 8);
            h.apply(FactoryCommand::PickToCursor(part("wood_belt")), sim);
            if let Some(at) = h.free_place(kind("wood_belt"), sim) {
                for i in 0..4 {
                    h.apply(FactoryCommand::Place(Placement::new(kind("wood_belt"), foundry_core::TilePos::new(at.x + i, at.y), 0)), sim);
                }
                h.apply(FactoryCommand::ClearCursor, sim);
                let first = middle(at, kind("wood_belt"));
                h.apply(FactoryCommand::Input(PlayerInput { aim: first, ..Default::default() }), sim);
                n.take_frame(h.frame(sim, 0), Instant::now());
                n.press(&content, false, first, Mods::default());
                for i in 0..4 {
                    let m = first.offset(i * TILE_SIZE, 0);
                    if let Some(c) = n.input(&content, m, view) {
                        run(h, sim, vec![c]);
                    }
                    mouse = Some(m);
                }
                for _ in 0..6 {
                    h.tick(sim);
                }
            }
        }
        UiState::Research => {
            h.apply(FactoryCommand::Windows { research: true, guide: false }, sim);
        }
        UiState::Guide => {
            give(h, mat("clay"), 40);
            h.apply(FactoryCommand::Windows { research: false, guide: true }, sim);
        }
        _ => {}
    }
    // The input of the mouse (the ghost at the mouse, the remove button, the alt mode).
    n.take_frame(h.frame(sim, 0), Instant::now());
    let aim = mouse.unwrap_or_default();
    if let Some(c) = n.input(&content, aim, view) {
        run(h, sim, vec![c]);
    }
    // Tick 0 is a multiple of every view period, so all views are in this frame.
    let mut frame = h.frame(sim, 0);
    if frame.guide.is_none() {
        frame.guide = Some(h.factory.guide_view());
    }
    n.take_frame(frame.clone(), Instant::now());
    let build = n.build_view(&content, mouse);
    (frame, build)
}

/// The game UI, drawn with no window.
struct OffscreenUi {
    renderer: egui_wgpu::Renderer,
    ui: SandboxUi,
    /// The camera and the factory views, for the shapes over the world (normal mode).
    overlay: Option<(Camera, NormalView)>,
    ctx: egui::Context,
    size: (u32, u32),
    pixels_per_point: f32,
    jobs: Vec<egui::ClippedPrimitive>,
}

impl OffscreenUi {
    fn new(device: &wgpu::Device, ctx: egui::Context, ui: SandboxUi, size: (u32, u32)) -> Self {
        Self {
            renderer: egui_wgpu::Renderer::new(device, CAPTURE_FORMAT, egui_wgpu::RendererOptions::default()),
            ui,
            overlay: None,
            ctx,
            size,
            pixels_per_point: 1.0,
            jobs: Vec::new(),
        }
    }

    /// Run the UI several times: the fonts become active in the second frame, and windows
    /// measure their size in their first frame.
    fn layout(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, stats: &StatsView, show_debug: bool) {
        for _ in 0..6 {
            // The UI scale is the egui zoom factor, so the screen in points depends on it.
            let ppp = self.ctx.zoom_factor();
            let points = egui::vec2(self.size.0 as f32, self.size.1 as f32) / ppp;
            let mut raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, points)),
                ..Default::default()
            };
            raw.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(1.0);
            let ui = &mut self.ui;
            let overlay_data = &self.overlay;
            let mut out = self.ctx.run_ui(raw, |root| {
                if show_debug {
                    debug_panel::draw(root, stats, false, false, &mut Vec::new());
                }
                let _ = ui.ui.show(root.ctx(), &ui.model);
                if let Some((camera, nv)) = overlay_data {
                    let painter = root.ctx().layer_painter(egui::LayerId::background());
                    let content = ui.model.content.clone();
                    let scene = overlay::Scene {
                        camera,
                        ppp: root.ctx().pixels_per_point(),
                        robot_at: None,
                        build: &nv.build,
                        content: &content,
                        atlas: ui.ui.atlas(),
                    };
                    overlay::draw(&painter, &nv.frame, &scene);
                }
            });
            for (id, deltas) in out.textures_delta.set.drain() {
                for delta in &deltas {
                    self.renderer.update_texture(device, queue, id, delta);
                }
            }
            for id in out.textures_delta.free.drain() {
                self.renderer.free_texture(&id);
            }
            self.pixels_per_point = out.pixels_per_point;
            self.jobs = self.ctx.tessellate(std::mem::take(&mut out.shapes), out.pixels_per_point);
        }
    }

    fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clear: Option<wgpu::Color>,
    ) {
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [self.size.0, self.size.1], pixels_per_point: self.pixels_per_point };
        // No paint callbacks are used, so there are no extra command buffers to send.
        let _ = ui::render_egui(&mut self.renderer, device, queue, encoder, target, &self.jobs, &screen, clear);
    }
}
