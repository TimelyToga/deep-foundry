//! The parallel movement pass (technical design section 6.1).
//!
//! 1. Take the dirty rectangle of each chunk as this tick's work. Chunks with no work sleep.
//! 2. Make sure every neighbor of a working chunk exists, so jobs never need to allocate.
//! 3. Fall pass: cells that are already falling move straight down. One job per column of
//!    chunks, from the bottom chunk to the top chunk. Falls are vertical, so a job touches only
//!    cells in its own column, and the jobs run in parallel. Because each column is done from
//!    the bottom up, a falling body moves as one piece across chunk borders (no gaps, no stripes).
//! 4. Run 4 passes. Each pass updates the chunks with one pair of (x mod 2, y mod 2).
//!    The jobs of one pass run in parallel (see the safety rule in `hood.rs`).
//!    The order of the 4 pairs changes from tick to tick, so no side and no direction across a
//!    chunk border is always first.
//! 5. After each pass, merge the dirty marks and "changed" bits of all jobs, in chunk order.
//!    This keeps the result the same for any number of threads.
//! 6. Level pass: calm liquid cells at the end of a top row that found nothing to do (the
//!    passes collect them) walk toward the nearest place to fall on the open surface, however
//!    far away. One job per row of chunks, first the even rows, then the odd rows. A cell moves
//!    only sideways inside its own row, and a job reads only the rows of its chunks and one row
//!    above and below, so jobs two chunk rows apart never touch the same cell. Without this pass,
//!    a top row more than 31 cells from a place to fall (the reach of a normal job) would rest,
//!    and wide liquid surfaces would stay in low steps.

use crate::chunk::{Chunk, LocalRect};
use crate::hood::Hood;
use crate::update::{fall_chunk, level_cells, update_chunk};
use crate::particles::Spawn;
use crate::{SimEvent, SimSettings};
use crate::react::ReactTable;
use crate::world::World;
use foundry_content::MaterialTable;
use foundry_core::{CellPos, ChunkPos, MaterialId, Rng};
use rayon::prelude::*;

/// Raw pointers to all chunks, for one pass. Null for chunks that do not exist.
struct ChunkPtrs(Vec<*mut Chunk>);

// SAFETY: jobs use the pointers only under the rule in `hood.rs`.
unsafe impl Sync for ChunkPtrs {}

impl ChunkPtrs {
    fn new(world: &mut World) -> Self {
        Self(world.chunks.iter_mut().map(|c| c.as_deref_mut().map_or(std::ptr::null_mut(), |c| c as *mut Chunk)).collect())
    }

    /// Material of a world cell. Outside the world: `outside`. A chunk that does not exist is air.
    ///
    /// # Safety
    /// No other thread may write the cell during the read (the level pass rule in the module
    /// documentation).
    unsafe fn mat(&self, w: i32, h: i32, outside: MaterialId, x: i32, y: i32) -> MaterialId {
        if x < 0 || y < 0 || x >= w * 64 || y >= h * 64 {
            return outside;
        }
        let p = self.0[((y >> 6) * w + (x >> 6)) as usize];
        if p.is_null() {
            return MaterialId::AIR;
        }
        let i = (((y & 63) << 6) | (x & 63)) as usize;
        // SAFETY: valid chunk pointer; element access only; see the function documentation.
        MaterialId(unsafe { *std::ptr::addr_of!((*p).mat).cast::<u16>().add(i) })
    }

    fn hood(&self, cx: i32, cy: i32, w: i32, h: i32) -> [*mut Chunk; 9] {
        let mut out = [std::ptr::null_mut(); 9];
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (x, y) = (cx + dx, cy + dy);
                if x >= 0 && y >= 0 && x < w && y < h {
                    out[((dy + 1) * 3 + dx + 1) as usize] = self.0[(y * w + x) as usize];
                }
            }
        }
        out
    }
}

type JobResult = (usize, [LocalRect; 9], u16, Vec<SimEvent>, Vec<Spawn>, Vec<CellPos>, Vec<CellPos>);

/// Read-only data every job needs.
#[derive(Clone, Copy)]
pub struct PassInput<'a> {
    pub mats: &'a MaterialTable,
    pub react: &'a ReactTable,
    pub settings: &'a SimSettings,
    /// False when there are too many particles: liquids do not splash.
    pub splash_ok: bool,
}

