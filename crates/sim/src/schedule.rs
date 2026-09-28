//! The parallel movement pass (technical design section 6.1).
//!
//! 1. Take this tick's work from the world's awake list: the dirty rectangle of each chunk that is
//!    inside a simulation area, sorted by (y, x). Chunks with no work leave the list. Chunks
//!    outside every simulation area wait in the paused set. The cost depends only on these chunks,
//!    never on the number of chunks in the world.
//! 2. Make sure every neighbor of a working chunk is live (made by the source or unpacked, in
//!    parallel), so jobs never need to allocate. Then look up the 3 × 3 chunk pointers of each job
//!    once for the whole tick.
//! 3. Fall pass: liquid cells that are already falling move straight down. One job per column of
//!    working chunks, from the bottom chunk to the top chunk. Falls are vertical, so a job touches
//!    only cells in its own column, and the jobs run in parallel. Because each column is done from
//!    the bottom up, a falling body moves as one piece across chunk borders (no gaps, no stripes).
//! 4. Run 4 passes. Each pass updates the chunks with one pair of (x mod 2, y mod 2). The jobs of
//!    one pass run in parallel (see the safety rule in `hood.rs`). The order of the 4 pairs
//!    changes from tick to tick, so no side and no direction across a chunk border is always first.
//! 5. After each pass, merge the dirty marks, "changed" and "touched" bits of all jobs, in
//!    (y, x) order. This keeps the result the same for any number of threads.
//! 6. Level pass: calm liquid cells near the end of a top row that found nothing to do (the passes
//!    collect them) look along the liquid surface for the nearest place to fall, however far away
//!    (at most `movement::MAX_LEVEL_SCAN` cells), and go toward it. One job per row of chunks,
//!    first the even rows, then the odd rows. A cell moves only sideways inside its own row of
//!    chunks, and a job reads only the cell rows of its chunk row and one cell row above and below,
//!    so jobs two chunk rows apart never touch the same cell. Without this pass, a top row more
//!    than 31 cells from a place to fall (the reach of a normal job) would rest, and wide liquid
//!    surfaces would stay in low steps.

use crate::chunk::{Chunk, FLAG_PARITY, LocalRect};
use crate::hood::Hood;
use crate::movement::{self, LevelCells, MAX_LEVEL_SCAN};
use crate::particles::Spawn;
use crate::react::ReactTable;
use crate::update::{fall_chunk, update_chunk};
use crate::world::{RawChunk, World};
use crate::{SimEvent, SimSettings};
use foundry_content::MaterialTable;
use foundry_core::{CHUNK_MASK, CHUNK_SHIFT, CellPos, ChunkPos, MaterialId, Rng};
use rayon::prelude::*;
use rustc_hash::FxHashMap;

/// The 3 × 3 chunk pointers of each job, for the whole tick. Null for chunks outside the world.
struct HoodPtrs(Vec<[*mut Chunk; 9]>);

// SAFETY: jobs use the pointers only under the rule in `hood.rs` (and the fall and level pass
// rules in the module documentation).
unsafe impl Sync for HoodPtrs {}

impl HoodPtrs {
    /// The pointers of job `i`. (A method, so that closures capture the whole `Sync` wrapper.)
    #[inline]
    fn get(&self, i: usize) -> [*mut Chunk; 9] {
        self.0[i]
    }
}

/// Read-only data every job needs.
#[derive(Clone, Copy)]
pub struct PassInput<'a> {
    pub mats: &'a MaterialTable,
    pub react: &'a ReactTable,
    pub settings: &'a SimSettings,
    /// False when there are too many particles: liquids do not splash.
    pub splash_ok: bool,
}

/// What a job hands back: marks, changed bits, touched bits, and the lists it filled.
struct JobOut {
    /// Index of the job's chunk in the work list.
    i: usize,
    marks: [LocalRect; 9],
    changed: u16,
    touched: u16,
    falling: [u64; 9],
    events: Vec<SimEvent>,
    spawns: Vec<Spawn>,
    levels: Vec<CellPos>,
    opened: Vec<CellPos>,
}

