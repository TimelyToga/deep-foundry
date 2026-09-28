//! The debug panel (key F3): simulation controls, debug views, light settings, the time of each
//! part of the simulation tick, and numbers about the renderer and the simulation.
//!
//! The debug views also have keys (default F4 awake chunks, F5 heat map, F6 chunk grid). They work
//! when the panel is closed too.

use egui::{Color32, RichText};
use foundry_core::DebugChunk;
use foundry_render::{Camera, RenderSettings};
use glam::DVec2;

/// Numbers for the panel.
#[derive(Debug, Clone, Default)]
pub struct StatsView {
    pub fps: f32,
    pub frame_ms: f32,
    pub cpu_ms: f32,
    pub tick: u64,
    pub tick_ms: f32,
    pub ticks_per_second: f32,
    pub awake_chunks: u32,
    pub loaded_chunks: u32,
    /// Changed chunks far from the view, packed in memory.
    pub packed_chunks: u32,
    pub gpu_chunks: u32,
    pub gpu_capacity: u32,
    pub drawn_chunks: u32,
    pub particles: u32,
    /// Size of the light map in texels (0 when the light is off).
    pub light_size: (u32, u32),
    pub zoom: f32,
    /// Cell under the mouse.
    pub cursor: Option<(i32, i32)>,
    /// Time of each part of the last tick in milliseconds (`SimStats::sections`).
    pub sections: Vec<(&'static str, f32)>,
}

/// The state that the panel shows and changes.
#[derive(Debug, Clone, Default)]
pub struct PanelState {
    pub paused: bool,
    /// The awake chunks view is on.
    pub awake_chunks: bool,
    pub render: RenderSettings,
    /// Key names of the three debug views: awake chunks, heat map, chunk grid.
    pub view_keys: [String; 3],
}

/// What the player asked for in the panel (or with a debug key).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DebugAction {
    TogglePause,
    Step,
    /// Show or hide the awake chunks and their update rectangles.
    ToggleAwakeChunks,
    /// Use these render settings (views, light).
    Render(RenderSettings),
}

pub const PANEL_WIDTH: f32 = 250.0;

/// Colors of the tick part bars, in turn.
const BAR_COLORS: [Color32; 6] = [
    Color32::from_rgb(96, 170, 255),
    Color32::from_rgb(255, 170, 70),
    Color32::from_rgb(120, 220, 120),
    Color32::from_rgb(230, 110, 200),
    Color32::from_rgb(250, 90, 80),
    Color32::from_rgb(200, 200, 110),
];

/// Draw the panel at the left side of the screen.
pub fn draw(ui: &mut egui::Ui, stats: &StatsView, state: &PanelState, actions: &mut Vec<DebugAction>) {
    let frame = egui::Frame::new()
        .fill(Color32::from_rgba_unmultiplied(24, 24, 26, 235))
        .inner_margin(egui::Margin::symmetric(12, 10));
    egui::Panel::left("debug-panel").resizable(false).exact_size(PANEL_WIDTH).frame(frame).show(ui, |ui| {
        // The UI style has dark text on light buttons; the labels here are on the dark panel.
        let widgets = &mut ui.visuals_mut().widgets;
        widgets.inactive.fg_stroke.color = Color32::WHITE;
        widgets.hovered.fg_stroke.color = Color32::WHITE;
        widgets.active.fg_stroke.color = Color32::WHITE;
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.label(RichText::new("Debug (F3)").heading().strong().color(Color32::from_rgb(255, 230, 192)));
            ui.add_space(6.0);
            simulation(ui, state, actions);
            ui.add_space(8.0);
            views(ui, state, actions);
            ui.add_space(8.0);
            light(ui, state, actions);
            ui.add_space(8.0);
            tick_bars(ui, stats);
            ui.add_space(8.0);
            numbers(ui, stats);
        });
    });
}

