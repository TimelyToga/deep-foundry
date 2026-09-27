//! The camera: which part of the world the screen shows.

use foundry_core::CellRect;
use glam::{DVec2, UVec2};

/// Which part of the world the screen shows.
///
/// Screen positions are in pixels of the render target, from its top-left corner.
/// World positions are in cells. y goes down in both.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// World position at the center of the screen, in cells. It can have a fraction.
    pub center: DVec2,
    /// Screen pixels per cell. 1.0 shows one cell in each pixel.
    pub zoom: f32,
    /// Size of the render target in pixels.
    pub viewport: UVec2,
}

impl Camera {
    pub fn new(center: DVec2, zoom: f32, viewport: UVec2) -> Self {
        Self { center, zoom, viewport }
    }

    /// World position of the top-left corner of the screen.
    ///
    /// The value is rounded to whole screen pixels. So at a whole-number zoom, each cell edge is on a
    /// pixel edge, and the cells look sharp. All other methods use this value, so they agree with
    /// what the renderer draws.
    pub fn top_left(&self) -> DVec2 {
        let z = self.zoom as f64;
        let exact = self.center - self.viewport.as_dvec2() * 0.5 / z;
        (exact * z).round() / z
    }

    /// The world position (in cells) at a screen position (in pixels).
    pub fn screen_to_cell(&self, screen: DVec2) -> DVec2 {
        self.top_left() + screen / self.zoom as f64
    }

    /// The screen position (in pixels) of a world position (in cells).
    pub fn cell_to_screen(&self, cell: DVec2) -> DVec2 {
        (cell - self.top_left()) * self.zoom as f64
    }

    /// All cells that are at least partly on the screen.
    pub fn visible_rect(&self) -> CellRect {
        let tl = self.top_left();
        let br = tl + self.viewport.as_dvec2() / self.zoom as f64;
        CellRect::new(tl.x.floor() as i32, tl.y.floor() as i32, br.x.ceil() as i32, br.y.ceil() as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_and_cell_round_trip() {
        let cam = Camera::new(DVec2::new(1000.3, 500.7), 3.0, UVec2::new(1920, 1080));
        let p = DVec2::new(123.0, 456.0);
        let back = cam.cell_to_screen(cam.screen_to_cell(p));
        assert!((back - p).length() < 1e-9);
        // The screen center is within half a pixel of the camera center.
        let c = cam.screen_to_cell(cam.viewport.as_dvec2() * 0.5);
        assert!((c - cam.center).abs().max_element() <= 0.5 / 3.0 + 1e-9);
    }

    #[test]
    fn visible_rect_covers_the_screen() {
        let cam = Camera::new(DVec2::new(100.0, 100.0), 2.0, UVec2::new(100, 60));
        let r = cam.visible_rect();
        assert_eq!(r, CellRect::new(75, 85, 125, 115));
        let cam = Camera::new(DVec2::new(100.25, 100.0), 2.0, UVec2::new(100, 60));
        let r = cam.visible_rect();
        assert_eq!(r.width(), 51, "a fraction adds one column");
    }

    #[test]
    fn top_left_is_on_the_pixel_grid() {
        let cam = Camera::new(DVec2::new(10.13, 20.71), 4.0, UVec2::new(801, 600));
        let tl = cam.top_left() * 4.0;
        assert_eq!(tl, tl.round());
    }
}
