//! The surface: biomes along x, the height of the ground in each column, and the features that
//! change it (lakes, rivers, oases, salt flats, coal outcrops, the start area).
//!
//! Everything here is a function of the column x and the world seed. A feature belongs to a
//! region of `REGION` cells and stays inside it, so a column only needs the feature of its own
//! region. In the start zone (|x| < `START_ZONE`) a fixed list of features replaces the region
//! features.

use crate::noise::{hash1, lerp, noise1, signed, smoothstep, sub_seed};
use crate::{Mats, WorldGen};
use std::cell::{Cell, OnceCell};

/// Width of a feature region in cells. A region has at most one surface feature.
pub const REGION: i32 = 512;
/// The start zone: x = -START_ZONE to START_ZONE. It has the fixed start features.
pub const START_ZONE: i32 = 1536;
/// Half-width of the flat place for the Hub at x = 0.
pub const HUB_FLAT: i32 = 110;
/// Width of the slope from the flat place to the natural ground.
const FLAT_BLEND: i32 = 160;
/// Average width of a biome zone in cells.
const ZONE: f64 = 3200.0;
/// Half-width of the change of the ground shape from one biome to the next.
const BLEND: f64 = 160.0;
/// The ground is never higher than this many cells above the surface level.
pub const MAX_RISE: i32 = 250;
/// The ground is never lower than this many cells below the surface level.
pub const MAX_DROP: i32 = 220;
/// The natural ground is computed every `NAT_STEP` columns and joined by straight lines.
const NAT_STEP: i32 = 4;
/// Entries of the cache of natural ground samples (covers 256 columns).
const NAT_CACHE: usize = 64;

/// Indices of the seeds in `Seeds`. Each noise and each random choice has its own seed.
pub(crate) mod sd {
    pub const BASE_A: usize = 0;
    pub const BASE_B: usize = 1;
    pub const TEMP: usize = 2; // 4 seeds
    pub const TUNDRA: usize = 6; // 4 seeds
    pub const DESERT: usize = 10;
    pub const DUNE_AMP: usize = 11;
    pub const DUNE_WARP: usize = 12;
    pub const TERRACE_A: usize = 13;
    pub const TERRACE_B: usize = 14;
    pub const ZONES: usize = 15;
    pub const ZONE_BIOME: usize = 16;
    pub const FEATURES: usize = 17;
    pub const BIOME_WARP: usize = 18;
    pub const SOIL: usize = 19;
    pub const GRASS: usize = 20;
    pub const SNOW: usize = 21;
    pub const TREES: usize = 22;
    pub const FOREST: usize = 23;
    pub const START: usize = 24;
    pub const LAYER_WARP: usize = 25;
    pub const LIME_WARP: usize = 26;
    pub const LIME: usize = 27;
    pub const COAL_WARP: usize = 28;
    pub const COAL: usize = 29;
    pub const COAL_BREAK: usize = 30;
    pub const PROV_X: usize = 31;
    pub const PROV_Y: usize = 32;
    pub const TUNNEL: usize = 33;
    pub const CAVERN: usize = 34;
    pub const VEIN: usize = 35;
    pub const POCKET: usize = 36;
    pub const POCKETS: usize = 38;
    pub const BEDROCK: usize = 39;
    pub const BED: usize = 40;
    pub const SED: usize = 41;
    pub const SED_WARP: usize = 42;
    pub const DECOR: usize = 37;
    pub const COUNT: usize = 43;
}

/// The seeds of one world, made from the world seed.
#[derive(Clone, Copy)]
pub(crate) struct Seeds([u32; sd::COUNT]);

impl Seeds {
    pub fn new(world: u64) -> Self {
        let mut s = [0; sd::COUNT];
        for (i, v) in s.iter_mut().enumerate() {
            *v = sub_seed(world, 0x5eed_0000 + i as u64);
        }
        Seeds(s)
    }