impl JobOut {
    fn from_hood(i: usize, hood: Hood) -> Self {
        JobOut {
            i,
            marks: hood.marks,
            changed: hood.changed,
            touched: hood.touched,
            falling: hood.falling,
            events: hood.events,
            spawns: hood.spawns,
            levels: hood.levels,
            opened: hood.opened,
        }
    }
}

/// Run the movement and reaction update for one tick. Returns the number of chunks that worked.
/// Events and new particles from the jobs are added to `events` and `spawns` in chunk order.
#[allow(clippy::too_many_arguments)]
pub fn movement_tick(
    world: &mut World,
    input: PassInput,
    tick: u64,
    seed: u64,
    stamp: u64,
    pool: Option<&rayon::ThreadPool>,
    events: &mut Vec<SimEvent>,
    spawns: &mut Vec<Spawn>,
) -> u32 {
    let mut work: Vec<(ChunkPos, LocalRect)> = Vec::new();
    world.take_work(&mut work);
    if work.is_empty() {
        return 0;
    }

    // Pointers to the neighbors. Neighbors that are not live yet are loaded first.
    let mut missing = Vec::new();
    let mut hoods = Vec::with_capacity(work.len());
    if !world.hood_ptrs(&work, &mut hoods, &mut missing) {
        world.load(&missing, true, pool);
        // Loading changes the maps, so look up all pointers again.
        missing.clear();
        let complete = world.hood_ptrs(&work, &mut hoods, &mut missing);
        debug_assert!(complete, "neighbors of working chunks are live: {missing:?}");
    }
    let ptrs = HoodPtrs(hoods);

    let parity = (tick & 1) as u8;
    let left_to_right = tick & 1 == 0;
    let outside = world.outside;

    // Step 3: the fall pass.
    let mut opened = fall_pass(world, &work, &ptrs, input, tick, seed, stamp, pool);

    // Step 4: the 4 passes. The order: x parity flips every 2 ticks, y parity every tick.
    let (flip_x, flip_y) = (((tick >> 1) & 1) as usize, (tick & 1) as usize);
    let mut levels: Vec<CellPos> = Vec::new();
    let mut jobs: Vec<usize> = Vec::with_capacity(work.len());
    for step in 0..4 {
        let k = (((step >> 1) ^ flip_y) << 1) | ((step & 1) ^ flip_x);
        jobs.clear();
        jobs.extend((0..work.len()).filter(|&i| {
            let c = work[i].0;
            ((c.y & 1) * 2 + (c.x & 1)) as usize == k
        }));
        if jobs.is_empty() {
            continue;
        }
        let run = |&i: &usize| -> JobOut {
            let (c, rect) = work[i];
            let rng = Rng::for_chunk(seed, tick, c, k as u64);
            // SAFETY: all chunks in this pass are 2 apart; see `hood.rs`.
            let mut hood = unsafe { Hood::new(ptrs.get(i), input, rng, parity, outside, c.origin()) };
            update_chunk(&mut hood, rect, left_to_right);
            JobOut::from_hood(i, hood)
        };
        let results: Vec<JobOut> = match pool {
            Some(p) => p.install(|| jobs.par_iter().map(run).collect()),
            None => jobs.par_iter().map(run).collect(),
        };
        for out in results {
            merge(world, &ptrs, work[out.i].0, out.i, &out, stamp);
            events.extend(out.events);
            spawns.extend(out.spawns);
            levels.extend(out.levels);
            opened.extend(out.opened);
        }
    }

    // Step 6: the level pass.
    if !levels.is_empty() || !opened.is_empty() {
        level_pass(world, input.mats, levels, opened, tick, stamp, pool);
    }
    work.len() as u32
}

