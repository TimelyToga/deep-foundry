//! Camera movement and the material brush. No window code here, so it has plain unit tests.

use foundry_core::{CHUNK_SIZE, CellPos, CellRect};
use foundry_render::Camera;
use glam::{DVec2, UVec2, Vec2};

/// Camera speed for the pan keys, in screen pixels per second.
const PAN_SPEED: f64 = 1100.0;
/// Each wheel step changes the zoom by this factor.
const ZOOM_STEP: f32 = 1.2;
/// How fast the zoom moves to its target (per second). Larger is faster.
const ZOOM_RATE: f32 = 18.0;
pub const MAX_ZOOM: f32 = 8.0;

/// The camera and how it moves: pan keys, drag and smooth zoom toward the cursor.
pub struct CameraControl {
    pub camera: Camera,
    target_zoom: f32,
    /// Screen position that stays on the same cell while the zoom changes.
    zoom_anchor: DVec2,
    pub min_zoom: f32,
    /// World size in cells. The camera center stays inside it.
    pub world: DVec2,
}

impl CameraControl {
    pub fn new(center: DVec2, zoom: f32, viewport: UVec2, world: DVec2) -> Self {
        Self {
            camera: Camera::new(center, zoom, viewport),
            target_zoom: zoom,
            zoom_anchor: viewport.as_dvec2() * 0.5,
            min_zoom: 1.0,
            world,
        }
    }

    pub fn set_viewport(&mut self, viewport: UVec2) {
        self.camera.viewport = viewport;
    }

    /// Zoom in (positive steps) or out (negative steps) toward a screen position.
    pub fn zoom_by(&mut self, steps: f32, anchor: DVec2) {
        if !steps.is_finite() {
            return;
        }
        self.target_zoom = (self.target_zoom * ZOOM_STEP.powf(steps)).clamp(self.min_zoom, MAX_ZOOM);
        self.zoom_anchor = anchor;
    }

    /// Move the view by a screen distance (for a mouse drag: the world moves with the mouse).
    pub fn drag(&mut self, screen_delta: DVec2) {
        self.camera.center -= screen_delta / self.camera.zoom as f64;
        self.clamp_center();
    }

    /// Move the zoom toward its target, apply the pan keys, and keep the camera in the world.
    /// `pan` is the pan key direction (x right, y down), each part -1, 0 or 1.
    pub fn update(&mut self, dt: f32, pan: Vec2, fast: bool) {
        self.target_zoom = self.target_zoom.clamp(self.min_zoom, MAX_ZOOM);
        let zoom = self.camera.zoom;
        if zoom != self.target_zoom {
            let k = 1.0 - (-dt * ZOOM_RATE).exp();
            let mut next = (zoom.ln() + (self.target_zoom.ln() - zoom.ln()) * k).exp();
            if (next / self.target_zoom).ln().abs() < 1e-3 {
                next = self.target_zoom;
            }
            // Keep the cell under the anchor in the same place on the screen.
            let from_center = self.zoom_anchor - self.camera.viewport.as_dvec2() * 0.5;
            let anchor_cell = self.camera.center + from_center / zoom as f64;
            self.camera.zoom = next;
            self.camera.center = anchor_cell - from_center / next as f64;
        }
        if pan != Vec2::ZERO {
            let speed = PAN_SPEED * if fast { 3.0 } else { 1.0 } / self.camera.zoom as f64;
            self.camera.center += pan.normalize().as_dvec2() * speed * dt as f64;
        }
        self.clamp_center();
    }

    fn clamp_center(&mut self) {
        if self.world.x > 0.0 && self.world.y > 0.0 {
            self.camera.center = self.camera.center.clamp(DVec2::ZERO, self.world);
        }
    }

    /// The area to ask the simulation for: the screen plus `margin` cells, grown to whole chunks.
    /// It changes only when the view crosses a chunk edge, so few `SetView` commands are sent.
    pub fn view_area(&self, margin: i32) -> CellRect {
        let r = self.camera.visible_rect().expand(margin);
        let down = |v: i32| v.div_euclid(CHUNK_SIZE) * CHUNK_SIZE;
        let up = |v: i32| (v + CHUNK_SIZE - 1).div_euclid(CHUNK_SIZE) * CHUNK_SIZE;
        CellRect::new(down(r.x0), down(r.y0), up(r.x1), up(r.y1))
    }
}

/// A brush stroke: the cells to paint so that a fast mouse move leaves no gaps.
#[derive(Default)]
pub struct Stroke {
    /// Last painted position, in cells. `None` when no button is down.
    last: Option<DVec2>,
    /// Tick of the last paint, so a still mouse paints once per tick, not once per frame.
    last_tick: u64,
}

impl Stroke {
    pub fn end(&mut self) {
        self.last = None;
    }

