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

use crate::noise::{Coarse, GH, GW, hash1, hash2, noise1, sampled_columns, smoothstep};
use crate::surface::{self, Biome, Column, Ctx, sd};
use crate::{Mats, OPEN, POWDER, SURFACE_LAYER, UPPER_STONE_END, WorldGen, trees};
use foundry_core::{CHUNK_SIZE, local_index};
use foundry_sim::ChunkCells;

/// A chunk whose lowest row is this far above the surface level is all air.
const SKY_CLEAR: i32 = surface::MAX_RISE + trees::MAX_HEIGHT + 8;
/// The deepest soil below the ground (dunes, lake beds), in cells.
const MAX_SOIL: i32 = 150;
/// Size of the square regions that hold the water and methane pockets.
const POCKET_REGION: i32 = 512;
/// Rows of one layer (stratum) of the surface layer, of the limestone band pattern, and of the
/// coal seam pattern.
const STRATUM: i32 = 22;
const LIME_SPACING: i32 = 48;
const COAL_SPACING: i32 = 150;
/// Largest shift (up or down) of the strata, the limestone bands and the coal seams.
const SED_WARP: i32 = 210;
const LIME_WARP: i32 = 59;
const COAL_WARP: i32 = 69;
const SED_TABLE: usize = 25;
/// Largest half-width of a tunnel, in noise units, and the smallest one (thinner tunnels are
/// left out, so there are no hair-thin cracks).
const TUNNEL_W: f32 = 0.07;
const MIN_TUNNEL_W: f32 = 0.022;
/// Largest half-width of an ore vein in the upper stone and below it, and the smallest one.
const VEIN_W: f32 = 0.05;
const DEEP_VEIN_W: f32 = 0.035;
const MIN_VEIN_W: f32 = 0.012;
/// Strength of the random change at the edges of caves and veins, in noise units.
const CAVE_EDGE: f32 = 0.01;
const VEIN_EDGE: f32 = 0.006;
const LIME_TABLE: usize = 7;
const COAL_TABLE: usize = 4;

/// Make the cells of one chunk. See the module documentation.
pub(crate) fn generate(wg: &WorldGen, cells: &mut ChunkCells) {
    let s = *wg.world_settings();
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
    /// Half-width of the tunnels here (noise units; 0: no tunnels).
    tunnel_width: [f32; GW],
    /// Half-width of the ore veins here (noise units).
    vein_width: [f32; GW],
}

/// The noise fields of a chunk.
struct Fields {
    tunnel: Coarse,
    cavern: Coarse,
    vein: Coarse,
    pocket: Coarse,
    tunnel_mask: Coarse,
    vein_mask: Coarse,
}

/// Values that are the same for a whole row.
struct RowInfo {
    /// How much cave there is (0 to 1), before the change near the ground.
    cave: f32,
    /// Largest half-width of the ore veins in noise units (0: no veins).
    vein: f32,
    prov_y: f32,
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
    near_bottom: bool,
    cols: [Column; GW],
    grid: [u16; GW * GH],
    /// The highest row of ground or water in the grid columns, and the lowest row of ground.
    min_top: i32,
    max_ground: i32,
    /// Per column: shift of the layer depth, the strata, the limestone bands and the coal seams;
    /// the ore province along x; rows of bedrock at the bottom.
    layer_warp: [i32; GW],
    sed_warp: [i32; GW],
    lime_warp: [i32; GW],
    coal_warp: [i32; GW],
    prov_x: [f32; GW],
    /// Thickness factor of the limestone bands along x: [even bands, odd bands].
    lime_scale: [[f32; GW]; 2],
    bedrock: [i32; GW],
    sed_base: i32,
    sed: [u16; SED_TABLE],
    lime_base: i32,
    lime: [i32; LIME_TABLE],
    coal_base: i32,
    coal: [i32; COAL_TABLE],
}