/// Run the movement and reaction update for one tick. Returns the number of chunks that worked.
/// Events from the jobs are added to `events` in chunk order.
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
    let (w, h) = (world.width_chunks, world.height_chunks);
    let mut passes: [Vec<(usize, LocalRect)>; 4] = Default::default();
    for (i, slot) in world.chunks.iter_mut().enumerate() {
        if let Some(c) = slot
            && !c.dirty.is_empty()
        {
            let work = std::mem::replace(&mut c.dirty, LocalRect::EMPTY);
            let (cx, cy) = (i as i32 % w, i as i32 / w);
            passes[((cy & 1) * 2 + (cx & 1)) as usize].push((i, work));
        }
    }
    let awake: usize = passes.iter().map(|p| p.len()).sum();
    if awake == 0 {
        return 0;
    }

    // Neighbors of working chunks must exist.
    for pass in &passes {
        for &(i, _) in pass {
            let (cx, cy) = (i as i32 % w, i as i32 / w);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    world.chunk_mut(ChunkPos::new(cx + dx, cy + dy));
                }
            }
        }
    }

    let parity = (tick & 1) as u8;
    let left_to_right = tick & 1 == 0;
    let outside = world.outside;

    let mut opened = fall_pass(world, input, &passes, tick, seed, stamp, pool);

    // The order of the 4 passes: x parity flips every 2 ticks, y parity every tick.
    let (flip_x, flip_y) = (((tick >> 1) & 1) as usize, (tick & 1) as usize);
    let mut levels: Vec<CellPos> = Vec::new();
    for step in 0..4 {
        let k = (((step >> 1) ^ flip_y) << 1) | ((step & 1) ^ flip_x);
        let pass = &passes[k];
        if pass.is_empty() {
            continue;
        }
        let ptrs = ChunkPtrs::new(world);
        let run = |&(i, work): &(usize, LocalRect)| -> JobResult {
            let (cx, cy) = (i as i32 % w, i as i32 / w);
            let rng = Rng::for_chunk(seed, tick, ChunkPos::new(cx, cy), k as u64);
            // SAFETY: all chunks in this pass are 2 apart; see `hood.rs`.
            let origin = ChunkPos::new(cx, cy).origin();
            let mut hood = unsafe { Hood::new(ptrs.hood(cx, cy, w, h), input, rng, parity, outside, origin) };
            update_chunk(&mut hood, work, left_to_right);
            (i, hood.marks, hood.changed, hood.events, hood.spawns, hood.levels, hood.opened)
        };
        let results: Vec<JobResult> = match pool {
            Some(p) => p.install(|| pass.par_iter().map(run).collect()),
            None => pass.par_iter().map(run).collect(),
        };
        drop(ptrs);
        for (i, marks, changed, job_events, job_spawns, job_levels, job_opened) in results {
            events.extend(job_events);
            spawns.extend(job_spawns);
            levels.extend(job_levels);
            opened.extend(job_opened);
            merge_marks(world, i, &marks, changed, stamp);
        }
    }
    if !levels.is_empty() || !opened.is_empty() {
        level_pass(world, input, levels, opened, tick, seed, stamp, pool);
    }
    awake as u32
}

/// The level pass (step 6 in the module documentation). `levels`: cells that may walk.
/// `opened`: places that top cells left in this tick; the far ends of the top row above them are
/// woken, so they find the new place to fall.
#[allow(clippy::too_many_arguments)]
fn level_pass(
    world: &mut World,
    input: PassInput,
    levels: Vec<CellPos>,
    opened: Vec<CellPos>,
    tick: u64,
    seed: u64,
    stamp: u64,
    pool: Option<&rayon::ThreadPool>,
) {
    let (w, h) = (world.width_chunks, world.height_chunks);
    // By chunk row, then from left to right (or right to left on odd ticks).
    let left_to_right = tick & 1 == 0;
    let mut items: Vec<(CellPos, bool)> = levels.into_iter().map(|p| (p, false)).chain(opened.into_iter().map(|p| (p, true))).collect();
    items.sort_unstable_by_key(|&(p, o)| (p.y >> 6, if left_to_right { p.x } else { -p.x }, p.y, o));
    let mut rows: Vec<&[(CellPos, bool)]> = Vec::new();
    let mut rest = &items[..];
    while let Some(first) = rest.first() {
        let n = rest.iter().take_while(|(p, _)| p.y >> 6 == first.0.y >> 6).count();
        rows.push(&rest[..n]);
        rest = &rest[n..];
    }
    let parity = (tick & 1) as u8;
    let outside = world.outside;
    for row_parity in 0..2 {
        let jobs: Vec<&[(CellPos, bool)]> = rows.iter().copied().filter(|r| (r[0].0.y >> 6) & 1 == row_parity).collect();
        if jobs.is_empty() {
            continue;
        }
        let ptrs = ChunkPtrs::new(world);
        let run = |row: &&[(CellPos, bool)]| -> (Vec<FallResult>, Vec<CellPos>) {
            // SAFETY: the level pass rule (module documentation): this job reads rows of its own
            // chunk row and one row above and below; no other job of this pass writes them.
            let far = |x: i32, y: i32| unsafe { ptrs.mat(w, h, outside, x, y) };
            let mut out: Vec<FallResult> = Vec::new();
            let mut wake: Vec<CellPos> = Vec::new();
            let mut start = 0;
            while start < row.len() {
                // The cells of one chunk share one hood.
                let pos = row[start].0.chunk();
                let n = row[start..].iter().take_while(|(p, _)| p.chunk() == pos).count();
                let i = (pos.y * w + pos.x) as usize;
                let rng = Rng::for_chunk(seed, tick, pos, 5);
                // SAFETY: as above; the hood moves cells only sideways inside this chunk row.
                let mut hood = unsafe { Hood::new(ptrs.hood(pos.x, pos.y, w, h), input, rng, parity, outside, pos.origin()) };
                if level_cells(&mut hood, &row[start..start + n], &far, &mut wake) {
                    out.push((i, hood.marks, hood.changed));
                }
                start += n;
            }
            (out, wake)
        };
        let results: Vec<(Vec<FallResult>, Vec<CellPos>)> = match pool {
            Some(p) => p.install(|| jobs.par_iter().map(run).collect()),
            None => jobs.par_iter().map(run).collect(),
        };
        drop(ptrs);
        for (job, wake) in results {
            for (i, marks, changed) in job {
                merge_marks(world, i, &marks, changed, stamp);
            }
            for p in wake {
                if let Some(c) = world.chunk_mut_if_exists(p.chunk()) {
                    c.dirty.add_point(p.x & 63, p.y & 63);
                }
            }
        }
    }
}

