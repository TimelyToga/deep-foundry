//! Trees: a broad tree with a round crown in the temperate biome, a conifer in the tundra.
//!
//! Each slot of `SLOT` cells along x can hold one tree. The tree, its size and its shape come
//! from the seed and the slot number, so every chunk draws the same tree. A chunk draws all
//! trees of the slots near it, in the order of the slot number: leaves go only into air, and a
//! trunk goes over leaves.

use crate::Mats;
use crate::noise::{GW, hash1, hash2, unit};
use crate::surface::{Biome, Ctx, sd};

/// Width of a tree slot in cells.
pub const SLOT: i32 = 40;
/// The farthest a tree reaches left or right of its trunk center.
pub const REACH: i32 = 26;
/// The highest a tree reaches above its base.
pub const MAX_HEIGHT: i32 = 96;
/// Rows of roots below the base.
const ROOTS: i32 = 5;
/// No trees on the flat place of the Hub and next to it.
const HUB_CLEAR: i32 = 150;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Broad,
    Conifer,
}

/// One tree. Positions are world cells.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Tree {
    /// The center column of the trunk.
    x: i32,
    /// The row where the trunk meets the ground (the lowest ground under the trunk).
    base: i32,
    height: i32,
    kind: Kind,
    /// Half-width of the trunk: the trunk is 2 × `half` + 1 cells wide.
    half: i32,
    /// Radius of the crown.
    crown: i32,
    h: u32,
}

/// An ellipse of leaves.
#[derive(Clone, Copy)]
struct Blob {
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
}

/// The tree of slot `k`, if the slot has one.
pub(crate) fn in_slot(ctx: &Ctx, k: i32) -> Option<Tree> {
    let h = hash1(ctx.sd.get(sd::TREES), k);
    let x = k * SLOT + 6 + (h % (SLOT - 12) as u32) as i32;
    if x.abs() < HUB_CLEAR {
        return None;
    }
    let gr = ctx.ground_at(x);
    let (left, right) = (ctx.ground_at(x - 2).g, ctx.ground_at(x + 2).g);
    let col = ctx.column(x, gr, left, right);
    if unit(hash1(h, 1)) >= ctx.tree_density(x, col.biome) {
        return None;
    }
    let g = &col.gr;
    if col.steep || g.water < g.g || g.beach > 0 || g.bed > 0 || g.salt > 0 {
        return None;
    }
    let base = g.g.max(left).max(right);
    let r = |shift: u32, n: u32| ((h >> shift) % n) as i32;
    Some(match col.biome {
        Biome::Tundra => {
            let height = 40 + r(8, 36);
            Tree { x, base, height, kind: Kind::Conifer, half: 1, crown: 9 + height / 8, h }
        }
        _ => {
            let height = 34 + r(8, 26);
            let half = if height > 50 { 2 } else { 1 };
            Tree { x, base, height, kind: Kind::Broad, half, crown: 11 + height / 5, h }
        }
    })
}

impl Tree {
    /// The top row that the tree reaches.
    fn top(&self) -> i32 {
        self.base - self.height - 2
    }

    fn blobs(&self) -> [Blob; 4] {
        let c = self.crown as f32;
        let j = |i: i32| ((hash1(self.h, 10 + i) >> 8) as f32 / 16_777_216.0 - 0.5) * c * 0.3;
        let (x, cy) = (self.x as f32, (self.base - self.height) as f32 + c * 0.62);
        [
            Blob { cx: x + j(0), cy, rx: c, ry: c * 0.62 },
            Blob { cx: x - c * 0.6 + j(1), cy: cy + c * 0.28, rx: c * 0.62, ry: c * 0.45 },
            Blob { cx: x + c * 0.6 + j(2), cy: cy + c * 0.22, rx: c * 0.62, ry: c * 0.45 },
            Blob { cx: x + j(3), cy: cy - c * 0.35, rx: c * 0.6, ry: c * 0.42 },
        ]
    }