impl<'a, 'b> Fill<'a, 'b> {
    fn new(wg: &'a WorldGen, ctx: &'b Ctx<'a>, x0: i32, y0: i32) -> Self {
        let s = wg.world_settings();
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
            near_bottom: false,
            cols: [Column::deep(); GW],
            grid: [0; GW * GH],
            min_top: i32::MIN / 2,
            max_ground: i32::MIN / 2,
            layer_warp: [0; GW],
            sed_warp: [0; GW],
            lime_warp: [0; GW],
            coal_warp: [0; GW],
            prov_x: [0.0; GW],
            lime_scale: [[0.0; GW]; 2],
            bedrock: [0; GW],
            sed_base: 0,
            sed: [0; SED_TABLE],
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
    #[inline(never)]
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
        let seeds = ctx.sd;
        let n = |which: usize, w: i32| move |x: i32| noise1(seeds.get(which), x, w);
        // A second noise from the same seed, with another pattern.
        let n2 = |which: usize, w: i32| move |x: i32| noise1(seeds.get(which) ^ 0x2545_F491, x, w);
        let layer = sampled_columns(self.x0, n(sd::LAYER_WARP, 900));
        let sed_a = sampled_columns(self.x0, n(sd::SED_WARP, 400));
        let sed_b = sampled_columns(self.x0, n2(sd::SED_WARP, 90));
        // The strata also dip and rise slowly along x, so the same depth has other strata in
        // other places.
        let sed_c = sampled_columns(self.x0, move |x| noise1(seeds.get(sd::SED_WARP) ^ 0x1B87_3593, x, 4200));
        let lime_a = sampled_columns(self.x0, n(sd::LIME_WARP, 500));
        let lime_b = sampled_columns(self.x0, n2(sd::LIME_WARP, 110));
        let coal_a = sampled_columns(self.x0, n(sd::COAL_WARP, 700));
        let coal_b = sampled_columns(self.x0, n2(sd::COAL_WARP, 150));
        self.prov_x = sampled_columns(self.x0, n(sd::PROV_X, 700));
        // Limestone bands swell and thin along x.
        for (k, scale) in self.lime_scale.iter_mut().enumerate() {
            let s = seeds.get(sd::LIME) ^ (k as u32 + 1).wrapping_mul(0x7FEB_352D);
            *scale = sampled_columns(self.x0, move |x| (1.0 + 0.8 * noise1(s, x, 240)).max(0.0));
        }
        for c in 0..GW {
            self.layer_warp[c] = (60.0 * layer[c]) as i32;
            self.sed_warp[c] = ((28.0 * sed_a[c] + 12.0 * sed_b[c] + 150.0 * sed_c[c]) as i32).clamp(-SED_WARP, SED_WARP);
            self.lime_warp[c] = ((40.0 * lime_a[c] + 12.0 * lime_b[c]) as i32).clamp(-LIME_WARP, LIME_WARP);
            self.coal_warp[c] = ((50.0 * coal_a[c] + 12.0 * coal_b[c]) as i32).clamp(-COAL_WARP, COAL_WARP);
        }
        self.near_bottom = self.y0 + GH as i32 >= self.bottom_y - 14;
        if self.near_bottom {
            for c in 0..GW {
                let x = self.xl + c as i32;
                self.bedrock[c] = 2 + (5.0 * (noise1(seeds.get(sd::BEDROCK), x, 23) + 1.0)).clamp(0.0, 10.0) as i32;
            }
        }
        // The strata, limestone bands and coal seams that this chunk can touch.
        self.sed_base = (self.y0 - SED_WARP).div_euclid(STRATUM);
        for i in 0..SED_TABLE {
            self.sed[i] = self.stratum(self.sed_base + i as i32);
        }
        self.lime_base = (self.y0 - LIME_WARP).div_euclid(LIME_SPACING);
        for (i, t) in self.lime.iter_mut().enumerate() {
            let h = hash1(seeds.get(sd::LIME), self.lime_base + i as i32);
            *t = if h % 100 < 32 { 6 + ((h >> 8) % 15) as i32 } else { 0 };
        }
        self.coal_base = (self.y0 - COAL_WARP).div_euclid(COAL_SPACING);
        for (i, t) in self.coal.iter_mut().enumerate() {
            let h = hash1(seeds.get(sd::COAL), self.coal_base + i as i32);
            *t = if h % 100 < 55 { 4 + ((h >> 8) % 6) as i32 } else { 0 };
        }
    }

