//! The window: winit events, the wgpu surface, egui, input and the frame loop.
//!
//! Each frame:
//! 1. Take the newest snapshot. Upload it (`Renderer::apply_snapshot`) and send `ForgetChunks` for
//!    chunks the renderer dropped.
//! 2. Run the egui panel.
//! 3. Move the camera, send brush strokes, and send `SetView` when the view area changed.
//! 4. Render the world, then egui, and present.

use crate::args::Args;
use crate::controls::{CameraControl, MAX_ZOOM, Stroke};
use crate::sim_thread::SimThread;
use crate::ui::{self, PaintMaterial, PanelAction, PanelState, StatsView};
use crate::demo;
use anyhow::{Context, Result};
use foundry_content::Content;
use foundry_core::{CellPos, CellRect, Command, MaterialId, PaintMode};
use foundry_render::headless::device_descriptor;
use foundry_render::{Renderer, wgpu};
use glam::{DVec2, UVec2, Vec2};
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
            game.sim.stop();
            if self.args.exit_after.is_some() {
                game.timing.print_summary(&game);
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

/// Frame timing, for the stats panel and `--exit-after`.
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
            game.sim.ticks_per_second(),
            self.tick_ms_total / self.tick_samples.max(1) as f64,
            game.stats.awake_chunks
        );
        let refresh = game.window.current_monitor().and_then(|m| m.refresh_rate_millihertz()).map_or(0.0, |r| r as f64 / 1000.0);
        println!(
            "window: {}x{} px, zoom {:.2}, present mode {:?}, display refresh {:.0} Hz",
            size.x, size.y, game.controls.camera.zoom, game.config.present_mode, refresh
        );
    }
}

struct Game {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,

    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
    /// egui textures to free after the current frame.
    egui_free: Vec<egui::TextureId>,

    sim: SimThread,
    controls: CameraControl,
    held: Held,
    stroke: Stroke,
    /// Reused list of brush positions.
    paint_points: Vec<CellPos>,
    mouse: DVec2,
    last_view: Option<CellRect>,

    materials: Vec<PaintMaterial>,
    selected: usize,
    brush_radius: u16,
    paused: bool,
    tick: u64,
    show_stats: bool,
    stats: StatsView,
    actions: Vec<PanelAction>,

    timing: Timing,
    exit_after: Option<Duration>,
    /// The surface said it is suboptimal. Configure it after the current frame.
    needs_configure: bool,
}

impl Game {
    fn new(event_loop: &ActiveEventLoop, args: &Args, content: Arc<Content>) -> Result<Self> {
        let demo = demo::build(content.clone(), args.world, args.seed);
        let world = DVec2::new(demo.sim.size_cells().0 as f64, demo.sim.size_cells().1 as f64);

        let mut attrs = Window::default_attributes().with_title("Deep Foundry").with_inner_size(LogicalSize::new(1600.0, 900.0));
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
        let present_mode = if args.no_vsync {
            [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Immediate]
                .into_iter()
                .find(|m| caps.present_modes.contains(m))
                .unwrap_or(wgpu::PresentMode::Fifo)
        } else {
            wgpu::PresentMode::Fifo
        };
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .context("the surface does not work with this GPU")?;
        config.format = format;
        config.present_mode = present_mode;
        config.desired_maximum_frame_latency = 2;
        config.alpha_mode = caps.alpha_modes[0];
        surface.configure(&device, &config);

        let renderer = Renderer::new(&device, &queue, format, &content);

        let egui_ctx = egui::Context::default();
        ui::apply_style(&egui_ctx);
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            Some(device.limits().max_texture_dimension_2d as usize),
        );
        let egui_renderer = egui_wgpu::Renderer::new(&device, format, egui_wgpu::RendererOptions::default());

        let viewport = UVec2::new(config.width, config.height);
        let zoom = args.zoom.unwrap_or_else(|| (viewport.x as f32 / 1100.0).round().clamp(1.0, MAX_ZOOM));
        let center = args.center.map_or(DVec2::from(demo.start_center), DVec2::from);
        let mut controls = CameraControl::new(center, zoom, viewport, world);
        controls.min_zoom = renderer.min_zoom(viewport, VIEW_MARGIN).max(1.0);

