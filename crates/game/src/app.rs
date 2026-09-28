//! The window: winit events, the wgpu surface, egui, input and the frame loop.
//!
//! The game starts in the main menu with no world. "New game" builds a demo world and starts the
//! simulation thread; "Load" and "Continue" load a save. The UI is `foundry_ui` (see `ui.rs`).
//!
//! Two game modes: the sandbox mode (paint any material, free camera) and the normal mode (the
//! robot, the factory, research and the Hub; the camera follows the robot). In the normal mode
//! the factory runs on the simulation thread (`factory_host.rs`), and `normal.rs` connects it to
//! the UI and the input.
//!
//! Each frame:
//! 1. Take the newest snapshot. Upload it (`Renderer::apply_snapshot`), keep its chunk images for
//!    the cell under the mouse, and show its notices.
//! 2. Update the UI model and run the UI. Apply the UI actions.
//! 3. Move the camera, send brush strokes, and send `SetView` when the view area changed.
//! 4. Render the world (if there is one), then egui, and present.

use crate::args::{Args, UiState};
use crate::controls::{CameraControl, MAX_ZOOM, Stroke};
use crate::debug_panel::{self, DebugAction, StatsView};
use crate::demo;
use crate::factory_host::{FactoryCommand, FactoryHost, GameCommand};
use crate::normal::NormalMode;
use crate::overlay;
use crate::saves::{self, SaveMeta};
use crate::sim_thread::SimThread;
use crate::smoke::{Smoke, Step};
use crate::ui::{self, SandboxUi};
use anyhow::{Context, Result};
use foundry_content::Content;
use foundry_factory::Guide;
use foundry_core::{CellPos, CellRect, CellTexel, ChunkPos, Command, DebugChunk, MaterialId, PaintMode};
use foundry_render::headless::device_descriptor;
use foundry_render::{Renderer, wgpu};
use foundry_sim::Simulation;
use foundry_ui::{GameMode, GameState, HoverView, MenuPage, PerfView, SettingChange, UiAction, WindowKind, WorldSize};
use glam::{DVec2, UVec2, Vec2};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

/// Extra cells around the screen that the simulation sends, so panning shows no empty chunks.
const VIEW_MARGIN: i32 = 64;

pub fn run(args: Args, content: Arc<Content>) -> Result<()> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App { args, content, game: None, error: None };
    event_loop.run_app(&mut app)?;
    match app.error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

