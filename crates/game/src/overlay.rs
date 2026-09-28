//! Shapes drawn over the world in the normal mode, with the egui painter of the background layer
//! (under the UI windows): the jetpack fuel gauge, the dig circle, a status icon over each
//! building that does not work, and the construction shapes (`construct_draw.rs`).
//!
//! The robot itself is drawn by the world renderer as sprites (`robot_sprite.rs`).
//!
//! The world renderer does not change. Buildings are body cells, so the renderer draws them.

use crate::construct::BuildView;
use crate::construct_draw;
use crate::factory_host::FactoryFrame;
use crate::player::{ROBOT_H, ROBOT_W, Robot};
use crate::tools;
use egui::{Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Stroke, StrokeKind, pos2, vec2};
use foundry_content::Content;
use foundry_core::CellRect;
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
    // The robot itself is a sprite in the world renderer (`robot_sprite.rs`).
    if let Some(r) = &frame.robot {
        let at = s.robot_at.unwrap_or_else(|| r.draw_top_left());
        fuel_gauge(painter, &v, r, at, frame.tick);
    }
    construct_draw::draw_over(painter, &v, frame, s.build, s.content, s.atlas);
    let b = s.build;
    // The dig circle, but not on a building (a click there opens it).
    let on_building = |m: foundry_core::CellPos| frame.hover.as_ref().is_some_and(|h| h.rect.contains(m));
    if !b.grid
        && !b.removing
        && let (Some(mouse), Some(r)) = (b.mouse, &frame.robot)
        && !on_building(mouse)
    {
        // The dig circle at the aim point (moved into reach).
        let aim = tools::clamp_aim(r, mouse);
        let c = v.pos(aim.x as f64 + 0.5, aim.y as f64 + 0.5);
        let radius = (tools::DIG_RADIUS as f32 + 0.5) * v.scale();
        painter.circle_stroke(c, radius, Stroke::new(1.0, Color32::from_white_alpha(110)));
    }
}

/// The jetpack fuel gauge: a thin bar behind the robot, as high as the robot.
/// - It shows while the jetpack runs and while the fuel is not full.
/// - Flying: orange. In the air with no push: yellow. Empty: the frame blinks red and "Jet
///   empty" shows. On the ground, before the refill starts: gray. Refilling: cyan, with an arrow.
fn fuel_gauge(p: &Painter, v: &View, r: &Robot, at: (f32, f32), tick: u64) {
    let fraction = r.fuel_fraction();
    if fraction >= 1.0 && !r.jetting {
        return;
    }
    let s = v.scale();
    let (x, y) = (at.0 as f64, at.1 as f64);
    // Behind the backpack.
    let bar_x = if r.facing >= 0 { x - 4.0 } else { x + ROBOT_W as f64 + 3.0 };
    let top = v.pos(bar_x, y + 1.0);
    let height = (ROBOT_H as f32 - 2.0) * s;
    let width = (s * 1.2).clamp(4.0, 9.0);
    let frame = Rect::from_min_size(top, vec2(width, height));
    let empty = r.fuel <= 0.0;
    let blink = (tick / 8).is_multiple_of(2);
    let (fill, label) = if r.jetting {
        (Color32::from_rgb(255, 160, 50), None)
    } else if empty && !r.on_ground {
        (Color32::from_rgb(230, 60, 50), Some("Jet empty"))
    } else if r.refilling() {
        (Color32::from_rgb(110, 225, 255), None)
    } else if r.on_ground {
        (Color32::from_rgb(130, 140, 155), None)
    } else {
        (Color32::from_rgb(240, 200, 90), None)
    };
    p.rect_filled(frame.expand(1.0), CornerRadius::same(2), Color32::from_black_alpha(170));
    let level = Rect::from_min_max(pos2(frame.left(), frame.bottom() - height * fraction), frame.max);
    p.rect_filled(level, CornerRadius::ZERO, fill);
    let edge = if empty && blink { Color32::from_rgb(255, 70, 60) } else { Color32::from_white_alpha(70) };
    p.rect_stroke(frame.expand(1.0), CornerRadius::same(2), Stroke::new(1.0, edge), StrokeKind::Outside);
    if r.refilling() {
        // An arrow up over the bar: the fuel comes back.
        let c = pos2(frame.center().x, frame.top() - 6.0);
        let k = width * 0.6;
        p.line_segment([c + vec2(-k, 3.0), c], Stroke::new(1.5, fill));
        p.line_segment([c + vec2(k, 3.0), c], Stroke::new(1.5, fill));
    }
    if let Some(text) = label.filter(|_| blink || !empty) {
        let pos = pos2(frame.center().x, frame.top() - 4.0);
        p.text(pos, Align2::CENTER_BOTTOM, text, FontId::proportional(12.0), Color32::from_rgb(255, 110, 90));
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
