//! Construction shapes over the world (egui painter, under the UI windows): the tile grid, the
//! ghost of the building in the hand with its ports, the drag line, the removal progress ring,
//! the outline of the building under the mouse, and the alt mode icons.

use crate::construct::{BuildView, DragLine, LocalGhost};
use crate::factory_host::FactoryFrame;
use crate::overlay::View;
use egui::{Color32, CornerRadius, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2};
use foundry_content::{Content, ItemRef, PortKind, Side};
use foundry_core::{CellPos, CellRect, TILE_SIZE, TilePos};
use foundry_ui::icons::IconAtlas;

const GOOD: Color32 = Color32::from_rgb(90, 230, 110);
const BAD: Color32 = Color32::from_rgb(245, 80, 60);
const UNKNOWN: Color32 = Color32::from_gray(210);

/// The cells of a footprint.
fn footprint(at: TilePos, size: (u8, u8)) -> CellRect {
    let o = at.origin();
    CellRect::new(o.x, o.y, o.x + size.0 as i32 * TILE_SIZE, o.y + size.1 as i32 * TILE_SIZE)
}

/// Shapes under the robot: the grid, the outline of the building under the mouse, the buildings
/// that wait for removal, and the alt mode icons.
pub fn draw_under(p: &Painter, v: &View, frame: &FactoryFrame, b: &BuildView, atlas: Option<&IconAtlas>) {
    if b.grid {
        grid(p, v);
    }
    if b.alt {
        alt_marks(p, v, frame, atlas);
    }
    for r in &frame.remove_queue {
        p.rect(v.cell_rect(*r), CornerRadius::ZERO, Color32::from_rgba_unmultiplied(220, 60, 40, 40), Stroke::new(1.5, BAD), StrokeKind::Inside);
    }
    // The building under the mouse gets a thin outline (not under a ghost).
    if b.ghost.is_none()
        && b.drag.is_none()
        && let Some(h) = frame.hover.as_ref().filter(|h| b.mouse.is_some_and(|m| h.rect.contains(m)))
    {
        p.rect_stroke(v.cell_rect(h.rect).expand(1.0), CornerRadius::same(2), Stroke::new(1.5, Color32::from_white_alpha(170)), StrokeKind::Outside);
    }
}

/// Shapes over the robot: the ghost, the drag line, the removal ring and the reason text.
pub fn draw_over(p: &Painter, v: &View, frame: &FactoryFrame, b: &BuildView, content: &Content, atlas: Option<&IconAtlas>) {
    if let Some(d) = &b.drag {
        drag_line(p, v, d, content, b.mouse, atlas);
    }
    if let Some(g) = &b.ghost {
        ghost(p, v, g, frame, content, b.mouse, atlas);
    }
    if let Some(r) = &frame.removing {
        removal_ring(p, v, r.rect, r.progress);
    }
}

/// Faint lines on the tile edges over the screen.
fn grid(p: &Painter, v: &View) {
    let r = v.camera.visible_rect();
    let t = TILE_SIZE;
    let stroke = Stroke::new(1.0, Color32::from_white_alpha(22));
    let (x0, x1) = (r.x0.div_euclid(t), r.x1.div_euclid(t) + 1);
    let (y0, y1) = (r.y0.div_euclid(t), r.y1.div_euclid(t) + 1);
    // Too many lines when zoomed far out: no grid then.
    if (x1 - x0) + (y1 - y0) > 600 {
        return;
    }
    for tx in x0..=x1 {
        let x = (tx * t) as f64;
        p.line_segment([v.pos(x, r.y0 as f64), v.pos(x, r.y1 as f64)], stroke);
    }
    for ty in y0..=y1 {
        let y = (ty * t) as f64;
        p.line_segment([v.pos(r.x0 as f64, y), v.pos(r.x1 as f64, y)], stroke);
    }
}

/// The body color of a building kind.
fn body_color(content: &Content, kind: foundry_core::BuildingKindId) -> Color32 {
    let def = content.factory.building_def(kind);
    let c = content.materials.colors.get(def.body.index()).and_then(|c| c.first()).copied().unwrap_or([160, 160, 160, 255]);
    Color32::from_rgb(c[0], c[1], c[2])
}

/// A footprint as a see-through building: the body color, a green or red tint, the tile lines and
/// the building icon.
fn footprint_shape(p: &Painter, v: &View, cells: CellRect, size: (u8, u8), body: Color32, tint: Color32, item: Option<ItemRef>, atlas: Option<&IconAtlas>) {
    let rect = v.cell_rect(cells);
    p.rect_filled(rect, CornerRadius::ZERO, body.gamma_multiply(0.45));
    p.rect_filled(rect, CornerRadius::ZERO, tint.gamma_multiply(0.28));
    let thin = Stroke::new(1.0, tint.gamma_multiply(0.35));
    for i in 1..size.0 as i32 {
        let x = (cells.x0 + i * TILE_SIZE) as f64;
        p.line_segment([v.pos(x, cells.y0 as f64), v.pos(x, cells.y1 as f64)], thin);
    }
    for i in 1..size.1 as i32 {
        let y = (cells.y0 + i * TILE_SIZE) as f64;
        p.line_segment([v.pos(cells.x0 as f64, y), v.pos(cells.x1 as f64, y)], thin);
    }
    if let (Some(item), Some(atlas)) = (item, atlas) {
        let side = (rect.width().min(rect.height()) * 0.62).clamp(8.0, 48.0);
        atlas.paint(p, item, Rect::from_center_size(rect.center(), vec2(side, side)), Color32::from_white_alpha(170));
    }
    p.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(2.0, tint), StrokeKind::Inside);
}

