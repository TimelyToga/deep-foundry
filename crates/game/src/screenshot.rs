//! `--screenshot`: build the demo world, run some ticks with no thread and no window,
//! render one frame with the real renderer, and save it as a PNG file.

use crate::args::Args;
use crate::demo;
use anyhow::{Context, Result};
use foundry_content::Content;
use foundry_core::Command;
use crate::ui::{self, PaintMaterial, PanelState, StatsView};
use foundry_render::headless::{CAPTURE_FORMAT, capture, capture_with, create_device};
use foundry_render::wgpu;
use foundry_render::{Camera, Renderer};
use glam::{DVec2, UVec2};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

pub fn run(args: &Args, out: &Path, content: Arc<Content>) -> Result<()> {
    let start = Instant::now();
    let demo = demo::build(content.clone(), args.world, args.seed);
    let mut sim = demo.sim;
    let built = start.elapsed();

    let size = args.image_size();
    let center = args.center.map_or(DVec2::from(demo.start_center), DVec2::from);
    let camera = Camera::new(center, args.zoom.unwrap_or(2.0), UVec2::new(size.0, size.1));

    let tick_start = Instant::now();
    for _ in 0..args.ticks {
        sim.tick();
    }
    let ticks_time = tick_start.elapsed();

    // Ask for the chunks on the screen, like the game does.
    sim.apply(Command::SetView { area: camera.visible_rect().expand(1) });
    let snapshot = sim.take_snapshot();

    let (device, queue) = create_device().context("no GPU adapter found")?;
    let mut renderer = Renderer::new(&device, &queue, CAPTURE_FORMAT, &content);
    renderer.set_time(args.ticks as f64 * foundry_core::TICK_SECONDS);
    let evicted = renderer.apply_snapshot(&snapshot, &camera);
    if !evicted.is_empty() {
        log::warn!("{} chunks did not fit on the GPU; use a larger zoom", evicted.len());
    }
    let pixels = if args.ui {
        let stats = StatsView {
            tick: snapshot.stats.tick,
            tick_ms: snapshot.stats.tick_ms,
            awake_chunks: snapshot.stats.awake_chunks,
            loaded_chunks: snapshot.stats.loaded_chunks,
            gpu_chunks: renderer.stats().resident_chunks,
            gpu_capacity: renderer.stats().chunk_capacity,
            zoom: camera.zoom,
            ..Default::default()
        };
        let mut panel = OffscreenPanel::new(&device, &content, size);
        panel.layout(&device, &queue, &stats);
        capture_with(&device, &queue, &mut renderer, &camera, |encoder, view| panel.draw(&device, &queue, encoder, view))
    } else {
        capture(&device, &queue, &mut renderer, &camera)
    };

    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("cannot make folder {}", dir.display()))?;
    }
    let image = image::RgbaImage::from_raw(size.0, size.1, pixels).context("image size does not match")?;
    image.save(out).with_context(|| format!("cannot write {}", out.display()))?;

    let view = camera.visible_rect();
    println!(
        "saved {} ({}x{}, zoom {}, center {:.1},{:.1}, cells {}..{} x {}..{}); world built in {:.0} ms, {} ticks in {:.0} ms, {} chunks drawn",
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
        built.as_secs_f64() * 1000.0,
        args.ticks,
        ticks_time.as_secs_f64() * 1000.0,
        renderer.stats().drawn_chunks,
    );
    Ok(())
}

/// The egui side panel, drawn with no window.
struct OffscreenPanel {
    ctx: egui::Context,
    renderer: egui_wgpu::Renderer,
    materials: Vec<PaintMaterial>,
    selected: usize,
    screen: egui_wgpu::ScreenDescriptor,
    jobs: Vec<egui::ClippedPrimitive>,
}

impl OffscreenPanel {
    fn new(device: &wgpu::Device, content: &Content, size: (u32, u32)) -> Self {
        let ctx = egui::Context::default();
        ui::apply_style(&ctx);
        let materials = ui::paint_materials(content);
        let selected = materials.iter().position(|m| content.materials.ids[m.id.index()] == "sand").unwrap_or(0);
        // Large images look like a high-density display.
        let pixels_per_point = if size.0 >= 2400 { 2.0 } else { 1.0 };
        Self {
            ctx,
            renderer: egui_wgpu::Renderer::new(device, CAPTURE_FORMAT, egui_wgpu::RendererOptions::default()),
            materials,
            selected,
            screen: egui_wgpu::ScreenDescriptor { size_in_pixels: [size.0, size.1], pixels_per_point },
            jobs: Vec::new(),
        }
    }

    /// Run the panel code. It runs twice, because egui measures text in the first pass.
    fn layout(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, stats: &StatsView) {
        let ppp = self.screen.pixels_per_point;
        let size = egui::vec2(self.screen.size_in_pixels[0] as f32, self.screen.size_in_pixels[1] as f32) / ppp;
        let mut brush = 6u16;
        for _ in 0..2 {
            let mut raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)), ..Default::default() };
            raw.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(ppp);
            let mut actions = Vec::new();
            let (materials, selected) = (&self.materials, &mut self.selected);
            let mut out = self.ctx.run_ui(raw, |ui| {
                let state = PanelState { materials, selected: &mut *selected, brush_radius: &mut brush, paused: false, show_stats: true, stats };
                ui::draw(ui, state, &mut actions);
            });
            for (id, deltas) in out.textures_delta.set.drain() {
                for delta in &deltas {
                    self.renderer.update_texture(device, queue, id, delta);
                }
            }
            for id in out.textures_delta.free.drain() {
                self.renderer.free_texture(&id);
            }
            self.jobs = self.ctx.tessellate(std::mem::take(&mut out.shapes), out.pixels_per_point);
        }
    }

    fn draw(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        // No paint callbacks are used, so there are no extra command buffers to send.
        let _ = ui::render_egui(&mut self.renderer, device, queue, encoder, target, &self.jobs, &self.screen);
    }
}
