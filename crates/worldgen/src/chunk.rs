//! Fill the cells of one chunk.
//!
//! The work is done on a grid that is one cell larger than the chunk on the left, right and
//! bottom sides (`noise::GW` × `noise::GH`). Every cell of the grid comes from world-position
//! functions only, so the border cells are the same as the cells of the neighbor chunks. The
//! border is needed for the last step: a powder cell with air, gas or liquid below it or
//! diagonally below it becomes a solid, so nothing moves when the chunk wakes up.
//!
//! Steps: columns (ground, water, soil) → cells (soil, rock, caves, veins, bands, pockets) →
//! features (coal outcrops, start blobs, water and methane pockets) → trees → support → write.

use crate::noise::{Coarse, GH, GW, fbm2, hash1, hash2, noise1, smoothstep};
use crate::surface::{self, Biome, Column, Ctx, sd};
use crate::{Mats, OPEN, POWDER, UPPER_STONE_END, WorldGen, trees};
use foundry_core::{CHUNK_SIZE, local_index};
use foundry_sim::ChunkCells;

/// A chunk whose lowest row is this far above the surface level is all air.
const SKY_CLEAR: i32 = surface::MAX_RISE + trees::MAX_HEIGHT + 8;
/// The deepest soil below the ground (dunes, lake beds), in cells.
const MAX_SOIL: i32 = 150;
/// Size of the square regions that hold the water and methane pockets.
const POCKET_REGION: i32 = 512;
/// Rows of the limestone band pattern, and of the coal seam pattern.
const LIME_SPACING: i32 = 40;
const COAL_SPACING: i32 = 150;
/// Largest shift (up or down) of the limestone bands and of the coal seams.
const LIME_WARP: i32 = 59;
const COAL_WARP: i32 = 69;
const LIME_TABLE: usize = 8;
const COAL_TABLE: usize = 4;

/// Make the cells of one chunk. See the module documentation.
pub(crate) fn generate(wg: &WorldGen, cells: &mut ChunkCells) {
    let s = *wg.settings();
    let (x0, y0) = (cells.left(), cells.top());
    if y0 + CHUNK_SIZE <= s.surface_y - SKY_CLEAR {
        return;
    }
    let ctx = Ctx::new(wg, cells.seed, x0 - trees::REACH - 4, x0 + CHUNK_SIZE + trees::REACH + 4);
    let mut f = Fill::new(wg, &ctx, x0, y0);
    f.columns();
    f.cells();
    f.features();
    f.trees();
    f.support();
    f.write(cells);
}

/// Values of the noise fields in one grid row.
struct FieldRow {
    tunnel: [f32; GW],
    cavern: [f32; GW],
    vein: [f32; GW],
    pocket: [f32; GW],
}

/// Values that are the same for a whole row.
struct RowInfo {
    /// How much cave there is (0 to 1), before the change near the ground.
    cave: f32,
    /// Half-width of the ore veins in noise units (0: no veins).
    vein: f32,
    prov_y: f32,
    coal: bool,
    lime: bool,
}

struct Fill<'a, 'b> {
    wg: &'a WorldGen,
    ctx: &'b Ctx<'a>,
    m: Mats,
    x0: i32,
    y0: i32,
    /// World x of grid column 0.
    xl: i32,
    surface_y: i32,
    bottom_y: i32,
    near_surface: bool,
    cols: [Column; GW],
    grid: [u16; GW * GH],
    /// The highest row of ground or water in the grid columns, and the lowest row of ground.
    min_top: i32,
    max_ground: i32,
    /// Per column: shift of the layer depth, the limestone bands and the coal seams; the ore
    /// province along x; rows of bedrock at the bottom.
    layer_warp: [i32; GW],
    lime_warp: [i32; GW],
    coal_warp: [i32; GW],
    prov_x: [f32; GW],
    bedrock: [i32; GW],
    lime_base: i32,
    lime: [i32; LIME_TABLE],
    coal_base: i32,
    coal: [i32; COAL_TABLE],
}

