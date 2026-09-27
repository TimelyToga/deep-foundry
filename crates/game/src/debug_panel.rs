//! The debug panel (key F3): simulation controls, the chunk overlay, and numbers about the
//! renderer and the simulation. It is the old side panel of the demo, without the material list
//! (the sandbox inventory has the materials now).

use egui::{Color32, RichText};
use foundry_core::DebugChunk;
use foundry_render::Camera;
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
    pub zoom: f32,
    /// Cell under the mouse.
    pub cursor: Option<(i32, i32)>,
}

/// What the player asked for in the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugAction {
    TogglePause,
    Step,
    /// Show or hide the chunk overlay.
    ToggleOverlay,
}

pub const PANEL_WIDTH: f32 = 230.0;

/// Draw the panel at the left side of the screen.
pub fn draw(ui: &mut egui::Ui, stats: &StatsView, paused: bool, overlay: bool, actions: &mut Vec<DebugAction>) {
    let frame = egui::Frame::new()
        .fill(Color32::from_rgba_unmultiplied(24, 24, 26, 235))
        .inner_margin(egui::Margin::symmetric(12, 10));
    egui::Panel::left("debug-panel").resizable(false).exact_size(PANEL_WIDTH).frame(frame).show(ui, |ui| {
        ui.label(RichText::new("Debug (F3)").heading().strong().color(Color32::from_rgb(255, 230, 192)));
        ui.add_space(6.0);
        section(ui, "Simulation");
        ui.horizontal(|ui| {
            let text = if paused { "Resume" } else { "Pause" };
            if ui.button(text).on_hover_text("Space").clicked() {
                actions.push(DebugAction::TogglePause);
            }
            if ui.add_enabled(paused, egui::Button::new("One tick")).on_hover_text("Period (.)").clicked() {
                actions.push(DebugAction::Step);
            }
        });
        ui.scope(|ui| {
            // The UI style has dark text on light buttons; this label is on the dark panel.
            let widgets = &mut ui.visuals_mut().widgets;
            widgets.inactive.fg_stroke.color = Color32::WHITE;
            widgets.hovered.fg_stroke.color = Color32::WHITE;
            let mut on = overlay;
            let hint = "Green: the part of a chunk that changed in the last tick.";
            if ui.checkbox(&mut on, "Chunk overlay").on_hover_text(hint).changed() {
                actions.push(DebugAction::ToggleOverlay);
            }
        });
        ui.add_space(8.0);
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
            row("zoom", format!("{:.2}", stats.zoom));
            if let Some((x, y)) = stats.cursor {
                row("cursor", format!("{x}, {y}"));
            }
        });
    });
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).strong().color(Color32::from_gray(170)));
    ui.separator();
}

/// Draw the chunk overlay: the changed part of each awake chunk, in screen space.
/// `pixels_per_point` converts screen pixels to egui points.
pub fn draw_overlay(painter: &egui::Painter, chunks: &[DebugChunk], camera: &Camera, pixels_per_point: f32) {
    let stroke = egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(80, 240, 120, 200));
    for c in chunks {
        let r = c.updated;
        let a = camera.cell_to_screen(DVec2::new(r.x0 as f64, r.y0 as f64));
        let b = camera.cell_to_screen(DVec2::new(r.x1 as f64, r.y1 as f64));
        let rect = egui::Rect::from_min_max(
            egui::pos2(a.x as f32 / pixels_per_point, a.y as f32 / pixels_per_point),
            egui::pos2(b.x as f32 / pixels_per_point, b.y as f32 / pixels_per_point),
        );
        painter.rect_stroke(rect, 0.0, stroke, egui::StrokeKind::Inside);
    }
}