    #[inline(always)]
    pub fn get(&self, i: usize) -> u32 {
        self.0[i]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Biome {
    #[default]
    Temperate,
    Desert,
    Tundra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FeatureKind {
    Lake,
    FrozenLake,
    River,
    Oasis,
    SaltFlat,
    /// A coal seam that comes up to the surface. `hw` is its reach; `depth` its thickness.
    CoalOutcrop,
}

/// A surface feature. Positions are world cells.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Feature {
    pub kind: FeatureKind,
    pub cx: i32,
    /// Half-width of the basin or flat.
    pub hw: i32,
    /// Depth of the basin below the water level (coal outcrop: thickness of the seam).
    pub depth: i32,
    /// Width of the slope from the feature to the natural ground.
    pub margin: i32,
    /// The ground level of the rim (a basin) or of the flat (salt flat), or the ground at `cx`
    /// (coal outcrop).
    pub flat: i32,
    /// Rows of ice on a frozen lake. Coal outcrop: 1 if the seam goes down to the right, -1 to
    /// the left.
    pub extra: i32,
}

impl Feature {
    /// The cells left and right of `cx` that the feature changes.
    pub fn reach(&self) -> i32 {
        self.hw + self.margin
    }

    fn is_basin(&self) -> bool {
        matches!(self.kind, FeatureKind::Lake | FeatureKind::FrozenLake | FeatureKind::River | FeatureKind::Oasis)
    }

    /// The top row of the coal seam of a coal outcrop in column `x`.
    pub fn seam_top(&self, x: i32) -> i32 {
        self.flat - 2 + ((x - self.cx) * self.extra * 5) / 12
    }
}

/// A blob of one material under the ground (start area only).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Blob {
    pub cx: i32,
    pub cy: i32,
    pub rx: i32,
    pub ry: i32,
    pub mat: u16,
}

impl Blob {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        let dx = (x - self.cx) as f32 / self.rx as f32;
        let dy = (y - self.cy) as f32 / self.ry as f32;
        dx * dx + dy * dy <= 1.0
    }
}

/// The fixed features of the start zone.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Start {
    pub lake: Feature,
    pub river: Feature,
    pub outcrop: Feature,
    /// Two malachite blobs, a clay blob and a sand patch.
    pub blobs: [Blob; 4],
}

/// The ground of one column, with the features.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Ground {
    /// The first row of ground.
    pub g: i32,
    /// The first row of water (the column has water in rows `water..g`). `water >= g`: no water.
    pub water: i32,
    /// Rows of ice at the top of the water.
    pub ice: i32,
    /// Rows of gravel with tin at the bottom of a lake or river.
    pub bed: i32,
    /// Chance of cassiterite in a bed cell, out of 256.
    pub tin: u32,
    /// Rows of clay under the bed.
    pub clay: i32,
    /// Rows of sand (or gravel at a frozen lake) at the top, at the shore.
    pub beach: i32,
    pub beach_gravel: bool,
    /// Rows of salt at the top (salt flat).
    pub salt: i32,
    /// Height of the sand dunes here (desert).
    pub dune: f32,
}

/// Everything that fills the cells of one column.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Column {
    pub gr: Ground,
    pub biome: Biome,
    /// Rows of grass at the top (temperate).
    pub grass: i32,
    /// Rows of snow at the top (tundra).
    pub snow: i32,
    /// Depth of all soil layers. Rock starts at `g + soil`.
    pub soil: i32,
    /// A steep slope: little soil, no grass or snow.
    pub steep: bool,
    /// A column that is not tundra: its distance to the nearest tundra zone, from 0 (at the
    /// border) to 1 (`BLEND` cells or more). Near the border its ground is as cold as the tundra
    /// ground, so the snow at the border does not melt (see `chunk::Fill::write`). 1 in the tundra.
    pub warm: f32,
}

impl Column {
    /// A column deep under the surface: rock everywhere.
    pub fn deep() -> Self {
        Column { gr: Ground { g: i32::MIN / 2, water: i32::MIN / 2, ..Ground::default() }, warm: 1.0, ..Column::default() }
    }
}

/// Seeds and cached features for the columns of one chunk (or one call of the public API).
pub(crate) struct Ctx<'a> {
    pub wg: &'a WorldGen,
    pub sd: Seeds,
    surface_y: i32,
    hub: OnceCell<f64>,
    start: OnceCell<Start>,
    /// The features of up to two regions: (region, feature).
    cache: [(i32, OnceCell<Option<Feature>>); 2],
    /// Natural ground samples: (x, ground row, dune height). x = i32::MAX: empty.
    nat: [Cell<(i32, f64, f64)>; NAT_CACHE],
    /// The last zone found: (zone, left border, right border).
    last_zone: Cell<(i32, f64, f64)>,
}

