//! Trees: a broad tree with a round crown in the temperate biome, a conifer in the tundra.
//! Also small things on the ground: bushes (leaves) and boulders (stone).
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
    /// A rubber tree: its trunk and branches are rubber tree wood (resin for rubber, Tier 1).
    rubber: bool,
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
            Tree { x, base, height, kind: Kind::Conifer, half: 1, crown: 9 + height / 8, h, rubber: false }
        }
        _ => {
            // Most trees are of middle size; some are small and some are tall.
            let height = match r(20, 10) {
                0..2 => 26 + r(8, 10),
                2..8 => 36 + r(8, 18),
                _ => 54 + r(8, 16),
            };
            // Thick trunks: wood is the first fuel and building material.
            let half = if height > 50 { 3 } else if height > 36 { 2 } else { 1 };
            // One broad tree in 5 is a rubber tree, from 600 cells away from the Hub on.
            let rubber = x.abs() > 600 && (h >> 4) % 5 == 0;
            Tree { x, base, height, kind: Kind::Broad, half, crown: (9 + height / 4).min(REACH - 2), h, rubber }
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
        let wood = if self.rubber { m.rubber_wood } else { m.wood };
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
                                    row[i] = wood;
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
                        row[i] = wood;
                    }
                }
            }
        }
    }
}

/// A bush or a boulder on the ground. Positions are world cells.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Decor {
    cx: i32,
    cy: i32,
    rx: i32,
    ry: i32,
    boulder: bool,
    h: u32,
}

/// The bush or boulder of slot `k`, if the slot has one. It stands between the trees of the
/// slots (at the middle of the slot, where no trunk is).
pub(crate) fn decor_in_slot(ctx: &Ctx, k: i32) -> Option<Decor> {
    let h = hash1(ctx.sd.get(sd::DECOR), k);
    let x = k * SLOT + (SLOT / 2 + 20 + (h % 12) as i32 - 6).rem_euclid(SLOT);
    if x.abs() < HUB_CLEAR {
        return None;
    }
    let roll = unit(hash1(h, 2));
    let gr = ctx.ground_at(x);
    let (left, right) = (ctx.ground_at(x - 2).g, ctx.ground_at(x + 2).g);
    let col = ctx.column(x, gr, left, right);
    let (bush, boulder) = match col.biome {
        Biome::Temperate => (0.4, 0.08),
        Biome::Tundra => (0.08, 0.12),
        Biome::Desert => (0.0, 0.06),
    };
    if roll >= bush + boulder {
        return None;
    }
    let g = &col.gr;
    if col.steep || g.water < g.g || g.bed > 0 || g.salt > 0 {
        return None;
    }
    let r = |shift: u32, n: u32| ((h >> shift) % n) as i32;
    let top = g.g.min(left).min(right);
    Some(if roll < bush {
        let ry = 3 + r(8, 3);
        Decor { cx: x, cy: top - ry + 1, rx: 5 + r(12, 5), ry, boulder: false, h }
    } else {
        let ry = 3 + r(8, 4);
        Decor { cx: x, cy: g.g.max(left).max(right) - ry / 2 + 1, rx: 4 + r(12, 6), ry, boulder: true, h }
    })
}

impl Decor {
    /// Draw into a grid like `Tree::draw`. Leaves go only into air; a boulder goes into air and
    /// ground but not into water.
    pub fn draw(&self, m: &Mats, grid: &mut [u16], xl: i32, y0: i32, rows: usize) {
        let (xa, xb) = ((self.cx - self.rx).max(xl), (self.cx + self.rx + 1).min(xl + GW as i32));
        let (ya, yb) = ((self.cy - self.ry).max(y0), (self.cy + self.ry + 1).min(y0 + rows as i32));
        for y in ya..yb {
            let dy = (y - self.cy) as f32 / self.ry as f32;
            let edge = (hash2(self.h, y, 0) % 3) as f32 * 0.4;
            let half = self.rx as f32 * (1.0 - dy * dy).max(0.0).sqrt() + edge - 0.4;
            for x in xa..xb {
                if ((x - self.cx) as f32).abs() > half {
                    continue;
                }
                let i = ((y - y0) as usize) * GW + (x - xl) as usize;
                if self.boulder {
                    if grid[i] != m.water && grid[i] != m.wood && grid[i] != m.rubber_wood && grid[i] != m.leaves {
                        grid[i] = m.stone;
                    }
                } else if grid[i] == 0 && hash2(self.h ^ 0xb005, x, y) % 100 >= 12 {
                    grid[i] = m.leaves;
                }
            }
        }
    }
}