impl<'a, 'b> Fill<'a, 'b> {
    fn new(wg: &'a WorldGen, ctx: &'b Ctx<'a>, x0: i32, y0: i32) -> Self {
        let s = wg.settings();
        Fill {
            wg,
            ctx,
            m: wg.m,
            x0,
            y0,
            xl: x0 - 1,
            surface_y: s.surface_y,
            bottom_y: s.bottom_y,
            near_surface: false,
            cols: [Column::deep(); GW],
            grid: [0; GW * GH],
            min_top: i32::MIN / 2,
            max_ground: i32::MIN / 2,
            layer_warp: [0; GW],
            lime_warp: [0; GW],
            coal_warp: [0; GW],
            prov_x: [0.0; GW],
            bedrock: [0; GW],
            lime_base: 0,
            lime: [0; LIME_TABLE],
            coal_base: 0,
            coal: [0; COAL_TABLE],
        }
    }

    #[inline(always)]
    fn seed(&self, which: usize) -> u32 {
        self.ctx.sd.get(which)
    }

    /// The ground, water and soil of each grid column, and the per-column values of the rock.
    fn columns(&mut self) {
        let ctx = self.ctx;
        self.near_surface = self.y0 < self.surface_y + surface::MAX_DROP + MAX_SOIL + 8;
        if self.near_surface {
            // Ground rows for x = xl - 2 to xl + GW + 1 (two more on each side, for the slope).
            let mut g = [0i32; GW + 4];
            let mut grounds = [surface::Ground::default(); GW];
            for (k, gk) in g.iter_mut().enumerate() {
                let x = self.xl - 2 + k as i32;
                let gr = ctx.ground_at(x);
                *gk = gr.g;
                if (2..GW + 2).contains(&k) {
                    grounds[k - 2] = gr;
                }
            }
            let (mut min_top, mut max_ground) = (i32::MAX, i32::MIN);
            for c in 0..GW {
                let col = ctx.column(self.xl + c as i32, grounds[c], g[c], g[c + 4]);
                min_top = min_top.min(col.gr.water.min(col.gr.g));
                max_ground = max_ground.max(col.gr.g);
                self.cols[c] = col;
            }
            self.min_top = min_top;
            self.max_ground = max_ground;
        }
        let near_bottom = self.y0 + GH as i32 >= self.bottom_y - 14;
        let seeds = ctx.sd;
        for c in 0..GW {
            let x = self.xl + c as i32;
            let n = |which: usize, w: i32| noise1(seeds.get(which), x, w);
            // A second noise from the same seed, with another pattern.
            let n2 = |which: usize, w: i32| noise1(seeds.get(which) ^ 0x2545_F491, x, w);
            self.layer_warp[c] = (60.0 * n(sd::LAYER_WARP, 900)) as i32;
            self.lime_warp[c] = ((36.0 * n(sd::LIME_WARP, 500) + 10.0 * n2(sd::LIME_WARP, 110)) as i32).clamp(-LIME_WARP, LIME_WARP);
            self.coal_warp[c] = ((50.0 * n(sd::COAL_WARP, 700) + 10.0 * n2(sd::COAL_WARP, 150)) as i32).clamp(-COAL_WARP, COAL_WARP);
            self.prov_x[c] = n(sd::PROV_X, 700);
            if near_bottom {
                self.bedrock[c] = 2 + (5.0 * (n(sd::BEDROCK, 23) + 1.0)).clamp(0.0, 10.0) as i32;
            }
        }
        // The limestone bands and coal seams that this chunk can touch.
        self.lime_base = (self.y0 - LIME_WARP).div_euclid(LIME_SPACING);
        for (i, t) in self.lime.iter_mut().enumerate() {
            let h = hash1(ctx.sd.get(sd::LIME), self.lime_base + i as i32);
            *t = if h % 100 < 35 { 4 + ((h >> 8) % 11) as i32 } else { 0 };
        }
        self.coal_base = (self.y0 - COAL_WARP).div_euclid(COAL_SPACING);
        for (i, t) in self.coal.iter_mut().enumerate() {
            let h = hash1(ctx.sd.get(sd::COAL), self.coal_base + i as i32);
            *t = if h % 100 < 55 { 3 + ((h >> 8) % 5) as i32 } else { 0 };
        }
    }