/// Add the dirty marks, "changed", "touched" and falling-row bits of one job (center chunk `c`,
/// job `i`) to the chunks, and queue the marked chunks.
fn merge(world: &mut World, ptrs: &HoodPtrs, c: ChunkPos, i: usize, out: &JobOut, stamp: u64) {
    let (changed, touched) = (out.changed, out.touched);
    for (s, mark) in out.marks.iter().enumerate() {
        let p = ptrs.get(i)[s];
        if p.is_null() {
            continue;
        }
        let pos = ChunkPos::new(c.x + (s as i32 % 3) - 1, c.y + (s as i32 / 3) - 1);
        // SAFETY: no job runs now, and `p` points to the live chunk at `pos`. The pointers stay
        // valid for the whole tick, because no chunk is added or removed during the passes.
        unsafe {
            if !mark.is_empty() {
                (*p).dirty.add_rect(*mark);
                world.queue_raw(pos, p);
            }
            if changed & (1 << s) != 0 {
                (*p).version = stamp;
            }
            if touched & (1 << s) != 0 {
                (*p).pristine = false;
            }
            (*p).falling_rows |= out.falling[s];
        }
    }
}

/// Move the falling cells of all working chunks (step 3 in the module documentation).
/// Returns the places that top liquid cells left (for the level pass).
#[allow(clippy::too_many_arguments)]
fn fall_pass(
    world: &mut World,
    work: &[(ChunkPos, LocalRect)],
    ptrs: &HoodPtrs,
    input: PassInput,
    tick: u64,
    seed: u64,
    stamp: u64,
    pool: Option<&rayon::ThreadPool>,
) -> Vec<CellPos> {
    // Job indices sorted by column, and from the bottom up in each column.
    let mut order: Vec<usize> = (0..work.len()).collect();
    order.sort_unstable_by_key(|&i| (work[i].0.x, -work[i].0.y));
    let mut columns: Vec<&[usize]> = Vec::new();
    let mut rest = &order[..];
    while let Some(&first) = rest.first() {
        let x = work[first].0.x;
        let n = rest.iter().take_while(|&&i| work[i].0.x == x).count();
        columns.push(&rest[..n]);
        rest = &rest[n..];
    }
    let parity = (tick & 1) as u8;
    let left_to_right = tick & 1 == 0;
    let outside = world.outside;
    let run = |column: &&[usize]| -> Vec<JobOut> {
        let mut out = Vec::new();
        for &i in column.iter() {
            let (c, rect) = work[i];
            // The fall pass does not use random numbers, but a hood needs a generator.
            let rng = Rng::for_chunk(seed, tick, c, 4);
            // SAFETY: a falling cell moves straight down, so this job reads and writes only cells
            // in its own column of chunks. Marks go to the hood and are merged after the pass.
            let mut hood = unsafe { Hood::new(ptrs.get(i), input, rng, parity, outside, c.origin()) };
            if fall_chunk(&mut hood, rect, left_to_right) {
                out.push(JobOut::from_hood(i, hood));
            }
        }
        out
    };
    let results: Vec<Vec<JobOut>> = match pool {
        Some(p) => p.install(|| columns.par_iter().map(run).collect()),
        None => columns.par_iter().map(run).collect(),
    };
    let mut opened = Vec::new();
    for out in results.into_iter().flatten() {
        merge(world, ptrs, work[out.i].0, out.i, &out, stamp);
        opened.extend(out.opened);
    }
    opened
}

/// Raw pointers to the chunks along the rows that the level pass reads.
struct RowChunks {
    map: FxHashMap<ChunkPos, RawChunk>,
    outside: MaterialId,
    height_cells: i32,
}

// SAFETY: jobs use the pointers only under the level pass rule (module documentation).
unsafe impl Sync for RowChunks {}

impl RowChunks {
    /// Material of a world cell. Outside the world and in chunks that are not in `map` (packed or
    /// not in memory): `outside`, so the level pass does not look past them.
    ///
    /// # Safety
    /// No other thread may write the cell during the read (the level pass rule).
    #[inline]
    unsafe fn mat(&self, x: i32, y: i32) -> MaterialId {
        if y < 0 || y >= self.height_cells {
            return self.outside;
        }
        match self.map.get(&ChunkPos::new(x >> CHUNK_SHIFT, y >> CHUNK_SHIFT)) {
            Some(RawChunk::Live(p)) => {
                let i = (((y & CHUNK_MASK) << CHUNK_SHIFT) | (x & CHUNK_MASK)) as usize;
                // SAFETY: a live chunk pointer, valid for the whole tick; element access only.
                MaterialId(unsafe { *std::ptr::addr_of!((**p).mat).cast::<u16>().add(i) })
            }
            Some(RawChunk::Air) => MaterialId::AIR,
            _ => self.outside,
        }
    }

