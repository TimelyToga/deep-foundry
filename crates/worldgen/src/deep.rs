//! Deep features: large caverns with glow moss (some with a lake and a rich ore deposit in a
//! wall), crystal geodes, lava chambers and gold deposits.
//!
//! The world is cut into deep regions of `DEEP_REGION` × `DEEP_REGION` cells. Each region can
//! have one feature of each kind, from the seed and the region position. Every feature stays at
//! least `MARGIN` cells inside its region, so a chunk (and the border of its grid) only needs the
//! features of its own region. All shapes are functions of the world position, so chunks meet
//! with no seams.
//!
//! | Feature | Depth below the surface level | What it is |
//! |---|---|---|
//! | cavern | 250 to 5200 | a room of 3 joined ellipses with rough walls; glow moss on the floor and hanging from the ceiling; some have a lake; most have a rich ore deposit in a side wall (copper, tin, iron or coal above 1800; chalcopyrite, gold or iron below) |
//! | geode | 1200 to 6500 | a hollow ball lined with glow crystal |
//! | lava chamber | 3000 and deeper | lava in a shell of obsidian, air above it |
//! | gold deposit | 1700 and deeper | a small blob of native gold in the rock |
//!
//! The start region has a fixed cavern about 330 cells under the building place right of the
//! Hub (`START_CAVE`), so the first cave is easy to find.

use super::Fill;
use crate::noise::{GH, GW, fbm2, hash1, hash2, noise1};
use crate::surface::sd;

/// Width and height of a deep region in cells.
pub(crate) const DEEP_REGION: i32 = 1024;
/// Every feature stays this many cells inside its region.
const MARGIN: i32 = 24;
/// The fixed cave of the start area: center x, depth below the surface level, half-width,
/// half-height.
pub(crate) const START_CAVE: (i32, i32, i32, i32) = (300, 330, 85, 28);

#[derive(Debug, Clone, Copy)]
struct Cavern {
    cx: i32,
    cy: i32,
    rx: i32,
    ry: i32,
    /// Water below this row (no lake: below the cavern).
    lake: i32,
    /// The ore of the deposit in a side wall (0: none), and the side (-1 left, 1 right).
    lode: u16,
    side: i32,
    h: u32,
}

impl Cavern {
    /// The three ellipses: (center x, center y, half-width, half-height).
    fn parts(&self) -> [(f32, f32, f32, f32); 3] {
        let (cx, cy, rx, ry) = (self.cx as f32, self.cy as f32, self.rx as f32, self.ry as f32);
        [
            (cx, cy, rx, ry),
            (cx - rx * 0.7, cy + ry * 0.45, rx * 0.45, ry * 0.75),
            (cx + rx * 0.65, cy - ry * 0.35, rx * 0.45, ry * 0.7),
        ]
    }

    /// True for a cell inside the cavern (air or lake).
    fn inside(&self, x: i32, y: i32) -> bool {
        let rough = 0.45 * fbm2(self.h ^ 0x000C_A7E5, x, y, 28, 14, 3);
        self.parts().iter().any(|&(cx, cy, rx, ry)| {
            let (dx, dy) = ((x as f32 - cx) / rx, (y as f32 - cy) / ry);
            dx * dx + dy * dy < 1.0 + rough
        })
    }

    /// The rich ore deposit: center and half-sizes.
    fn lode(&self) -> (i32, i32, i32, i32) {
        (self.cx + self.side * (self.rx * 9 / 10), self.cy + self.ry * 2 / 5, 12 + (self.h % 9) as i32, 8 + ((self.h >> 4) % 7) as i32)
    }

    /// The cells that a chunk must look at: (x0, y0, x1, y1), end exclusive.
    fn bounds(&self) -> (i32, i32, i32, i32) {
        let w = self.rx * 13 / 10 + 22;
        let hh = self.ry * 13 / 10 + 18;
        (self.cx - w, self.cy - hh, self.cx + w + 1, self.cy + hh + 1)
    }
}

/// A stalactite (`down`: it hangs from the ceiling) or a stalagmite (it stands on the floor): a
/// thin triangle of rock.
#[derive(Debug, Clone, Copy)]
struct Spike {
    x: i32,
    /// The row where it touches the ceiling or the floor.
    base: i32,
    half_w: i32,
    len: i32,
    down: bool,
}

impl Spike {
    fn has(&self, x: i32, y: i32) -> bool {
        let k = if self.down { y - self.base } else { self.base - y };
        (0..self.len).contains(&k) && (x - self.x).abs() * self.len <= self.half_w * (self.len - k)
    }
}

/// Columns between two spikes (on average).
const SPIKE_SLOT: i32 = 9;

