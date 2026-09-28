//! Shapes drawn over the world in the normal mode, with the egui painter of the background layer
//! (under the UI windows): the robot, the dig circle, a status icon over each building that does
//! not work, and the construction shapes (`construct_draw.rs`).
//!
//! The world renderer does not change. Buildings are body cells, so the renderer draws them.

use crate::construct::BuildView;
use crate::construct_draw;
use crate::factory_host::FactoryFrame;
use crate::player::{ROBOT_H, ROBOT_W, Robot};
use crate::tools;
use egui::{Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Stroke, StrokeKind, pos2, vec2};
use foundry_content::Content;
use foundry_core::{CellPos, CellRect};
use foundry_factory::Status;
use foundry_render::Camera;
use foundry_ui::icons::IconAtlas;
use glam::DVec2;

/// Screen points for world cells.
pub struct View<'a> {
    pub camera: &'a Camera,
    pub ppp: f32,
}

impl View<'_> {
    pub fn pos(&self, x: f64, y: f64) -> Pos2 {
        let s = self.camera.cell_to_screen(DVec2::new(x, y));
        pos2(s.x as f32 / self.ppp, s.y as f32 / self.ppp)
    }

    pub fn rect(&self, x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
        Rect::from_min_max(self.pos(x0, y0), self.pos(x1, y1))
    }

    pub fn cell_rect(&self, r: CellRect) -> Rect {
        self.rect(r.x0 as f64, r.y0 as f64, r.x1 as f64, r.y1 as f64)
    }

    /// Points per cell.
    pub fn scale(&self) -> f32 {
        self.camera.zoom / self.ppp
    }
}

/// What `draw` needs besides the factory frame.
pub struct Scene<'a> {
    pub camera: &'a Camera,
    /// Screen pixels per UI point.
    pub ppp: f32,
    /// The robot's top-left corner to draw (between two ticks); `None` uses the frame.
    pub robot_at: Option<(f32, f32)>,
    pub build: &'a BuildView,
    pub content: &'a Content,
    /// The item icons of the UI (for the ghost and the alt mode).
    pub atlas: Option<&'a IconAtlas>,
}

/// Draw everything for this frame.
pub fn draw(painter: &Painter, frame: &FactoryFrame, s: &Scene) {
    let v = View { camera: s.camera, ppp: s.ppp };
    construct_draw::draw_under(painter, &v, frame, s.build, s.atlas);
    for m in frame.marks.iter().filter(|m| m.status.is_problem()) {
        status_mark(painter, &v, m.rect, m.status);
    }
    for (r, text) in &frame.labels {
        label(painter, &v, *r, text);
    }
    let b = s.build;
    let aim = aim_point(frame, b.mouse);
    if let Some(r) = &frame.robot {
        let at = s.robot_at.unwrap_or((r.left as f32 + r.rem.0, r.top as f32 + r.rem.1));
        robot(painter, &v, r, at, frame, aim);
    }
    construct_draw::draw_over(painter, &v, frame, s.build, s.content, s.atlas);
    // The dig circle, but not on a building (a click there opens it).
    let on_building = |m: foundry_core::CellPos| frame.hover.as_ref().is_some_and(|h| h.rect.contains(m));
    if !b.grid && !b.removing && frame.robot.is_some() && b.mouse.is_some_and(|m| !on_building(m)) {
        // The dig circle at the aim point (moved into reach).
        let c = v.pos(aim.x as f64 + 0.5, aim.y as f64 + 0.5);
        let radius = (tools::DIG_RADIUS as f32 + 0.5) * v.scale();
        painter.circle_stroke(c, radius, Stroke::new(1.0, Color32::from_white_alpha(110)));
    }
}

/// The aim point of the tools to draw: the mouse cell of this frame, moved into reach of the
/// robot. So the dig circle and the end of the tool beam follow the mouse in every frame, also
/// when the next tick is late. With no mouse over the world (`mouse` is `None`): the aim point
/// that the last tick used.
pub fn aim_point(frame: &FactoryFrame, mouse: Option<CellPos>) -> CellPos {
    match (mouse, &frame.robot) {
        (Some(m), Some(r)) => tools::clamp_aim(r, m),
        _ => frame.aim,
    }
}