    fn row_info(&self, y: i32) -> RowInfo {
        let dz = (y - self.surface_y) as f32;
        let cave = (0.55 + 0.45 * smoothstep((dz - 450.0) / 300.0)) * (1.0 - 0.3 * smoothstep((dz - 1700.0) / 300.0));
        let vein = if dz < UPPER_STONE_END as f32 { 0.03 * smoothstep((dz - 450.0) / 250.0) } else { 0.022 };
        RowInfo {
            cave,
            vein,
            prov_y: noise1(self.seed(sd::PROV_Y), y, 450),
            coal: (60.0..1950.0).contains(&dz),
            lime: (250.0..2150.0).contains(&dz),
        }
    }

    /// Fill the grid with sky, water, soil and rock.
    fn cells(&mut self) {
        let first_ground = if self.near_surface { (self.min_top - self.y0).clamp(0, GH as i32) as usize } else { 0 };
        if first_ground >= GH {
            return; // All air (or trees, drawn later).
        }
        let min_ground = self.cols.iter().map(|c| c.gr.g).min().unwrap_or(i32::MIN / 2);
        let first_rock = (min_ground - self.y0).clamp(0, GH as i32) as usize;
        let (x0, y0) = (self.x0, self.y0);
        let ctx = self.ctx;
        let sd = &ctx.sd;
        let field = |seed: usize, wx: i32, wy: i32| {
            let s = sd.get(seed);
            Coarse::new(x0, y0, first_rock, move |x, y| fbm2(s, x, y, wx, wy, 2))
        };
        let fields = (first_rock < GH).then(|| {
            [field(sd::TUNNEL, 320, 170), field(sd::CAVERN, 150, 110), field(sd::VEIN, 200, 120), field(sd::POCKET, 90, 60)]
        });
        let mut fr = FieldRow { tunnel: [0.0; GW], cavern: [0.0; GW], vein: [0.0; GW], pocket: [0.0; GW] };
        let m = self.m;
        for r in first_ground..GH {
            let y = y0 + r as i32;
            if let Some(f) = &fields
                && r >= first_rock
            {
                f[0].row(r, &mut fr.tunnel);
                f[1].row(r, &mut fr.cavern);
                f[2].row(r, &mut fr.vein);
                f[3].row(r, &mut fr.pocket);
            }
            let ri = self.row_info(y);
            for c in 0..GW {
                let col = self.cols[c];
                let v = if y < col.gr.g {
                    if y < col.gr.water {
                        0
                    } else if y < col.gr.water + col.gr.ice {
                        m.ice
                    } else {
                        m.water
                    }
                } else {
                    let d = y - col.gr.g;
                    let x = self.xl + c as i32;
                    match self.soil(&col, d, x, y, fr.pocket[c]) {
                        Some(s) => s,
                        None => self.rock(c, x, y, d, &fr, &ri, col.biome),
                    }
                };
                self.grid[r * GW + c] = v;
            }
        }
    }

    /// The soil material `d` rows below the ground of a column, or `None` below the soil.
    #[inline(always)]
    fn soil(&self, col: &Column, d: i32, x: i32, y: i32, pocket: f32) -> Option<u16> {
        if d >= col.soil {
            return None;
        }
        let m = &self.m;
        let g = &col.gr;
        let mut d = d;
        if g.salt > 0 {
            if d < g.salt {
                return Some(m.salt);
            }
            if d < g.salt + 8 {
                return Some(m.clay);
            }
            return Some(m.sand);
        }
        if g.bed > 0 {
            if d < g.bed {
                let tin = hash2(self.seed(sd::BED), x, y) & 255 < g.tin;
                return Some(if tin { m.cassiterite } else { m.gravel });
            }
            d -= g.bed;
            if d < g.clay {
                return Some(m.clay);
            }
            if d < g.clay + 6 {
                return Some(if g.beach_gravel { m.gravel } else { m.sand });
            }
            return Some(match col.biome {
                Biome::Temperate => m.dirt,
                Biome::Desert => m.sand,
                Biome::Tundra => m.frozen_ground,
            });
        }
        if d < g.beach {
            return Some(if g.beach_gravel { m.gravel } else { m.sand });
        }
        Some(match col.biome {
            Biome::Temperate => {
                if d < col.grass {
                    m.grass
                } else if pocket > 0.42 {
                    m.clay
                } else if pocket < -0.62 && d > 2 {
                    m.malachite
                } else {
                    m.dirt
                }
            }
            Biome::Desert => {
                if pocket > 0.55 {
                    m.clay
                } else {
                    m.sand
                }
            }
            Biome::Tundra => {
                if d < col.snow {
                    m.snow
                } else if pocket > 0.5 {
                    m.gravel
                } else {
                    m.frozen_ground
                }
            }
        })
    }