fn ghost(p: &Painter, v: &View, g: &LocalGhost, frame: &FactoryFrame, content: &Content, mouse: Option<CellPos>, atlas: Option<&IconAtlas>) {
    let cells = footprint(g.request.at, g.size);
    // The check result is for the ghost of an earlier frame. Use it only if it is for this place.
    let known = frame.ghost.as_ref().filter(|fg| fg.request == g.request);
    let tint = match known.map(|k| k.error.is_none()) {
        Some(true) => GOOD,
        Some(false) => BAD,
        None => UNKNOWN,
    };
    let def = content.factory.building_def(g.request.kind);
    footprint_shape(p, v, cells, g.size, body_color(content, g.request.kind), tint, Some(ItemRef::Part(def.part)), atlas);
    if let Some(k) = known {
        for port in &k.ports {
            port_arrow(p, v, port.tile, port.side, port.kind);
        }
        if let Some(e) = &k.error {
            reason(p, v, e, mouse, cells);
        }
    }
}

fn drag_line(p: &Painter, v: &View, d: &DragLine, content: &Content, mouse: Option<CellPos>, atlas: Option<&IconAtlas>) {
    // The footprints placed so far: a green frame (the buildings are drawn by the world).
    for at in &d.sent {
        if d.stop.as_ref().is_some_and(|(s, _)| s == at) {
            continue;
        }
        let r = v.cell_rect(footprint(*at, d.size));
        p.rect(r, CornerRadius::ZERO, GOOD.gamma_multiply(0.12), Stroke::new(1.5, GOOD.gamma_multiply(0.8)), StrokeKind::Inside);
    }
    // The direction of the line: an arrow at its end.
    if let (Some(dir), Some(last)) = (d.dir, d.sent.last()) {
        let c = footprint(*last, d.size);
        let (cx, cy) = ((c.x0 + c.x1) as f64 * 0.5, (c.y0 + c.y1) as f64 * 0.5);
        let (dx, dy) = dir.step();
        let half = (d.size.0 as f64 * dx.abs() as f64 + d.size.1 as f64 * dy.abs() as f64) * TILE_SIZE as f64 * 0.5;
        arrow(p, v, (cx + dx as f64 * (half + 3.0), cy + dy as f64 * (half + 3.0)), (dx as f64, dy as f64), 3.0, GOOD);
    }
    if let Some((at, text)) = &d.stop {
        let cells = footprint(*at, d.size);
        let def = content.factory.building_def(d.kind);
        footprint_shape(p, v, cells, d.size, body_color(content, d.kind), BAD, Some(ItemRef::Part(def.part)), atlas);
        reason(p, v, text, mouse, cells);
    }
}

/// The reason why a ghost cannot be placed, in a dark red box next to the mouse (or over the
/// ghost when there is no mouse).
fn reason(p: &Painter, v: &View, text: &str, mouse: Option<CellPos>, cells: CellRect) {
    let font = FontId::proportional(16.0);
    let galley = p.layout_no_wrap(text.to_string(), font, Color32::from_rgb(255, 150, 130));
    let size = galley.size() + vec2(14.0, 8.0);
    let min = match mouse {
        Some(m) => v.pos(m.x as f64 + 0.5, m.y as f64 + 0.5) + vec2(20.0, 16.0),
        None => {
            let top = v.pos((cells.x0 + cells.x1) as f64 * 0.5, cells.y0 as f64);
            top - vec2(size.x * 0.5, size.y + 8.0)
        }
    };
    // Keep the box on the screen.
    let screen = p.clip_rect();
    let min = pos2(min.x.min(screen.right() - size.x - 4.0).max(screen.left() + 4.0), min.y.min(screen.bottom() - size.y - 4.0).max(4.0));
    let bg = Rect::from_min_size(min, size);
    p.rect(bg, CornerRadius::same(3), Color32::from_rgba_unmultiplied(45, 12, 10, 235), Stroke::new(1.0, BAD.gamma_multiply(0.7)), StrokeKind::Inside);
    p.galley(bg.min + vec2(7.0, 4.0), galley, Color32::WHITE);
}

