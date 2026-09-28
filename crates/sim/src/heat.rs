//! Heat flow and phase changes (technical design section 6.5, game design section 8).
//!
//! # Heat flow
//!
//! Every cell has a temperature in whole °C (`Chunk::temp`). In each tick, heat flows between each
//! cell and its 4 neighbors (left, right, up, down):
//!
//! - The heat that flows from cell b into cell a is `k_ab × (T_b − T_a)`. Cell a changes by
//!   `flow / C_a` and cell b by `−flow / C_b` (C is the heat capacity of the material). So the heat
//!   energy `Σ C × T` stays the same.
//! - `k_ab = min(g_a, g_b)`, with `g = min(CONDUCT_SCALE × conductivity, MAX_STEP × C)` for each
//!   material. The smaller conductivity of the two sets the flow (close to the harmonic mean, and
//!   fast). The limit `MAX_STEP × C` makes sure that one neighbor changes a cell by at most
//!   `MAX_STEP` (0.2) of the difference. With 4 neighbors that is at most 0.8, so a cell never
//!   goes past the temperature of its neighbors (the update is stable). Metals reach this limit,
//!   so all metals conduct heat at the same, highest speed.
//! - Two neighbors that differ by less than `MIN_DIFF` (2 °C) do not exchange heat. Without this,
//!   the random rounding (below) moves single degrees back and forth for a long time, and areas
//!   never come to rest.
//! - The new temperature is rounded at random: up with a chance equal to the fraction. So small
//!   flows are not lost, and on average no heat is made or lost.
//! - Air cells (material 0) also move `AIR_RATE` of the way toward the air temperature of their
//!   row (`Simulation::set_air_temperature`) in each tick. Other gases carry their heat when they
//!   move, because the movement pass swaps temperatures with the cells.
//!
//! # Phase changes
//!
//! After the flow, each cell checks the phase changes of its material (from the data files):
//! `melt` and `boil` at or above their temperature, `freeze` and `condense` below it. The cell
//! becomes the new material in place. It keeps its temperature and shade, gets a new life (for
//! materials that fade), and loses its motion. The cell and its 8 neighbors are marked for the
//! movement pass, and the chunk gets a new version (the renderer shows the change). There is no
//! latent heat: a phase change does not use or give heat.
//!
//! # Which chunks
//!
//! The heat pass works on the chunks that the movement pass worked on in this tick
//! (`World::worked_chunks`). A chunk stays "heat-active" while a temperature in it changed in the
//! last `QUIET_TICKS` ticks, or while one of its edges differs from a neighbor chunk that also
//! works. A heat-active chunk puts one cell that does not move (air or a solid, if it has one) into
//! its dirty rectangle. So the movement pass works on it in the next tick, and the heat pass sees
//! it again. A chunk with no change goes to heat-sleep: it adds no mark.
//!
//! So heat-active chunks follow the rules of all work in `world.rs`: only chunks inside a
//! simulation area work; a chunk outside waits in the paused set with its mark, is saved with it,
//! and continues when an anchor comes near. Writing a cell (`set_cell`, `fill_chunk`, `paint`)
//! marks the chunk dirty, so it also wakes the heat of the chunk.
//!
//! Heat crosses a chunk edge only when both chunks work in this tick. If a neighbor does not work
//! and its edge differs by `MIN_DIFF` or more, it gets a dirty mark next to the edge (it works in
//! the next tick), and no heat crosses that edge in this tick. So heat is never lost at an edge.
//! Cells outside the world do not take or give heat.
//!
//! A chunk that does not change is never written, so a chunk that is at rest at the temperatures
//! of its chunk source stays pristine. New chunks from the source start at the default
//! temperatures of their materials; air at `DEFAULT_TEMPERATURE` is in balance while the air
//! temperature of its rows is also `DEFAULT_TEMPERATURE` (see `docs/design/requests/heat.md`).
//!
//! # Renderer
//!
//! A temperature change alone does not always give the chunk a new version (the renderer would
//! upload the chunk again in every tick). Only glowing cells matter to the picture: the pass adds
//! the largest change of cells at or above `GLOW_MIN` to `Chunk::glow_drift`, and sets a new
//! version when it reaches `GLOW_STEP`. So the picture of a glowing cell is at most `GLOW_STEP`
//! degrees behind.
//!
//! # Parallel passes
//!
//! 1. Each working chunk copies its 4 edges (temperatures and materials) into
//!    `Chunk::heat_edge_temp` and `Chunk::heat_edge_mat`, and sets `Chunk::heat_stamp`.
//! 2. Each working chunk computes its new temperatures and phase changes, and writes them into its
//!    own cells. It reads its own cells, the edge copies of working neighbors, and the cells of
//!    neighbors that do not work (nobody writes those in the pass). So two jobs never touch the
//!    same data. Random numbers come from the chunk position and the tick. The result is the same
//!    for any number of threads.
//! 3. The dirty marks of all jobs are added to the chunks, in the order of the work list.