    /// The material of stratum `k` of the surface layer: stone, or a loose material. Deeper
    /// strata are more often stone. Some strata continue the one above, so strata have
    /// different thicknesses.
    fn stratum(&self, k: i32) -> u16 {
        let h = hash1(self.seed(sd::SED), k);
        let k = if (h >> 20).is_multiple_of(3) { k - 1 } else { k };
        let h = hash1(self.seed(sd::SED), k);
        let depth = (k * STRATUM - self.surface_y) as f32;
        let stone = 12.0 + 58.0 * smoothstep((depth - 80.0) / 480.0);
        if ((h % 100) as f32) < stone {
            return self.m.stone;
        }
        match (h >> 8) % 100 {
            0..36 => self.m.dirt,
            36..64 => self.m.clay,
            64..80 => self.m.sand,
            _ => self.m.gravel,
        }
    }

    fn row_info(&self, y: i32) -> RowInfo {
        let dz = (y - self.surface_y) as f32;
        let cave = (0.35 + 0.65 * smoothstep((dz - 500.0) / 200.0)) * (1.0 - 0.25 * smoothstep((dz - 1700.0) / 300.0));
        let vein = if dz < UPPER_STONE_END as f32 { VEIN_W * smoothstep((dz - 520.0) / 150.0) } else { DEEP_VEIN_W };
        RowInfo { cave, vein, prov_y: noise1(self.seed(sd::PROV_Y), y, 450) }
    }

    #[inline(never)]
    fn fields(&self, first_row: usize) -> Fields {
        let (x0, y0) = (self.x0, self.y0);
        let sd = &self.ctx.sd;
        let fbm = |seed: usize, wx: i32, wy: i32| Coarse::fbm(x0, y0, first_row, 8, sd.get(seed), wx, wy, 2);
        let mask = |seed: usize, wx: i32, wy: i32| Coarse::fbm(x0, y0, first_row, 32, sd.get(seed) ^ 0x68E3_1DA4, wx, wy, 1);
        Fields {
            tunnel: fbm(sd::TUNNEL, 480, 240),
            cavern: fbm(sd::CAVERN, 200, 140),
            vein: fbm(sd::VEIN, 260, 150),
            pocket: fbm(sd::POCKET, 90, 60),
            tunnel_mask: mask(sd::TUNNEL, 420, 300),
            vein_mask: mask(sd::VEIN, 300, 220),
        }
    }