    /// The rock at a cell `d` rows below the ground, in grid column `c`.
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    fn rock(&self, c: usize, x: i32, y: i32, d: i32, fr: &FieldRow, ri: &RowInfo, biome: Biome) -> u16 {
        let m = &self.m;
        if y >= self.bottom_y - self.bedrock[c].max(2) {
            return m.bedrock;
        }
        let dz = y - self.surface_y + self.layer_warp[c];
        let jit = (hash2(self.seed(sd::JITTER), x, y) >> 24) as f32 * (0.08 / 256.0) - 0.04;
        // Caves: long tunnels where one noise is near 0, and caverns where another is high.
        // No caves in the first 50 rows under the ground.
        let mut cave = ri.cave;
        if d < 210 {
            cave *= smoothstep((d - 50) as f32 / 160.0);
        }
        if cave > 0.0 && ((fr.tunnel[c] + jit).abs() < 0.1 * cave || fr.cavern[c] + jit > 1.0 - 0.45 * cave) {
            return 0;
        }
        // Ore veins: thin lines where the vein noise is near 0. The ore changes slowly with the
        // position (the ore province).
        if ri.vein > 0.0 && (fr.vein[c] + jit * 0.5).abs() < ri.vein {
            let p = self.prov_x[c] + ri.prov_y;
            return if p < -0.3 {
                m.magnetite
            } else if p > 0.3 {
                m.chalcopyrite
            } else {
                m.hematite
            };
        }
        if ri.coal {
            let yy = y + self.coal_warp[c];
            let seam = yy.div_euclid(COAL_SPACING);
            let t = self.coal[(seam - self.coal_base) as usize];
            if yy.rem_euclid(COAL_SPACING) < t && noise1(self.seed(sd::COAL_BREAK) ^ seam as u32, x, 220) > -0.15 {
                return m.coal;
            }
        }
        if ri.lime {
            let yy = y + self.lime_warp[c];
            let band = yy.div_euclid(LIME_SPACING);
            if yy.rem_euclid(LIME_SPACING) < self.lime[(band - self.lime_base) as usize] {
                return m.limestone;
            }
        }
        let p = fr.pocket[c] + jit;
        if p > 0.6 {
            // Loose pockets: clay or sand near the surface, gravel below.
            return if d < 60 && biome == Biome::Temperate {
                m.clay
            } else if d < 60 && biome == Biome::Desert {
                m.sand
            } else {
                m.gravel
            };
        }
        if p < -0.62 {
            if d < 220 && biome == Biome::Temperate {
                return m.malachite;
            }
            if (600..UPPER_STONE_END).contains(&dz) {
                return m.sand;
            }
        }
        if dz < UPPER_STONE_END {
            m.stone
        } else if dz < crate::DEEP_ROCK_END {
            m.granite
        } else {
            m.basalt
        }
    }

    /// Coal outcrops, the blobs of the start area, and the water and methane pockets.
    fn features(&mut self) {
        if self.near_surface {
            self.outcrops();
            // The start blobs are all within 400 cells of x = 0.
            if self.x0 < 600 && self.x0 + CHUNK_SIZE > -600 {
                self.start_blobs();
            }
        }
        self.pockets();
    }

    /// True for a cell of soil or rock (not air, water or ice on water).
    #[inline(always)]
    fn is_ground(&self, r: usize, c: usize) -> bool {
        let y = self.y0 + r as i32;
        y >= self.cols[c].gr.g && self.grid[r * GW + c] != 0
    }