impl Cavern {
    /// The spikes that can touch grid columns x0..x1. A spike is at most 3 cells from its column.
    fn spikes(&self, x0: i32, x1: i32) -> Vec<Spike> {
        let mut out = vec![];
        for k in (x0 - 4).div_euclid(SPIKE_SLOT)..=(x1 + 4).div_euclid(SPIKE_SLOT) {
            let h = hash1(self.h ^ 0x5B1C_E000, k);
            let x = k * SPIKE_SLOT + (h % SPIKE_SLOT as u32) as i32;
            let (top, bottom) = (self.cy - self.ry * 2, self.cy + self.ry * 2);
            // The ceiling: the first open row from the top; the floor: the last.
            let Some(ceil) = (top..bottom).find(|&y| self.inside(x, y)) else { continue };
            let floor = (top..bottom).rev().find(|&y| self.inside(x, y)).unwrap_or(ceil);
            let room = floor - ceil;
            if room < 12 {
                continue;
            }
            let len = (4 + ((h >> 8) % 12) as i32).min(room / 3);
            let half_w = 1 + ((h >> 16) % 3) as i32;
            if h >> 24 & 1 == 0 {
                out.push(Spike { x, base: ceil, half_w, len, down: true });
            }
            if h >> 25 & 1 == 0 && floor < self.lake {
                out.push(Spike { x, base: floor, half_w, len: len * 2 / 3, down: false });
            }
        }
        out
    }
}

#[derive(Debug, Clone, Copy)]
struct Ball {
    cx: i32,
    cy: i32,
    r: i32,
    h: u32,
}

#[derive(Debug, Clone, Copy)]
struct Chamber {
    cx: i32,
    cy: i32,
    rx: i32,
    ry: i32,
    level: i32,
    h: u32,
}

/// The features of one deep region.
#[derive(Debug, Clone, Copy, Default)]
struct Features {
    cavern: Option<Cavern>,
    geode: Option<Ball>,
    lava: Option<Chamber>,
    gold: Option<Ball>,
}

impl Fill<'_, '_> {
    /// Make the deep features that touch this chunk.
    #[inline(never)]
    pub(super) fn deep(&mut self) {
        let (rx, ry) = (self.x0.div_euclid(DEEP_REGION), self.y0.div_euclid(DEEP_REGION));
        let f = self.deep_features(rx, ry);
        let (gx0, gy0) = (self.xl, self.y0);
        let (gx1, gy1) = (self.xl + GW as i32, self.y0 + GH as i32);
        let overlaps = |b: (i32, i32, i32, i32)| b.0 < gx1 && b.2 > gx0 && b.1 < gy1 && b.3 > gy0;
        if let Some(c) = f.cavern
            && overlaps(c.bounds())
        {
            self.cavern(&c);
        }
        if let Some(g) = f.gold
            && overlaps((g.cx - g.r, g.cy - g.r, g.cx + g.r + 1, g.cy + g.r + 1))
        {
            self.gold(&g);
        }
        if let Some(g) = f.geode
            && overlaps((g.cx - g.r, g.cy - g.r, g.cx + g.r + 1, g.cy + g.r + 1))
        {
            self.geode(&g);
        }
        if let Some(l) = f.lava
            && overlaps((l.cx - l.rx - 6, l.cy - l.ry - 6, l.cx + l.rx + 7, l.cy + l.ry + 7))
        {
            self.lava(&l);
        }
    }