    /// Positions to paint for the mouse now at `cell`. Circles are placed at most half a radius apart.
    /// A mouse that does not move paints again only when the simulation tick has changed.
    pub fn advance(&mut self, cell: DVec2, radius: u16, tick: u64, out: &mut Vec<CellPos>) {
        let to_cell = |p: DVec2| CellPos::new(p.x.floor() as i32, p.y.floor() as i32);
        match self.last {
            None => out.push(to_cell(cell)),
            Some(from) => {
                let dist = (cell - from).length();
                if dist < 0.5 {
                    if tick == self.last_tick {
                        return;
                    }
                    out.push(to_cell(cell));
                } else {
                    let step = (radius as f64 * 0.5).max(1.0);
                    let n = (dist / step).ceil().max(1.0) as usize;
                    for i in 1..=n {
                        out.push(to_cell(from + (cell - from) * (i as f64 / n as f64)));
                    }
                }
            }
        }
        self.last = Some(cell);
        self.last_tick = tick;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn control() -> CameraControl {
        CameraControl::new(DVec2::new(500.0, 500.0), 2.0, UVec2::new(800, 600), DVec2::new(2048.0, 1024.0))
    }

    #[test]
    fn zoom_keeps_the_cell_under_the_cursor() {
        let mut c = control();
        let cursor = DVec2::new(700.0, 100.0);
        let before = c.camera.screen_to_cell(cursor);
        c.zoom_by(3.0, cursor);
        for _ in 0..200 {
            c.update(1.0 / 120.0, Vec2::ZERO, false);
        }
        assert!((c.camera.zoom - 2.0 * 1.2f32.powi(3)).abs() < 1e-4);
        let after = c.camera.screen_to_cell(cursor);
        // Equal up to the rounding of the view to whole pixels.
        assert!((after - before).abs().max_element() < 1.0 / c.camera.zoom as f64, "{before} {after}");
    }

    #[test]
    fn zoom_stays_in_range() {
        let mut c = control();
        c.min_zoom = 1.5;
        c.zoom_by(-50.0, DVec2::ZERO);
        for _ in 0..500 {
            c.update(1.0 / 60.0, Vec2::ZERO, false);
        }
        assert_eq!(c.camera.zoom, 1.5);
        c.zoom_by(50.0, DVec2::ZERO);
        for _ in 0..500 {
            c.update(1.0 / 60.0, Vec2::ZERO, false);
        }
        assert_eq!(c.camera.zoom, MAX_ZOOM);
    }

    #[test]
    fn pan_and_drag_move_the_view() {
        let mut c = control();
        c.update(0.5, Vec2::new(1.0, 0.0), false);
        assert!((c.camera.center.x - (500.0 + PAN_SPEED * 0.5 / 2.0)).abs() < 1e-6);
        c.drag(DVec2::new(20.0, -10.0));
        assert!((c.camera.center.y - 505.0).abs() < 1e-9);
        // The center stays in the world.
        c.drag(DVec2::new(1e7, 1e7));
        assert_eq!(c.camera.center, DVec2::new(0.0, 0.0));
    }

    #[test]
    fn view_area_is_chunk_aligned_with_margin() {
        let c = control();
        let v = c.view_area(64);
        let s = c.camera.visible_rect();
        assert!(v.x0 <= s.x0 - 64 && v.y0 <= s.y0 - 64 && v.x1 >= s.x1 + 64 && v.y1 >= s.y1 + 64);
        for n in [v.x0, v.y0, v.x1, v.y1] {
            assert_eq!(n.rem_euclid(CHUNK_SIZE), 0);
        }
    }

    #[test]
    fn fast_stroke_has_no_gaps() {
        let mut s = Stroke::default();
        let mut out = vec![];
        s.advance(DVec2::new(10.0, 10.0), 4, 1, &mut out);
        assert_eq!(out, vec![CellPos::new(10, 10)]);
        out.clear();
        s.advance(DVec2::new(110.0, 10.0), 4, 1, &mut out);
        // 100 cells with circles at most 2 cells apart.
        assert_eq!(out.len(), 50);
        assert_eq!(*out.last().unwrap(), CellPos::new(110, 10));
        for w in out.windows(2) {
            assert!((w[1].x - w[0].x).abs() <= 2);
        }
    }

    #[test]
    fn still_mouse_paints_once_per_tick() {
        let mut s = Stroke::default();
        let mut out = vec![];
        let p = DVec2::new(5.0, 5.0);
        s.advance(p, 2, 7, &mut out);
        s.advance(p, 2, 7, &mut out);
        assert_eq!(out.len(), 1);
        s.advance(p, 2, 8, &mut out);
        assert_eq!(out.len(), 2);
        s.end();
        s.advance(p, 2, 8, &mut out);
        assert_eq!(out.len(), 3, "a new stroke paints at once");
    }
}