/// A filled arrow with its tip at `tip` (in cells), pointing along `dir`.
fn arrow(p: &Painter, v: &View, tip: (f64, f64), dir: (f64, f64), len: f64, color: Color32) {
    let (dx, dy) = dir;
    let base = (tip.0 - dx * len, tip.1 - dy * len);
    let (px, py) = (-dy * len * 0.8, dx * len * 0.8);
    let pts = vec![v.pos(tip.0, tip.1), v.pos(base.0 + px, base.1 + py), v.pos(base.0 - px, base.1 - py)];
    p.add(Shape::convex_polygon(pts, color, Stroke::new(1.0, Color32::from_black_alpha(170))));
}

/// The color of a port kind: bulk yellow, fluids blue, parts white, heat and exhaust orange,
/// power and signals yellow-green.
pub fn port_color(kind: PortKind) -> Color32 {
    match kind {
        PortKind::BulkIn | PortKind::BulkOut => Color32::from_rgb(235, 195, 90),
        PortKind::FluidIn | PortKind::FluidOut | PortKind::Pipe => Color32::from_rgb(90, 165, 245),
        PortKind::PartIn | PortKind::PartOut | PortKind::Tube => Color32::from_rgb(235, 235, 235),
        PortKind::Heat | PortKind::Exhaust => Color32::from_rgb(245, 120, 60),
        PortKind::Power | PortKind::Signal => Color32::from_rgb(210, 240, 80),
    }
}

/// A port on the edge of its tile: an arrow into the building for inputs, out of it for outputs
/// and exhausts, a dot for connections (pipes, heat, power).
pub fn port_arrow(p: &Painter, v: &View, tile: TilePos, side: Side, kind: PortKind) {
    let t = TILE_SIZE as f64;
    let (cx, cy) = (tile.x as f64 * t + t * 0.5, tile.y as f64 * t + t * 0.5);
    let (dx, dy) = match side {
        Side::Up => (0.0, -1.0),
        Side::Down => (0.0, 1.0),
        Side::Left => (-1.0, 0.0),
        Side::Right => (1.0, 0.0),
    };
    // The point on the edge of the tile.
    let (ex, ey) = (cx + dx * t * 0.5, cy + dy * t * 0.5);
    let color = port_color(kind);
    let out = match kind {
        PortKind::BulkIn | PortKind::FluidIn | PortKind::PartIn => Some(false),
        PortKind::BulkOut | PortKind::FluidOut | PortKind::PartOut | PortKind::Exhaust => Some(true),
        _ => None,
    };
    let len = t * 0.4;
    match out {
        // Outputs: from the edge outward. Inputs: from outside to the edge.
        Some(true) => arrow(p, v, (ex + dx * len, ey + dy * len), (dx, dy), len, color),
        Some(false) => arrow(p, v, (ex, ey), (-dx, -dy), len, color),
        None => {
            let r = (t as f32 * 0.16 * v.scale()).max(3.0);
            p.circle(v.pos(ex, ey), r, color, Stroke::new(1.0, Color32::from_black_alpha(170)));
        }
    }
}

/// The removal progress: a ring around the building that fills clockwise.
fn removal_ring(p: &Painter, v: &View, cells: CellRect, progress: f32) {
    let rect = v.cell_rect(cells);
    p.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(220, 60, 40, 50));
    let c = rect.center();
    let radius = (rect.width().max(rect.height()) * 0.5 + 6.0).max(12.0);
    p.circle_stroke(c, radius, Stroke::new(5.0, Color32::from_black_alpha(150)));
    let n = 48;
    let end = (progress.clamp(0.0, 1.0) * n as f32).ceil() as usize;
    if end > 0 {
        let pts: Vec<Pos2> = (0..=end)
            .map(|i| {
                let a = -std::f32::consts::FRAC_PI_2 + (i as f32 / n as f32).min(progress) * std::f32::consts::TAU;
                c + vec2(a.cos(), a.sin()) * radius
            })
            .collect();
        p.add(Shape::line(pts, Stroke::new(3.5, Color32::from_rgb(255, 170, 60))));
    }
}

/// Alt mode: the product of each machine's recipe in a small box, and belt directions.
fn alt_marks(p: &Painter, v: &View, frame: &FactoryFrame, atlas: Option<&IconAtlas>) {
    for m in &frame.marks {
        let rect = v.cell_rect(m.rect);
        if let (Some(item), Some(atlas)) = (m.output, atlas) {
            let side = (rect.width().min(rect.height()) * 0.7).clamp(14.0, 40.0);
            let r = Rect::from_center_size(rect.center(), vec2(side, side));
            p.rect(r.expand(2.0), CornerRadius::same(4), Color32::from_black_alpha(170), Stroke::new(1.0, Color32::from_white_alpha(60)), StrokeKind::Inside);
            atlas.paint(p, item, r, Color32::WHITE);
        }
        if m.belt != 0 {
            let dir = m.belt as f64;
            let (cx, cy) = ((m.rect.x0 + m.rect.x1) as f64 * 0.5, m.rect.y0 as f64 + 2.5);
            arrow(p, v, (cx + dir * 2.5, cy), (dir, 0.0), 2.5, Color32::from_rgb(255, 225, 120));
        }
    }
}
