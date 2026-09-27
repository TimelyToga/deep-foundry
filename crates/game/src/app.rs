//! The window: winit events, the wgpu surface, egui, input and the frame loop.
//!
//! The game starts in the main menu with no world. "New game" builds a demo world and starts the
//! simulation thread; "Load" and "Continue" load a save. The UI is `foundry_ui` in the sandbox
//! mode (see `ui.rs`).
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
use crate::saves::{self, SaveMeta};
use crate::sim_thread::SimThread;
use crate::smoke::{Smoke, Step};
use crate::ui::{self, SandboxUi};
use anyhow::{Context, Result};
use foundry_content::Content;
use foundry_core::{CellPos, CellRect, CellTexel, ChunkPos, Command, DebugChunk, MaterialId, PaintMode};
use foundry_render::headless::device_descriptor;
use foundry_render::{Renderer, wgpu};
use foundry_sim::Simulation;
use foundry_ui::{GameState, HoverView, MenuPage, PerfView, SettingChange, UiAction, WindowKind};
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
    ui: SandboxUi,
    world: Option<World>,
    /// Default camera position for new worlds (`--center`).
    start_center: Option<DVec2>,
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
            ui,
            world: None,
            start_center: args.center.map(DVec2::from),
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
        game.enter(args.start_state(), args.seed, args.world);
        Ok(game)
    }

    /// Go to a start screen (`--ui-state`).
    fn enter(&mut self, state: UiState, seed: u64, world: (i32, i32)) {
        if state.has_world() {
            self.start_new_world(seed, world);
        }
        match state {
            UiState::Menu | UiState::Playing => {}
            UiState::NewGame => self.ui.ui.open_menu(MenuPage::NewGame),
            UiState::Load => self.ui.ui.open_menu(MenuPage::Load),
            UiState::Settings => self.ui.ui.open_menu(MenuPage::Settings),
            UiState::Inventory => self.ui.ui.open_window(WindowKind::Character),
            UiState::Debug => self.ui.model.settings.show_debug = true,
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
                self.held = Held::default();
                self.stroke.end();
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
    fn reset_view(&mut self, center: DVec2, world_cells: DVec2) {
        self.renderer.clear_chunks();
        self.cells.clear();
        self.debug_chunks.clear();
        self.last_view = None;
        self.tick = 0;
        self.stroke.end();
        let viewport = self.controls.camera.viewport;
        let zoom = self.start_zoom.unwrap_or_else(|| (viewport.x as f32 / 1100.0).round().clamp(1.0, MAX_ZOOM));
        self.controls = CameraControl::new(center, zoom, viewport, world_cells);
        self.controls.min_zoom = self.renderer.min_zoom(viewport, VIEW_MARGIN).max(1.0);
    }

    fn start_new_world(&mut self, seed: u64, chunks: (i32, i32)) {
        self.world = None; // Stops the old simulation thread.
        let demo = demo::build(self.content.clone(), chunks, seed);
        let (w, h) = demo.sim.size_cells();
        let center = self.start_center.unwrap_or(DVec2::from(demo.start_center));
        self.reset_view(center, DVec2::new(w as f64, h as f64));
        self.world = Some(World { sim: SimThread::start(demo.sim), seed, chunks, user_paused: false, overlay: false });
        self.ui.model.state = GameState::Playing;
    }

    /// Load a save when no world runs (from the main menu). The file is read here, then the
    /// simulation thread starts with it.
    fn load_from_menu(&mut self, path: &Path, now: Instant) {
        match Simulation::load_file(self.content.clone(), path) {
            Ok((sim, report)) => {
                let (w, h) = sim.size_cells();
                let meta = saves::read_meta(path).unwrap_or_default();
                self.world = None;
                self.reset_view(DVec2::new(w as f64 * 0.48, h as f64 * 0.55), DVec2::new(w as f64, h as f64));
                let chunks = (w / foundry_core::CHUNK_SIZE, h / foundry_core::CHUNK_SIZE);
                self.world = Some(World { sim: SimThread::start(sim), seed: meta.seed, chunks, user_paused: false, overlay: false });
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

    fn load(&mut self, id: &str, now: Instant) {
        let path = self.ui.saves_dir.join(id);
        let Some(world) = self.world.as_mut() else {
            self.load_from_menu(&path, now);
            return;
        };
        // The simulation thread loads the file and reports the result in a notice. If the load
        // fails, the old world stays: `ResendAll` makes it send its chunks again.
        let meta = saves::read_meta(&path).unwrap_or_default();
        world.seed = meta.seed;
        world.user_paused = false;
        world.sim.send(Command::LoadWorld { path });
        let view = self.last_view.unwrap_or_else(|| self.controls.view_area(VIEW_MARGIN));
        world.sim.send(Command::SetView { area: view });
        world.sim.send(Command::ResendAll);
        world.sim.send(Command::SetPaused(false));
        self.renderer.clear_chunks();
        self.cells.clear();
        self.ui.model.state = GameState::Playing;
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
        self.held = Held::default();
        self.stroke.end();
        self.ui.refresh_saves();
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
        if self.ui.sandbox_action(&action) {
            return;
        }
        match action {
            UiAction::NewGame { seed, size } => {
                self.start_new_world(seed, size.chunks());
                self.ui.message(format!("New world: seed {seed}"), now);
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
            | UiAction::Craft { .. }
            | UiAction::CancelCraft { .. }
            | UiAction::SetRecipe { .. }
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
        self.ui.model.hover = if playing && self.mouse_inside && !over_ui { self.hover_cell() } else { None };
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

        // 2. The UI.
        self.update_model(frame_start);
        let raw_input = self.egui_state.take_egui_input(&self.window);
        let ppp = self.egui_ctx.pixels_per_point();
        let brush_pos = egui::pos2((self.mouse.x as f32) / ppp, (self.mouse.y as f32) / ppp);
        let brush_radius = (self.ui.brush_radius() as f32 + 0.5) * self.controls.camera.zoom / ppp;
        let show_brush = self.playing() && self.mouse_inside;
        let show_debug = self.ui.model.settings.show_debug && self.world.is_some();
        let (paused, overlay) = self.world.as_ref().map_or((false, false), |w| (w.user_paused, w.overlay));
        let mut actions = Vec::new();
        let mut debug_actions = Vec::new();
        let mut full_output = {
            let ui = &mut self.ui;
            let stats = &self.stats;
            let debug_chunks = &self.debug_chunks;
            let camera = &self.controls.camera;
            self.egui_ctx.run_ui(raw_input, |root| {
                if show_debug {
                    debug_panel::draw(root, stats, paused, overlay, &mut debug_actions);
                }
                actions = ui.ui.show(root.ctx(), &ui.model);
                let painter = root.ctx().layer_painter(egui::LayerId::background());
                if overlay {
                    debug_panel::draw_overlay(&painter, debug_chunks, camera, root.ctx().pixels_per_point());
                }
                if show_brush && !root.ctx().is_pointer_over_egui() {
                    painter.circle_stroke(brush_pos, brush_radius, egui::Stroke::new(1.0, egui::Color32::from_white_alpha(140)));
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

        // 3. Camera, brush and view area.
        if self.playing() {
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
        }
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

