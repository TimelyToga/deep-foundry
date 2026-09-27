//! Item icons.
//!
//! There is no art yet, so this module draws small pixel-art icons (32 x 32 art pixels) from
//! the [`IconSpec`] of each item. It builds them once into egui textures.
//!
//! This is the one place that maps an [`ItemId`] to its picture: [`IconAtlas::paint`].
//! When real art exists, change [`IconAtlas::build`] to load the art (by `ItemInfo::key`)
//! instead of calling [`draw_icon`].
//!
//! Each icon is stored twice: at 32 px (for small slots) and at 64 px (each art pixel is
//! 2 x 2). `paint` picks the size that is closest to the size on the screen.

use crate::item::{Catalog, IconShape, IconSpec, ItemId, MachineGlyph};
use egui::{Color32, ColorImage, Painter, Pos2, Rect, TextureHandle, TextureId, TextureOptions};
use std::collections::HashMap;

/// Size of an icon in art pixels.
pub const ART: usize = 32;

type Rgba = [u8; 4];

const CLEAR: Rgba = [0, 0, 0, 0];

/// A 32 x 32 RGBA image.
#[derive(Clone)]
pub struct Canvas {
    pub px: Vec<Rgba>,
}

impl Default for Canvas {
    fn default() -> Self {
        Self::new()
    }
}

impl Canvas {
    pub fn new() -> Self {
        Self { px: vec![CLEAR; ART * ART] }
    }

    fn inside(x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as usize) < ART && (y as usize) < ART
    }

    pub fn get(&self, x: i32, y: i32) -> Rgba {
        if Self::inside(x, y) { self.px[y as usize * ART + x as usize] } else { CLEAR }
    }

    pub fn set(&mut self, x: i32, y: i32, c: Rgba) {
        if Self::inside(x, y) {
            self.px[y as usize * ART + x as usize] = c;
        }
    }

    fn opaque(&self, x: i32, y: i32) -> bool {
        self.get(x, y)[3] > 0
    }

    /// Fill every pixel whose center passes the test. The test gets the pixel center.
    fn fill_where(&mut self, mut test: impl FnMut(f32, f32) -> bool, mut color: impl FnMut(i32, i32) -> Rgba) {
        for y in 0..ART as i32 {
            for x in 0..ART as i32 {
                if test(x as f32 + 0.5, y as f32 + 0.5) {
                    let c = color(x, y);
                    self.set(x, y, c);
                }
            }
        }
    }

    /// Fill a rectangle (inclusive corners).
    fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgba) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.set(x, y, c);
            }
        }
    }

    fn circle(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        self.fill_where(|x, y| (x - cx).powi(2) + (y - cy).powi(2) <= r * r, |_, _| c);
    }

    fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, c: Rgba) {
        self.fill_where(|x, y| ((x - cx) / rx).powi(2) + ((y - cy) / ry).powi(2) <= 1.0, |_, _| c);
    }

    /// Fill a convex or simple polygon.
    fn poly(&mut self, pts: &[(f32, f32)], c: Rgba) {
        self.poly_with(pts, |_, _| c);
    }

    fn poly_with(&mut self, pts: &[(f32, f32)], color: impl FnMut(i32, i32) -> Rgba) {
        let pts = pts.to_vec();
        self.fill_where(move |x, y| point_in_poly(&pts, x, y), color);
    }

    /// A thick line with round ends.
    fn line(&mut self, a: (f32, f32), b: (f32, f32), width: f32, c: Rgba) {
        self.fill_where(|x, y| dist_to_segment((x, y), a, b) <= width * 0.5, |_, _| c);
    }

    /// Light on the top and left edges, shadow on the bottom and right edges of every shape.
    fn bevel(&mut self, light: f32, dark: f32) {
        let src = self.clone();
        for y in 0..ART as i32 {
            for x in 0..ART as i32 {
                let c = src.get(x, y);
                if c[3] == 0 {
                    continue;
                }
                let edge_tl = !src.opaque(x, y - 1) || !src.opaque(x - 1, y);
                let edge_br = !src.opaque(x, y + 1) || !src.opaque(x + 1, y);
                if edge_tl && !edge_br {
                    self.set(x, y, lit(c, light));
                } else if edge_br && !edge_tl {
                    self.set(x, y, lit(c, dark));
                }
            }
        }
    }

    /// A dark line around all shapes. The line color is the neighbor color, darkened.
    fn outline(&mut self, strength: f32) {
        let src = self.clone();
        for y in 0..ART as i32 {
            for x in 0..ART as i32 {
                if src.opaque(x, y) {
                    continue;
                }
                let n = [(0, -1), (0, 1), (-1, 0), (1, 0)]
                    .iter()
                    .map(|(dx, dy)| src.get(x + dx, y + dy))
                    .find(|c| c[3] > 0);
                if let Some(c) = n {
                    let d = lit(c, strength);
                    self.set(x, y, [d[0], d[1], d[2], 255.min(c[3] as u16 + 40) as u8]);
                }
            }
        }
    }

    /// The image scaled by an integer factor (nearest pixel), as RGBA bytes.
    pub fn to_rgba(&self, scale: usize) -> Vec<u8> {
        let size = ART * scale;
        let mut out = vec![0u8; size * size * 4];
        for y in 0..size {
            for x in 0..size {
                let c = self.px[(y / scale) * ART + x / scale];
                let i = (y * size + x) * 4;
                out[i..i + 4].copy_from_slice(&c);
            }
        }
        out
    }
}