fn robot(p: &Painter, v: &View, r: &Robot, at: (f32, f32), frame: &FactoryFrame, aim: CellPos) {
    let (x, y) = (at.0 as f64, at.1 as f64);
    let (w, h) = (ROBOT_W as f64, ROBOT_H as f64);
    let outline = Stroke::new(1.0, Color32::from_rgb(30, 26, 22));
    let s = v.scale();
    let round = CornerRadius::same((s * 1.2).min(6.0) as u8);
    // Legs: a walking robot moves them.
    let step = if r.on_ground && r.vel.0.abs() > 0.1 { ((frame.tick / 6) % 2) as f64 } else { 0.5 };
    let leg = Color32::from_rgb(70, 70, 78);
    p.rect(v.rect(x + 1.0, y + h - 4.0, x + 3.0, y + h - step), CornerRadius::ZERO, leg, outline, StrokeKind::Inside);
    p.rect(v.rect(x + w - 3.0, y + h - 4.0, x + w - 1.0, y + h - (1.0 - step)), CornerRadius::ZERO, leg, outline, StrokeKind::Inside);
    // Body and head.
    let body = Color32::from_rgb(232, 160, 48);
    p.rect(v.rect(x, y + 5.0, x + w, y + h - 3.5), round, body, outline, StrokeKind::Inside);
    p.rect(v.rect(x + 1.0, y, x + w - 1.0, y + 5.5), round, Color32::from_rgb(210, 205, 196), outline, StrokeKind::Inside);
    // The visor looks in the facing direction.
    let (ex0, ex1) = if r.facing < 0 { (x + 1.5, x + 4.5) } else { (x + w - 4.5, x + w - 1.5) };
    p.rect_filled(v.rect(ex0, y + 1.5, ex1, y + 3.5), CornerRadius::ZERO, Color32::from_rgb(90, 220, 255));
    // A band on the body.
    p.rect_filled(v.rect(x + 1.0, y + 8.0, x + w - 1.0, y + 9.0), CornerRadius::ZERO, Color32::from_rgb(120, 70, 20));
    // The tool beam while digging or spraying.
    if frame.digging || frame.spraying {
        let from = v.pos(x + w * 0.5 + r.facing as f64 * 3.0, y + 7.0);
        let to = v.pos(aim.x as f64 + 0.5, aim.y as f64 + 0.5);
        let c = if frame.digging { Color32::from_rgba_unmultiplied(255, 220, 120, 190) } else { Color32::from_rgba_unmultiplied(140, 200, 255, 190) };
        p.line_segment([from, to], Stroke::new((s * 0.6).clamp(1.0, 3.0), c));
        p.circle_filled(to, (s * 1.5).clamp(2.0, 6.0), c);
    }
}

/// A name tag over the top middle of a building.
fn label(p: &Painter, v: &View, r: foundry_core::CellRect, text: &str) {
    let top = v.pos((r.x0 + r.x1) as f64 * 0.5, r.y0 as f64) - vec2(0.0, 6.0);
    let font = FontId::proportional(14.0);
    let galley = p.layout_no_wrap(text.to_string(), font.clone(), Color32::WHITE);
    let bg = Rect::from_center_size(top - vec2(0.0, galley.size().y * 0.5), galley.size() + vec2(12.0, 4.0));
    p.rect_filled(bg, CornerRadius::same(3), Color32::from_black_alpha(170));
    p.text(top, Align2::CENTER_BOTTOM, text, font, Color32::from_rgb(255, 214, 140));
}

/// A round icon with "!" over the top middle of a building that does not work.
fn status_mark(p: &Painter, v: &View, r: foundry_core::CellRect, status: Status) {
    let color = match status {
        Status::Broken | Status::TooHot | Status::NoPower => Color32::from_rgb(220, 60, 50),
        _ => Color32::from_rgb(240, 190, 40),
    };
    let top = v.pos((r.x0 + r.x1) as f64 * 0.5, r.y0 as f64);
    let radius = (v.scale() * 3.0).clamp(7.0, 14.0);
    let c = top - vec2(0.0, radius + 3.0);
    p.circle(c, radius, color, Stroke::new(1.5, Color32::from_black_alpha(200)));
    p.text(c, Align2::CENTER_CENTER, "!", FontId::proportional(radius * 1.5), Color32::from_rgb(30, 20, 10));
}

#[cfg(test)]
#[path = "overlay_tests.rs"]
mod tests;