use crate::chunk::{Chunk, FLAG_PARITY, LocalRect};
use crate::world::World;
use foundry_content::{MaterialTable, Phase};
use foundry_core::{CHUNK_AREA, CHUNK_MASK, CHUNK_SHIFT, ChunkPos, DEFAULT_TEMPERATURE, MaterialId, Rng};
use rayon::prelude::*;
use std::cell::RefCell;
use std::ptr::{addr_of, addr_of_mut};

/// Heat conduction in one tick for a conductivity of 1.0 (see the module documentation).
pub const CONDUCT_SCALE: f32 = 0.1;
/// One neighbor changes a cell by at most this part of the temperature difference in one tick.
/// With 4 neighbors the total stays at or below 0.8, so the update is stable.
pub const MAX_STEP: f32 = 0.2;
/// Air cells move this part of the way toward the air temperature of their row in each tick.
pub const AIR_RATE: f32 = 1.0 / 256.0;
/// Two neighbor cells that differ by less than this (°C) do not exchange heat.
pub const MIN_DIFF: i16 = 2;
/// A chunk with no temperature change for this many ticks is at rest for heat.
pub const QUIET_TICKS: u8 = 30;
/// Cells at or above this temperature (°C) glow in the renderer.
pub const GLOW_MIN: i16 = 400;
/// The heat pass gives a chunk a new version when its glowing cells may have changed by this much
/// (°C) since the last new version.
pub const GLOW_STEP: u16 = 8;
/// The lowest temperature a cell can have (°C).
pub const MIN_TEMP: i16 = -273;
/// The highest temperature a cell can have (°C).
pub const MAX_TEMP: i16 = 30_000;

/// Salt for the random numbers of the heat pass.
const HEAT_SALT: u64 = 0x6865_6174_2070_6173;

/// Heat values of one material, for the per-cell loop.
#[derive(Debug, Clone, Copy, PartialEq)]
struct MatHeat {
    /// Conductance: `min(CONDUCT_SCALE × conductivity, MAX_STEP × heat capacity)`.
    g: f32,
    /// 1 / heat capacity.
    inv_c: f32,
    /// The cell changes at or above this temperature (melt or boil). `i16::MAX`: never.
    up: i16,
    /// The cell changes below this temperature (freeze or condense). `i16::MIN`: never.
    down: i16,
}

/// The heat values of all materials, made from the material table.
#[derive(Debug, Clone)]
pub struct HeatTable {
    mats: Vec<MatHeat>,
    /// What a cell becomes at or above `MatHeat::up`.
    up_into: Vec<MaterialId>,
    /// What a cell becomes below `MatHeat::down`.
    down_into: Vec<MaterialId>,
    /// Air or solid: the movement pass does not move such a cell. The dirty mark that keeps a
    /// chunk heat-active goes on such a cell when the chunk has one.
    still: Vec<bool>,
    life: Vec<Option<(u8, u8)>>,
}