fn point_in_poly(pts: &[(f32, f32)], x: f32, y: f32) -> bool {
    let mut inside = false;
    let mut j = pts.len() - 1;
    for i in 0..pts.len() {
        let (xi, yi) = pts[i];
        let (xj, yj) = pts[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn dist_to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (px, py) = p;
    let (ax, ay) = a;
    let (bx, by) = b;
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 { 0.0 } else { (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0) };
    let (cx, cy) = (ax + t * dx, ay + t * dy);
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

/// Multiply the brightness.
fn lit(c: Rgba, f: f32) -> Rgba {
    let m = |v: u8| (v as f32 * f).round().clamp(0.0, 255.0) as u8;
    [m(c[0]), m(c[1]), m(c[2]), c[3]]
}

fn mixc(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round().clamp(0.0, 255.0) as u8;
    [l(a[0], b[0]), l(a[1], b[1]), l(a[2], b[2]), l(a[3], b[3])]
}

fn solid(c: Rgba) -> Rgba {
    [c[0], c[1], c[2], 255]
}

/// A small integer hash for pixel noise.
fn hash(x: i32, y: i32, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x27d4_eb2d) ^ (y as u32).wrapping_mul(0x1656_67b1) ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h
}

/// The colors of one icon.
struct Pal {
    base: Rgba,
    shades: Vec<Rgba>,
    seed: u32,
}

impl Pal {
    fn new(spec: &IconSpec) -> Self {
        let shades: Vec<Rgba> = if spec.colors.is_empty() {
            vec![[128, 128, 128, 255]]
        } else {
            spec.colors.iter().map(|&c| solid(c)).collect()
        };
        let seed = shades.iter().fold(7u32, |a, c| a.wrapping_mul(31).wrapping_add(u32::from_le_bytes(*c)));
        Self { base: shades[0], shades, seed }
    }

    /// A shade picked by pixel noise.
    fn noisy(&self, x: i32, y: i32) -> Rgba {
        self.shades[hash(x, y, self.seed) as usize % self.shades.len()]
    }

    fn at(&self, f: f32) -> Rgba {
        lit(self.base, f)
    }
}

/// Draw the icon of a spec.
pub fn draw_icon(spec: &IconSpec) -> Canvas {
    let mut c = Canvas::new();
    let p = Pal::new(spec);
    match spec.shape {
        IconShape::Pile => pile(&mut c, &p),
        IconShape::Drop => drop(&mut c, &p),
        IconShape::Gas => gas(&mut c, &p),
        IconShape::Block => block(&mut c, &p),
        IconShape::Flame => flame_icon(&mut c),
        IconShape::Gear => gear(&mut c, &p, 15.5, 15.5, 14.0, 10.8, 4.0, 8),
        IconShape::Plate => plate(&mut c, &p),
        IconShape::Ingot => ingot(&mut c, &p),
        IconShape::Rod => rod(&mut c, &p),
        IconShape::WireCoil => wire_coil(&mut c, &p),
        IconShape::Pipe => pipe(&mut c, &p),
        IconShape::Brick => brick(&mut c, &p),
        IconShape::Circuit => circuit(&mut c, &p),
        IconShape::Kit => kit(&mut c, &p),
        IconShape::Vial => vial(&mut c, &p),
        IconShape::Pane => pane(&mut c, &p),
        IconShape::Sheet => sheet(&mut c, &p),
        IconShape::Bolt => bolt(&mut c, &p),
        IconShape::VacuumTube => vacuum_tube(&mut c, &p),
        IconShape::Machine(g) => machine(&mut c, &p, g, spec.tier),
        IconShape::Belt => belt(&mut c, &p, spec.tier),
        IconShape::Crate => crate_icon(&mut c, &p, spec.tier),
        IconShape::Barrel => barrel(&mut c, &p),
        IconShape::Wall => wall(&mut c, &p),
        IconShape::Ladder => ladder(&mut c, &p),
        IconShape::Campfire => campfire(&mut c, &p),
        IconShape::Workbench => workbench(&mut c, &p),
        IconShape::Hopper => hopper(&mut c, &p),
        IconShape::Mold => mold(&mut c, &p),
        IconShape::Crucible => crucible(&mut c, &p),
        IconShape::Tank => tank(&mut c, &p, spec.tier),
        IconShape::Cable => cable(&mut c, &p),
        IconShape::SolarPanel => solar(&mut c, &p),
        IconShape::Battery => battery(&mut c, &p),
    }
    c
}

/// The icon for items the catalog does not know: a gray box with a question mark.
pub fn unknown_icon() -> Canvas {
    let mut c = Canvas::new();
    c.rect(5, 5, 26, 26, [96, 96, 96, 255]);
    c.bevel(1.3, 0.65);
    c.outline(0.3);
    let q = [(12, 10), (13, 9), (14, 9), (15, 9), (16, 9), (17, 9), (18, 10), (19, 11), (19, 12), (18, 13), (17, 14), (16, 15), (16, 16), (16, 17), (16, 20), (16, 21)];
    for (x, y) in q {
        c.rect(x - 1, y, x, y, [235, 235, 235, 255]);
    }
    c
}

// ---------------------------------------------------------------- bulk materials

fn pile(c: &mut Canvas, p: &Pal) {
    let base_y = 27.0;
    for x in 2..30 {
        let dx = (x as f32 + 0.5 - 16.0) / 13.8;
        if dx.abs() >= 1.0 {
            continue;
        }
        let bump = (hash(x, 0, p.seed) % 3) as f32 * 0.5 - 0.5;
        let h = 17.5 * (1.0 - dx * dx).powf(0.85) + bump;
        let top = (base_y - h).round() as i32;
        for y in top..=base_y as i32 {
            let depth = (y - top) as f32 / h.max(1.0);
            let light = 1.18 - 0.22 * depth - 0.12 * dx;
            c.set(x, y, lit(p.noisy(x, y), light));
        }
    }
    c.bevel(1.25, 0.8);
    // Grains: a few light pixels.
    for i in 0..14 {
        let x = 6 + (hash(i, 1, p.seed) % 20) as i32;
        let y = 14 + (hash(i, 2, p.seed) % 13) as i32;
        if c.opaque(x, y) && c.opaque(x, y - 1) {
            let v = c.get(x, y);
            c.set(x, y, lit(v, 1.3));
        }
    }
    c.outline(0.35);
}

fn drop(c: &mut Canvas, p: &Pal) {
    let (cx, cy, r) = (16.0, 19.5, 9.0);
    let inside = |x: f32, y: f32| {
        if (x - cx).powi(2) + (y - cy).powi(2) <= r * r {
            return true;
        }
        if (3.0..cy).contains(&y) {
            let t = (y - 3.0) / (cy - 3.0);
            return (x - cx).abs() <= r * t.powf(1.25);
        }
        false
    };
    let bright = p.base[0] as u32 + p.base[1] as u32 + p.base[2] as u32 > 500;
    c.fill_where(inside, |x, y| {
        let t = ((x as f32 - 10.0) + (y as f32 - 12.0)) / 24.0;
        let f = 1.2 - 0.45 * t.clamp(0.0, 1.0);
        lit(p.noisy(x, y), f)
    });
    // Molten metals are bright: add a hot core.
    if bright {
        c.fill_where(|x, y| (x - 15.0).powi(2) + (y - 20.0).powi(2) <= 16.0, |x, y| mixc(c_get_static(p, x, y), [255, 250, 200, 255], 0.35));
    }
    c.bevel(1.2, 0.75);
    // Highlight.
    for (x, y) in [(11, 16), (11, 17), (11, 18), (12, 15), (12, 14), (11, 19)] {
        c.set(x, y, [255, 255, 255, 230]);
    }
    c.outline(0.35);
}

fn c_get_static(p: &Pal, x: i32, y: i32) -> Rgba {
    lit(p.noisy(x, y), 1.1)
}

fn gas(c: &mut Canvas, p: &Pal) {
    let blobs = [(11.0, 19.0, 6.5), (18.0, 14.5, 7.5), (23.5, 20.0, 5.5), (16.5, 21.5, 6.0), (8.0, 23.0, 4.0)];
    let base = p.base;
    c.fill_where(
        |x, y| blobs.iter().any(|&(bx, by, r)| (x - bx).powi(2) + (y - by).powi(2) <= r * r),
        |x, y| {
            let f = 1.25 - 0.4 * ((y as f32 - 8.0) / 20.0).clamp(0.0, 1.0);
            let v = lit(mixc(base, p.noisy(x, y), 0.4), f);
            [v[0], v[1], v[2], 225]
        },
    );
    c.bevel(1.25, 0.8);
    c.outline(0.45);
}

fn block(c: &mut Canvas, p: &Pal) {
    let top = [(16.0, 3.0), (29.0, 9.5), (16.0, 16.0), (3.0, 9.5)];
    let left = [(3.0, 9.5), (16.0, 16.0), (16.0, 29.0), (3.0, 22.5)];
    let right = [(16.0, 16.0), (29.0, 9.5), (29.0, 22.5), (16.0, 29.0)];
    c.poly_with(&left, |x, y| lit(p.noisy(x, y), 0.92));
    c.poly_with(&right, |x, y| lit(p.noisy(x, y), 0.68));
    c.poly_with(&top, |x, y| lit(p.noisy(x, y), 1.22));
    c.outline(0.35);
}

fn flame_shape(c: &mut Canvas, cx: f32, bottom: f32, h: f32, w: f32, col: Rgba) {
    c.fill_where(
        |x, y| {
            let t = (bottom - y) / h; // 0 at the bottom, 1 at the tip
            if !(0.0..=1.0).contains(&t) {
                return false;
            }
            let half = w * (1.0 - t).powf(0.7) * (0.55 + 0.45 * (t * 3.2).sin().abs().max(0.4));
            let sway = (t * 5.0).sin() * 1.2 * t;
            (x - cx - sway).abs() <= half
        },
        |_, _| col,
    );
}

fn flame_icon(c: &mut Canvas) {
    flame_shape(c, 16.0, 29.0, 26.0, 10.0, [220, 60, 30, 255]);
    flame_shape(c, 16.0, 29.0, 19.0, 7.0, [255, 150, 40, 255]);
    flame_shape(c, 16.0, 29.0, 11.0, 4.0, [255, 238, 150, 255]);
    c.outline(0.4);
}

// ---------------------------------------------------------------- parts

#[allow(clippy::too_many_arguments)]
fn gear(c: &mut Canvas, p: &Pal, cx: f32, cy: f32, r_out: f32, r_in: f32, hole: f32, teeth: u32) {
    let step = std::f32::consts::TAU / teeth as f32;
    let base = p.base;
    c.fill_where(
        |x, y| {
            let (dx, dy) = (x - cx, y - cy);
            let r = (dx * dx + dy * dy).sqrt();
            let a = dy.atan2(dx) + std::f32::consts::PI;
            let frac = (a / step).fract();
            let limit = if (0.28..0.72).contains(&frac) { r_out } else { r_in };
            r <= limit && r >= hole
        },
        |x, y| {
            let t = ((x as f32 - cx) + (y as f32 - cy)) / (2.0 * r_out);
            lit(base, 1.12 - 0.3 * t)
        },
    );
    // A ring groove.
    let groove = (hole + r_in) * 0.5 + 0.5;
    c.fill_where(
        |x, y| {
            let r = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
            (r - groove).abs() <= 0.6
        },
        |_, _| lit(base, 0.72),
    );
    c.bevel(1.3, 0.7);
    c.outline(0.3);
}

fn plate(c: &mut Canvas, p: &Pal) {
    for (i, dy) in [7.0f32, 0.0].iter().enumerate() {
        let top = [(2.0, 15.0 + dy), (16.0, 8.0 + dy), (30.0, 15.0 + dy), (16.0, 22.0 + dy)];
        let front_l = [(2.0, 15.0 + dy), (16.0, 22.0 + dy), (16.0, 25.0 + dy), (2.0, 18.0 + dy)];
        let front_r = [(16.0, 22.0 + dy), (30.0, 15.0 + dy), (30.0, 18.0 + dy), (16.0, 25.0 + dy)];
        let f = if i == 0 { 0.9 } else { 1.0 };
        c.poly(&front_l, p.at(0.78 * f));
        c.poly(&front_r, p.at(0.6 * f));
        c.poly_with(&top, |x, y| mixc(p.at(1.12 * f), lit(p.noisy(x, y), 1.12 * f), 0.35));
        // A light line along the far edges of the top face.
        for x in 3..16 {
            let y = (15.0 + dy - (x as f32 - 2.0) * 0.5).round() as i32;
            c.set(x, y, p.at(1.4 * f));
        }
    }
    c.outline(0.3);
}

fn ingot(c: &mut Canvas, p: &Pal) {
    let top = [(9.0, 10.0), (24.0, 10.0), (27.0, 16.0), (6.0, 16.0)];
    let front = [(6.0, 16.0), (27.0, 16.0), (29.0, 25.0), (4.0, 25.0)];
    let left = [(9.0, 10.0), (6.0, 16.0), (4.0, 25.0), (3.0, 24.0), (7.0, 11.0)];
    c.poly(&front, p.at(0.9));
    c.poly(&left, p.at(0.62));
    c.poly_with(&top, |x, y| mixc(p.at(1.3), lit(p.noisy(x, y), 1.3), 0.3));
    for x in 9..24 {
        c.set(x, 11, p.at(1.55));
    }
    for x in 7..27 {
        c.set(x, 17, p.at(1.05));
    }
    c.outline(0.3);
}

fn rod(c: &mut Canvas, p: &Pal) {
    for k in [-6.0f32, 0.0, 6.0] {
        let a = (7.0 + k * 0.7, 26.0 + k * 0.7);
        let b = (25.0 + k * 0.7, 8.0 + k * 0.7);
        let (ax, ay) = (a.0 - k * 0.0, a.1);
        c.line((ax, ay), b, 3.6, p.at(0.95));
        // Highlight along the top side.
        c.line((ax - 0.8, ay - 0.8), (b.0 - 0.8, b.1 - 0.8), 1.0, p.at(1.4));
        // Cut face at the top end.
        c.circle(b.0, b.1, 1.6, p.at(1.25));
    }
    c.outline(0.3);
}

fn wire_coil(c: &mut Canvas, p: &Pal) {
    let flange = [110, 80, 58, 255];
    // Coil body.
    c.fill_where(
        |x, y| (8.0..24.0).contains(&x) && (8.0..25.0).contains(&y),
        |x, y| {
            let t = (y as f32 - 8.0) / 17.0;
            let shade = 1.3 - 0.9 * (t - 0.25).abs().min(0.75);
            let stripe = if (x + y) % 3 == 0 { 0.72 } else { 1.0 };
            lit(p.base, shade * stripe)
        },
    );
    c.ellipse(7.0, 16.5, 3.0, 12.0, flange);
    c.ellipse(25.0, 16.5, 3.0, 12.0, lit(flange, 0.8));
    c.ellipse(25.0, 16.5, 1.2, 3.0, [40, 30, 24, 255]);
    c.bevel(1.25, 0.75);
    c.outline(0.3);
}

fn pipe(c: &mut Canvas, p: &Pal) {
    let cyl = |y: i32, y0: f32, y1: f32| {
        let t = (y as f32 + 0.5 - y0) / (y1 - y0);
        1.35 - 0.8 * (t - 0.3).abs() * 1.4
    };
    c.fill_where(|x, y| (5.0..27.0).contains(&x) && (11.0..21.0).contains(&y), |_, y| p.at(cyl(y, 11.0, 21.0)));
    for x0 in [2.0f32, 25.0] {
        c.fill_where(|x, y| (x0..x0 + 5.0).contains(&x) && (8.0..24.0).contains(&y), |_, y| p.at(cyl(y, 8.0, 24.0) * 0.92));
    }
    // Bolt dots on the flanges.
    for (x, y) in [(4, 10), (4, 21), (27, 10), (27, 21)] {
        c.set(x, y, p.at(0.55));
    }
    c.outline(0.3);
}

fn brick(c: &mut Canvas, p: &Pal) {
    let bricks = [(3, 19, 15, 27), (17, 19, 29, 27), (9, 9, 22, 17)];
    for (i, &(x0, y0, x1, y1)) in bricks.iter().enumerate() {
        let f = if i == 2 { 1.08 } else { 1.0 };
        for y in y0..=y1 {
            for x in x0..=x1 {
                let v = lit(p.noisy(x, y), f);
                c.set(x, y, v);
            }
        }
        // Top face of each brick, lighter.
        for x in x0..=x1 {
            let v = c.get(x, y0);
            c.set(x, y0, lit(v, 1.3));
            let v = c.get(x, y0 + 1);
            c.set(x, y0 + 1, lit(v, 1.15));
        }
        for y in y0..=y1 {
            let v = c.get(x1, y);
            c.set(x1, y, lit(v, 0.7));
        }
        for x in x0..=x1 {
            let v = c.get(x, y1);
            c.set(x, y1, lit(v, 0.7));
        }
    }
    c.outline(0.3);
}

fn circuit(c: &mut Canvas, p: &Pal) {
    let board = p.base;
    c.rect(4, 6, 27, 26, board);
    c.bevel(1.25, 0.7);
    let gold = [226, 186, 84, 255];
    for &(a, b) in &[((6.0, 10.0), (12.0, 10.0)), ((6.0, 22.0), (12.0, 22.0)), ((20.0, 13.0), (26.0, 13.0)), ((20.0, 19.0), (25.0, 19.0)), ((16.0, 8.0), (16.0, 12.0)), ((16.0, 21.0), (16.0, 25.0))] {
        c.line(a, b, 1.0, gold);
    }
    for (x, y) in [(6, 10), (6, 22), (25, 13), (25, 19), (16, 8), (16, 24)] {
        c.set(x, y, [255, 230, 150, 255]);
    }
    c.rect(11, 12, 20, 20, [36, 36, 40, 255]);
    c.rect(11, 12, 20, 12, [64, 64, 70, 255]);
    for i in 0..4 {
        c.set(12 + i * 2, 11, [200, 200, 200, 255]);
        c.set(12 + i * 2, 21, [200, 200, 200, 255]);
    }
    c.outline(0.3);
}

fn glass_color() -> Rgba {
    [200, 225, 235, 150]
}

fn kit(c: &mut Canvas, p: &Pal) {
    // Glass bottle.
    let glass = glass_color();
    c.circle(16.0, 20.5, 9.0, glass);
    c.rect(13, 6, 18, 13, glass);
    // Liquid in the lower part.
    let liquid = p.base;
    c.fill_where(
        |x, y| (x - 16.0).powi(2) + (y - 20.5).powi(2) <= 7.8 * 7.8 && y >= 16.0,
        |x, y| lit(liquid, 1.15 - 0.3 * ((x as f32 - 10.0 + y as f32 - 16.0) / 20.0).clamp(0.0, 1.0)),
    );
    // Top of the liquid.
    for x in 10..23 {
        if c.get(x, 16)[3] > 0 {
            c.set(x, 16, lit(liquid, 1.45));
        }
    }
    // Cork.
    c.rect(12, 3, 19, 6, [150, 104, 64, 255]);
    c.rect(12, 3, 19, 3, [188, 140, 92, 255]);
    // Shine.
    for (x, y) in [(11, 17), (11, 18), (11, 19), (12, 15), (14, 8), (14, 9), (14, 10)] {
        c.set(x, y, [255, 255, 255, 220]);
    }
    c.outline(0.35);
}

fn vial(c: &mut Canvas, p: &Pal) {
    let glass = glass_color();
    c.fill_where(|x, y| (12.0..20.0).contains(&x) && (5.0..24.0).contains(&y), |_, _| glass);
    c.circle(16.0, 24.0, 4.0, glass);
    c.rect(11, 3, 20, 5, lit(glass, 1.1));
    if p.base != [200, 225, 235, 255] {
        c.fill_where(|x, y| (13.0..19.0).contains(&x) && (17.0..27.0).contains(&y) && (x - 16.0).powi(2) + (y - 24.0).powi(2) <= 9.5 || ((13.0..19.0).contains(&x) && (17.0..24.0).contains(&y)), |_, _| p.base);
    }
    for y in 7..21 {
        c.set(13, y, [255, 255, 255, 220]);
    }
    c.outline(0.35);
}

fn pane(c: &mut Canvas, p: &Pal) {
    let pts = [(5.0, 4.0), (27.0, 8.0), (27.0, 28.0), (5.0, 24.0)];
    c.poly(&pts, [p.base[0], p.base[1], p.base[2], 170]);
    // Shine lines.
    c.line((9.0, 14.0), (15.0, 8.0), 1.2, [255, 255, 255, 230]);
    c.line((10.0, 19.0), (19.0, 10.0), 1.0, [255, 255, 255, 180]);
    c.bevel(1.3, 0.75);
    c.outline(0.45);
}

fn sheet(c: &mut Canvas, p: &Pal) {
    c.poly_with(&[(4.0, 7.0), (27.0, 7.0), (27.0, 20.0), (19.0, 28.0), (4.0, 28.0)], |x, y| lit(p.noisy(x, y), 1.0));
    c.poly(&[(27.0, 20.0), (19.0, 28.0), (20.0, 20.0)], p.at(1.5));
    c.bevel(1.35, 0.7);
    c.outline(0.35);
}

fn bolt(c: &mut Canvas, p: &Pal) {
    c.rect(13, 11, 18, 28, p.at(0.95));
    for y in (13..28).step_by(3) {
        c.rect(13, y, 18, y, p.at(0.65));
    }
    c.poly(&[(9.0, 6.0), (22.0, 6.0), (24.0, 9.0), (22.0, 12.0), (9.0, 12.0), (7.0, 9.0)], p.at(1.15));
    c.rect(9, 6, 22, 7, p.at(1.4));
    c.bevel(1.25, 0.7);
    c.outline(0.3);
}

fn vacuum_tube(c: &mut Canvas, p: &Pal) {
    let glass = glass_color();
    c.ellipse(16.0, 12.5, 8.0, 10.0, glass);
    c.rect(10, 21, 21, 26, [60, 58, 56, 255]);
    c.rect(10, 21, 21, 21, [90, 88, 86, 255]);
    for x in [12, 15, 18, 20] {
        c.rect(x, 27, x, 29, [200, 200, 200, 255]);
    }
    c.rect(13, 9, 18, 18, [120, 120, 130, 200]);
    c.line((14.0, 12.0), (17.0, 12.0), 1.0, p.base);
    c.line((14.0, 15.0), (17.0, 15.0), 1.0, [255, 170, 60, 255]);
    for y in 6..15 {
        c.set(11, y, [255, 255, 255, 220]);
    }
    c.outline(0.35);
}

// ---------------------------------------------------------------- buildings

fn tier_rgba(tier: u8) -> Rgba {
    let t = crate::theme::tier_color(tier);
    [t.r(), t.g(), t.b(), 255]
}

fn machine(c: &mut Canvas, p: &Pal, glyph: MachineGlyph, tier: u8) {
    let body = p.base;
    // Feet.
    c.rect(5, 27, 8, 29, lit(body, 0.45));
    c.rect(23, 27, 26, 29, lit(body, 0.45));
    // Body with cut corners.
    c.poly_with(&[(4.0, 5.0), (28.0, 5.0), (29.0, 6.0), (29.0, 28.0), (3.0, 28.0), (3.0, 6.0)], |x, y| mixc(body, lit(p.noisy(x, y), 1.0), 0.3));
    // Tier band.
    let band = tier_rgba(tier);
    c.rect(3, 6, 28, 9, band);
    c.bevel(1.3, 0.62);
    // Inset panel for the glyph.
    c.rect(9, 12, 22, 25, lit(body, 0.35));
    c.rect(9, 12, 22, 12, lit(body, 0.22));
    c.rect(9, 12, 9, 25, lit(body, 0.25));
    c.rect(9, 25, 22, 25, lit(body, 0.6));
    c.rect(22, 12, 22, 25, lit(body, 0.6));
    // Rivets.
    for (x, y) in [(5, 11), (26, 11), (5, 25), (26, 25)] {
        c.set(x, y, lit(body, 1.45));
        c.set(x + 1, y + 1, lit(body, 0.6));
    }
    glyph_draw(c, glyph, mixc(band, [255, 255, 255, 255], 0.35));
    c.outline(0.3);
}

/// Draw a machine glyph in the panel from (10, 13) to (21, 24).
fn glyph_draw(c: &mut Canvas, glyph: MachineGlyph, col: Rgba) {
    let (cx, cy) = (16.0, 19.0);
    match glyph {
        MachineGlyph::None => {}
        MachineGlyph::Gear => {
            let step = std::f32::consts::TAU / 6.0;
            c.fill_where(
                |x, y| {
                    let (dx, dy) = (x - cx, y - cy);
                    let r = (dx * dx + dy * dy).sqrt();
                    let frac = ((dy.atan2(dx) + std::f32::consts::PI) / step).fract();
                    let lim = if (0.25..0.75).contains(&frac) { 5.3 } else { 4.0 };
                    r <= lim && r >= 1.6
                },
                |_, _| col,
            );
        }
        MachineGlyph::Flame => {
            flame_shape(c, cx, 24.5, 11.0, 4.6, [240, 90, 40, 255]);
            flame_shape(c, cx, 24.5, 7.0, 2.6, [255, 210, 90, 255]);
        }
        MachineGlyph::Hammer => {
            c.rect(11, 14, 20, 17, col);
            c.rect(15, 17, 16, 24, lit(col, 0.8));
        }
        MachineGlyph::Crusher => {
            for i in 0..3 {
                let x = 11 + i * 4;
                c.poly(&[(x as f32, 14.0), (x as f32 + 4.0, 14.0), (x as f32 + 2.0, 18.5)], col);
                c.poly(&[(x as f32, 24.0), (x as f32 + 4.0, 24.0), (x as f32 + 2.0, 19.5)], col);
            }
        }
        MachineGlyph::Drop => {
            c.circle(cx, 20.5, 3.6, [110, 170, 255, 255]);
            c.poly(&[(cx - 3.3, 19.5), (cx + 3.3, 19.5), (cx, 13.5)], [110, 170, 255, 255]);
        }
        MachineGlyph::Flask => {
            c.rect(15, 13, 16, 16, col);
            c.poly(&[(14.0, 16.0), (18.0, 16.0), (21.5, 24.0), (10.5, 24.0)], col);
            c.poly(&[(12.5, 20.0), (19.5, 20.0), (21.5, 24.0), (10.5, 24.0)], [120, 220, 120, 255]);
        }
        MachineGlyph::Arrow => {
            c.rect(11, 17, 17, 20, col);
            c.poly(&[(17.0, 13.5), (22.0, 18.5), (17.0, 23.5)], col);
        }
        MachineGlyph::Lightning => {
            c.poly(&[(17.5, 13.0), (11.5, 19.5), (15.5, 19.5), (13.5, 25.0), (20.5, 17.5), (16.5, 17.5), (19.0, 13.0)], [255, 220, 70, 255]);
        }
        MachineGlyph::Fan => {
            for k in 0..3 {
                let a = k as f32 * std::f32::consts::TAU / 3.0;
                let (dx, dy) = (a.cos(), a.sin());
                c.line((cx, cy), (cx + dx * 5.0 - dy * 1.5, cy + dy * 5.0 + dx * 1.5), 2.6, col);
            }
            c.circle(cx, cy, 1.4, lit(col, 0.6));
        }
        MachineGlyph::Magnet => {
            c.fill_where(
                |x, y| {
                    let r = ((x - cx).powi(2) + (y - 17.0).powi(2)).sqrt();
                    ((2.2..=5.2).contains(&r) && y <= 17.0) || ((10.8..13.8).contains(&x) || (18.2..21.2).contains(&x)) && (17.0..24.0).contains(&y)
                },
                |_, y| if y >= 21 { [230, 70, 60, 255] } else { col },
            );
        }
        MachineGlyph::Drill => {
            c.poly(&[(11.0, 14.0), (21.0, 14.0), (16.0, 25.0)], col);
            c.line((12.5, 17.0), (19.5, 16.0), 1.0, lit(col, 0.6));
            c.line((13.5, 20.0), (18.5, 19.0), 1.0, lit(col, 0.6));
        }
        MachineGlyph::Wire => {
            for i in 0..4 {
                let x = 11.5 + i as f32 * 3.0;
                c.ellipse(x, cy, 1.5, 4.5, [220, 130, 70, 255]);
            }
            c.fill_where(|x, y| (11.0..21.0).contains(&x) && (18.0..20.0).contains(&y), |_, _| lit(col, 0.5));
        }
        MachineGlyph::Plus => {
            c.rect(14, 14, 17, 23, col);
            c.rect(11, 17, 20, 20, col);
        }
    }
}

fn belt(c: &mut Canvas, p: &Pal, tier: u8) {
    let frame = p.base;
    // Legs.
    c.rect(6, 22, 7, 28, lit(frame, 0.55));
    c.rect(24, 22, 25, 28, lit(frame, 0.55));
    // Frame and belt.
    c.rect(2, 11, 29, 21, lit(frame, 1.0));
    c.rect(2, 13, 29, 19, [44, 42, 40, 255]);
    for x in [6.0f32, 12.0, 18.0, 24.0] {
        c.circle(x + 0.5, 16.5, 2.4, lit(frame, 0.8));
        c.set(x as i32, 16, lit(frame, 1.3));
    }
    c.bevel(1.3, 0.6);
    // Arrows on the top surface.
    let arrow = mixc(tier_rgba(tier), [255, 230, 120, 255], 0.5);
    for x0 in [6, 14, 22] {
        c.set(x0, 9, arrow);
        c.set(x0 + 1, 10, arrow);
        c.set(x0 + 2, 11, arrow);
        c.set(x0 + 1, 12, arrow);
        c.set(x0, 13, arrow);
    }
    c.rect(2, 11, 29, 11, lit(frame, 1.35));
    c.outline(0.3);
}

fn crate_icon(c: &mut Canvas, p: &Pal, tier: u8) {
    let wood = p.base;
    c.fill_where(|x, y| (4.0..28.0).contains(&x) && (6.0..28.0).contains(&y), |x, y| {
        let plank = if (y - 6) % 5 == 4 { 0.7 } else { 1.0 };
        lit(p.noisy(x, y), plank)
    });
    // Frame.
    let edge = if tier >= 1 { [110, 110, 116, 255] } else { lit(wood, 0.75) };
    c.rect(4, 6, 27, 8, edge);
    c.rect(4, 25, 27, 27, edge);
    c.rect(4, 6, 6, 27, edge);
    c.rect(25, 6, 27, 27, edge);
    c.line((7.0, 24.0), (24.0, 9.0), 2.2, lit(wood, 0.85));
    c.bevel(1.3, 0.65);
    if tier >= 1 {
        for (x, y) in [(5, 7), (26, 7), (5, 26), (26, 26)] {
            c.set(x, y, [190, 190, 196, 255]);
        }
    }
    c.outline(0.3);
}

fn barrel(c: &mut Canvas, p: &Pal) {
    c.fill_where(
        |x, y| {
            if !(4.0..29.0).contains(&y) {
                return false;
            }
            let t = (y - 4.0) / 25.0;
            let half = 8.0 + 2.2 * (t * std::f32::consts::PI).sin();
            (x - 16.0).abs() <= half
        },
        |x, y| {
            let t = (x as f32 - 6.0) / 20.0;
            let f = 1.25 - 0.6 * (t - 0.3).abs() * 1.3;
            let stave = if (x - 4) % 4 == 0 { 0.8 } else { 1.0 };
            lit(p.noisy(x, y), f * stave)
        },
    );
    let hoop = [90, 92, 98, 255];
    for y in [7, 16, 25] {
        for x in 4..29 {
            if c.opaque(x, y) {
                c.set(x, y, hoop);
                c.set(x, y + 1, lit(hoop, 0.7));
            }
        }
    }
    c.ellipse(16.0, 4.5, 7.5, 1.5, lit(p.base, 0.6));
    c.outline(0.3);
}

fn wall(c: &mut Canvas, p: &Pal) {
    c.fill_where(|x, y| (3.0..29.0).contains(&x) && (3.0..29.0).contains(&y), |x, y| {
        let row = (y - 3) / 5;
        let offset = if row % 2 == 0 { 0 } else { 4 };
        let mortar = (y - 3) % 5 == 4 || (x - 3 + offset) % 8 == 7;
        if mortar { [70, 64, 60, 255] } else { p.noisy(x, y) }
    });
    c.bevel(1.25, 0.65);
    c.outline(0.3);
}

fn ladder(c: &mut Canvas, p: &Pal) {
    c.rect(7, 2, 9, 29, p.at(1.0));
    c.rect(22, 2, 24, 29, p.at(0.9));
    for y in (5..29).step_by(5) {
        c.rect(10, y, 21, y + 1, p.at(0.85));
    }
    c.bevel(1.3, 0.65);
    c.outline(0.3);
}

fn campfire(c: &mut Canvas, p: &Pal) {
    flame_shape(c, 16.0, 24.0, 20.0, 7.0, [230, 80, 30, 255]);
    flame_shape(c, 16.0, 24.0, 13.0, 4.5, [255, 170, 50, 255]);
    flame_shape(c, 16.0, 24.0, 7.0, 2.2, [255, 240, 160, 255]);
    c.line((5.0, 28.0), (26.0, 22.0), 3.4, p.at(0.9));
    c.line((6.0, 22.0), (27.0, 28.0), 3.4, p.at(0.75));
    for x in [4, 10, 16, 22, 28] {
        c.circle(x as f32, 29.0, 1.8, [110, 108, 104, 255]);
    }
    c.outline(0.35);
}

fn workbench(c: &mut Canvas, p: &Pal) {
    c.rect(5, 16, 7, 29, p.at(0.7));
    c.rect(24, 16, 26, 29, p.at(0.7));
    c.rect(5, 24, 26, 25, p.at(0.6));
    c.fill_where(|x, y| (3.0..29.0).contains(&x) && (12.0..17.0).contains(&y), |x, y| lit(p.noisy(x, y), 1.05));
    c.bevel(1.3, 0.65);
    // A hammer on top.
    c.rect(9, 9, 21, 10, [150, 104, 64, 255]);
    c.rect(19, 5, 22, 11, [150, 150, 156, 255]);
    c.rect(19, 5, 22, 5, [200, 200, 206, 255]);
    c.outline(0.3);
}

fn hopper(c: &mut Canvas, p: &Pal) {
    c.poly_with(&[(3.0, 4.0), (29.0, 4.0), (19.0, 21.0), (13.0, 21.0)], |x, y| {
        let plank = if (y - 4) % 4 == 3 { 0.75 } else { 1.0 };
        lit(p.noisy(x, y), plank)
    });
    c.rect(13, 21, 18, 28, p.at(0.8));
    c.rect(3, 4, 28, 6, p.at(1.25));
    c.bevel(1.3, 0.65);
    c.outline(0.3);
}

fn mold(c: &mut Canvas, p: &Pal) {
    c.poly(&[(2.0, 16.0), (16.0, 10.0), (30.0, 16.0), (16.0, 22.0)], p.at(1.2));
    c.poly(&[(2.0, 16.0), (16.0, 22.0), (16.0, 27.0), (2.0, 21.0)], p.at(0.9));
    c.poly(&[(16.0, 22.0), (30.0, 16.0), (30.0, 21.0), (16.0, 27.0)], p.at(0.7));
    // The cavity with metal in it.
    c.poly(&[(8.0, 16.0), (16.0, 12.5), (24.0, 16.0), (16.0, 19.5)], [60, 44, 36, 255]);
    c.poly(&[(10.0, 16.0), (16.0, 13.5), (22.0, 16.0), (16.0, 18.5)], [255, 150, 60, 255]);
    c.outline(0.3);
}

fn crucible(c: &mut Canvas, p: &Pal) {
    c.poly_with(&[(5.0, 8.0), (27.0, 8.0), (23.0, 28.0), (9.0, 28.0)], |x, y| {
        let t = (x as f32 - 5.0) / 22.0;
        lit(p.noisy(x, y), 1.2 - 0.5 * t)
    });
    c.ellipse(16.0, 8.5, 11.0, 3.0, lit(p.base, 0.45));
    c.ellipse(16.0, 9.0, 9.0, 2.0, [255, 160, 50, 255]);
    c.ellipse(15.0, 8.8, 4.0, 1.0, [255, 230, 150, 255]);
    c.outline(0.3);
}

fn tank(c: &mut Canvas, p: &Pal, tier: u8) {
    c.fill_where(|x, y| (6.0..26.0).contains(&x) && (5.0..28.0).contains(&y), |x, _| {
        let t = (x as f32 - 6.0) / 20.0;
        p.at(1.3 - 0.8 * (t - 0.3).abs() * 1.3)
    });
    c.ellipse(16.0, 5.0, 10.0, 2.5, p.at(1.35));
    let band = tier_rgba(tier);
    for y in [10, 22] {
        c.rect(6, y, 25, y + 1, band);
    }
    c.rect(4, 26, 27, 28, lit(p.base, 0.5));
    c.outline(0.3);
}

fn cable(c: &mut Canvas, p: &Pal) {
    c.fill_where(
        |x, y| {
            let r = ((x - 15.0).powi(2) + (y - 15.0).powi(2)).sqrt();
            (6.0..11.5).contains(&r)
        },
        |x, y| {
            let a = ((y as f32 - 15.0).atan2(x as f32 - 15.0) * 4.0).sin();
            lit(p.base, 1.0 + 0.2 * a)
        },
    );
    c.line((22.0, 22.0), (29.0, 29.0), 3.0, p.base);
    c.set(29, 29, [230, 150, 80, 255]);
    c.bevel(1.3, 0.65);
    c.outline(0.3);
}

fn solar(c: &mut Canvas, p: &Pal) {
    c.rect(15, 18, 17, 28, [110, 110, 116, 255]);
    c.rect(10, 27, 22, 29, [90, 90, 96, 255]);
    let pts = [(3.0, 10.0), (23.0, 4.0), (29.0, 16.0), (9.0, 22.0)];
    c.poly_with(&pts, |x, y| {
        let grid = (x + y * 3) % 6 == 0 || (x * 3 - y) % 7 == 0;
        if grid { lit(p.base, 1.5) } else { p.base }
    });
    c.bevel(1.4, 0.7);
    c.outline(0.3);
}

fn battery(c: &mut Canvas, p: &Pal) {
    c.rect(8, 4, 11, 7, [200, 60, 50, 255]);
    c.rect(20, 4, 23, 7, [70, 70, 74, 255]);
    c.rect(5, 7, 26, 28, p.base);
    c.rect(5, 7, 26, 10, lit(p.base, 1.3));
    c.bevel(1.3, 0.65);
    c.poly(&[(17.5, 12.0), (11.5, 19.0), (15.5, 19.0), (13.5, 26.0), (20.5, 17.0), (16.5, 17.0), (19.0, 12.0)], [255, 220, 70, 255]);
    c.outline(0.3);
}

// ---------------------------------------------------------------- atlas

const SMALL: usize = ART; // 32 px
const LARGE: usize = ART * 2; // 64 px
const PAD_S: usize = 1;
const PAD_L: usize = 2;
const CELL_S: usize = SMALL + 2 * PAD_S;
const CELL_L: usize = LARGE + 2 * PAD_L;
const COLS: usize = 16;

/// The icons of all catalog items in egui textures.
pub struct IconAtlas {
    revision: u64,
    index: HashMap<ItemId, usize>,
    rows: usize,
    small: TextureHandle,
    large: TextureHandle,
    large_sharp: TextureHandle,
}

impl IconAtlas {
    /// Draw all icons of the catalog and upload them. Slot 0 is the "unknown" icon.
    pub fn build(ctx: &egui::Context, catalog: &Catalog) -> Self {
        let mut canvases = vec![unknown_icon()];
        let mut index = HashMap::new();
        // Items with the same spec share one picture.
        let mut by_spec: Vec<(IconSpec, usize)> = vec![];
        for info in catalog.items() {
            let slot = match by_spec.iter().find(|(s, _)| *s == info.icon) {
                Some((_, i)) => *i,
                None => {
                    canvases.push(draw_icon(&info.icon));
                    by_spec.push((info.icon.clone(), canvases.len() - 1));
                    canvases.len() - 1
                }
            };
            index.insert(info.id, slot);
        }
        let rows = canvases.len().div_ceil(COLS);
        let small = pack(&canvases, 1, CELL_S, PAD_S, rows);
        let large = pack(&canvases, 2, CELL_L, PAD_L, rows);
        let linear = TextureOptions::LINEAR;
        let sharp = TextureOptions::NEAREST;
        Self {
            revision: catalog.revision,
            index,
            rows,
            small: ctx.load_texture("foundry-icons-32", small.clone(), linear),
            large: ctx.load_texture("foundry-icons-64", large.clone(), linear),
            large_sharp: ctx.load_texture("foundry-icons-64-sharp", large, sharp),
        }
    }

    /// The catalog revision this atlas was built for.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The texture and the UV rectangle of an item icon, for a size on the screen in pixels.
    pub fn texture_uv(&self, item: ItemId, pixels: f32) -> (TextureId, Rect) {
        let slot = self.index.get(&item).copied().unwrap_or(0);
        let (col, row) = (slot % COLS, slot / COLS);
        let (tex, cell, pad, size) = if pixels <= 40.0 {
            (&self.small, CELL_S, PAD_S, SMALL)
        } else if pixels < 96.0 {
            (&self.large, CELL_L, PAD_L, LARGE)
        } else {
            (&self.large_sharp, CELL_L, PAD_L, LARGE)
        };
        let w = (COLS * cell) as f32;
        let h = (self.rows * cell) as f32;
        let x0 = (col * cell + pad) as f32;
        let y0 = (row * cell + pad) as f32;
        let uv = Rect::from_min_max(Pos2::new(x0 / w, y0 / h), Pos2::new((x0 + size as f32) / w, (y0 + size as f32) / h));
        (tex.id(), uv)
    }

    /// Paint the icon of an item into `rect`. `tint` is multiplied (use white for no change).
    pub fn paint(&self, painter: &Painter, item: ItemId, rect: Rect, tint: Color32) {
        let (tex, uv) = self.texture_uv(item, rect.width() * painter.pixels_per_point());
        painter.image(tex, rect, uv, tint);
    }
}

fn pack(canvases: &[Canvas], scale: usize, cell: usize, pad: usize, rows: usize) -> ColorImage {
    let w = COLS * cell;
    let h = rows * cell;
    let mut rgba = vec![0u8; w * h * 4];
    let size = ART * scale;
    for (i, canvas) in canvases.iter().enumerate() {
        let (col, row) = (i % COLS, i / COLS);
        let img = canvas.to_rgba(scale);
        for y in 0..size {
            let dst = ((row * cell + pad + y) * w + col * cell + pad) * 4;
            let src = y * size * 4;
            rgba[dst..dst + size * 4].copy_from_slice(&img[src..src + size * 4]);
        }
    }
    ColorImage::from_rgba_unmultiplied([w, h], &rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shape_draws_something_inside_the_margin() {
        let shapes = [
            IconShape::Pile,
            IconShape::Drop,
            IconShape::Gas,
            IconShape::Block,
            IconShape::Flame,
            IconShape::Gear,
            IconShape::Plate,
            IconShape::Ingot,
            IconShape::Rod,
            IconShape::WireCoil,
            IconShape::Pipe,
            IconShape::Brick,
            IconShape::Circuit,
            IconShape::Kit,
            IconShape::Vial,
            IconShape::Pane,
            IconShape::Sheet,
            IconShape::Bolt,
            IconShape::VacuumTube,
            IconShape::Machine(MachineGlyph::Gear),
            IconShape::Belt,
            IconShape::Crate,
            IconShape::Barrel,
            IconShape::Wall,
            IconShape::Ladder,
            IconShape::Campfire,
            IconShape::Workbench,
            IconShape::Hopper,
            IconShape::Mold,
            IconShape::Crucible,
            IconShape::Tank,
            IconShape::Cable,
            IconShape::SolarPanel,
            IconShape::Battery,
        ];
        for shape in shapes {
            let c = draw_icon(&IconSpec::new(shape, [180, 120, 60, 255]));
            let filled = c.px.iter().filter(|p| p[3] > 0).count();
            assert!(filled > 60, "{shape:?} draws only {filled} pixels");
        }
    }
}