    /// Lay down the rock layers of each column below its soil: the strata of the surface layer,
    /// coal seams, limestone bands, stone, granite, basalt and bedrock. Each column is filled in
    /// runs of rows with the same material: a run ends where a band, a stratum or a layer ends.
    #[inline(never)]
    fn layers(&mut self) {
        let m = self.m;
        let seed_break = self.seed(sd::COAL_BREAK);
        // Depths (below the surface level, with the layer shift) where the rules change.
        const EDGES: [i32; 7] = [60, 500, SURFACE_LAYER, UPPER_STONE_END, 1950, 2150, crate::DEEP_ROCK_END];
        for c in 0..GW {
            let (g, soil) = (self.cols[c].gr.g, self.cols[c].soil);
            let r0 = (g.saturating_add(soil) - self.y0).clamp(0, GH as i32);
            if r0 >= GH as i32 {
                continue;
            }
            let x = self.xl + c as i32;
            let lw = self.layer_warp[c];
            let bed_from = if self.near_bottom { self.bottom_y - self.bedrock[c].max(2) } else { i32::MAX };
            // Positions in the three band patterns: (pattern number, row in it).
            let y_start = self.y0 + r0;
            let at = |yy: i32, n: i32| (yy.div_euclid(n), yy.rem_euclid(n));
            let (mut sk, mut sr) = at(y_start + self.sed_warp[c], STRATUM);
            let (mut lk, mut lr) = at(y_start + self.lime_warp[c], LIME_SPACING);
            let (mut ck, mut cr) = at(y_start + self.coal_warp[c], COAL_SPACING);
            // A coal seam has gaps along x.
            let seam_here = |k: i32| noise1(seed_break ^ k as u32, x, 220) > -0.2;
            let mut seam_ok = seam_here(ck);
            let mut r = r0;
            while r < GH as i32 {
                let y = self.y0 + r;
                let dz = y - self.surface_y + lw;
                // Rows until a depth rule changes.
                let mut run = GH as i32 - r;
                for e in EDGES {
                    if e > dz {
                        run = run.min(e - dz);
                        break;
                    }
                }
                let coal_t = self.coal[(ck - self.coal_base) as usize];
                let lime_t = (self.lime[(lk - self.lime_base) as usize] as f32 * self.lime_scale[(lk & 1) as usize][c]) as i32;
                let in_coal = seam_ok && (60..1950).contains(&dz) && cr < coal_t;
                let in_lime = (500..2150).contains(&dz) && lr < lime_t;
                run = run.min(if cr < coal_t { coal_t - cr } else { COAL_SPACING - cr });
                let v = if y >= bed_from {
                    run = GH as i32 - r;
                    m.bedrock
                } else if in_coal {
                    m.coal
                } else if dz < SURFACE_LAYER {
                    run = run.min(STRATUM - sr);
                    self.sed[(sk - self.sed_base) as usize]
                } else if in_lime {
                    run = run.min(lime_t - lr);
                    m.limestone
                } else {
                    if (500..2150).contains(&dz) {
                        run = run.min(LIME_SPACING - lr);
                    }
                    if dz < UPPER_STONE_END {
                        m.stone
                    } else if dz < crate::DEEP_ROCK_END {
                        m.granite
                    } else {
                        m.basalt
                    }
                };
                if y < bed_from {
                    run = run.min(bed_from - y);
                }
                let run = run.max(1);
                for rr in r..r + run {
                    self.grid[rr as usize * GW + c] = v;
                }
                r += run;
                // Move the band positions on by `run` rows.
                sr += run;
                sk += sr / STRATUM;
                sr %= STRATUM;
                lr += run;
                lk += lr / LIME_SPACING;
                lr %= LIME_SPACING;
                cr += run;
                if cr >= COAL_SPACING {
                    ck += cr / COAL_SPACING;
                    cr %= COAL_SPACING;
                    seam_ok = seam_here(ck);
                }
            }
        }
    }

    /// For each block of 8 × 8 cells between lattice points: true if a cave, a vein or a pocket
    /// may be in it. The fields change in straight lines between lattice points, so the largest
    /// and smallest values of a block are at its corners. `blocks[j][i]` is the block from lattice
    /// row j and column i (grid rows 8j to 8j + 8, grid columns 8i - 7 to 8i + 1).
    #[inline(never)]
    fn carve_blocks(&self, f: &Fields, first_row: usize) -> [[bool; 10]; 9] {
        let mut out = [[false; 10]; 9];
        // The largest tunnel width, cave amount and vein width (see `row_info`).
        let (tunnel, vein) = (TUNNEL_W + CAVE_EDGE, VEIN_W * 1.4 + VEIN_EDGE);
        for (j, row) in out.iter_mut().enumerate().skip(first_row / 8) {
            let y = self.y0 + (j * 8) as i32;
            let cave = self.row_info(y).cave.max(self.row_info(y + 8).cave);
            let cavern = 1.0 - 0.4 * cave - CAVE_EDGE;
            for (i, o) in row.iter_mut().enumerate() {
                let corners = |c: &Coarse| {
                    let l = &c.lattice;
                    let v = [l[j][i], l[j][i + 1], l[j + 1][i], l[j + 1][i + 1]];
                    (v.iter().copied().fold(f32::MAX, f32::min), v.iter().copied().fold(f32::MIN, f32::max))
                };
                let near_zero = |c: &Coarse, w: f32| {
                    let (lo, hi) = corners(c);
                    lo < w && hi > -w
                };
                let (plo, phi) = corners(&f.pocket);
                let (_, chi) = corners(&f.cavern);
                *o = near_zero(&f.tunnel, tunnel) || chi > cavern || near_zero(&f.vein, vein) || phi > 0.53 || plo < -0.60;
            }
        }
        out
    }