    /// The live chunk and the cell index of a world cell, if the chunk has cells in memory.
    #[inline]
    fn cell(&self, p: CellPos) -> Option<(*mut Chunk, usize)> {
        if p.y < 0 || p.y >= self.height_cells {
            return None;
        }
        match self.map.get(&p.chunk()) {
            Some(RawChunk::Live(c)) => Some((*c, p.local_index())),
            _ => None,
        }
    }
}

/// The cells one level pass job may use: it reads the cell rows of chunk row `cy` and one row
/// above and below, and writes the cell rows of chunk row `cy` (see the module documentation).
struct LevelJob<'a> {
    chunks: &'a RowChunks,
    mats: &'a MaterialTable,
    cy: i32,
    parity: u8,
    stamp: u64,
    /// Cells to update in the next tick (the marks are added after the pass).
    wake: Vec<CellPos>,
}

impl LevelCells for LevelJob<'_> {
    fn mats(&self) -> &MaterialTable {
        self.mats
    }

    fn mat(&self, p: CellPos) -> MaterialId {
        if (p.y >> CHUNK_SHIFT) - self.cy > 1 || self.cy - (p.y >> CHUNK_SHIFT) > 1 {
            return self.chunks.outside;
        }
        // SAFETY: rows the job may read (see `LevelJob`).
        unsafe { self.chunks.mat(p.x, p.y) }
    }

    fn motion(&self, p: CellPos) -> u8 {
        match self.chunks.cell(p) {
            // SAFETY: as in `mat`; the row check is in `writes_row` for writes, and a motion byte
            // is only read for cells the job also reads the material of.
            Some((c, i)) if (p.y >> CHUNK_SHIFT) - self.cy <= 1 && self.cy - (p.y >> CHUNK_SHIFT) <= 1 => unsafe {
                *std::ptr::addr_of!((*c).motion).cast::<u8>().add(i)
            },
            _ => 0,
        }
    }

    fn set_motion(&mut self, p: CellPos, v: u8) {
        if !self.writes_row(p.y) {
            return;
        }
        if let Some((c, i)) = self.chunks.cell(p) {
            // SAFETY: a cell row of this job's chunk row; no other job reads or writes it.
            unsafe { *std::ptr::addr_of_mut!((*c).motion).cast::<u8>().add(i) = v }
        }
    }

    fn writes_row(&self, y: i32) -> bool {
        y >> CHUNK_SHIFT == self.cy
    }

    fn swap(&mut self, a: CellPos, b: CellPos) -> bool {
        if !self.writes_row(a.y) || !self.writes_row(b.y) {
            return false;
        }
        let (Some((ca, ia)), Some((cb, ib))) = (self.chunks.cell(a), self.chunks.cell(b)) else { return false };
        // SAFETY: both cells are in this job's chunk row, which no other job reads or writes, and
        // they are two different cells.
        unsafe {
            use std::ptr::{addr_of_mut, swap};
            swap(addr_of_mut!((*ca).mat).cast::<u16>().add(ia), addr_of_mut!((*cb).mat).cast::<u16>().add(ib));
            swap(addr_of_mut!((*ca).temp).cast::<i16>().add(ia), addr_of_mut!((*cb).temp).cast::<i16>().add(ib));
            swap(addr_of_mut!((*ca).shade).cast::<u8>().add(ia), addr_of_mut!((*cb).shade).cast::<u8>().add(ib));
            swap(addr_of_mut!((*ca).life).cast::<u8>().add(ia), addr_of_mut!((*cb).life).cast::<u8>().add(ib));
            swap(addr_of_mut!((*ca).motion).cast::<u8>().add(ia), addr_of_mut!((*cb).motion).cast::<u8>().add(ib));
            for (c, i) in [(ca, ia), (cb, ib)] {
                let f = addr_of_mut!((*c).flags).cast::<u8>().add(i);
                *f = (*f & !FLAG_PARITY) | self.parity;
                (*c).version = self.stamp;
                (*c).pristine = false;
            }
        }
        self.wake.push(a);
        self.wake.push(b);
        true
    }

    fn wake(&mut self, p: CellPos) {
        self.wake.push(p);
    }
}