impl<'a> Ctx<'a> {
    /// A context for the columns `x_lo..=x_hi` (other columns work too, but slower).
    pub fn new(wg: &'a WorldGen, seed: u64, x_lo: i32, x_hi: i32) -> Self {
        let r0 = x_lo.div_euclid(REGION);
        let r1 = x_hi.div_euclid(REGION).max(r0 + 1);
        Ctx {
            wg,
            sd: Seeds::new(seed),
            surface_y: wg.world_settings().surface_y,
            hub: OnceCell::new(),
            start: OnceCell::new(),
            cache: [(r0, OnceCell::new()), (r1, OnceCell::new())],
            nat: std::array::from_fn(|_| Cell::new((i32::MAX, 0.0, 0.0))),
            last_zone: Cell::new((0, f64::NAN, f64::NAN)),
        }
    }

    pub fn m(&self) -> &Mats {
        &self.wg.m
    }

    #[inline]
    fn n(&self, seed: usize, x: i32, w: i32) -> f64 {
        noise1(self.sd.get(seed), x, w) as f64
    }

    // ---- Biome zones ----

    /// The x of the border between zone `i - 1` and zone `i`.
    fn boundary(&self, i: i32) -> f64 {
        let j = signed(hash1(self.sd.get(sd::ZONES), i)) as f64;
        match i {
            0 => -1750.0 + 100.0 * j,
            1 => 1750.0 + 100.0 * j,
            _ => (i as f64 - 0.5) * ZONE + 600.0 * j,
        }
    }

    /// The zone that holds column `x`, with the x of its left and right borders.
    fn zone(&self, x: i32) -> (i32, f64, f64) {
        let xf = x as f64;
        let last = self.last_zone.get();
        if xf >= last.1 && xf < last.2 {
            return last;
        }
        let mut i = (xf / ZONE + 0.5).floor() as i32;
        while xf < self.boundary(i) {
            i -= 1;
        }
        while xf >= self.boundary(i + 1) {
            i += 1;
        }
        let z = (i, self.boundary(i), self.boundary(i + 1));
        self.last_zone.set(z);
        z
    }

    /// The biome of zone `i`. Zone 0 (the start) is temperate, zone 1 a desert, zone -1 a tundra.
    fn zone_biome(&self, i: i32) -> Biome {
        match i {
            0 => Biome::Temperate,
            1 => Biome::Desert,
            -1 => Biome::Tundra,
            _ => match hash1(self.sd.get(sd::ZONE_BIOME), i) % 10 {
                0..4 => Biome::Temperate,
                4..7 => Biome::Desert,
                _ => Biome::Tundra,
            },
        }
    }

    /// The biome of the ground and soil in column `x`. The border between two zones is not a
    /// straight line: it moves a little with noise.
    pub fn biome_at(&self, x: i32) -> Biome {
        self.zone_biome(self.zone(self.warped(x)).0)
    }

    /// Column `x` moved by the noise that moves the zone borders (see `biome_at`).
    fn warped(&self, x: i32) -> i32 {
        x + (40.0 * self.n(sd::BIOME_WARP, x, 90)) as i32
    }

    /// See `Column::warm`.
    fn warm_at(&self, x: i32) -> f32 {
        let xw = self.warped(x);
        let (i, l, r) = self.zone(xw);
        let tundra = |i: i32| self.zone_biome(i) == Biome::Tundra;
        if tundra(i) {
            return 1.0;
        }
        let xf = xw as f64;
        let mut d = f64::MAX;
        if tundra(i - 1) {
            d = d.min(xf - l);
        }
        if tundra(i + 1) {
            d = d.min(r - xf);
        }
        (d / BLEND).clamp(0.0, 1.0) as f32
    }