    /// Draw the tree into a grid of `GW` columns (world x = `xl` + column) and `rows` rows
    /// (world y = `y0` + row). Leaves go only into air. The trunk goes over anything but water;
    /// below the base (the roots) only over ground.
    pub fn draw(&self, m: &Mats, grid: &mut [u16], xl: i32, y0: i32, rows: usize) {
        let y_lo = self.top().max(y0);
        let y_hi = (self.base + ROOTS + 1).min(y0 + rows as i32);
        let x_lo = (self.x - REACH).max(xl);
        let x_hi = (self.x + REACH + 1).min(xl + GW as i32);
        if y_lo >= y_hi || x_lo >= x_hi {
            return;
        }
        let blobs = self.blobs();
        let trunk_top = match self.kind {
            Kind::Broad => self.base - self.height * 4 / 5,
            Kind::Conifer => self.base - self.height,
        };
        let c = self.crown as f32;
        for y in y_lo..y_hi {
            let row = &mut grid[((y - y0) as usize) * GW..((y - y0) as usize + 1) * GW];
            // Leaves: a span of columns for each part of the crown in this row.
            let mut spans = [(0i32, -1i32); 4];
            match self.kind {
                Kind::Broad => {
                    for (i, b) in blobs.iter().enumerate() {
                        let dy = (y as f32 + 0.5 - b.cy) / b.ry;
                        if dy.abs() < 1.0 {
                            let edge = (hash2(self.h, y, i as i32) % 3) as f32 - 1.0;
                            let half = b.rx * (1.0 - dy * dy).sqrt() + edge * 0.7;
                            spans[i] = ((b.cx - half).round() as i32, (b.cx + half).round() as i32);
                        }
                    }
                }
                Kind::Conifer => {
                    let t = y - (self.base - self.height);
                    let bottom = self.height * 3 / 4;
                    if (0..bottom).contains(&t) {
                        let tier = (t % 10) as f32 / 10.0;
                        let half = ((1.0 + t as f32 * 0.32) * (0.5 + 0.5 * tier)).min(c);
                        let edge = (hash2(self.h, y, 9) % 3) as f32 * 0.5;
                        let half = (half + edge).round() as i32;
                        spans[0] = (self.x - half, self.x + half);
                    }
                }
            }
            for &(a, b) in &spans {
                for x in a.max(x_lo)..=b.min(x_hi - 1) {
                    let i = (x - xl) as usize;
                    if row[i] == 0 && hash2(self.h ^ 0x1eaf, x, y) % 100 >= 7 {
                        row[i] = m.leaves;
                    }
                }
            }
            // Branches of a broad tree: two lines from the trunk up and out into the crown.
            if self.kind == Kind::Broad {
                let (b0, b1) = (self.base - self.height / 2, self.base - self.height + self.crown * 2 / 3);
                if y <= b0 && y >= b1 {
                    let t = (b0 - y) as f32 / (b0 - b1).max(1) as f32;
                    for dir in [-1, 1] {
                        let reach = c * if dir < 0 { 0.55 } else { 0.5 };
                        let bx = self.x + dir * (self.half + (t * reach) as i32);
                        for x in [bx, bx + dir] {
                            if (x_lo..x_hi).contains(&x) {
                                let i = (x - xl) as usize;
                                if row[i] == 0 || row[i] == m.leaves {
                                    row[i] = m.wood;
                                }
                            }
                        }
                    }
                }
            }
            // The trunk, wider at the foot. Roots go into the ground.
            if y >= trunk_top {
                let flare = if y > self.base - 4 { 1 } else { 0 };
                let half = self.half + flare;
                for x in (self.x - half).max(x_lo)..=(self.x + half).min(x_hi - 1) {
                    let i = (x - xl) as usize;
                    if row[i] != m.water && (y <= self.base || row[i] != 0) {
                        row[i] = m.wood;
                    }
                }
            }
        }
    }
}