    /// Fill the grid with sky, water, soil and rock.
    #[inline(never)]
    fn cells(&mut self) {
        let first_ground = if self.near_surface { (self.min_top - self.y0).clamp(0, GH as i32) as usize } else { 0 };
        if first_ground >= GH {
            return; // All air (or trees, drawn later).
        }
        self.layers();
        let min_ground = self.cols.iter().map(|c| c.gr.g).min().unwrap_or(i32::MIN / 2);
        let first_rock = (min_ground - self.y0).clamp(0, GH as i32) as usize;
        let fields = (first_rock < GH).then(|| self.fields(first_rock));
        let blocks = fields.as_ref().map(|f| self.carve_blocks(f, first_rock));
        let mut fr = FieldRow {
            tunnel: [0.0; GW],
            cavern: [0.0; GW],
            vein: [0.0; GW],
            pocket: [0.0; GW],
            tunnel_width: [0.0; GW],
            vein_width: [0.0; GW],
        };
        let m = self.m;
        for r in first_ground..GH {
            let y = self.y0 + r as i32;
            let ri = self.row_info(y);
            if let Some(f) = &fields
                && r >= first_rock
            {
                f.tunnel.row(r, &mut fr.tunnel);
                f.cavern.row(r, &mut fr.cavern);
                f.vein.row(r, &mut fr.vein);
                f.pocket.row(r, &mut fr.pocket);
                f.tunnel_mask.row(r, &mut fr.tunnel_width);
                f.vein_mask.row(r, &mut fr.vein_width);
                for v in fr.tunnel_width.iter_mut() {
                    *v = TUNNEL_W * ri.cave * (0.35 + 0.65 * smoothstep((*v + 0.1) / 0.4));
                }
                for v in fr.vein_width.iter_mut() {
                    *v = ri.vein * (*v * 2.0 + 0.2).clamp(0.0, 1.4);
                }
            }
            for c in 0..GW {
                let col = &self.cols[c];
                let i = r * GW + c;
                if y < col.gr.g {
                    self.grid[i] = if y < col.gr.water {
                        0
                    } else if y < col.gr.water + col.gr.ice {
                        m.ice
                    } else {
                        m.water
                    };
                    continue;
                }
                let d = y - col.gr.g;
                let x = self.xl + c as i32;
                if d < col.soil {
                    self.grid[i] = self.soil(col, d, x, y, fr.pocket[c]);
                    continue;
                }
                let base = self.grid[i];
                if base != m.bedrock && blocks.as_ref().is_some_and(|b| b[r / 8][c.div_ceil(8)]) {
                    self.grid[i] = self.carve(base, c, x, y, d, &fr, &ri, col.biome);
                }
            }
        }
    }

    /// The soil material `d` rows below the ground of a column (`d` is less than `col.soil`).
    #[inline(always)]
    fn soil(&self, col: &Column, d: i32, x: i32, y: i32, pocket: f32) -> u16 {
        let m = &self.m;
        let g = &col.gr;
        let mut d = d;
        if g.salt > 0 {
            if d < g.salt {
                return m.salt;
            }
            // Clay under the salt, thinner at the edges of the flat.
            if d < g.salt * 3 {
                return m.clay;
            }
            return m.sand;
        }
        if g.bed > 0 {
            if d < g.bed {
                let tin = hash2(self.seed(sd::BED), x, y) & 255 < g.tin;
                return if tin { m.cassiterite } else { m.gravel };
            }
            d -= g.bed;
            if d < g.clay {
                return m.clay;
            }
            if d < g.clay + 6 {
                return if g.beach_gravel { m.gravel } else { m.sand };
            }
            return match col.biome {
                Biome::Temperate => m.dirt,
                Biome::Desert => m.sand,
                Biome::Tundra => m.frozen_ground,
            };
        }
        if d < g.beach {
            return if g.beach_gravel { m.gravel } else { m.sand };
        }
        match col.biome {
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
        }
    }