impl HeatTable {
    pub fn new(mats: &MaterialTable) -> Self {
        let n = mats.len();
        let mut t = HeatTable {
            mats: Vec::with_capacity(n),
            up_into: Vec::with_capacity(n),
            down_into: Vec::with_capacity(n),
            still: Vec::with_capacity(n),
            life: mats.life.clone(),
        };
        for m in 0..n {
            let c = mats.heat_capacity[m].max(0.01);
            let g = (CONDUCT_SCALE * mats.conductivity[m].max(0.0)).min(MAX_STEP * c);
            // Melt and boil: the lower one comes first when the cell warms up.
            let mut up = (i16::MAX, MaterialId(m as u16));
            for ch in [mats.melt[m], mats.boil[m]].into_iter().flatten() {
                if ch.at < up.0 && ch.into.index() != m {
                    up = (ch.at, ch.into);
                }
            }
            // Freeze and condense: the higher one comes first when the cell cools down.
            let mut down = (i16::MIN, MaterialId(m as u16));
            for ch in [mats.freeze[m], mats.condense[m]].into_iter().flatten() {
                if ch.at > down.0 && ch.into.index() != m {
                    down = (ch.at, ch.into);
                }
            }
            t.mats.push(MatHeat { g, inv_c: 1.0 / c, up: up.0, down: down.0 });
            t.up_into.push(up.1);
            t.down_into.push(down.1);
            t.still.push(m == 0 || matches!(mats.phase[m], Phase::Empty | Phase::Solid));
        }
        t
    }

    /// The material that a cell of material `m` at temperature `t` changes into, if it changes phase.
    pub fn phase_change(&self, m: MaterialId, t: i16) -> Option<MaterialId> {
        let e = self.mats[m.index()];
        if t >= e.up {
            Some(self.up_into[m.index()])
        } else if t < e.down {
            Some(self.down_into[m.index()])
        } else {
            None
        }
    }

    /// The conductance `g` of a material (see the module documentation).
    pub fn conductance(&self, m: MaterialId) -> f32 {
        self.mats[m.index()].g
    }
}

/// What the heat pass did in one tick (for tests and measurements).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HeatStats {
    /// Chunks the pass worked on.
    pub chunks: u32,
    /// Cells whose temperature changed.
    pub changed_cells: u32,
    /// Cells that changed phase.
    pub phase_changes: u32,
    /// Chunks that stay heat-active (they marked themselves for the next tick).
    pub active_chunks: u32,
}

/// Run the heat pass on the world. Called after movement and particles.
/// `air_temperature[y]` is the air temperature of cell row y.
pub fn step(
    world: &mut World,
    mats: &MaterialTable,
    air_temperature: &[i16],
    tick: u64,
    seed: u64,
    stamp: u64,
    pool: Option<&rayon::ThreadPool>,
) -> HeatStats {
    let work: Vec<(ChunkPos, LocalRect)> = world.worked_chunks().iter().map(|&c| (c, LocalRect::FULL)).collect();
    if work.is_empty() {
        return HeatStats::default();
    }
    // The movement pass made every neighbor of a working chunk live. A neighbor that is not live
    // (not expected) gets a null pointer: no heat crosses that edge.
    let mut hoods = Vec::with_capacity(work.len());
    let mut missing = Vec::new();
    world.hood_ptrs(&work, &mut hoods, &mut missing);
    let ptrs = Ptrs(hoods);
    let table = HeatTable::new(mats);
    let last_air = air_temperature.last().copied().unwrap_or(DEFAULT_TEMPERATURE);
    let parity = (tick & 1) as u8;

    // Pass 1: copy the edges.
    let copy = |i: usize| {
        // SAFETY: each job writes only its own chunk; no job reads other chunks in this pass.
        unsafe { copy_edges(ptrs.get(i)[4], stamp) }
    };
    match pool {
        Some(p) => p.install(|| (0..work.len()).into_par_iter().for_each(copy)),
        None => (0..work.len()).into_par_iter().for_each(copy),
    }

    // Pass 2: flow and phase changes.
    let run = |i: usize| -> JobOut {
        let c = work[i].0;
        let mut air = [0.0f32; 64];
        for (y, a) in air.iter_mut().enumerate() {
            let row = (c.y << CHUNK_SHIFT) + y as i32;
            *a = air_temperature.get(row.max(0) as usize).copied().unwrap_or(last_air) as f32;
        }
        let job = ChunkJob {
            table: &table,
            ptrs: ptrs.get(i),
            air,
            stamp,
            parity,
            rng: Rng::for_chunk(seed, tick, c, HEAT_SALT),
        };
        // SAFETY: see the module documentation (pass 2) and `ChunkJob::run`.
        with_scratch(|s| unsafe { job.run(s) })
    };
    let outs: Vec<JobOut> = match pool {
        Some(p) => p.install(|| (0..work.len()).into_par_iter().map(run).collect()),
        None => (0..work.len()).into_par_iter().map(run).collect(),
    };

    // Pass 3: dirty marks, in the order of the work list.
    let mut stats = HeatStats { chunks: work.len() as u32, ..HeatStats::default() };
    for (i, out) in outs.iter().enumerate() {
        stats.changed_cells += out.changed_cells;
        stats.phase_changes += out.phase_changes;
        stats.active_chunks += out.active as u32;
        let c = work[i].0;
        for (s, mark) in out.marks.iter().enumerate() {
            let p = ptrs.get(i)[s];
            if mark.is_empty() || p.is_null() {
                continue;
            }
            let pos = ChunkPos::new(c.x + (s as i32 % 3) - 1, c.y + (s as i32 / 3) - 1);
            // SAFETY: no job runs now, and `p` points to the live chunk at `pos` (no chunk was
            // added or removed since `hood_ptrs`).
            unsafe {
                (*p).dirty.add_rect(*mark);
                world.queue_raw(pos, p);
            }
        }
    }
    stats
}