    fn outcrops(&mut self) {
        let ctx = self.ctx;
        let mut last: Option<surface::Feature> = None;
        for c in 0..GW {
            let x = self.xl + c as i32;
            // Most columns share the outcrop of the column before.
            let f = match last {
                Some(f) if (x - f.cx).abs() < f.hw => Some(f),
                _ => ctx.outcrop_near(x),
            };
            last = f;
            let Some(f) = f else { continue };
            let dx = (x - f.cx).abs();
            if dx >= f.hw {
                continue;
            }
            let thick = (f.depth as f32 * smoothstep((f.hw - dx) as f32 / 40.0)).round() as i32;
            let top = f.seam_top(x);
            // The main seam, and a thin one below it.
            for (a, b) in [(top, top + thick), (top + thick + 14, top + thick + 14 + thick / 2)] {
                for y in a.max(self.y0)..b.min(self.y0 + GH as i32) {
                    let r = (y - self.y0) as usize;
                    if self.is_ground(r, c) {
                        self.grid[r * GW + c] = self.m.coal;
                    }
                }
            }
        }
    }

    fn start_blobs(&mut self) {
        let blobs = self.ctx.start().blobs;
        for b in blobs {
            let (xa, xb) = ((b.cx - b.rx).max(self.xl), (b.cx + b.rx + 1).min(self.xl + GW as i32));
            let (ya, yb) = ((b.cy - b.ry).max(self.y0), (b.cy + b.ry + 1).min(self.y0 + GH as i32));
            for y in ya..yb {
                for x in xa..xb {
                    let (r, c) = ((y - self.y0) as usize, (x - self.xl) as usize);
                    let keep = b.mat == self.m.malachite && hash2(self.seed(sd::START), x, y) % 4 == 0;
                    if b.contains(x, y) && !keep && self.is_ground(r, c) {
                        self.grid[r * GW + c] = b.mat;
                    }
                }
            }
        }
    }

    /// Water pockets and methane pockets in the upper stone, each inside a shell of stone.
    fn pockets(&mut self) {
        let (rx, ry) = (self.x0.div_euclid(POCKET_REGION), self.y0.div_euclid(POCKET_REGION));
        let m = self.m;
        for p in pockets_in(self.ctx, self.surface_y, rx, ry).into_iter().flatten() {
            let (ox, oy) = (p.rx + SHELL, p.ry + SHELL);
            let (xa, xb) = ((p.cx - ox).max(self.xl), (p.cx + ox + 1).min(self.xl + GW as i32));
            let (ya, yb) = ((p.cy - oy).max(self.y0), (p.cy + oy + 1).min(self.y0 + GH as i32));
            for y in ya..yb {
                for x in xa..xb {
                    let (dx, dy) = ((x - p.cx) as f32, (y - p.cy) as f32);
                    let outer = (dx / ox as f32).powi(2) + (dy / oy as f32).powi(2);
                    if outer > 1.0 {
                        continue;
                    }
                    let inner = (dx / p.rx as f32).powi(2) + (dy / p.ry as f32).powi(2);
                    let v = if inner > 1.0 {
                        m.stone
                    } else if y < p.level {
                        0
                    } else if p.methane {
                        m.methane
                    } else {
                        m.water
                    };
                    self.grid[(y - self.y0) as usize * GW + (x - self.xl) as usize] = v;
                }
            }
        }
    }

    fn trees(&mut self) {
        if !self.near_surface {
            return;
        }
        // Trees stand on the ground and reach at most MAX_HEIGHT above it.
        let y1 = self.y0 + GH as i32;
        if self.y0 > self.max_ground + 8 || y1 < self.min_top - trees::MAX_HEIGHT - 8 {
            return;
        }
        let k0 = (self.xl - trees::REACH).div_euclid(trees::SLOT);
        let k1 = (self.xl + GW as i32 + trees::REACH).div_euclid(trees::SLOT);
        for k in k0..=k1 {
            if let Some(t) = trees::in_slot(self.ctx, k) {
                t.draw(&self.m, &mut self.grid, self.xl, self.y0, GH);
            }
        }
    }