    /// The features of deep region (rx, ry).
    fn deep_features(&self, rx: i32, ry: i32) -> Features {
        let seed = self.seed(sd::DEEP);
        let base = (rx * DEEP_REGION, ry * DEEP_REGION);
        let mut f = Features::default();
        // A center for a feature of half-size (w, h), inside the region with the margin.
        let place = |h: u32, w: i32, hh: i32| {
            let free_x = (DEEP_REGION - 2 * (MARGIN + w)).max(1) as u32;
            let free_y = (DEEP_REGION - 2 * (MARGIN + hh)).max(1) as u32;
            (base.0 + MARGIN + w + (h % free_x) as i32, base.1 + MARGIN + hh + ((h >> 11) % free_y) as i32)
        };
        let depth = |y: i32| y - self.surface_y;
        let bottom = self.bottom_y - 160;

        // The cavern.
        let h = hash2(seed ^ 0xCA7E_0001, rx, ry);
        let start = (START_CAVE.0.div_euclid(DEEP_REGION), (self.surface_y + START_CAVE.1).div_euclid(DEEP_REGION));
        let cavern = if (rx, ry) == start {
            let (cx, d, crx, cry) = START_CAVE;
            Some(Cavern { cx, cy: self.surface_y + d, rx: crx, ry: cry, lake: i32::MAX, lode: self.m.malachite, side: -1, h })
        } else if h % 100 < 70 {
            let (crx, cry) = (50 + (hash1(h, 1) % 81) as i32, 20 + (hash1(h, 2) % 26) as i32);
            let (cx, cy) = place(hash1(h, 3), crx * 13 / 10 + 22, cry * 13 / 10 + 18);
            let lake = if hash1(h, 4) % 100 < 35 { cy + cry * 7 / 20 } else { i32::MAX };
            let ores = if depth(cy) < 1800 {
                [self.m.malachite, self.m.cassiterite, self.m.hematite, self.m.magnetite, self.m.coal]
            } else {
                [self.m.chalcopyrite, self.m.native_gold, self.m.magnetite, self.m.hematite, self.m.native_gold]
            };
            let lode = if hash1(h, 5) % 100 < 75 { ores[(hash1(h, 6) % 5) as usize] } else { 0 };
            let side = if hash1(h, 7).is_multiple_of(2) { -1 } else { 1 };
            Some(Cavern { cx, cy, rx: crx, ry: cry, lake, lode, side, h }).filter(|c| (250..5200).contains(&depth(c.cy)) && c.cy + c.ry * 2 < bottom)
        } else {
            None
        };
        f.cavern = cavern;

        // The geode.
        let h = hash2(seed ^ 0x6E0D_0002, rx, ry);
        if h % 100 < 45 {
            let r = 9 + (hash1(h, 1) % 12) as i32;
            let (cx, cy) = place(hash1(h, 2), r, r);
            if (1200..6500).contains(&depth(cy)) && cy + r < bottom {
                f.geode = Some(Ball { cx, cy, r, h });
            }
        }
        // The lava chamber.
        let h = hash2(seed ^ 0x1A7A_0003, rx, ry);
        if h % 100 < 40 {
            let (lrx, lry) = (30 + (hash1(h, 1) % 36) as i32, 12 + (hash1(h, 2) % 15) as i32);
            let (cx, cy) = place(hash1(h, 3), lrx + 6, lry + 6);
            if depth(cy) >= 3000 && cy + lry + 6 < bottom {
                f.lava = Some(Chamber { cx, cy, rx: lrx, ry: lry, level: cy - lry / 5, h });
            }
        }
        // The gold deposit.
        let h = hash2(seed ^ 0x601D_0004, rx, ry);
        if h % 100 < 60 {
            let r = 6 + (hash1(h, 1) % 8) as i32;
            let (cx, cy) = place(hash1(h, 2), r, r);
            if depth(cy) >= 1700 && cy + r < bottom {
                f.gold = Some(Ball { cx, cy, r, h });
            }
        }
        // Features of one region must not cut into each other: drop the smaller ones that would.
        if let (Some(c), Some(g)) = (f.cavern, f.geode) {
            let b = c.bounds();
            if g.cx + g.r > b.0 && g.cx - g.r < b.2 && g.cy + g.r > b.1 && g.cy - g.r < b.3 {
                f.geode = None;
            }
        }
        if let (Some(c), Some(l)) = (f.cavern, f.lava) {
            let b = c.bounds();
            if l.cx + l.rx + 6 > b.0 && l.cx - l.rx - 6 < b.2 && l.cy + l.ry + 6 > b.1 && l.cy - l.ry - 6 < b.3 {
                f.lava = None;
            }
        }
        f
    }

    /// Set a cell of the grid, if (x, y) is in the grid.
    #[inline(always)]
    fn put(&mut self, x: i32, y: i32, v: u16) {
        let (c, r) = (x - self.xl, y - self.y0);
        if (0..GW as i32).contains(&c) && (0..GH as i32).contains(&r) {
            self.grid[r as usize * GW + c as usize] = v;
        }
    }

    #[inline(always)]
    fn at(&self, x: i32, y: i32) -> Option<u16> {
        let (c, r) = (x - self.xl, y - self.y0);
        ((0..GW as i32).contains(&c) && (0..GH as i32).contains(&r)).then(|| self.grid[r as usize * GW + c as usize])
    }