/// The 3 × 3 chunk pointers of each job. Null for chunks outside the world.
struct Ptrs(Vec<[*mut Chunk; 9]>);

// SAFETY: the jobs use the pointers only as the module documentation says (passes 1 and 2).
unsafe impl Sync for Ptrs {}

impl Ptrs {
    /// The pointers of job `i`. (A method, so that closures capture the whole `Sync` wrapper.)
    #[inline]
    fn get(&self, i: usize) -> [*mut Chunk; 9] {
        self.0[i]
    }
}

/// Copy the 4 edges of a chunk into its `heat_edge_*` fields and set its `heat_stamp` (pass 1).
///
/// # Safety
/// `p` must point to a live chunk that no other thread uses during the call.
unsafe fn copy_edges(p: *mut Chunk, stamp: u64) {
    // SAFETY: see the function documentation. Only fields are borrowed, never the whole chunk.
    unsafe {
        let temp = &*addr_of!((*p).temp);
        let mat = &*addr_of!((*p).mat);
        let et = &mut *addr_of_mut!((*p).heat_edge_temp);
        let em = &mut *addr_of_mut!((*p).heat_edge_mat);
        for k in 0..64 {
            for (side, i) in [k, 63 * 64 + k, k * 64, k * 64 + 63].into_iter().enumerate() {
                et[side * 64 + k] = temp[i];
                em[side * 64 + k] = mat[i];
            }
        }
        *addr_of_mut!((*p).heat_stamp) = stamp;
    }
}

/// Width of a row in the scratch arrays: 64 cells plus one cell on each side.
const W: usize = 66;

/// Per-thread arrays for one chunk job. Kept between jobs, so the pass does not allocate.
struct Scratch {
    /// Temperatures of the chunk plus a border of one cell (the neighbors' edges), row by row
    /// with row width `W`. Cell (x, y) of the chunk is at `(y + 1) * W + x + 1`.
    t: [f32; W * W],
    /// Conductance `g` of the same cells. 0 on a border where no heat may cross.
    g: [f32; W * W],
    /// 1 / heat capacity of each chunk cell (index `y * 64 + x`).
    inv_c: [f32; CHUNK_AREA],
    /// `MatHeat::up` and `MatHeat::down` of each chunk cell.
    up: [i16; CHUNK_AREA],
    down: [i16; CHUNK_AREA],
    /// The new temperature of each chunk cell.
    new_t: [i16; CHUNK_AREA],
}

thread_local! {
    static SCRATCH: RefCell<Option<Box<Scratch>>> = const { RefCell::new(None) };
}

fn with_scratch<R>(f: impl FnOnce(&mut Scratch) -> R) -> R {
    SCRATCH.with(|cell| {
        let mut slot = cell.borrow_mut();
        let s = slot.get_or_insert_with(|| {
            Box::new(Scratch {
                t: [0.0; W * W],
                g: [0.0; W * W],
                inv_c: [0.0; CHUNK_AREA],
                up: [0; CHUNK_AREA],
                down: [0; CHUNK_AREA],
                new_t: [0; CHUNK_AREA],
            })
        });
        f(s)
    })
}

/// One side of a chunk: the neighbor there, and where its edge cells are.
struct Side {
    /// Hood slot of the neighbor (row by row, 4 is the chunk itself).
    slot: usize,
    /// Start of the neighbor's facing edge in its `heat_edge_*` arrays.
    copy: usize,
}