    /// Cut caves, ore veins and pockets into the rock layer `base` at a cell `d` rows below the
    /// ground, in grid column `c`.
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    fn carve(&self, base: u16, c: usize, x: i32, y: i32, d: i32, fr: &FieldRow, ri: &RowInfo, biome: Biome) -> u16 {
        let m = &self.m;
        // -1 to 1: a small random change at the edges of shapes, so they look rough.
        let jit = self.wg.jitter[((y & 63) << 6 | (x & 63)) as usize];
        // Caves: long tunnels where one noise is near 0 (only where the tunnel mask allows),
        // and caverns where another noise is high. Fewer caves near the ground, and none in the
        // first 50 rows under it.
        let (mut tunnel, mut cave) = (fr.tunnel_width[c], ri.cave);
        if d < 210 {
            let f = smoothstep((d - 50) as f32 / 160.0);
            tunnel *= f;
            cave *= f;
        }
        if tunnel < MIN_TUNNEL_W {
            tunnel = 0.0;
        }
        let e = jit * CAVE_EDGE;
        if (fr.tunnel[c] + e).abs() < tunnel || fr.cavern[c] + e > 1.0 - 0.4 * cave {
            return 0;
        }
        // Ore veins: lines where the vein noise is near 0, thick in some places and missing in
        // others. The ore changes slowly with the position (the ore province).
        let vw = fr.vein_width[c];
        if vw > MIN_VEIN_W && (fr.vein[c] + jit * VEIN_EDGE).abs() < vw {
            let p = self.prov_x[c] + ri.prov_y;
            return if p < -0.3 {
                m.magnetite
            } else if p > 0.3 {
                m.chalcopyrite
            } else {
                m.hematite
            };
        }
        if base == m.coal {
            return base;
        }
        let p = fr.pocket[c] + jit * 0.02;
        let dz = y - self.surface_y + self.layer_warp[c];
        if dz < SURFACE_LAYER {
            // The strata of the surface layer: stone boulders in the loose strata, gravel in
            // the stone strata, and malachite (temperate, near the ground) or clay.
            if p > 0.55 {
                return if base == m.stone { m.gravel } else { m.stone };
            }
            if p < -0.62 {
                return if d < 220 && biome == Biome::Temperate { m.malachite } else { m.clay };
            }
            return base;
        }
        if p > 0.6 {
            m.gravel
        } else if p < -0.64 && dz < UPPER_STONE_END {
            m.sand
        } else {
            base
        }
    }