struct App {
    args: Args,
    content: Arc<Content>,
    game: Option<Game>,
    error: Option<anyhow::Error>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.game.is_some() {
            return;
        }
        match Game::new(event_loop, &self.args, self.content.clone()) {
            Ok(g) => self.game = Some(g),
            Err(e) => {
                self.error = Some(e);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let Some(game) = &mut self.game {
            game.window_event(event_loop, event);
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(game) = &self.game {
            game.window.request_redraw();
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(mut game) = self.game.take() {
            if let Some(failure) = game.smoke.as_ref().and_then(|s| s.failure.clone()) {
                self.error = Some(anyhow::anyhow!("smoke test failed: {failure}"));
            }
            if self.args.exit_after.is_some() {
                game.timing.print_summary(&game);
            }
            if let Some(w) = game.world.as_mut() {
                w.sim.stop();
            }
        }
    }
}

/// Keys and buttons that are held down.
#[derive(Default)]
struct Held {
    left: bool,
    right: bool,
    up: bool,
    down: bool,
    shift: bool,
    paint: bool,
    erase: bool,
    drag: bool,
}

/// Frame timing, for the stats and `--exit-after`.
struct Timing {
    start: Instant,
    last_frame: Instant,
    frames: u64,
    /// Smoothed values for the panel.
    frame_ms: f32,
    cpu_ms: f32,
    /// Every frame time, only with `--exit-after`.
    samples: Vec<f32>,
    cpu_total_ms: f64,
    tick_ms_total: f64,
    tick_samples: u64,
    /// Number of frames in the first second.
    warmup_frames: usize,
}

impl Timing {
    fn new(record: Option<f64>) -> Self {
        let now = Instant::now();
        let capacity = record.map_or(0, |s| (s * 500.0) as usize);
        Self {
            start: now,
            last_frame: now,
            frames: 0,
            frame_ms: 16.0,
            cpu_ms: 0.0,
            samples: Vec::with_capacity(capacity),
            cpu_total_ms: 0.0,
            tick_ms_total: 0.0,
            tick_samples: 0,
            warmup_frames: 0,
        }
    }

    fn print_summary(&self, game: &Game) {
        let secs = self.last_frame.duration_since(self.start).as_secs_f64();
        let size = game.controls.camera.viewport;
        println!("frames: {} in {:.2} s (average FPS {:.1})", self.frames, secs, self.frames as f64 / secs.max(1e-9));
        // The first second has the window opening and the first upload of all chunks. Leave it out.
        let steady = &self.samples[self.warmup_frames.min(self.samples.len())..];
        if !steady.is_empty() {
            let mut sorted = steady.to_vec();
            sorted.sort_by(f32::total_cmp);
            let pct = |p: f64| sorted[((sorted.len() as f64 - 1.0) * p).round() as usize];
            let total_ms: f64 = steady.iter().map(|&v| v as f64).sum();
            println!(
                "after the first second: average FPS {:.1}, average frame time {:.2} ms (median {:.2}, 99th percentile {:.2}, worst {:.2} ms)",
                steady.len() as f64 * 1000.0 / total_ms.max(1e-9),
                total_ms / steady.len() as f64,
                pct(0.5),
                pct(0.99),
                pct(1.0)
            );
        }
        println!("average CPU time per frame: {:.2} ms", self.cpu_total_ms / self.frames.max(1) as f64);
        println!(
            "simulation: {:.1} ticks/s, average tick {:.2} ms, awake chunks {}",
            game.world.as_ref().map_or(0.0, |w| w.sim.ticks_per_second()),
            self.tick_ms_total / self.tick_samples.max(1) as f64,
            game.stats.awake_chunks
        );
        let refresh =
            game.window.current_monitor().and_then(|m| m.refresh_rate_millihertz()).map_or(0.0, |r| r as f64 / 1000.0);
        println!(
            "window: {}x{} px, zoom {:.2}, present mode {:?}, display refresh {:.0} Hz",
            size.x, size.y, game.controls.camera.zoom, game.config.present_mode, refresh
        );
    }
}

/// A running world: the simulation thread and what the game knows about it.
struct World {
    sim: SimThread,
    seed: u64,
    /// World size in chunks.
    chunks: (i32, i32),
    /// Paused with the pause key (Space), not with the pause menu.
    user_paused: bool,
    /// The chunk overlay of the debug panel is on.
    overlay: bool,
    /// The normal mode (robot and factory). `None` in the sandbox mode.
    normal: Option<NormalMode>,
}

impl World {
    fn new(sim: SimThread, seed: u64, chunks: (i32, i32), normal: bool) -> Self {
        Self { sim, seed, chunks, user_paused: false, overlay: false, normal: normal.then(NormalMode::new) }
    }
}

struct Game {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    present_modes: Vec<wgpu::PresentMode>,
    renderer: Renderer,

    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
    /// egui textures to free after the current frame.
    egui_free: Vec<egui::TextureId>,

    content: Arc<Content>,
    /// The guide goals (data, loaded once).
    guide: Arc<Guide>,
    ui: SandboxUi,
    world: Option<World>,
    /// The mode of `--ui-state` start worlds (`--mode`).
    start_mode: GameMode,
    /// Default camera position for new worlds (`--center`).
    start_center: Option<DVec2>,
    /// The world shape from the command line (`--world`, `--depth`).
    start_shape: demo::Shape,
    start_zoom: Option<f32>,
    controls: CameraControl,
    held: Held,
    stroke: Stroke,
    /// Reused list of brush positions.
    paint_points: Vec<CellPos>,
    mouse: DVec2,
    mouse_inside: bool,
    last_view: Option<CellRect>,
    /// The chunk images the renderer has, for the cell under the mouse. The same chunks as on the
    /// GPU: a chunk leaves this map when the renderer drops it.
    cells: HashMap<ChunkPos, Box<[CellTexel]>>,
    debug_chunks: Vec<DebugChunk>,
    tick: u64,
    stats: StatsView,

    timing: Timing,
    exit_after: Option<Duration>,
    /// The surface said it is suboptimal. Configure it after the current frame.
    needs_configure: bool,
    /// `--smoke-test`.
    smoke: Option<Smoke>,
}

impl Game {
    fn new(event_loop: &ActiveEventLoop, args: &Args, content: Arc<Content>) -> Result<Self> {
        let mut attrs =
            Window::default_attributes().with_title("Deep Foundry").with_inner_size(LogicalSize::new(1600.0, 900.0));
        if let Some((w, h)) = args.size {
            attrs = attrs.with_inner_size(PhysicalSize::new(w, h));
        }
        let window = Arc::new(event_loop.create_window(attrs)?);
        let size = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let surface = instance.create_surface(window.clone())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .context("no GPU adapter for this window")?;
        let (device, queue) = pollster::block_on(adapter.request_device(&device_descriptor(&adapter)))?;
        let info = adapter.get_info();
        log::info!("GPU: {} ({:?})", info.name, info.backend);

        let caps = surface.get_capabilities(&adapter);
        // egui and the world shaders write sRGB values directly, so prefer a format that is not "...Srgb".
        let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .context("the surface does not work with this GPU")?;
        config.format = format;
        config.present_mode = present_mode(&caps.present_modes, !args.no_vsync);
        config.desired_maximum_frame_latency = 2;
        config.alpha_mode = caps.alpha_modes[0];
        surface.configure(&device, &config);

        let renderer = Renderer::new(&device, &queue, format, &content);

        let egui_ctx = egui::Context::default();
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            Some(device.limits().max_texture_dimension_2d as usize),
        );
        let egui_renderer = egui_wgpu::Renderer::new(&device, format, egui_wgpu::RendererOptions::default());

        let saves_dir = args.saves.clone().unwrap_or_else(saves::default_dir);
        let mut ui = SandboxUi::new(&egui_ctx, content.clone(), saves_dir);
        ui.model.settings.ui_scale = args.ui_scale;
        ui.model.settings.vsync = !args.no_vsync;

        let viewport = UVec2::new(config.width, config.height);
        let controls = CameraControl::new(DVec2::ZERO, 2.0, viewport, DVec2::ZERO);

        let guide = Arc::new(Guide::load_default().unwrap_or_else(|e| {
            log::warn!("cannot load the guide: {e}");
            Guide::default()
        }));
        let mut game = Self {
            window,
            surface,
            device,
            queue,
            config,
            present_modes: caps.present_modes,
            renderer,
            egui_ctx,
            egui_state,
            egui_renderer,
            egui_free: Vec::new(),
            content,
            guide,
            ui,
            world: None,
            start_mode: args.start_mode(),
            start_center: args.center.map(DVec2::from),
            start_shape: args.shape(),
            start_zoom: args.zoom,
            controls,
            held: Held::default(),
            stroke: Stroke::default(),
            paint_points: Vec::with_capacity(256),
            mouse: DVec2::ZERO,
            mouse_inside: false,
            last_view: None,
            cells: HashMap::new(),
            debug_chunks: Vec::new(),
            tick: 0,
            stats: StatsView::default(),
            timing: Timing::new(args.exit_after),
            exit_after: args.exit_after.map(Duration::from_secs_f64),
            needs_configure: false,
            smoke: args.smoke_test.then(Smoke::new),
        };
        game.enter(args.start_state(), args.seed, args.shape());
        Ok(game)
    }

    /// Go to a start screen (`--ui-state`).
    fn enter(&mut self, state: UiState, seed: u64, shape: demo::Shape) {
        if state.has_world() {
            self.start_new_world(seed, shape, self.start_mode);
        }
        match state {
            UiState::Menu | UiState::Playing => {}
            UiState::NewGame => self.ui.ui.open_menu(MenuPage::NewGame),
            UiState::Load => self.ui.ui.open_menu(MenuPage::Load),
            UiState::Settings => self.ui.ui.open_menu(MenuPage::Settings),
            UiState::Inventory => self.ui.ui.open_window(WindowKind::Character),
            UiState::Debug => self.ui.model.settings.show_debug = true,
            UiState::Research => self.ui.ui.open_window(WindowKind::Research),
            UiState::Guide => self.ui.ui.open_window(WindowKind::Guide),
            // Screens for `--screenshot`. In the window they start the normal game.
            UiState::Building | UiState::Ghost | UiState::Hub => {}
            UiState::Pause | UiState::Save => {
                self.pause();
                if state == UiState::Save {
                    self.ui.ui.open_menu(MenuPage::Save);
                }
            }
        }
    }

    fn playing(&self) -> bool {
        self.world.is_some() && self.ui.state() == GameState::Playing
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, event: WindowEvent) {
        let response = self.egui_state.on_window_event(&self.window, &event);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => self.resize(UVec2::new(size.width, size.height)),
            WindowEvent::RedrawRequested => self.frame(event_loop),
            WindowEvent::KeyboardInput { event, .. } => {
                let down = event.state == ElementState::Pressed;
                let PhysicalKey::Code(code) = event.physical_key else { return };
                // A text field has the keyboard: only key releases count.
                if down && self.egui_ctx.text_edit_focused() {
                    return;
                }
                self.key(code, down, event.repeat);
            }
            WindowEvent::ModifiersChanged(m) => self.held.shift = m.state().shift_key(),
            WindowEvent::Focused(false) => {
                // Key and button releases are not sent to a window without focus.
                self.release_all();
            }
            WindowEvent::CursorLeft { .. } => self.mouse_inside = false,
            WindowEvent::PinchGesture { delta, .. } => {
                // Trackpad pinch. `delta` is the change of scale; it can be NaN.
                if delta.is_finite() && self.playing() && !self.egui_ctx.is_pointer_over_egui() {
                    let steps = ((1.0 + delta).max(0.1).ln() / 1.2f64.ln()) as f32;
                    self.controls.zoom_by(steps, self.mouse);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_inside = true;
                let p = DVec2::new(position.x, position.y);
                if self.held.drag {
                    self.controls.drag(p - self.mouse);
                }
                self.mouse = p;
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let down = state == ElementState::Pressed;
                // Start an action only in the world, not over the UI. Always end it.
                if down && (!self.playing() || response.consumed || self.egui_ctx.is_pointer_over_egui()) {
                    return;
                }
                if self.is_normal() && matches!(button, MouseButton::Left | MouseButton::Right) {
                    let left = button == MouseButton::Left;
                    let (content, mouse) = (self.content.clone(), self.mouse_cell());
                    let Some(n) = self.normal_mut() else { return };
                    if down {
                        let cmds = n.press(&content, left, mouse);
                        self.send_all(cmds);
                    } else if left {
                        n.held.dig = false;
                    } else {
                        n.held.spray = false;
                    }
                    return;
                }
                match button {
                    MouseButton::Left => self.held.paint = down,
                    MouseButton::Right => self.held.erase = down,
                    MouseButton::Middle => self.held.drag = down,
                    _ => {}
                }
                if !self.held.paint && !self.held.erase {
                    self.stroke.end();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if !self.playing() || response.consumed || self.egui_ctx.is_pointer_over_egui() {
                    return;
                }
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 60.0,
                };
                self.controls.zoom_by(steps, self.mouse);
            }
            _ => {}
        }
    }

    /// Game keys. The UI reads E, P, Esc, 1-0 and Shift + 1-0 itself; the game does not use them.
    fn key(&mut self, code: KeyCode, down: bool, repeat: bool) {
        if self.is_normal() {
            self.normal_key(code, down, repeat);
            return;
        }
        match code {
            KeyCode::KeyA | KeyCode::ArrowLeft => self.held.left = down,
            KeyCode::KeyD | KeyCode::ArrowRight => self.held.right = down,
            KeyCode::KeyW | KeyCode::ArrowUp => self.held.up = down,
            KeyCode::KeyS | KeyCode::ArrowDown => self.held.down = down,
            _ => {}
        }
        if !down || !self.playing() {
            return;
        }
        match code {
            KeyCode::Space if !repeat => self.debug_action(DebugAction::TogglePause),
            KeyCode::Period => self.debug_action(DebugAction::Step),
            KeyCode::BracketLeft => self.ui.set_brush_radius(self.ui.brush_radius().saturating_sub(1)),
            KeyCode::BracketRight => self.ui.set_brush_radius(self.ui.brush_radius() + 1),
            KeyCode::F3 if !repeat => {
                let s = &mut self.ui.model.settings;
                s.show_debug = !s.show_debug;
            }
            KeyCode::KeyQ if !repeat => {
                self.ui.sandbox_action(&UiAction::ClearHand);
            }
            _ => {}
        }
    }

    /// Keys of the normal mode. The UI reads E, T, G, P, Esc and the quickbar keys itself.
    fn normal_key(&mut self, code: KeyCode, down: bool, repeat: bool) {
        let playing = self.playing();
        let content = self.content.clone();
        let Some(n) = self.normal_mut() else { return };
        match code {
            KeyCode::KeyA | KeyCode::ArrowLeft => n.held.left = down && playing,
            KeyCode::KeyD | KeyCode::ArrowRight => n.held.right = down && playing,
            KeyCode::KeyW | KeyCode::ArrowUp | KeyCode::Space => n.held.jump = down && playing,
            KeyCode::KeyF => n.held.scan = down && playing,
            _ => {}
        }
        if !down || repeat || !playing {
            return;
        }
        match code {
            KeyCode::KeyR if n.building_in_hand(&content).is_some() => n.rotation = (n.rotation + 1) % 4,
            KeyCode::KeyQ => {
                let mut cmds = vec![];
                n.action(&UiAction::ClearHand, &mut cmds);
                self.send_all(cmds);
            }
            KeyCode::F3 => {
                let s = &mut self.ui.model.settings;
                s.show_debug = !s.show_debug;
            }
            _ => {}
        }
    }

    fn resize(&mut self, size: UVec2) {
        if size.x == 0 || size.y == 0 {
            return;
        }
        self.config.width = size.x;
        self.config.height = size.y;
        self.surface.configure(&self.device, &self.config);
        self.controls.set_viewport(size);
        self.controls.min_zoom = self.renderer.min_zoom(size, VIEW_MARGIN).max(1.0);
    }

    // ------------------------------------------------------------ worlds

    /// Forget the old world's view data and put the camera at `center`.
    /// `normal`: the normal mode starts closer (the robot is small).
    fn reset_view(&mut self, center: DVec2, world_cells: DVec2, normal: bool) {
        self.renderer.clear_chunks();
        self.cells.clear();
        self.debug_chunks.clear();
        self.last_view = None;
        self.tick = 0;
        self.stroke.end();
        let viewport = self.controls.camera.viewport;
        let base = if normal { 550.0 } else { 1100.0 };
        let zoom = self.start_zoom.unwrap_or_else(|| (viewport.x as f32 / base).round().clamp(1.0, MAX_ZOOM));
        self.controls = CameraControl::new(center, zoom, viewport, world_cells);
        self.controls.min_zoom = self.renderer.min_zoom(viewport, VIEW_MARGIN).max(1.0);
    }

    /// The world for a new game of this size: a box from `--world`, else an endless world whose
    /// depth depends on the size.
    fn shape_for(&self, size: WorldSize) -> demo::Shape {
        match self.start_shape {
            demo::Shape::Box { .. } => self.start_shape,
            demo::Shape::Infinite { .. } => demo::Shape::Infinite {
                depth_chunks: match size {
                    WorldSize::Small => 64,
                    WorldSize::Normal => 128,
                    WorldSize::Large => 256,
                },
            },
        }
    }

    fn start_new_world(&mut self, seed: u64, shape: demo::Shape, mode: GameMode) {
        self.world = None; // Stops the old simulation thread.
        let mut demo = demo::build(self.content.clone(), shape, seed);
        let (w, h) = demo.sim.size_cells();
        let chunks = (w / foundry_core::CHUNK_SIZE, h / foundry_core::CHUNK_SIZE);
        let mut center = self.start_center.unwrap_or(DVec2::from(demo.start_center));
        let host = match mode {
            GameMode::Sandbox => None,
            GameMode::Normal => {
                match FactoryHost::new_game(self.content.clone(), self.guide.clone(), &mut demo.sim, demo.start_center.0 as i32) {
                    Ok(h) => {
                        let (x, y) = h.robot.center();
                        center = DVec2::new(x as f64, y as f64);
                        Some(h)
                    }
                    Err(e) => {
                        log::error!("cannot start a normal game: {e}");
                        None
                    }
                }
            }
        };
        let normal = host.is_some();
        self.reset_view(center, DVec2::new(w as f64, h as f64), normal);
        self.ui.set_mode(if normal { GameMode::Normal } else { GameMode::Sandbox });
        self.world = Some(World::new(SimThread::start_with(demo.sim, host), seed, chunks, normal));
        self.ui.model.state = GameState::Playing;
    }

    /// Load a save when no world runs (from the main menu). The file is read here, then the
    /// simulation thread starts with it.
    fn load_from_menu(&mut self, path: &Path, now: Instant) {
        // The factory side file of a normal game (none for a sandbox save).
        let host = match FactoryHost::load_file(self.content.clone(), self.guide.clone(), path) {
            Ok(h) => h,
            Err(e) => {
                self.ui.message(format!("Load failed: {e}"), now);
                return;
            }
        };
        match Simulation::load_file_with_resolver(self.content.clone(), &demo::resolve_source, path) {
            Ok((sim, report)) => {
                let (w, h) = sim.size_cells();
                let meta = saves::read_meta(path).unwrap_or_default();
                self.world = None;
                let center = match (&host, sim.view()) {
                    (Some(h), _) => {
                        let (x, y) = h.robot.center();
                        DVec2::new(x as f64, y as f64)
                    }
                    (None, Some(v)) => DVec2::new((v.x0 + v.x1) as f64 / 2.0, (v.y0 + v.y1) as f64 / 2.0),
                    (None, None) => DVec2::new(w as f64 * 0.48, h as f64 * 0.55),
                };
                let normal = host.is_some();
                self.reset_view(center, DVec2::new(w as f64, h as f64), normal);
                let chunks = (w / foundry_core::CHUNK_SIZE, h / foundry_core::CHUNK_SIZE);
                self.ui.set_mode(if normal { GameMode::Normal } else { GameMode::Sandbox });
                self.world = Some(World::new(SimThread::start_with(sim, host), meta.seed, chunks, normal));
                self.ui.model.state = GameState::Playing;
                let mut text = format!("Game loaded: {}", save_name(path));
                if !report.unknown_materials.is_empty() {
                    text.push_str(&format!(". Unknown materials became air: {}", report.unknown_materials.join(", ")));
                }
                self.ui.message(text, now);
            }
            Err(e) => self.ui.message(format!("Load failed: {e}"), now),
        }
    }

    /// Load a save. The files are read on this thread, then a new simulation thread starts with
    /// them, so a save of the other mode can be loaded too. If the load fails, the old world stays.
    fn load(&mut self, id: &str, now: Instant) {
        let path = self.ui.saves_dir.join(id);
        self.load_from_menu(&path, now);
    }

    fn save(&mut self, name: &str, now: Instant) {
        let Some(world) = &self.world else { return };
        let dir = self.ui.saves_dir.clone();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.ui.message(format!("Save failed: cannot make the folder {}: {e}", dir.display()), now);
            return;
        }
        let path = saves::world_path(&dir, name);
        world.sim.send(Command::SaveWorld { path: path.clone() });
        let meta = SaveMeta { seed: world.seed, chunks: world.chunks, ticks: self.tick };
        if let Err(e) = saves::write_meta(&path, meta) {
            log::warn!("cannot write the save info file: {e}");
        }
    }

    fn pause(&mut self) {
        if let Some(w) = &self.world {
            w.sim.send(Command::SetPaused(true));
        }
        self.ui.model.state = GameState::Paused;
        self.release_all();
        self.ui.refresh_saves();
    }

    /// Forget all held keys and buttons (the window lost the focus, or a menu opened).
    fn release_all(&mut self) {
        self.held = Held::default();
        self.stroke.end();
        if let Some(n) = self.normal_mut() {
            n.held = Default::default();
        }
    }

    fn normal_mut(&mut self) -> Option<&mut NormalMode> {
        self.world.as_mut().and_then(|w| w.normal.as_mut())
    }

    fn is_normal(&self) -> bool {
        self.world.as_ref().is_some_and(|w| w.normal.is_some())
    }

    /// The cell under the mouse.
    fn mouse_cell(&self) -> CellPos {
        let c = self.controls.camera.screen_to_cell(self.mouse);
        CellPos::new(c.x.floor() as i32, c.y.floor() as i32)
    }

    /// Send commands to the simulation thread.
    fn send_all(&self, cmds: Vec<GameCommand>) {
        if let Some(w) = &self.world {
            for c in cmds {
                w.sim.send(c);
            }
        }
    }

    fn quit_to_menu(&mut self) {
        self.world = None;
        self.renderer.clear_chunks();
        self.cells.clear();
        self.debug_chunks.clear();
        self.ui.model.state = GameState::MainMenu;
        self.ui.model.hover = None;
        self.ui.refresh_saves();
    }

    fn handle_action(&mut self, action: UiAction, event_loop: &ActiveEventLoop, now: Instant) {
        if let Some(w) = self.world.as_mut()
            && let Some(n) = w.normal.as_mut()
        {
            let mut cmds = Vec::new();
            if n.action(&action, &mut cmds) {
                for c in cmds {
                    w.sim.send(c);
                }
                return;
            }
        } else if self.ui.sandbox_action(&action) {
            return;
        }
        match action {
            UiAction::NewGame { seed, size, mode } => {
                self.start_new_world(seed, self.shape_for(size), mode);
                let what = if mode == GameMode::Normal { "New game" } else { "New sandbox world" };
                self.ui.message(format!("{what}: seed {seed}"), now);
            }
            UiAction::Continue => match self.ui.model.saves.first().map(|s| s.id.clone()) {
                Some(id) => self.load(&id, now),
                None => self.ui.message("There is no saved game yet.", now),
            },
            UiAction::Pause => self.pause(),
            UiAction::Resume => {
                if let Some(w) = &self.world {
                    w.sim.send(Command::SetPaused(w.user_paused));
                }
                self.ui.model.state = GameState::Playing;
            }
            UiAction::Save { name, .. } => self.save(&name, now),
            UiAction::Load(id) => self.load(&id, now),
            UiAction::DeleteSave(id) => {
                match saves::delete(&self.ui.saves_dir, &id) {
                    Ok(()) => self.ui.message(format!("Deleted: {}", save_name(Path::new(&id))), now),
                    Err(e) => self.ui.message(format!("Delete failed: {e}"), now),
                }
                self.ui.refresh_saves();
            }
            UiAction::QuitToMenu => self.quit_to_menu(),
            UiAction::QuitGame => event_loop.exit(),
            UiAction::ChangeSetting(change) => self.change_setting(change),
            // No factory, research or alerts in the sandbox yet.
            UiAction::OpenWindow(_)
            | UiAction::CloseWindow(_)
            | UiAction::OpenPowerNetwork(_)
            | UiAction::ClickSlot { .. }
            | UiAction::SelectHotbar(_)
            | UiAction::SetHotbar { .. }
            | UiAction::ClearHand
            | UiAction::EmptyTank(_)
            | UiAction::Craft { .. }
            | UiAction::CancelCraft { .. }
            | UiAction::SetRecipe { .. }
            | UiAction::StartResearch(_)
            | UiAction::ShowAlert(_) => {}
        }
    }

    fn change_setting(&mut self, change: SettingChange) {
        let s = &mut self.ui.model.settings;
        match change {
            SettingChange::UiScale(v) => s.ui_scale = v,
            SettingChange::ShowFps(v) => s.show_fps = v,
            SettingChange::ShowDebug(v) => s.show_debug = v,
            SettingChange::Vsync(v) => {
                s.vsync = v;
                self.config.present_mode = present_mode(&self.present_modes, v);
                self.surface.configure(&self.device, &self.config);
            }
            // The simulation has no number settings yet. The liquids work adds them
            // (`Simulation::settings_mut`); then this sends them to the simulation thread.
            SettingChange::Simulation { key, value } => log::info!("simulation setting {key} = {value} (not used yet)"),
        }
    }

    fn debug_action(&mut self, action: DebugAction) {
        let Some(w) = self.world.as_mut() else { return };
        match action {
            DebugAction::TogglePause => {
                w.user_paused = !w.user_paused;
                w.sim.send(Command::SetPaused(w.user_paused));
            }
            DebugAction::Step => {
                if w.user_paused {
                    w.sim.send(Command::Step);
                }
            }
            DebugAction::ToggleOverlay => {
                w.overlay = !w.overlay;
                w.sim.send(Command::SetDebug(w.overlay));
                if !w.overlay {
                    self.debug_chunks.clear();
                }
            }
        }
    }

    // ------------------------------------------------------------ frame

    /// Take the newest snapshot: upload it, keep its chunks for the cell under the mouse,
    /// and show its notices.
    fn take_snapshot(&mut self, now: Instant) {
        // The factory views of the normal mode.
        if let Some(w) = self.world.as_mut()
            && let Some(n) = w.normal.as_mut()
            && let Some(frame) = w.sim.take_factory()
        {
            for notice in n.take_frame(frame, now) {
                self.ui.message(notice, now);
            }
        }
        let Some(world) = &self.world else { return };
        let Some(mut snapshot) = world.sim.take_snapshot() else { return };
        let evicted = self.renderer.apply_snapshot(&snapshot, &self.controls.camera);
        for image in snapshot.chunks.drain(..) {
            self.cells.insert(image.pos, image.texels);
        }
        if !evicted.is_empty() {
            for pos in &evicted {
                self.cells.remove(pos);
            }
            world.sim.send(Command::ForgetChunks(evicted));
        }
        if snapshot.tick != self.tick {
            self.timing.tick_ms_total += snapshot.stats.tick_ms as f64;
            self.timing.tick_samples += 1;
        }
        self.tick = snapshot.tick;
        self.stats.tick = snapshot.stats.tick;
        self.stats.tick_ms = snapshot.stats.tick_ms;
        self.stats.awake_chunks = snapshot.stats.awake_chunks;
        self.stats.loaded_chunks = snapshot.stats.loaded_chunks;
        self.stats.packed_chunks = snapshot.stats.packed_chunks;
        let (w, h) = snapshot.world_cells;
        self.controls.world = DVec2::new(w as f64, h as f64);
        if world.overlay {
            self.debug_chunks = std::mem::take(&mut snapshot.debug_chunks);
        }
        if !snapshot.notices.is_empty() {
            for notice in snapshot.notices.drain(..) {
                self.ui.message(notice, now);
            }
            self.ui.refresh_saves();
        }
    }

    /// The cell under the mouse, from the chunk images.
    fn hover_cell(&self) -> Option<HoverView> {
        let c = self.controls.camera.screen_to_cell(self.mouse);
        let pos = CellPos::new(c.x.floor() as i32, c.y.floor() as i32);
        let (w, h) = (self.controls.world.x as i32, self.controls.world.y as i32);
        if pos.x < 0 || pos.y < 0 || pos.x >= w || pos.y >= h {
            return None;
        }
        let texel = self.cells.get(&pos.chunk())?.get(pos.local_index())?;
        Some(HoverView::Cell { pos, material: MaterialId(texel[0]), temperature: texel[1] as i16 as f32 })
    }

    /// Put the numbers of this frame into the UI model.
    fn update_model(&mut self, now: Instant) {
        self.ui.update_message(now);
        let playing = self.playing();
        let over_ui = self.egui_ctx.is_pointer_over_egui();
        let mouse = self.mouse_cell();
        let building = self.world.as_ref().and_then(|w| w.normal.as_ref()).and_then(|n| n.hover(mouse));
        self.ui.model.hover = if playing && self.mouse_inside && !over_ui { building.or_else(|| self.hover_cell()) } else { None };
        if let Some(n) = self.world.as_ref().and_then(|w| w.normal.as_ref()) {
            n.fill_model(&mut self.ui.model);
        }
        let fps = 1000.0 / self.timing.frame_ms.max(0.001);
        self.ui.model.fps = fps;
        self.ui.model.perf = self.ui.model.settings.show_fps.then(|| PerfView {
            fps,
            tick_ms: self.stats.tick_ms,
            ticks_per_second: self.world.as_ref().map_or(0.0, |w| w.sim.ticks_per_second()),
            awake_chunks: self.stats.awake_chunks,
            loaded_chunks: self.stats.loaded_chunks,
        });
        if let Some(s) = self.ui.model.sandbox.as_mut() {
            s.sim_paused = self.world.as_ref().is_some_and(|w| w.user_paused);
        }
        let r = self.renderer.stats();
        let s = &mut self.stats;
        s.frame_ms = self.timing.frame_ms;
        s.fps = fps;
        s.cpu_ms = self.timing.cpu_ms;
        s.ticks_per_second = self.world.as_ref().map_or(0.0, |w| w.sim.ticks_per_second());
        s.gpu_chunks = r.resident_chunks;
        s.gpu_capacity = r.chunk_capacity;
        s.drawn_chunks = r.drawn_chunks;
        s.zoom = self.controls.camera.zoom;
        let c = self.controls.camera.screen_to_cell(self.mouse);
        s.cursor = Some((c.x.floor() as i32, c.y.floor() as i32));
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        let frame_start = Instant::now();
        let dt = frame_start.duration_since(self.timing.last_frame).as_secs_f32();
        self.timing.last_frame = frame_start;
        self.timing.frames += 1;
        if self.timing.frames > 1 {
            self.timing.frame_ms += (dt * 1000.0 - self.timing.frame_ms) * 0.05;
            if self.exit_after.is_some() {
                self.timing.samples.push(dt * 1000.0);
                if frame_start.duration_since(self.timing.start) < Duration::from_secs(1) {
                    self.timing.warmup_frames = self.timing.samples.len();
                }
            }
        }
        let dt_raw = dt;
        let dt = dt.min(0.05);

        // 1. The newest snapshot.
        self.take_snapshot(frame_start);
        // In the normal mode the camera follows the robot. It moves before the UI, so the shapes
        // over the world (robot, ghost) use the same camera as the world.
        if self.is_normal() {
            self.follow_robot(dt, frame_start);
        }

        // 2. The UI.
        self.update_model(frame_start);
        let raw_input = self.egui_state.take_egui_input(&self.window);
        let ppp = self.egui_ctx.pixels_per_point();
        let brush_pos = egui::pos2((self.mouse.x as f32) / ppp, (self.mouse.y as f32) / ppp);
        let brush_radius = (self.ui.brush_radius() as f32 + 0.5) * self.controls.camera.zoom / ppp;
        let show_brush = self.playing() && self.mouse_inside;
        let show_debug = self.ui.model.settings.show_debug && self.world.is_some();
        let (paused, overlay) = self.world.as_ref().map_or((false, false), |w| (w.user_paused, w.overlay));
        let mouse_cell = self.mouse_cell();
        let mut actions = Vec::new();
        let mut debug_actions = Vec::new();
        let mut full_output = {
            let ui = &mut self.ui;
            let stats = &self.stats;
            let debug_chunks = &self.debug_chunks;
            let camera = &self.controls.camera;
            let normal = self.world.as_ref().and_then(|w| w.normal.as_ref());
            let content = &self.content;
            self.egui_ctx.run_ui(raw_input, |root| {
                if show_debug {
                    debug_panel::draw(root, stats, paused, overlay, &mut debug_actions);
                }
                actions = ui.ui.show(root.ctx(), &ui.model);
                let painter = root.ctx().layer_painter(egui::LayerId::background());
                if overlay {
                    debug_panel::draw_overlay(&painter, debug_chunks, camera, root.ctx().pixels_per_point());
                }
                let over_ui = root.ctx().is_pointer_over_egui();
                match normal {
                    Some(n) => {
                        let mouse = (show_brush && !over_ui).then_some(mouse_cell);
                        let ghost = mouse.and_then(|m| n.ghost(content, m));
                        let at = n.robot_pos(frame_start);
                        overlay::draw(&painter, camera, root.ctx().pixels_per_point(), &n.frame, at, ghost.as_ref(), mouse);
                    }
                    None => {
                        if show_brush && !over_ui {
                            painter.circle_stroke(brush_pos, brush_radius, egui::Stroke::new(1.0, egui::Color32::from_white_alpha(140)));
                        }
                    }
                }
            })
        };
        self.egui_state.handle_platform_output(&self.window, std::mem::take(&mut full_output.platform_output));
        // egui textures: upload new ones now. Free old ones after this frame is sent to the GPU.
        for (id, deltas) in full_output.textures_delta.set.drain() {
            for delta in &deltas {
                self.egui_renderer.update_texture(&self.device, &self.queue, id, delta);
            }
        }
        self.egui_free.extend(full_output.textures_delta.free.drain());
        let pixels_per_point = full_output.pixels_per_point;
        let paint_jobs = self.egui_ctx.tessellate(std::mem::take(&mut full_output.shapes), pixels_per_point);
        for action in actions {
            self.handle_action(action, event_loop, frame_start);
        }
        for action in debug_actions {
            self.debug_action(action);
        }
        self.smoke_step(event_loop, frame_start);
        self.send_normal_input();

        // 3. Camera, brush and view area.
        if self.playing() && !self.is_normal() {
            let pan = Vec2::new(
                (self.held.right as i32 - self.held.left as i32) as f32,
                (self.held.down as i32 - self.held.up as i32) as f32,
            );
            self.controls.update(dt, pan, self.held.shift);
            self.paint();
        }
        if let Some(world) = &self.world {
            let view = self.controls.view_area(VIEW_MARGIN);
            if self.last_view != Some(view) {
                world.sim.send(Command::SetView { area: view });
                self.last_view = Some(view);
            }
        }

        // 4. Draw.
        let cpu_before_acquire = frame_start.elapsed();
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) => f,
            wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
                // Draw this frame, then configure again (not now: the frame texture is still in use).
                self.needs_configure = true;
                f
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                self.free_egui_textures();
                return;
            }
            other => {
                // Occluded (the window is hidden) or Timeout. Wait a little, so the loop does not use a
                // full CPU core while nothing is shown.
                log::debug!("no surface texture this frame: {other:?}");
                self.free_egui_textures();
                std::thread::sleep(Duration::from_millis(8));
                return;
            }
        };
        let acquired = Instant::now();
        let target = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });

        let has_world = self.world.is_some();
        if has_world {
            self.renderer.set_time(frame_start.duration_since(self.timing.start).as_secs_f64());
            self.renderer.render(&mut encoder, &target, &self.controls.camera);
        }

        // egui on top. With no world, egui clears the screen first.
        let screen =
            egui_wgpu::ScreenDescriptor { size_in_pixels: [self.config.width, self.config.height], pixels_per_point };
        let clear = (!has_world).then_some(wgpu::Color::BLACK);
        let egui_cmds = ui::render_egui(
            &mut self.egui_renderer,
            &self.device,
            &self.queue,
            &mut encoder,
            &target,
            &paint_jobs,
            &screen,
            clear,
        );
        self.queue.submit(egui_cmds.into_iter().chain([encoder.finish()]));
        self.free_egui_textures();
        self.window.pre_present_notify();
        let before_present = Instant::now();
        self.queue.present(frame);
        if self.needs_configure {
            log::debug!("surface is suboptimal; configuring it again");
            self.surface.configure(&self.device, &self.config);
            self.needs_configure = false;
        }

        let cpu = cpu_before_acquire + before_present.duration_since(acquired);
        let total = frame_start.elapsed();
        if total > Duration::from_millis(12) {
            log::debug!(
                "slow frame {:.1} ms (interval {:.1} ms): before acquire {:.2}, acquire {:.2}, draw {:.2}, present {:.2} ms; uploads {}",
                total.as_secs_f64() * 1000.0,
                dt_raw * 1000.0,
                cpu_before_acquire.as_secs_f64() * 1000.0,
                acquired.duration_since(frame_start + cpu_before_acquire).as_secs_f64() * 1000.0,
                before_present.duration_since(acquired).as_secs_f64() * 1000.0,
                before_present.elapsed().as_secs_f64() * 1000.0,
                self.renderer.stats().uploaded_chunks,
            );
        }
        let cpu_ms = cpu.as_secs_f32() * 1000.0;
        self.timing.cpu_ms += (cpu_ms - self.timing.cpu_ms) * 0.05;
        self.timing.cpu_total_ms += cpu_ms as f64;

        if let Some(limit) = self.exit_after
            && frame_start.duration_since(self.timing.start) >= limit
        {
            event_loop.exit();
        }
    }

    /// The camera follows the robot (normal mode). The zoom still moves smoothly.
    fn follow_robot(&mut self, dt: f32, now: Instant) {
        let Some((x, y)) = self.world.as_ref().and_then(|w| w.normal.as_ref()).and_then(|n| n.robot_pos(now)) else { return };
        if self.playing() {
            self.controls.update(dt, Vec2::ZERO, false);
        }
        let (x, y) = (x + crate::player::ROBOT_W as f32 * 0.5, y + crate::player::ROBOT_H as f32 * 0.5);
        let target = DVec2::new(x as f64, y as f64 - 8.0);
        let k = 1.0 - (-dt as f64 * 12.0).exp();
        let c = &mut self.controls.camera.center;
        *c += (target - *c) * k;
        if (target - *c).length() > 400.0 {
            *c = target;
        }
    }

    /// Send the player input and the open windows to the factory (normal mode), when they changed.
    fn send_normal_input(&mut self) {
        let content = self.content.clone();
        let mouse = self.mouse_cell();
        let view = self.controls.camera.visible_rect();
        let (research, guide) = (self.ui.ui.is_open(WindowKind::Research), self.ui.ui.is_open(WindowKind::Guide));
        let Some(w) = self.world.as_mut() else { return };
        let Some(n) = w.normal.as_mut() else { return };
        if let Some(c) = n.input(&content, mouse, view) {
            w.sim.send(c);
        }
        if let Some(c) = n.windows(research, guide) {
            w.sim.send(c);
        }
    }

    /// Run the next step of `--smoke-test`.
    fn smoke_step(&mut self, event_loop: &ActiveEventLoop, now: Instant) {
        let Some(step) = self.smoke.as_mut().and_then(|s| s.next()) else { return };
        match step {
            Step::Act(action) => {
                println!("smoke test: {action:?}");
                self.handle_action(action, event_loop, now);
            }
            Step::PaintSand => {
                if let (Some(w), Some(sand)) = (&self.world, self.content.material("sand")) {
                    let c = self.controls.camera.center;
                    let center = CellPos::new(c.x as i32, c.y as i32 - 60);
                    w.sim.send(Command::Paint { center, radius: 8, material: sand, mode: PaintMode::Replace, temperature: None });
                }
            }
            Step::Expect(text) => {
                let found = self.ui.message_log.iter().any(|m| m.contains(text));
                let Some(smoke) = self.smoke.as_mut() else { return };
                if found {
                    println!("smoke test: saw \"{text}\"");
                    smoke.passed_expect();
                    self.ui.message_log.clear();
                } else if !smoke.retry(Step::Expect(text)) {
                    eprintln!("smoke test failed: {:?}; messages: {:?}", smoke.failure, self.ui.message_log);
                    event_loop.exit();
                }
            }
            Step::Done => {
                println!("smoke test passed");
                event_loop.exit();
            }
            step => {
                // The normal-mode steps. `Ok(true)`: done; `Ok(false)`: try again next frame.
                let result = self.smoke_normal(&step);
                let Some(smoke) = self.smoke.as_mut() else { return };
                match result {
                    Ok(true) => {
                        println!("smoke test: {step:?} ok");
                        smoke.passed_expect();
                    }
                    Ok(false) => {
                        if !smoke.retry(step) {
                            eprintln!("smoke test failed: {:?}; messages: {:?}", smoke.failure, self.ui.message_log);
                            event_loop.exit();
                        }
                    }
                    Err(e) => {
                        smoke.failure = Some(format!("{step:?}: {e}"));
                        eprintln!("smoke test failed: {e}; messages: {:?}", self.ui.message_log);
                        event_loop.exit();
                    }
                }
            }
        }
    }

    /// One normal-mode step of the smoke test.
    fn smoke_normal(&mut self, step: &Step) -> Result<bool, String> {
        let content = self.content.clone();
        let near_clay = self.nearest_cell(content.material("clay"));
        let open_kind = self.ui.model.building.as_ref().map(|b| b.kind);
        let Some(w) = self.world.as_mut() else { return Err("no world".into()) };
        let Some(n) = w.normal.as_mut() else { return Err("not the normal mode".into()) };
        let mut cmds: Vec<GameCommand> = vec![];
        let item_count = |n: &NormalMode, id: &str| -> Result<u32, String> {
            let item = content.item(id).ok_or(format!("unknown item {id}"))?;
            let f = &n.frame;
            let tanks: u32 = f.inventory.tanks.iter().filter(|t| t.item == Some(item)).map(|t| t.units).sum();
            let slots: u32 = f.inventory.slots.iter().flatten().filter(|s| s.item == item).map(|s| s.count).sum();
            let hand = f.cursor.filter(|c| c.item == item).map_or(0, |c| c.count);
            Ok(tanks + slots + hand)
        };
        let done = match *step {
            Step::DigClay => {
                let at = near_clay.ok_or("no clay in reach of the robot")?;
                n.aim_override = Some(at);
                n.held.dig = true;
                true
            }
            Step::StopTools => {
                n.held = Default::default();
                n.aim_override = None;
                true
            }
            Step::Have(id, count) => item_count(n, id)? >= count,
            Step::Craft(id, count) => {
                let recipe = content.factory.recipe(id).ok_or(format!("unknown recipe {id}"))?;
                n.action(&UiAction::Craft { recipe, count }, &mut cmds);
                true
            }
            Step::PlaceNear(id) => {
                let part = content.factory.part(id).ok_or(format!("unknown part {id}"))?;
                cmds.push(FactoryCommand::PickToCursor(part).into());
                cmds.push(FactoryCommand::PlaceNear.into());
                true
            }
            Step::OpenPlaced => {
                let (_, r) = n.frame.last_placed.ok_or("nothing was placed")?;
                let center = CellPos::new((r.x0 + r.x1) / 2, (r.y0 + r.y1) / 2);
                n.aim_override = Some(center);
                // Click when the host reports the building under the aim point.
                if n.building_under(center) {
                    cmds = n.press(&content, true, center);
                    n.aim_override = None;
                    true
                } else {
                    false
                }
            }
            Step::WindowOf(id) => {
                let kind = content.factory.building(id).ok_or(format!("unknown building {id}"))?;
                open_kind == Some(kind)
            }
            _ => true,
        };
        for c in cmds {
            w.sim.send(c);
        }
        Ok(done)
    }

    /// The cell of a material in the chunk images that is nearest to the robot and in reach.
    fn nearest_cell(&self, material: Option<MaterialId>) -> Option<CellPos> {
        let m = material?;
        let r = self.world.as_ref()?.normal.as_ref()?.frame.robot?;
        let (cx, cy) = r.center();
        let (cx, cy) = (cx as i32, cy as i32);
        let reach = crate::tools::REACH as i32 - 8;
        let mut best: Option<(i32, CellPos)> = None;
        for y in cy - reach..cy + reach {
            for x in cx - reach..cx + reach {
                let p = CellPos::new(x, y);
                let d = (x - cx).pow(2) + (y - cy).pow(2);
                if d > reach * reach || best.is_some_and(|(bd, _)| bd <= d) {
                    continue;
                }
                if self.cells.get(&p.chunk()).and_then(|c| c.get(p.local_index())).is_some_and(|t| t[0] == m.0) {
                    best = Some((d, p));
                }
            }
        }
        best.map(|(_, p)| p)
    }

    fn free_egui_textures(&mut self) {
        for id in self.egui_free.drain(..) {
            self.egui_renderer.free_texture(&id);
        }
    }

    /// Send paint commands for the mouse stroke. Left paints the material in the hand;
    /// right erases (paints air).
    fn paint(&mut self) {
        if !self.held.paint && !self.held.erase {
            return;
        }
        let Some(world) = &self.world else { return };
        let material = if self.held.paint {
            match self.ui.brush_material() {
                Some(m) => m,
                None => return,
            }
        } else {
            MaterialId::AIR
        };
        let radius = self.ui.brush_radius();
        let cell = self.controls.camera.screen_to_cell(self.mouse);
        self.paint_points.clear();
        self.stroke.advance(cell, radius, self.tick, &mut self.paint_points);
        for &center in &self.paint_points {
            world.sim.send(Command::Paint { center, radius, material, mode: PaintMode::Replace, temperature: None });
        }
    }
}

/// Fifo waits for the display refresh (vertical sync). Without it, prefer Mailbox, then Immediate.
fn present_mode(available: &[wgpu::PresentMode], vsync: bool) -> wgpu::PresentMode {
    if vsync {
        return wgpu::PresentMode::Fifo;
    }
    [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Immediate]
        .into_iter()
        .find(|m| available.contains(m))
        .unwrap_or(wgpu::PresentMode::Fifo)
}

/// The save name of a world file (its name without the extension).
fn save_name(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