/// Up, down, left, right.
const SIDES: [Side; 4] = [Side { slot: 1, copy: 64 }, Side { slot: 7, copy: 0 }, Side { slot: 3, copy: 192 }, Side { slot: 5, copy: 128 }];

impl Side {
    /// Index of the k-th own edge cell on this side (0 to 4095).
    #[inline]
    fn own(&self, k: usize) -> usize {
        match self.slot {
            1 => k,
            7 => 63 * 64 + k,
            3 => k * 64,
            _ => k * 64 + 63,
        }
    }

    /// Index of the neighbor cell that faces the k-th own edge cell, in the neighbor chunk.
    #[inline]
    fn far(&self, k: usize) -> usize {
        match self.slot {
            1 => 63 * 64 + k,
            7 => k,
            3 => k * 64 + 63,
            _ => k * 64,
        }
    }

    /// Index of the k-th border cell on this side in the scratch arrays `t` and `g`.
    #[inline]
    fn border(&self, k: usize) -> usize {
        match self.slot {
            1 => k + 1,
            7 => 65 * W + k + 1,
            3 => (k + 1) * W,
            _ => (k + 1) * W + 65,
        }
    }
}

/// What one chunk job hands back.
struct JobOut {
    /// Cells to update in the next tick, for each of the 9 chunks around the job's chunk.
    marks: [LocalRect; 9],
    changed_cells: u32,
    phase_changes: u32,
    /// The chunk stays heat-active.
    active: bool,
}

/// The work on one chunk in pass 2.
struct ChunkJob<'a> {
    table: &'a HeatTable,
    ptrs: [*mut Chunk; 9],
    /// Air temperature of the 64 rows of the chunk.
    air: [f32; 64],
    stamp: u64,
    parity: u8,
    rng: Rng,
}