    /// Coal outcrops, the blobs of the start area, and the water and methane pockets.
    #[inline(never)]
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
                    let keep = b.mat == self.m.malachite && hash2(self.seed(sd::START), x, y).is_multiple_of(4);
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
        let seed = self.seed(sd::POCKETS);
        for p in pockets_in(self.ctx, self.surface_y, rx, ry).into_iter().flatten() {
            // The pocket is an ellipse with wobbly sides: its width changes from row to row and
            // its height from column to column.
            let (ox, oy) = ((p.rx as f32 * (1.0 + 1.3 * WOBBLE)) as i32 + SHELL, (p.ry as f32 * (1.0 + 1.3 * WOBBLE)) as i32 + SHELL);
            let (xa, xb) = ((p.cx - ox).max(self.xl), (p.cx + ox + 1).min(self.xl + GW as i32));
            let (ya, yb) = ((p.cy - oy).max(self.y0), (p.cy + oy + 1).min(self.y0 + GH as i32));
            let mut half_h = [0.0f32; GW];
            for x in xa..xb {
                half_h[(x - xa) as usize] = p.ry as f32 * (1.0 + WOBBLE * noise1(seed ^ 0x51ED_270B, x + p.cx, 23));
            }
            for y in ya..yb {
                let half_w = p.rx as f32 * (1.0 + WOBBLE * noise1(seed ^ 0x2F4B_19A1, y + p.cy, 17));
                for x in xa..xb {
                    let (dx, dy) = ((x - p.cx) as f32, (y - p.cy) as f32);
                    let half_h = half_h[(x - xa) as usize];
                    let s = SHELL as f32;
                    let outer = (dx / (half_w + s)).powi(2) + (dy / (half_h + s)).powi(2);
                    if outer > 1.0 {
                        continue;
                    }
                    let inner = (dx / half_w).powi(2) + (dy / half_h).powi(2);
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

    #[inline(never)]
    fn trees(&mut self) {
        if !self.near_surface {
            return;
        }
        // Trees stand on the ground and reach at most MAX_HEIGHT above it.
        let y1 = self.y0 + GH as i32;
        if self.y0 > self.max_ground + 16 || y1 < self.min_top - trees::MAX_HEIGHT - 8 {
            return;
        }
        let k0 = (self.xl - trees::REACH).div_euclid(trees::SLOT);
        let k1 = (self.xl + GW as i32 + trees::REACH).div_euclid(trees::SLOT);
        for k in k0..=k1 {
            if let Some(t) = trees::in_slot(self.ctx, k) {
                t.draw(&self.m, &mut self.grid, self.xl, self.y0, GH);
            }
            if let Some(d) = trees::decor_in_slot(self.ctx, k) {
                d.draw(&self.m, &mut self.grid, self.xl, self.y0, GH);
            }
        }
    }

    /// A powder cell with air, gas or liquid below it or diagonally below it becomes a solid.
    #[inline(never)]
    fn support(&mut self) {
        let kind: &[u8; 65536] = &self.wg.kind;
        let open = |v: u16| kind[v as usize] & OPEN != 0;
        for r in 0..CHUNK_SIZE as usize {
            let row = &self.grid[r * GW..(r + 1) * GW];
            if !row.iter().any(|&v| kind[v as usize] & POWDER != 0) {
                continue;
            }
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
    #[inline(never)]
    fn write(&self, cells: &mut ChunkCells) {
        let air = self.wg.air_temperature_rows();
        let m = &self.m;
        let cold = self.near_surface && self.cols.iter().any(|c| c.biome == Biome::Tundra);
        for r in 0..CHUNK_SIZE as usize {
            let y = self.y0 + r as i32;
            let src = &self.grid[r * GW + 1..r * GW + 1 + CHUNK_SIZE as usize];
            let dst = local_index(0, r as i32);
            cells.mat[dst..dst + CHUNK_SIZE as usize].copy_from_slice(src);
            let temp = &mut cells.temp[dst..dst + CHUNK_SIZE as usize];
            if cold {
                for (lx, &v) in src.iter().enumerate() {
                    let col = &self.cols[lx + 1];
                    if v != 0 && col.biome == Biome::Tundra && y < col.gr.g + 160 {
                        temp[lx] = if v == m.water { 2 } else { (-12 + (y - col.gr.g).max(0) / 12).min(-2) as i16 };
                    }
                }
            } else if y - self.surface_y > 900 {
                let t = air.get(y as usize).copied().unwrap_or(foundry_core::DEFAULT_TEMPERATURE);
                for (o, &v) in temp.iter_mut().zip(src) {
                    if v != 0 {
                        *o = t;
                    }
                }
            }
        }
    }
}

/// Thickness of the stone shell around a pocket.
const SHELL: i32 = 4;
/// How much the width and height of a pocket change along its sides (0.18: 18 percent).
const WOBBLE: f32 = 0.18;

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
        let level = if methane || (h2 >> 24).is_multiple_of(3) { cy - pry - 1 } else { cy - pry * 3 / 10 };
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
