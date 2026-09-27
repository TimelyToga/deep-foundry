//! Shapes drawn over the world in the normal mode, with the egui painter of the background layer
//! (under the UI windows): the robot, the dig circle, the ghost of the building in the hand, and
//! a status icon over each building that does not work.
//!
//! The world renderer does not change. Buildings are body cells, so the renderer draws them.

use crate::factory_host::{FactoryFrame, GhostRequest};
use crate::player::{ROBOT_H, ROBOT_W, Robot};
use crate::tools;
use egui::{Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2};
use foundry_content::{PortKind, Side};
use foundry_core::{CellPos, TILE_SIZE};
use foundry_factory::Status;
use foundry_render::Camera;
use glam::DVec2;

/// Screen points for world cells.
struct View<'a> {
    camera: &'a Camera,
    ppp: f32,
}

impl View<'_> {
    fn pos(&self, x: f64, y: f64) -> Pos2 {
        let s = self.camera.cell_to_screen(DVec2::new(x, y));
        pos2(s.x as f32 / self.ppp, s.y as f32 / self.ppp)
    }

    fn rect(&self, x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
        Rect::from_min_max(self.pos(x0, y0), self.pos(x1, y1))
    }

    /// Points per cell.
    fn scale(&self) -> f32 {
        self.camera.zoom / self.ppp
    }
}

/// The building in the hand at the mouse, as the main thread sees it now.
pub struct LocalGhost {
    pub request: GhostRequest,
    /// Size in tiles after the rotation.
    pub size: (u8, u8),
}

/// Draw everything for this frame. `robot_at` is the robot's top-left corner to draw (between
/// two ticks); `None` uses the position in the frame.
pub fn draw(
    painter: &Painter,
    camera: &Camera,
    ppp: f32,
    frame: &FactoryFrame,
    robot_at: Option<(f32, f32)>,
    ghost: Option<&LocalGhost>,
    mouse: Option<CellPos>,
) {
    let v = View { camera, ppp };
    for m in &frame.marks {
        status_mark(painter, &v, m.rect, m.status);
    }
    for (r, text) in &frame.labels {
        label(painter, &v, *r, text);
    }
    if let Some(r) = &frame.robot {
        let at = robot_at.unwrap_or((r.left as f32 + r.rem.0, r.top as f32 + r.rem.1));
        robot(painter, &v, r, at, frame);
    }
    match ghost {
        Some(g) => draw_ghost(painter, &v, g, frame),
        None => {
            if let (Some(_), Some(r)) = (mouse, &frame.robot) {
                // The dig circle at the aim point (moved into reach).
                let aim = tools::clamp_aim(r, mouse.unwrap_or_default());
                let c = v.pos(aim.x as f64 + 0.5, aim.y as f64 + 0.5);
                let radius = (tools::DIG_RADIUS as f32 + 0.5) * v.scale();
                painter.circle_stroke(c, radius, Stroke::new(1.0, Color32::from_white_alpha(110)));
            }
        }
    }
}

fn robot(p: &Painter, v: &View, r: &Robot, at: (f32, f32), frame: &FactoryFrame) {
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
        let to = v.pos(frame.aim.x as f64 + 0.5, frame.aim.y as f64 + 0.5);
        let c = if frame.digging { Color32::from_rgba_unmultiplied(255, 220, 120, 190) } else { Color32::from_rgba_unmultiplied(140, 200, 255, 190) };
        p.line_segment([from, to], Stroke::new((s * 0.6).clamp(1.0, 3.0), c));
        p.circle_filled(to, (s * 1.5).clamp(2.0, 6.0), c);
    }
}