impl ChunkJob<'_> {
    /// # Safety
    /// Pass 1 must be done for all working chunks. During the call, no other thread may write the
    /// center chunk, or read it other than `heat_stamp` and `heat_edge_*`, and no thread may write
    /// a neighbor chunk that did not do pass 1 in this tick.
    unsafe fn run(mut self, s: &mut Scratch) -> JobOut {
        let p = self.ptrs[4];
        let table = self.table;
        let mut out = JobOut { marks: [LocalRect::EMPTY; 9], changed_cells: 0, phase_changes: 0, active: false };
        // SAFETY: only this job uses these fields of the center chunk (see the function documentation).
        let (temp, mat) = unsafe { (&mut *addr_of_mut!((*p).temp), &*addr_of!((*p).mat)) };

        // The chunk's own cells.
        for y in 0..64 {
            let (src, dst) = (y * 64, (y + 1) * W + 1);
            for x in 0..64 {
                let e = table.mats[mat[src + x] as usize];
                s.t[dst + x] = temp[src + x] as f32;
                s.g[dst + x] = e.g;
                s.inv_c[src + x] = e.inv_c;
                s.up[src + x] = e.up;
                s.down[src + x] = e.down;
            }
        }

        // The border: the facing edges of the 4 neighbors.
        let mut edge_differs = false;
        for side in &SIDES {
            let q = self.ptrs[side.slot];
            // SAFETY: a neighbor with this tick's stamp did pass 1, and nobody writes its edge
            // copies now. A neighbor without it does not work in this tick, so nobody writes it.
            let working = !q.is_null() && unsafe { *addr_of!((*q).heat_stamp) } == self.stamp;
            if working {
                let (et, em) = unsafe { (&*addr_of!((*q).heat_edge_temp), &*addr_of!((*q).heat_edge_mat)) };
                for k in 0..64 {
                    let (b, tn) = (side.border(k), et[side.copy + k]);
                    let g = table.mats[em[side.copy + k] as usize].g;
                    s.t[b] = tn as f32;
                    s.g[b] = g;
                    let own = side.own(k);
                    edge_differs |= differs(temp[own], tn) && g.min(s.g[own_border(own)]) > 0.0;
                }
                continue;
            }
            for k in 0..64 {
                let b = side.border(k);
                s.t[b] = 0.0;
                s.g[b] = 0.0;
            }
            if q.is_null() {
                continue;
            }
            // A neighbor that does not work: if its edge differs, it works in the next tick.
            let (nt, nm) = unsafe { (&*addr_of!((*q).temp), &*addr_of!((*q).mat)) };
            for k in 0..64 {
                let (own, far) = (side.own(k), side.far(k));
                if differs(temp[own], nt[far]) && table.mats[nm[far] as usize].g.min(s.g[own_border(own)]) > 0.0 {
                    out.marks[side.slot].add_point((far as i32) & CHUNK_MASK, (far as i32) >> CHUNK_SHIFT);
                    break;
                }
            }
        }

        // The flow.
        let seed = self.rng.next_u32();
        for y in 0..64 {
            let r = (y + 1) * W + 1;
            let o = y * 64;
            flow_row(
                FlowRow {
                    t: &s.t[r - W - 1..r + W + 65],
                    g: &s.g[r - W - 1..r + W + 65],
                    inv_c: &s.inv_c[o..o + 64],
                    mat: &mat[o..o + 64],
                    air: self.air[y],
                    seed,
                    first: o as u32,
                },
                &mut s.new_t[o..o + 64],
            );
        }

        // Write the rows that changed, and note the change of glowing cells.
        let mut hot = 0i32;
        for y in 0..64 {
            let o = y * 64;
            let (new, old) = (&s.new_t[o..o + 64], &mut temp[o..o + 64]);
            if new == &old[..] {
                continue;
            }
            for (&a, &b) in old.iter().zip(new) {
                if a != b {
                    out.changed_cells += 1;
                    if a.max(b) >= GLOW_MIN {
                        hot = hot.max((a as i32 - b as i32).abs());
                    }
                }
            }
            old.copy_from_slice(new);
        }

        // Phase changes.
        for y in 0..64 {
            let o = y * 64;
            let (t, up, down) = (&temp[o..o + 64], &s.up[o..o + 64], &s.down[o..o + 64]);
            let mut hit = false;
            for x in 0..64 {
                hit |= (t[x] >= up[x]) | (t[x] < down[x]);
            }
            if !hit {
                continue;
            }
            for x in 0..64 {
                if t[x] >= up[x] || t[x] < down[x] {
                    // SAFETY: as above; the cell is in the center chunk.
                    unsafe { self.change_phase(&mut out, x as i32, y as i32, t[x] >= up[x]) };
                }
            }
        }

        // Heat activity, pristine and version.
        // SAFETY: as above; only this job uses these fields of the center chunk.
        unsafe {
            let changed = out.changed_cells > 0 || out.phase_changes > 0;
            let quiet = &mut *addr_of_mut!((*p).heat_quiet);
            *quiet = if changed { 0 } else { quiet.saturating_add(1).min(QUIET_TICKS) };
            if changed {
                *addr_of_mut!((*p).pristine) = false;
            }
            let drift = &mut *addr_of_mut!((*p).glow_drift);
            *drift = drift.saturating_add(hot.min(u16::MAX as i32) as u16);
            let version = &mut *addr_of_mut!((*p).version);
            if *drift >= GLOW_STEP {
                *version = self.stamp;
            }
            if *version == self.stamp {
                *drift = 0;
            }
            out.active = *quiet < QUIET_TICKS || edge_differs;
            if out.active {
                let mat = &*addr_of!((*p).mat);
                let i = mat.iter().position(|&m| table.still[m as usize]).unwrap_or(0) as i32;
                out.marks[4].add_point(i & CHUNK_MASK, i >> CHUNK_SHIFT);
            }
        }
        out
    }

    /// Change the phase of the center chunk cell (x, y): `up` is melt or boil, else freeze or condense.
    ///
    /// # Safety
    /// As `run`.
    unsafe fn change_phase(&mut self, out: &mut JobOut, x: i32, y: i32, up: bool) {
        let p = self.ptrs[4];
        let i = ((y << CHUNK_SHIFT) | x) as usize;
        // SAFETY: see `run`.
        unsafe {
            let mat = &mut *addr_of_mut!((*p).mat);
            let m = mat[i] as usize;
            let into = if up { self.table.up_into[m] } else { self.table.down_into[m] };
            mat[i] = into.0;
            (*addr_of_mut!((*p).life))[i] = match self.table.life[into.index()] {
                Some((lo, hi)) => lo + self.rng.below((hi - lo) as u32 + 1) as u8,
                None => 0,
            };
            (*addr_of_mut!((*p).motion))[i] = 0;
            let flags = &mut (*addr_of_mut!((*p).flags))[i];
            // The cell counts as not updated in the next tick, so it can move at once.
            *flags = (*flags & !FLAG_PARITY) | self.parity;
            *addr_of_mut!((*p).version) = self.stamp;
        }
        out.phase_changes += 1;
        // The cell and its 8 neighbors update in the next tick.
        if x > 0 && x < CHUNK_MASK && y > 0 && y < CHUNK_MASK {
            out.marks[4].add_rect(LocalRect { x0: x - 1, y0: y - 1, x1: x + 2, y1: y + 2 });
            return;
        }
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (nx, ny) = (x + dx, y + dy);
                let slot = (((ny >> CHUNK_SHIFT) + 1) * 3 + (nx >> CHUNK_SHIFT) + 1) as usize;
                if !self.ptrs[slot].is_null() {
                    out.marks[slot].add_point(nx & CHUNK_MASK, ny & CHUNK_MASK);
                }
            }
        }
    }
}