        let materials = ui::paint_materials(&content);
        let selected = materials.iter().position(|m| content.materials.ids[m.id.index()] == "sand").unwrap_or(0);
        let sim = SimThread::start(demo.sim);

        Ok(Self {
            window,
            surface,
            device,
            queue,
            config,
            renderer,
            egui_ctx,
            egui_state,
            egui_renderer,
            egui_free: Vec::new(),
            sim,
            controls,
            held: Held::default(),
            stroke: Stroke::default(),
            paint_points: Vec::with_capacity(256),
            mouse: DVec2::ZERO,
            last_view: None,
            materials,
            selected,
            brush_radius: 6,
            paused: false,
            tick: 0,
            show_stats: true,
            stats: StatsView::default(),
            actions: Vec::with_capacity(4),
            timing: Timing::new(args.exit_after),
            exit_after: args.exit_after.map(Duration::from_secs_f64),
            needs_configure: false,
        })
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, event: WindowEvent) {
        let response = self.egui_state.on_window_event(&self.window, &event);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => self.resize(UVec2::new(size.width, size.height)),
            WindowEvent::RedrawRequested => self.frame(event_loop),
            WindowEvent::KeyboardInput { event, .. } => {
                if self.egui_ctx.text_edit_focused() {
                    return;
                }
                let down = event.state == ElementState::Pressed;
                let PhysicalKey::Code(code) = event.physical_key else { return };
                self.key(code, down, event.repeat);
            }
            WindowEvent::ModifiersChanged(m) => self.held.shift = m.state().shift_key(),
            WindowEvent::Focused(false) => {
                // Key and button releases are not sent to a window without focus.
                self.held = Held::default();
                self.stroke.end();
            }
            WindowEvent::CursorMoved { position, .. } => {
                let p = DVec2::new(position.x, position.y);
                if self.held.drag {
                    self.controls.drag(p - self.mouse);
                }
                self.mouse = p;
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let down = state == ElementState::Pressed;
                // Start an action only when the mouse is not over the panel. Always end it.
                if down && (response.consumed || self.egui_ctx.is_pointer_over_egui()) {
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
                if response.consumed || self.egui_ctx.is_pointer_over_egui() {
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

    fn key(&mut self, code: KeyCode, down: bool, repeat: bool) {
        match code {
            KeyCode::KeyA | KeyCode::ArrowLeft => self.held.left = down,
            KeyCode::KeyD | KeyCode::ArrowRight => self.held.right = down,
            KeyCode::KeyW | KeyCode::ArrowUp => self.held.up = down,
            KeyCode::KeyS | KeyCode::ArrowDown => self.held.down = down,
            _ if !down => {}
            KeyCode::Space if !repeat => self.actions.push(PanelAction::TogglePause),
            KeyCode::Period => self.actions.push(PanelAction::Step),
            KeyCode::F3 if !repeat => self.show_stats = !self.show_stats,
            KeyCode::BracketLeft => self.brush_radius = self.brush_radius.saturating_sub(1),
            KeyCode::BracketRight => self.brush_radius = (self.brush_radius + 1).min(ui::MAX_BRUSH),
            _ => {
                let digit = match code {
                    KeyCode::Digit1 => 1,
                    KeyCode::Digit2 => 2,
                    KeyCode::Digit3 => 3,
                    KeyCode::Digit4 => 4,
                    KeyCode::Digit5 => 5,
                    KeyCode::Digit6 => 6,
                    KeyCode::Digit7 => 7,
                    KeyCode::Digit8 => 8,
                    KeyCode::Digit9 => 9,
                    _ => 0,
                };
                if digit > 0 && digit <= self.materials.len() {
                    self.selected = digit - 1;
                }
            }
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
        if let Some(snapshot) = self.sim.take_snapshot() {
            let evicted = self.renderer.apply_snapshot(&snapshot, &self.controls.camera);
            if !evicted.is_empty() {
                self.sim.send(Command::ForgetChunks(evicted));
            }
            if snapshot.tick != self.tick {
                self.timing.tick_ms_total += snapshot.stats.tick_ms as f64;
                self.timing.tick_samples += 1;
            }
            self.tick = snapshot.tick;
            self.paused = snapshot.paused;
            self.stats.tick = snapshot.stats.tick;
            self.stats.tick_ms = snapshot.stats.tick_ms;
            self.stats.awake_chunks = snapshot.stats.awake_chunks;
            self.stats.loaded_chunks = snapshot.stats.loaded_chunks;
            let (w, h) = snapshot.world_cells;
            self.controls.world = DVec2::new(w as f64, h as f64);
        }

        // 2. The panel.
        self.update_stats();
        let raw_input = self.egui_state.take_egui_input(&self.window);
        // The brush outline at the mouse, in egui points.
        let ppp = self.egui_ctx.pixels_per_point();
        let brush = egui::pos2((self.mouse.x as f32) / ppp, (self.mouse.y as f32) / ppp);
        let brush_radius = (self.brush_radius as f32 + 0.5) * self.controls.camera.zoom / ppp;
        let mut full_output = {
            let state = PanelState {
                materials: &self.materials,
                selected: &mut self.selected,
                brush_radius: &mut self.brush_radius,
                paused: self.paused,
                show_stats: self.show_stats,
                stats: &self.stats,
            };
            let mut state = Some(state);
            let actions = &mut self.actions;
            self.egui_ctx.run_ui(raw_input, |ui| {
                if let Some(s) = state.take() {
                    ui::draw(ui, s, actions);
                }
                if !ui.ctx().is_pointer_over_egui() {
                    let painter = ui.ctx().layer_painter(egui::LayerId::background());
                    painter.circle_stroke(brush, brush_radius, egui::Stroke::new(1.0, egui::Color32::from_white_alpha(140)));
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
        for action in self.actions.drain(..) {
            match action {
                PanelAction::TogglePause => {
                    self.paused = !self.paused;
                    self.sim.send(Command::SetPaused(self.paused));
                }
                PanelAction::Step => {
                    if self.paused {
                        self.sim.send(Command::Step);
                    }
                }
            }
        }

        // 3. Camera, brush and view area.
        let pan = Vec2::new(
            (self.held.right as i32 - self.held.left as i32) as f32,
            (self.held.down as i32 - self.held.up as i32) as f32,
        );
        self.controls.update(dt, pan, self.held.shift);
        self.paint();
        let view = self.controls.view_area(VIEW_MARGIN);
        if self.last_view != Some(view) {
            self.sim.send(Command::SetView { area: view });
            self.last_view = Some(view);
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

        self.renderer.set_time(frame_start.duration_since(self.timing.start).as_secs_f64());
        self.renderer.render(&mut encoder, &target, &self.controls.camera);

        // egui on top.
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [self.config.width, self.config.height], pixels_per_point };
        let egui_cmds =
            ui::render_egui(&mut self.egui_renderer, &self.device, &self.queue, &mut encoder, &target, &paint_jobs, &screen);
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

    fn free_egui_textures(&mut self) {
        for id in self.egui_free.drain(..) {
            self.egui_renderer.free_texture(&id);
        }
    }

    /// Send paint commands for the mouse stroke.
    fn paint(&mut self) {
        if !self.held.paint && !self.held.erase {
            return;
        }
        let material = if self.held.paint {
            self.materials.get(self.selected).map_or(MaterialId::AIR, |m| m.id)
        } else {
            MaterialId::AIR
        };
        let cell = self.controls.camera.screen_to_cell(self.mouse);
        self.paint_points.clear();
        self.stroke.advance(cell, self.brush_radius, self.tick, &mut self.paint_points);
        for &center in &self.paint_points {
            self.sim.send(Command::Paint { center, radius: self.brush_radius, material, mode: PaintMode::Replace, temperature: None });
        }
    }

    fn update_stats(&mut self) {
        let r = self.renderer.stats();
        let s = &mut self.stats;
        s.frame_ms = self.timing.frame_ms;
        s.fps = 1000.0 / self.timing.frame_ms.max(0.001);
        s.cpu_ms = self.timing.cpu_ms;
        s.ticks_per_second = self.sim.ticks_per_second();
        s.gpu_chunks = r.resident_chunks;
        s.gpu_capacity = r.chunk_capacity;
        s.drawn_chunks = r.drawn_chunks;
        s.zoom = self.controls.camera.zoom;
        let c = self.controls.camera.screen_to_cell(self.mouse);
        s.cursor = Some((c.x.floor() as i32, c.y.floor() as i32));
    }
}