    /// The biome of column `x`, the biome of the nearest other zone, and the weight of the other
    /// one for the ground shape (0 to 0.5; 0.5 at the border).
    fn biome_mix(&self, x: i32) -> (Biome, Biome, f64) {
        let (i, l, r) = self.zone(x);
        let own = self.zone_biome(i);
        let xf = x as f64;
        let (dl, dr) = (xf - l, r - xf);
        if dl < BLEND && dl <= dr {
            (own, self.zone_biome(i - 1), 0.5 * (1.0 - smoothstep((dl / BLEND) as f32) as f64))
        } else if dr < BLEND {
            (own, self.zone_biome(i + 1), 0.5 * (1.0 - smoothstep((dr / BLEND) as f32) as f64))
        } else {
            (own, own, 0.0)
        }
    }

    // ---- Ground height ----

    /// Steps with steep sides (cliffs), `h` cells high. The steps come from a slow noise: where
    /// it crosses a step level, the ground rises or falls quickly.
    fn terrace(&self, x: i32, h: f64) -> f64 {
        if h <= 0.0 {
            return 0.0;
        }
        let m = self.n(sd::TERRACE_A, x, 780) + 0.35 * self.n(sd::TERRACE_B, x, 260);
        let s = (m + 1.6) * 1.5;
        let f = s.floor();
        const EDGE: f64 = 0.07;
        let step = f + smoothstep(((s - f - (1.0 - EDGE)) / EDGE) as f32) as f64;
        -(step - 2.4) * h
    }

    /// Height of the sand dunes (cells up). Each dune rises slowly on the left and falls more
    /// steeply on the right, but never steeper than sand can stand.
    fn dunes(&self, x: i32) -> f64 {
        let amp = 30.0 + 22.0 * self.n(sd::DUNE_AMP, x, 1300);
        let xw = x + (50.0 * self.n(sd::DUNE_WARP, x, 700)) as i32;
        let profile = |t: f64, peak: f64| {
            if t < peak {
                smoothstep((t / peak) as f32) as f64
            } else {
                1.0 - smoothstep(((t - peak) / (1.0 - peak)) as f32) as f64
            }
        };
        let big = profile(xw.rem_euclid(360) as f64 / 360.0, 0.68);
        let small = profile((xw + 97).rem_euclid(150) as f64 / 150.0, 0.65);
        amp.max(0.0) * (big + 0.25 * small)
    }

    /// Hills are lower near the start.
    fn calm(x: i32) -> f64 {
        0.7 + 0.3 * smoothstep((x.abs() - 900) as f32 / 600.0) as f64
    }

    /// The ground shape of one biome: (offset from the base level, dune height).
    fn biome_height(&self, b: Biome, x: i32, calm: f64) -> (f64, f64) {
        match b {
            Biome::Temperate => {
                let hills = 36.0 * self.n(sd::TEMP, x, 520)
                    + 13.0 * self.n(sd::TEMP + 1, x, 190)
                    + 4.0 * self.n(sd::TEMP + 2, x, 64)
                    + 1.3 * self.n(sd::TEMP + 3, x, 21);
                (-hills * calm + self.terrace(x, 22.0 * calm * calm), 0.0)
            }
            Biome::Tundra => {
                let hills = 44.0 * self.n(sd::TUNDRA, x, 610)
                    + 16.0 * self.n(sd::TUNDRA + 1, x, 220)
                    + 6.0 * self.n(sd::TUNDRA + 2, x, 70)
                    + 2.0 * self.n(sd::TUNDRA + 3, x, 19);
                (-hills + self.terrace(x, 34.0), 0.0)
            }
            Biome::Desert => {
                let d = self.dunes(x);
                (-10.0 * self.n(sd::DESERT, x, 700) - d, d)
            }
        }
    }