fn simulation(ui: &mut egui::Ui, state: &PanelState, actions: &mut Vec<DebugAction>) {
    section(ui, "Simulation");
    ui.horizontal(|ui| {
        let text = if state.paused { "Resume" } else { "Pause" };
        if ui.button(dark(text)).on_hover_text("Space").clicked() {
            actions.push(DebugAction::TogglePause);
        }
        if ui.add_enabled(state.paused, egui::Button::new(dark("One tick"))).on_hover_text("Period (.)").clicked() {
            actions.push(DebugAction::Step);
        }
    });
}

fn views(ui: &mut egui::Ui, state: &PanelState, actions: &mut Vec<DebugAction>) {
    section(ui, "Views");
    let [chunk_key, heat_key, grid_key] = &state.view_keys;
    let mut on = state.awake_chunks;
    let hint = "Yellow: awake chunks. Green: the cells that the last tick updated.";
    if ui.checkbox(&mut on, format!("Awake chunks ({chunk_key})")).on_hover_text(hint).changed() {
        actions.push(DebugAction::ToggleAwakeChunks);
    }
    let mut r = state.render;
    let mut changed = false;
    changed |= ui
        .checkbox(&mut r.heat_map, format!("Heat map ({heat_key})"))
        .on_hover_text("Blue: cold. Dark green: 20 °C. Then yellow, orange, red and white (1600 °C).")
        .changed();
    changed |= ui.checkbox(&mut r.chunk_grid, format!("Chunk grid ({grid_key})")).on_hover_text("Chunks are 64 x 64 cells, tiles 8 x 8").changed();
    changed |= ui.checkbox(&mut r.light_only, "Light map only").changed();
    if changed {
        actions.push(DebugAction::Render(r));
    }
}

fn light(ui: &mut egui::Ui, state: &PanelState, actions: &mut Vec<DebugAction>) {
    section(ui, "Light");
    let mut r = state.render;
    let mut changed = false;
    ui.horizontal(|ui| {
        changed |= ui.checkbox(&mut r.lighting, "Light").changed();
        changed |= ui.checkbox(&mut r.bloom, "Bloom").changed();
        changed |= ui.checkbox(&mut r.heat_shimmer, "Shimmer").changed();
    });
    let mut slider = |ui: &mut egui::Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, name: &str, hint: &str| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(name).weak());
            changed |= ui.add(egui::Slider::new(value, range).max_decimals(3)).on_hover_text(hint).changed();
        });
    };
    slider(ui, &mut r.ambient, 0.0..=0.3, "Ambient", "Light that is everywhere, also deep underground");
    slider(ui, &mut r.sky_light, 0.0..=1.5, "Sky", "Brightness of the sky light");
    slider(ui, &mut r.light_keep, 0.7..=0.97, "Reach", "Light kept for each 4 cells of distance");
    slider(ui, &mut r.bloom_strength, 0.0..=1.0, "Bloom", "Strength of the glow around bright light");
    ui.horizontal(|ui| {
        ui.label(RichText::new("Steps").weak());
        let hint = "Spread steps of the light: more steps, farther light, more GPU time";
        changed |= ui.add(egui::Slider::new(&mut r.light_steps, 4..=64)).on_hover_text(hint).changed();
    });
    if changed {
        actions.push(DebugAction::Render(r));
    }
}

/// A bar for each part of the last tick. The full bar width is 4 ms; a longer part fills it.
fn tick_bars(ui: &mut egui::Ui, stats: &StatsView) {
    section(ui, "Tick time (ms)");
    const FULL_MS: f32 = 4.0;
    egui::Grid::new("debug-tick-bars").num_columns(3).spacing(egui::vec2(6.0, 3.0)).show(ui, |ui| {
        for (i, (name, ms)) in stats.sections.iter().enumerate() {
            ui.label(RichText::new(*name).weak().small());
            let (rect, _) = ui.allocate_exact_size(egui::vec2(96.0, 8.0), egui::Sense::hover());
            let p = ui.painter();
            p.rect_filled(rect, 1.0, Color32::from_gray(50));
            let fill = (ms / FULL_MS).clamp(0.0, 1.0);
            if fill > 0.0 {
                let bar = egui::Rect::from_min_size(rect.min, egui::vec2((rect.width() * fill).max(1.0), rect.height()));
                p.rect_filled(bar, 1.0, BAR_COLORS[i % BAR_COLORS.len()]);
            }
            ui.label(RichText::new(format!("{ms:.2}")).monospace().small());
            ui.end_row();
        }
    });
    if stats.sections.is_empty() {
        ui.label(RichText::new("No tick yet").weak());
    }
}

