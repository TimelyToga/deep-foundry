//! `--screenshot`: build the demo world, run some ticks with no thread and no window,
//! render one frame with the real renderer and the UI, and save it as a PNG file.
//!
//! `--ui-state` picks the screen (main menu, pause menu, game, materials window, ...).
//! `--no-ui` draws only the world.

use crate::args::{Args, UiState};
use crate::debug_panel::{self, StatsView};
use crate::demo;
use crate::ui::{self, SandboxUi};
use anyhow::{Context, Result};
use foundry_content::Content;
use foundry_core::{CellPos, Command};
use foundry_render::headless::{CAPTURE_FORMAT, capture, capture_with, create_device};
use foundry_render::wgpu;
use foundry_render::{Camera, Renderer};
use foundry_ui::{GameState, HoverView, MenuPage, PerfView, WindowKind};
use glam::{DVec2, UVec2};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

pub fn run(args: &Args, out: &Path, content: Arc<Content>) -> Result<()> {
    let start = Instant::now();
    let state = if args.no_ui { UiState::Playing } else { args.ui_state.unwrap_or(UiState::Playing) };
    let demo = demo::build(content.clone(), args.shape(), args.seed);
    let mut sim = demo.sim;
    let built = start.elapsed();

    let size = args.image_size();
    let center = args.center.map_or(DVec2::from(demo.start_center), DVec2::from);
    let camera = Camera::new(center, args.zoom.unwrap_or(2.0), UVec2::new(size.0, size.1));

    // Ask for the chunks on the screen, like the game does. The view is also the anchor: only
    // chunks near it are made and updated.
    sim.apply(Command::SetView { area: camera.visible_rect().expand(1) });
    let tick_start = Instant::now();
    for _ in 0..args.ticks {
        sim.tick();
    }
    let ticks_time = tick_start.elapsed();
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
            UiState::Playing | UiState::Inventory | UiState::Debug => {
                ui.model.state = GameState::Playing;
                if state == UiState::Inventory {
                    ui.ui.open_window(WindowKind::Character);
                }
            }
        }
        if state.has_world() {
            // The HUD shows the cell at the image center (there is no mouse).
            let pos = CellPos::new(center.x.floor() as i32, center.y.floor() as i32);
            let cell = sim.cell(pos);
            ui.model.hover = Some(HoverView::Cell { pos, material: cell.material, temperature: cell.temperature as f32 });
        }
        let mut panel = OffscreenUi::new(&device, ctx, ui, size);
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

/// The game UI, drawn with no window.
struct OffscreenUi {
    renderer: egui_wgpu::Renderer,
    ui: SandboxUi,
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
            let mut out = self.ctx.run_ui(raw, |root| {
                if show_debug {
                    debug_panel::draw(root, stats, false, false, &mut Vec::new());
                }
                let _ = ui.ui.show(root.ctx(), &ui.model);
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