/// Add the dirty marks and "changed" bits of one job (center chunk index `i`) to the chunks.
fn merge_marks(world: &mut World, i: usize, marks: &[LocalRect; 9], changed: u16, stamp: u64) {
    let (w, h) = (world.width_chunks, world.height_chunks);
    let (cx, cy) = (i as i32 % w, i as i32 / w);
    for (s, mark) in marks.iter().enumerate() {
        let (x, y) = (cx + (s as i32 % 3) - 1, cy + (s as i32 / 3) - 1);
        if x < 0 || y < 0 || x >= w || y >= h {
            continue;
        }
        if let Some(c) = world.chunks[(y * w + x) as usize].as_deref_mut() {
            c.dirty.add_rect(*mark);
            if changed & (1 << s) != 0 {
                c.version = stamp;
            }
        }
    }
}

/// Marks and "changed" bits of one chunk in the fall pass and the level pass.
type FallResult = (usize, [LocalRect; 9], u16);

/// Move the falling cells of all working chunks (step 3 in the module documentation).
/// Returns the places that top liquid cells left (for the level pass).
fn fall_pass(
    world: &mut World,
    input: PassInput,
    passes: &[Vec<(usize, LocalRect)>; 4],
    tick: u64,
    seed: u64,
    stamp: u64,
    pool: Option<&rayon::ThreadPool>,
) -> Vec<CellPos> {
    let (w, h) = (world.width_chunks, world.height_chunks);
    // All working chunks, sorted by column, and from the bottom up in each column.
    let mut work: Vec<(usize, LocalRect)> = passes.iter().flatten().copied().collect();
    work.sort_unstable_by_key(|&(i, _)| (i as i32 % w, -(i as i32 / w)));
    let mut columns: Vec<&[(usize, LocalRect)]> = Vec::new();
    let mut rest = &work[..];
    while let Some(&(first, _)) = rest.first() {
        let cx = first as i32 % w;
        let n = rest.iter().take_while(|&&(i, _)| i as i32 % w == cx).count();
        columns.push(&rest[..n]);
        rest = &rest[n..];
    }
    let parity = (tick & 1) as u8;
    let left_to_right = tick & 1 == 0;
    let outside = world.outside;
    let ptrs = ChunkPtrs::new(world);
    let run = |column: &&[(usize, LocalRect)]| -> (Vec<FallResult>, Vec<CellPos>) {
        let mut out = Vec::with_capacity(column.len());
        let mut opened = Vec::new();
        for &(i, work) in column.iter() {
            let (cx, cy) = (i as i32 % w, i as i32 / w);
            let pos = ChunkPos::new(cx, cy);
            // The fall pass does not use random numbers, but a hood needs a generator.
            let rng = Rng::for_chunk(seed, tick, pos, 4);
            // SAFETY: a falling cell moves straight down, so this job reads and writes only cells
            // in its own column of chunks. Marks go to the hood and are merged after the pass.
            let mut hood = unsafe { Hood::new(ptrs.hood(cx, cy, w, h), input, rng, parity, outside, pos.origin()) };
            if fall_chunk(&mut hood, work, left_to_right) {
                out.push((i, hood.marks, hood.changed));
                opened.append(&mut hood.opened);
            }
        }
        (out, opened)
    };
    let results: Vec<(Vec<FallResult>, Vec<CellPos>)> = match pool {
        Some(p) => p.install(|| columns.par_iter().map(run).collect()),
        None => columns.par_iter().map(run).collect(),
    };
    drop(ptrs);
    let mut opened = Vec::new();
    for (job, job_opened) in results {
        for (i, marks, changed) in job {
            merge_marks(world, i, &marks, changed, stamp);
        }
        opened.extend(job_opened);
    }
    opened
}