    /// A powder cell with air, gas or liquid below it or diagonally below it becomes a solid.
    fn support(&mut self) {
        let kind = &self.wg.kind;
        let open = |v: u16| kind[v as usize] & OPEN != 0;
        for r in 0..CHUNK_SIZE as usize {
            for c in 1..GW - 1 {
                let v = self.grid[r * GW + c];
                if kind[v as usize] & POWDER == 0 {
                    continue;
                }
                let below = (r + 1) * GW + c;
                if open(self.grid[below]) || open(self.grid[below - 1]) || open(self.grid[below + 1]) {
                    self.grid[r * GW + c] = self.wg.support[v as usize];
                }
            }
        }
    }

    /// Copy the chunk out of the grid, with temperatures: cold in the tundra, warm deep down.
    fn write(&self, cells: &mut ChunkCells) {
        let air = self.wg.air_temperature_rows();
        let m = &self.m;
        for r in 0..CHUNK_SIZE as usize {
            let y = self.y0 + r as i32;
            let src = &self.grid[r * GW + 1..r * GW + 1 + CHUNK_SIZE as usize];
            let dst = local_index(0, r as i32);
            cells.mat[dst..dst + CHUNK_SIZE as usize].copy_from_slice(src);
            let deep = y - self.surface_y > 900;
            let row_temp = air.get(y as usize).copied().unwrap_or(foundry_core::DEFAULT_TEMPERATURE);
            for (lx, &v) in src.iter().enumerate() {
                if v == 0 {
                    continue;
                }
                let col = &self.cols[lx + 1];
                let t = if self.near_surface && col.biome == Biome::Tundra && y < col.gr.g + 160 {
                    if v == m.water { 2 } else { (-12 + (y - col.gr.g).max(0) / 12).min(-2) as i16 }
                } else if deep {
                    row_temp
                } else {
                    continue;
                };
                cells.temp[dst + lx] = t;
            }
        }
    }
}

/// Thickness of the stone shell around a pocket.
const SHELL: i32 = 4;

/// A pocket of water (with air above it) or of methane, in the upper stone.
#[derive(Debug, Clone, Copy)]
struct Pocket {
    cx: i32,
    cy: i32,
    rx: i32,
    ry: i32,
    /// The first row of water or methane; the rows above it are air.
    level: i32,
    methane: bool,
}

/// The pockets of the pocket region (rx, ry). Each pocket stays at least 10 cells inside its
/// region, so a chunk (and its border of one cell) only needs the pockets of its own region.
fn pockets_in(ctx: &Ctx, surface_y: i32, rx: i32, ry: i32) -> [Option<Pocket>; 2] {
    let mut out: [Option<Pocket>; 2] = [None; 2];
    for i in 0..2 {
        let h = hash2(ctx.sd.get(sd::POCKETS) ^ (i as u32).wrapping_mul(0x9E37_79B9), rx, ry);
        let h2 = hash1(h, 3);
        let roll = h % 100;
        let methane = (38..52).contains(&roll);
        if roll >= 52 {
            continue;
        }
        let cx = rx * POCKET_REGION + 80 + ((h >> 8) % 352) as i32;
        let cy = ry * POCKET_REGION + 80 + (h2 % 352) as i32;
        let depth = cy - surface_y;
        let (lo, hi) = if methane { (650, 2000) } else { (320, 2000) };
        if !(lo..hi).contains(&depth) {
            continue;
        }
        let (prx, pry) = if methane {
            (14 + ((h2 >> 10) % 21) as i32, 8 + ((h2 >> 16) % 11) as i32)
        } else {
            (24 + ((h2 >> 10) % 37) as i32, 12 + ((h2 >> 16) % 19) as i32)
        };
        let level = if methane || (h2 >> 24) % 3 == 0 { cy - pry - 1 } else { cy - pry * 3 / 10 };
        let p = Pocket { cx, cy, rx: prx, ry: pry, level, methane };
        // The second pocket must not touch the first.
        if let (1, Some(a)) = (i, out[0]) {
            let apart = (a.cx - p.cx).abs() > a.rx + p.rx + 2 * SHELL + 4 || (a.cy - p.cy).abs() > a.ry + p.ry + 2 * SHELL + 4;
            if !apart {
                continue;
            }
        }
        out[i] = Some(p);
    }
    out
}