fn draw_ghost(p: &Painter, v: &View, g: &LocalGhost, frame: &FactoryFrame) {
    let at = g.request.at;
    let (x0, y0) = ((at.x * TILE_SIZE) as f64, (at.y * TILE_SIZE) as f64);
    let (x1, y1) = (x0 + (g.size.0 as i32 * TILE_SIZE) as f64, y0 + (g.size.1 as i32 * TILE_SIZE) as f64);
    let rect = v.rect(x0, y0, x1, y1);
    // The check result is for the ghost of the last frame. Use it only if it is for this place.
    let known = frame.ghost.as_ref().filter(|fg| fg.request == g.request);
    let (fill, line) = match known.map(|k| k.error.is_none()) {
        Some(true) => (Color32::from_rgba_unmultiplied(60, 200, 80, 70), Color32::from_rgb(90, 230, 110)),
        Some(false) => (Color32::from_rgba_unmultiplied(220, 50, 40, 70), Color32::from_rgb(240, 80, 60)),
        None => (Color32::from_rgba_unmultiplied(200, 200, 200, 50), Color32::from_gray(210)),
    };
    p.rect(rect, CornerRadius::ZERO, fill, Stroke::new(1.5, line), StrokeKind::Inside);
    // Tile lines.
    let thin = Stroke::new(1.0, line.gamma_multiply(0.35));
    for i in 1..g.size.0 as i32 {
        let x = x0 + (i * TILE_SIZE) as f64;
        p.line_segment([v.pos(x, y0), v.pos(x, y1)], thin);
    }
    for i in 1..g.size.1 as i32 {
        let y = y0 + (i * TILE_SIZE) as f64;
        p.line_segment([v.pos(x0, y), v.pos(x1, y)], thin);
    }
    if let Some(k) = known {
        for port in &k.ports {
            port_arrow(p, v, port.tile.x, port.tile.y, port.side, port.kind);
        }
        if let Some(e) = &k.error {
            let font = FontId::proportional(15.0);
            let pos = pos2(rect.center().x, rect.top() - 6.0);
            let galley = p.layout_no_wrap(e.clone(), font.clone(), Color32::WHITE);
            let bg = Rect::from_center_size(pos - vec2(0.0, galley.size().y * 0.5), galley.size() + vec2(12.0, 6.0));
            p.rect_filled(bg, CornerRadius::same(3), Color32::from_rgba_unmultiplied(40, 10, 8, 220));
            p.text(pos, Align2::CENTER_BOTTOM, e, font, Color32::from_rgb(255, 140, 120));
        }
    }
}

/// A small arrow on the side of a port tile: into the building for inputs, out for outputs, a
/// dot for the other kinds.
fn port_arrow(p: &Painter, v: &View, tx: i32, ty: i32, side: Side, kind: PortKind) {
    let t = TILE_SIZE as f64;
    let (cx, cy) = (tx as f64 * t + t * 0.5, ty as f64 * t + t * 0.5);
    let (dx, dy) = match side {
        Side::Up => (0.0, -1.0),
        Side::Down => (0.0, 1.0),
        Side::Left => (-1.0, 0.0),
        Side::Right => (1.0, 0.0),
    };
    // The point on the edge of the tile.
    let (ex, ey) = (cx + dx * t * 0.5, cy + dy * t * 0.5);
    let color = match kind {
        PortKind::BulkIn | PortKind::BulkOut => Color32::from_rgb(230, 190, 90),
        PortKind::FluidIn | PortKind::FluidOut | PortKind::Pipe => Color32::from_rgb(90, 160, 240),
        PortKind::PartIn | PortKind::PartOut | PortKind::Tube => Color32::from_rgb(220, 220, 220),
        PortKind::Heat | PortKind::Exhaust => Color32::from_rgb(240, 110, 60),
        PortKind::Power | PortKind::Signal => Color32::from_rgb(250, 230, 80),
    };
    let dir = match kind {
        PortKind::BulkIn | PortKind::FluidIn | PortKind::PartIn => -1.0,
        PortKind::BulkOut | PortKind::FluidOut | PortKind::PartOut | PortKind::Exhaust => 1.0,
        _ => 0.0,
    };
    let s = t * 0.28;
    if dir == 0.0 {
        p.circle_filled(v.pos(ex, ey), (s as f32 * v.scale()).max(2.0), color);
        return;
    }
    // Tip outside the edge for outputs, inside for inputs.
    let tip = (ex + dx * s * dir, ey + dy * s * dir);
    let base = (ex - dx * s * dir, ey - dy * s * dir);
    let (px, py) = (-dy * s, dx * s);
    let pts = vec![v.pos(tip.0, tip.1), v.pos(base.0 + px, base.1 + py), v.pos(base.0 - px, base.1 - py)];
    p.add(Shape::convex_polygon(pts, color, Stroke::new(1.0, Color32::from_black_alpha(160))));
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