/// The level pass (step 6 in the module documentation). `levels`: cells that may walk.
/// `opened`: places that top cells left in this tick; the far ends of the top row above them are
/// woken, so they find the new place to fall.
#[allow(clippy::too_many_arguments)]
fn level_pass(world: &mut World, mats: &MaterialTable, levels: Vec<CellPos>, opened: Vec<CellPos>, tick: u64, stamp: u64, pool: Option<&rayon::ThreadPool>) {
    // By chunk row, then from left to right (or right to left on odd ticks).
    let left_to_right = tick & 1 == 0;
    let mut items: Vec<(CellPos, bool)> = levels.into_iter().map(|p| (p, false)).chain(opened.into_iter().map(|p| (p, true))).collect();
    items.sort_unstable_by_key(|&(p, o)| (p.y >> CHUNK_SHIFT, if left_to_right { p.x } else { -p.x }, p.y, o));
    items.dedup();
    let mut rows: Vec<&[(CellPos, bool)]> = Vec::new();
    let mut rest = &items[..];
    while let Some(first) = rest.first() {
        let n = rest.iter().take_while(|(p, _)| p.y >> CHUNK_SHIFT == first.0.y >> CHUNK_SHIFT).count();
        rows.push(&rest[..n]);
        rest = &rest[n..];
    }

    // The chunks that the jobs may read: for each chunk row with items, the chunks from the
    // leftmost item to the rightmost item plus the scan distance, in that row and the rows next to it.
    let scan_chunks = MAX_LEVEL_SCAN / 64 + 1;
    let mut map = FxHashMap::default();
    for row in &rows {
        let cy = row[0].0.y >> CHUNK_SHIFT;
        let (x0, x1) = row.iter().fold((i32::MAX, i32::MIN), |(a, b), (p, _)| (a.min(p.x >> CHUNK_SHIFT), b.max(p.x >> CHUNK_SHIFT)));
        for y in cy - 1..=cy + 1 {
            for x in x0 - scan_chunks..=x1 + scan_chunks {
                let c = ChunkPos::new(x, y);
                map.entry(c).or_insert_with(|| world.raw_chunk(c));
            }
        }
    }
    let chunks = RowChunks { map, outside: world.outside, height_cells: world.height_cells() };

    let parity = (tick & 1) as u8;
    let mut wakes: Vec<CellPos> = Vec::new();
    for row_parity in 0..2 {
        let jobs: Vec<&[(CellPos, bool)]> = rows.iter().copied().filter(|r| (r[0].0.y >> CHUNK_SHIFT) & 1 == row_parity).collect();
        if jobs.is_empty() {
            continue;
        }
        let run = |row: &&[(CellPos, bool)]| -> Vec<CellPos> {
            let cy = row[0].0.y >> CHUNK_SHIFT;
            let mut job = LevelJob { chunks: &chunks, mats, cy, parity, stamp, wake: Vec::new() };
            for &(p, opened) in row.iter() {
                if opened {
                    movement::wake_row_ends(&mut job, p);
                } else {
                    movement::level(&mut job, p);
                }
            }
            job.wake
        };
        let results: Vec<Vec<CellPos>> = match pool {
            Some(p) => p.install(|| jobs.par_iter().map(run).collect()),
            None => jobs.par_iter().map(run).collect(),
        };
        wakes.extend(results.into_iter().flatten());
    }
    // After both halves: waking may load chunks, and the jobs hold pointers.
    for p in wakes {
        world.mark_dirty_around(p);
    }
}