    /// The ground row of column `x` without features, as a float, and the dune height. Made from
    /// samples every `NAT_STEP` columns (see `natural_exact`).
    fn natural(&self, x: i32) -> (f64, f64) {
        let x4 = x.div_euclid(NAT_STEP) * NAT_STEP;
        let a = self.natural_sample(x4);
        if x == x4 {
            return a;
        }
        let b = self.natural_sample(x4 + NAT_STEP);
        let t = (x - x4) as f64 / NAT_STEP as f64;
        (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
    }

    fn natural_sample(&self, x: i32) -> (f64, f64) {
        let e = &self.nat[(x.div_euclid(NAT_STEP) as usize) % NAT_CACHE];
        let (cx, h, d) = e.get();
        if cx == x {
            return (h, d);
        }
        let (h, d) = self.natural_exact(x);
        e.set((x, h, d));
        (h, d)
    }

    /// The ground row of column `x` without features, and the dune height.
    fn natural_exact(&self, x: i32) -> (f64, f64) {
        let base = self.surface_y as f64 + 75.0 * self.n(sd::BASE_A, x, 2900) + 32.0 * self.n(sd::BASE_B, x, 1150);
        let (a, b, w) = self.biome_mix(x);
        let calm = Self::calm(x);
        let (ha, da) = self.biome_height(a, x, calm);
        let (hb, db) = if w > 0.0 && b != a { self.biome_height(b, x, calm) } else { (ha, da) };
        (base + lerp(ha as f32, hb as f32, w as f32) as f64, lerp(da as f32, db as f32, w as f32) as f64)
    }

    /// The row of the flat place for the Hub: the natural ground at x = 0, on a tile line.
    fn hub_level(&self) -> f64 {
        *self.hub.get_or_init(|| {
            let h = self.natural(0).0;
            (h / 8.0).round() * 8.0
        })
    }

    // ---- Features ----

    /// A basin feature with its rim level from the natural ground.
    fn basin(&self, kind: FeatureKind, cx: i32, hw: i32, depth: i32, margin: i32, extra: i32) -> Feature {
        let avg = (self.natural(cx - hw).0 + self.natural(cx).0 + self.natural(cx + hw).0) / 3.0;
        Feature { kind, cx, hw, depth, margin, flat: avg.round() as i32, extra }
    }

    /// The fixed features of the start zone. The seed moves them a little.
    pub fn start(&self) -> &Start {
        self.start.get_or_init(|| {
            let s = self.sd.get(sd::START);
            let jit = |i: i32, r: f32| (signed(hash1(s, i)) * r) as i32;
            let lake = self.basin(FeatureKind::Lake, -680 + jit(0, 40.0), 120, 34, 56, 0);
            let river = self.basin(FeatureKind::River, 560 + jit(1, 30.0), 44, 16, 34, 0);
            let ocx = 930 + jit(2, 40.0);
            let outcrop = Feature {
                kind: FeatureKind::CoalOutcrop,
                cx: ocx,
                hw: 170,
                depth: 9,
                margin: 0,
                flat: self.natural(ocx).0.round() as i32,
                extra: 1,
            };
            let m = self.m();
            // The blobs are not under the lake or the river, so the ground there has no feature.
            let blob = |cx: i32, depth: i32, rx: i32, ry: i32, mat: u16| {
                let g = self.base_height(cx).0.round() as i32;
                Blob { cx, cy: g + depth, rx, ry, mat }
            };
            let blobs = [
                blob(-300 + jit(3, 20.0), 11, 17, 8, m.malachite),
                blob(330 + jit(4, 20.0), 12, 13, 7, m.malachite),
                blob(62, 8, 22, 5, m.clay),
                // Sand at the surface left of the Hub, for the first goal of the guide.
                blob(-78 + jit(5, 6.0), 2, 26, 7, m.sand),
            ];
            Start { lake, river, outcrop, blobs }
        })
    }

    /// The feature of region `r` (none in the start zone and near the border of a biome zone).
    fn make_region_feature(&self, r: i32) -> Option<Feature> {
        let x0 = r * REGION;
        if x0 + REGION > -START_ZONE && x0 < START_ZONE {
            return None;
        }
        let mid = x0 + REGION / 2;
        let (i, l, rr) = self.zone(mid);
        if (mid as f64 - l).min(rr - mid as f64) < 450.0 {
            return None;
        }
        let h = hash1(self.sd.get(sd::FEATURES), r);
        let h2 = hash1(self.sd.get(sd::FEATURES) ^ 0x5BD1_E995, r);
        let roll = h % 100;
        use FeatureKind::*;
        let kind = match self.zone_biome(i) {
            Biome::Temperate if roll < 28 => Lake,
            Biome::Temperate if roll < 50 => River,
            Biome::Temperate if roll < 62 => CoalOutcrop,
            Biome::Tundra if roll < 30 => FrozenLake,
            Biome::Tundra if roll < 40 => CoalOutcrop,
            Biome::Desert if roll < 35 => SaltFlat,
            Biome::Desert if roll < 47 => Oasis,
            _ => return None,
        };
        let pick = |lo: u32, hi: u32, shift: u32| (lo + (h2 >> shift) % (hi - lo + 1)) as i32;
        let (hw, depth, margin) = match kind {
            Lake | FrozenLake => (pick(70, 140, 0), pick(22, 40, 8), 50),
            River => (pick(30, 50, 0), pick(12, 18, 8), 30),
            Oasis => (pick(40, 70, 0), pick(12, 20, 8), 40),
            SaltFlat => (pick(90, 150, 0), 0, 90),
            CoalOutcrop => (170, pick(6, 11, 8), 0),
        };
        let room = (REGION / 2 - hw - margin - 8).max(0);
        let cx = mid + (signed(hash1(h2, 7)) * room as f32) as i32;
        Some(match kind {
            SaltFlat => {
                // A low, flat basin: a little below the lowest natural ground across it.
                let low = (-2..=2).map(|i| self.natural(cx + i * hw / 2).0).fold(f64::MIN, f64::max);
                Feature { kind, cx, hw, depth, margin, flat: low.round() as i32 + 4, extra: 0 }
            }
            CoalOutcrop => {
                let dir = if h & (1 << 20) != 0 { 1 } else { -1 };
                Feature { kind, cx, hw, depth, margin, flat: self.natural(cx).0.round() as i32, extra: dir }
            }
            // 11 to 13 rows of ice: from -12 °C at the top to -2 °C at the water, 1 °C per row, so
            // no heat flows through the ice and the water under it stays liquid (see
            // `chunk::Fill::write`).
            FrozenLake => self.basin(kind, cx, hw, depth, margin, 11 + (h2 >> 16) as i32 % 3),
            _ => self.basin(kind, cx, hw, depth, margin, 0),
        })
    }

    fn region_feature(&self, r: i32) -> Option<Feature> {
        for (cr, cell) in &self.cache {
            if *cr == r {
                return *cell.get_or_init(|| self.make_region_feature(r));
            }
        }
        self.make_region_feature(r)
    }

    /// The feature that changes the ground of column `x`, if any.
    fn feature_at(&self, x: i32) -> Option<Feature> {
        if x.abs() < START_ZONE {
            let s = self.start();
            return [s.lake, s.river].into_iter().find(|f| (x - f.cx).abs() < f.reach());
        }
        self.region_feature(x.div_euclid(REGION)).filter(|f| (x - f.cx).abs() < f.reach())
    }

    /// The coal outcrop feature of the region of `x` (or of the start zone), if any.
    pub fn outcrop_near(&self, x: i32) -> Option<Feature> {
        let f = if x.abs() < START_ZONE { Some(self.start().outcrop) } else { self.region_feature(x.div_euclid(REGION)) };
        f.filter(|f| f.kind == FeatureKind::CoalOutcrop)
    }

    /// The natural ground with the flat place for the Hub: (ground row as a float, dune height).
    fn base_height(&self, x: i32) -> (f64, f64) {
        let (h, dune) = self.natural(x);
        let d = x.abs();
        if d >= HUB_FLAT + FLAT_BLEND {
            return (h, dune);
        }
        let w = smoothstep((HUB_FLAT + FLAT_BLEND - d) as f32 / FLAT_BLEND as f32) as f64;
        (h + (self.hub_level() - h) * w, dune)
    }

    /// The ground of column `x`: the natural height, the flat place for the Hub, and the feature.
    pub fn ground_at(&self, x: i32) -> Ground {
        let (mut h, dune) = self.base_height(x);
        let mut gr = Ground { dune: dune as f32, ..Ground::default() };
        let mut water = None;
        if let Some(f) = self.feature_at(x) {
            let d = (x - f.cx).abs();
            let flat = f.flat as f64;
            if d >= f.hw {
                // The slope from the rim to the natural ground.
                let w = smoothstep((f.reach() - d) as f32 / f.margin.max(1) as f32) as f64;
                h = lerp(h as f32, flat as f32, w as f32) as f64;
                if f.is_basin() && d < f.hw + f.margin * 2 / 3 {
                    gr.beach = if d < f.hw + f.margin / 3 { 7 } else { 4 };
                    gr.beach_gravel = f.kind == FeatureKind::FrozenLake;
                }
            } else if f.is_basin() {
                let u = d as f64 / f.hw as f64;
                let shape = 0.5 * (1.0 + (std::f64::consts::PI * u).cos());
                h = flat + (f.depth + 3) as f64 * shape;
                let level = f.flat + 3;
                water = Some(level);
                let deep = (1.0 - u) as f32;
                gr.bed = 2 + (4.0 * deep).round() as i32 + if f.kind == FeatureKind::River { 2 } else { 0 };
                let most = match f.kind {
                    FeatureKind::River => 150.0,
                    FeatureKind::Lake => 110.0,
                    _ => 40.0,
                };
                gr.tin = (most * deep.powf(1.5)) as u32 + 8;
                gr.clay = 5;
                gr.beach_gravel = f.kind == FeatureKind::FrozenLake;
                if f.kind == FeatureKind::FrozenLake {
                    gr.ice = f.extra;
                }
            } else if f.kind == FeatureKind::SaltFlat {
                h = flat;
                gr.salt = 1 + (3.0 * smoothstep((f.hw - d) as f32 / 24.0)).round() as i32;
            }
        }
        let s = self.surface_y;
        gr.g = (h.round() as i32).clamp(s - MAX_RISE, s + MAX_DROP);
        gr.water = water.filter(|&w| w < gr.g).unwrap_or(gr.g);
        if gr.water >= gr.g {
            gr.ice = 0;
        }
        gr
    }

    /// The column at `x`. `left2` and `right2` are the ground rows at x - 2 and x + 2 (to find
    /// steep slopes).
    pub fn column(&self, x: i32, gr: Ground, left2: i32, right2: i32) -> Column {
        let biome = self.biome_at(x);
        let steep = (right2 - left2).abs() >= 7;
        let soil_noise = self.n(sd::SOIL, x, 170) as f32;
        let mut c = Column { gr, biome, steep, warm: self.warm_at(x), ..Column::default() };
        match biome {
            Biome::Temperate => {
                c.soil = 12 + (6.0 * (soil_noise + 1.0)) as i32;
                c.grass = 1 + hash1(self.sd.get(sd::GRASS), x).is_multiple_of(4) as i32;
            }
            Biome::Desert => {
                c.soil = 16 + gr.dune.round() as i32 + (4.0 * (soil_noise + 1.0)) as i32;
            }
            Biome::Tundra => {
                c.soil = 18 + (5.0 * (soil_noise + 1.0)) as i32;
                c.snow = 3 + (1.6 * (self.n(sd::SNOW, x, 90) as f32 + 1.0)) as i32;
            }
        }
        if steep {
            c.soil = c.soil.min(3);
            c.grass = 0;
            c.snow = 0;
        }
        if gr.bed > 0 {
            c.soil = c.soil.max(gr.bed + gr.clay + 6 + 3);
        }
        if gr.bed > 0 || gr.beach > 0 || gr.salt > 0 {
            c.grass = 0;
            c.snow = 0;
        }
        if gr.salt > 0 {
            c.soil = c.soil.max(gr.salt * 3 + 6);
        }
        c
    }

    /// Density of trees at `x` (0 to 1).
    pub fn tree_density(&self, x: i32, biome: Biome) -> f32 {
        match biome {
            Biome::Temperate => {
                let forest = (0.25 + 0.6 * self.n(sd::FOREST, x, 1100) as f32).clamp(0.05, 0.85);
                // Groups of trees on both sides of the Hub.
                if (160..480).contains(&x.abs()) { forest.max(0.8) } else { forest }
            }
            Biome::Tundra => (0.2 + 0.3 * self.n(sd::FOREST, x, 900) as f32).clamp(0.0, 0.45),
            Biome::Desert => 0.0,
        }
    }
}