    /// The rows and columns of the grid in a box (end exclusive).
    fn grid_box(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> (i32, i32, i32, i32) {
        (x0.max(self.xl), y0.max(self.y0), x1.min(self.xl + GW as i32), y1.min(self.y0 + GH as i32))
    }

    fn cavern(&mut self, cv: &Cavern) {
        let (bx0, by0, bx1, by1) = cv.bounds();
        let (x0, y0, x1, y1) = self.grid_box(bx0, by0, bx1, by1);
        let m = self.m;
        let moss_seed = cv.h ^ 0x0055_0001;
        let spikes = cv.spikes(x0, x1);
        let open = |x: i32, y: i32| cv.inside(x, y) && !spikes.iter().any(|s| s.has(x, y));
        for y in y0..y1 {
            for x in x0..x1 {
                if cv.inside(x, y) {
                    let v = if spikes.iter().any(|s| s.has(x, y)) {
                        m.stone
                    } else if y >= cv.lake {
                        m.water
                    } else {
                        // Vines of glow moss hang from the ceiling.
                        let len = 1 + (hash2(moss_seed, x, 9) % 7) as i32;
                        let vine = hash2(moss_seed, x, 7).is_multiple_of(3) && noise1(moss_seed, x, 40) > -0.4 && (1..=len).any(|k| !open(x, y - k));
                        if vine { m.glow_moss } else { 0 }
                    };
                    self.put(x, y, v);
                    continue;
                }
                let cell = self.at(x, y).unwrap_or(0);
                if cell == 0 || cell == m.water {
                    continue;
                }
                // The rich ore deposit in a side wall.
                let lode = cv.lode != 0 && {
                    let (lx, ly, lrx, lry) = cv.lode();
                    let (dx, dy) = ((x - lx) as f32 / lrx as f32, (y - ly) as f32 / lry as f32);
                    dx * dx + dy * dy < 1.0 && hash2(cv.h ^ 0x10DE, x, y) % 10 < 7
                };
                if lode {
                    self.put(x, y, cv.lode);
                    continue;
                }
                // Glow moss on the floor, in patches, one or two cells thick.
                let floor = open(x, y - 1) && y - 1 < cv.lake;
                let floor2 = open(x, y - 2) && y - 2 < cv.lake && !open(x, y - 1);
                let patch = noise1(moss_seed ^ 0x77, x, 30);
                if (floor && patch > -0.35) || (floor2 && patch > 0.15) {
                    self.put(x, y, m.glow_moss);
                }
            }
        }
        // Moss on the floor cells of the stalagmites too (they are inside the cavern).
        for s in spikes.iter().filter(|s| !s.down) {
            let (x, y) = (s.x, s.base - s.len);
            if hash1(moss_seed, x).is_multiple_of(2) && (x0..x1).contains(&x) && (y0..y1).contains(&y) && self.at(x, y) == Some(m.stone) {
                self.put(x, y, m.glow_moss);
            }
        }
    }

    fn geode(&mut self, g: &Ball) {
        let (x0, y0, x1, y1) = self.grid_box(g.cx - g.r, g.cy - g.r, g.cx + g.r + 1, g.cy + g.r + 1);
        let m = self.m;
        for y in y0..y1 {
            for x in x0..x1 {
                let (dx, dy) = ((x - g.cx) as f32, (y - g.cy) as f32);
                let d = (dx * dx + dy * dy).sqrt() + 1.5 * noise1(g.h, x * 5 + y, 5);
                if d >= g.r as f32 {
                    continue;
                }
                let v = if d < (g.r - 4) as f32 {
                    // Crystal points grow into the hollow from the shell.
                    if d > (g.r - 7) as f32 && hash2(g.h, x, y).is_multiple_of(3) { m.glow_crystal } else { 0 }
                } else if hash2(g.h ^ 0xB45A, x, y) % 10 < 8 {
                    m.glow_crystal
                } else {
                    m.basalt
                };
                self.put(x, y, v);
            }
        }
    }

    fn lava(&mut self, l: &Chamber) {
        let s = 6;
        let (x0, y0, x1, y1) = self.grid_box(l.cx - l.rx - s, l.cy - l.ry - s, l.cx + l.rx + s + 1, l.cy + l.ry + s + 1);
        let m = self.m;
        for y in y0..y1 {
            for x in x0..x1 {
                let wob = 0.15 * noise1(l.h, x + y * 3, 11);
                let (dx, dy) = ((x - l.cx) as f32, (y - l.cy) as f32);
                let outer = (dx / (l.rx + s) as f32).powi(2) + (dy / (l.ry + s) as f32).powi(2);
                if outer > 1.0 + wob {
                    continue;
                }
                let inner = (dx / l.rx as f32).powi(2) + (dy / l.ry as f32).powi(2);
                let v = if inner > 1.0 + wob {
                    m.obsidian
                } else if y >= l.level {
                    m.lava
                } else {
                    0
                };
                self.put(x, y, v);
            }
        }
    }

    fn gold(&mut self, g: &Ball) {
        let (x0, y0, x1, y1) = self.grid_box(g.cx - g.r, g.cy - g.r, g.cx + g.r + 1, g.cy + g.r + 1);
        let m = self.m;
        for y in y0..y1 {
            for x in x0..x1 {
                let (dx, dy) = ((x - g.cx) as f32, (y - g.cy) as f32);
                if dx * dx + dy * dy < (g.r * g.r) as f32 && hash2(g.h, x, y) % 100 < 45 && self.at(x, y).is_some_and(|v| v != 0 && v != m.water) {
                    self.put(x, y, m.native_gold);
                }
            }
        }
    }
}