fn numbers(ui: &mut egui::Ui, stats: &StatsView) {
    section(ui, "Numbers");
    egui::Grid::new("debug-stats").num_columns(2).spacing(egui::vec2(10.0, 2.0)).show(ui, |ui| {
        let mut row = |name: &str, value: String| {
            ui.label(RichText::new(name).weak());
            ui.label(RichText::new(value).monospace());
            ui.end_row();
        };
        row("FPS", format!("{:.0}", stats.fps));
        row("frame", format!("{:.2} ms", stats.frame_ms));
        row("CPU / frame", format!("{:.2} ms", stats.cpu_ms));
        row("tick", format!("{}", stats.tick));
        row("tick time", format!("{:.2} ms", stats.tick_ms));
        row("ticks / s", format!("{:.1}", stats.ticks_per_second));
        row("awake chunks", format!("{} / {}", stats.awake_chunks, stats.loaded_chunks));
        row("packed chunks", format!("{}", stats.packed_chunks));
        row("GPU chunks", format!("{} / {}", stats.gpu_chunks, stats.gpu_capacity));
        row("drawn chunks", format!("{}", stats.drawn_chunks));
        row("particles", format!("{}", stats.particles));
        row("light map", format!("{} x {}", stats.light_size.0, stats.light_size.1));
        row("zoom", format!("{:.2}", stats.zoom));
        if let Some((x, y)) = stats.cursor {
            row("cursor", format!("{x}, {y}"));
        }
    });
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).strong().color(Color32::from_gray(170)));
    ui.separator();
}

/// Button text: the buttons are light, so their text is dark.
fn dark(text: &str) -> RichText {
    RichText::new(text).color(Color32::from_gray(20))
}

/// Draw the awake chunks view: a yellow outline for each awake chunk, and the part of it that the
/// last tick updated in green. `pixels_per_point` converts screen pixels to egui points.
pub fn draw_overlay(painter: &egui::Painter, chunks: &[DebugChunk], camera: &Camera, pixels_per_point: f32) {
    let to_rect = |x0: i32, y0: i32, x1: i32, y1: i32| {
        let a = camera.cell_to_screen(DVec2::new(x0 as f64, y0 as f64));
        let b = camera.cell_to_screen(DVec2::new(x1 as f64, y1 as f64));
        egui::Rect::from_min_max(
            egui::pos2(a.x as f32 / pixels_per_point, a.y as f32 / pixels_per_point),
            egui::pos2(b.x as f32 / pixels_per_point, b.y as f32 / pixels_per_point),
        )
    };
    let chunk_stroke = egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(250, 210, 60, 150));
    let rect_stroke = egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(80, 240, 120, 220));
    let rect_fill = Color32::from_rgba_unmultiplied(80, 240, 120, 28);
    for c in chunks {
        let o = c.pos.origin();
        let size = foundry_core::CHUNK_SIZE;
        painter.rect_stroke(to_rect(o.x, o.y, o.x + size, o.y + size), 0.0, chunk_stroke, egui::StrokeKind::Inside);
        let r = c.updated;
        if !r.is_empty() {
            let rect = to_rect(r.x0, r.y0, r.x1, r.y1);
            painter.rect_filled(rect, 0.0, rect_fill);
            painter.rect_stroke(rect, 0.0, rect_stroke, egui::StrokeKind::Inside);
        }
    }
}