/// True if two neighbor temperatures differ enough to exchange heat.
#[inline(always)]
fn differs(a: i16, b: i16) -> bool {
    (a as i32 - b as i32).abs() >= MIN_DIFF as i32
}

/// Index in the scratch arrays `t` and `g` of the chunk cell with index `i` (0 to 4095).
#[inline(always)]
fn own_border(i: usize) -> usize {
    ((i >> 6) + 1) * W + (i & 63) + 1
}

/// The input of `flow_row`: one row of 64 cells.
struct FlowRow<'a> {
    /// Temperatures of the row above, the row and the row below, each `W` wide, starting one
    /// cell left of the row above (`3 * W - 1` values, so the last row may end early).
    t: &'a [f32],
    /// Conductance of the same cells.
    g: &'a [f32],
    inv_c: &'a [f32],
    mat: &'a [u16],
    /// Air temperature of the row.
    air: f32,
    seed: u32,
    /// Index of the first cell of the row in the chunk (for the random numbers).
    first: u32,
}

/// Compute the new temperatures of one row of 64 cells. The loop has no branches, so the compiler
/// can use SIMD instructions.
#[inline]
fn flow_row(r: FlowRow, out: &mut [i16]) {
    const MIN: f32 = MIN_DIFF as f32;
    let (t, g) = (r.t, r.g);
    // Offsets of the cells in `t` and `g`: the cell itself is at `W + 1 + x`.
    let (tu, tl, tc, tr, td) = (&t[1..65], &t[W..W + 64], &t[W + 1..W + 65], &t[W + 2..W + 66], &t[2 * W + 1..2 * W + 65]);
    let (gu, gl, gc, gr, gd) = (&g[1..65], &g[W..W + 64], &g[W + 1..W + 65], &g[W + 2..W + 66], &g[2 * W + 1..2 * W + 65]);
    let (inv_c, mat, out) = (&r.inv_c[..64], &r.mat[..64], &mut out[..64]);
    for x in 0..64 {
        let c = tc[x];
        let k = gc[x];
        let flow = |gn: f32, tn: f32| {
            let d = tn - c;
            if d.abs() >= MIN { k.min(gn) * d } else { 0.0 }
        };
        let sum = flow(gu[x], tu[x]) + flow(gl[x], tl[x]) + flow(gr[x], tr[x]) + flow(gd[x], td[x]);
        let relax = if mat[x] == 0 { AIR_RATE } else { 0.0 };
        let dt = sum * inv_c[x] + relax * (r.air - c);
        let v = c + (dt + unit(r.seed, r.first + x as u32)).floor();
        out[x] = v.clamp(MIN_TEMP as f32, MAX_TEMP as f32) as i32 as i16;
    }
}

/// A random number in `0.0..1.0` from a seed and a cell index (a hash, so it fits SIMD).
#[inline(always)]
fn unit(seed: u32, i: u32) -> f32 {
    let mut h = i.wrapping_mul(0x9E37_79B9) ^ seed;
    h ^= h >> 16;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 16;
    (h >> 8) as f32 * (1.0 / 16_777_216.0)
}

#[cfg(test)]
mod tests;
