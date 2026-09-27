//! The parallel movement pass (technical design section 6.1).
//!
//! 1. Take this tick's work from the world's awake list: the dirty rectangle of each chunk that is
//!    inside a simulation area, sorted by (y, x). Chunks with no work leave the list. Chunks
//!    outside every simulation area wait in the paused set. The cost depends only on these chunks,
//!    never on the number of chunks in the world.
//! 2. Make sure every neighbor of a working chunk is live (made by the source or unpacked, in
//!    parallel), so jobs never need to allocate. Then look up the 3 × 3 chunk pointers of each job
//!    once for the whole tick.
//! 3. Run 4 passes. Pass k updates the chunks where (x mod 2, y mod 2) is the k-th pair.
//!    The jobs of one pass run in parallel (see the safety rule in `hood.rs`).
//! 4. After each pass, merge the dirty marks, "changed" and "touched" bits of all jobs, in
//!    (y, x) order. This keeps the result the same for any number of threads.

use crate::SimEvent;
use crate::chunk::{Chunk, LocalRect};
use crate::hood::Hood;
use crate::react::ReactTable;
use crate::update::update_chunk;
use crate::world::World;
use foundry_content::MaterialTable;
use foundry_core::{ChunkPos, Rng};
use rayon::prelude::*;

/// The 3 × 3 chunk pointers of each job, for the whole tick. Null for chunks outside the world.
struct HoodPtrs(Vec<[*mut Chunk; 9]>);

// SAFETY: jobs use the pointers only under the rule in `hood.rs`.
unsafe impl Sync for HoodPtrs {}

impl HoodPtrs {
    /// The pointers of job `i`. (A method, so that closures capture the whole `Sync` wrapper.)
    #[inline]
    fn get(&self, i: usize) -> [*mut Chunk; 9] {
        self.0[i]
    }
}

/// (job index, marks, changed bits, touched bits, events)
type JobResult = (usize, [LocalRect; 9], u16, u16, Vec<SimEvent>);

/// Run the movement and reaction update for one tick. Returns the number of chunks that worked.
/// Events from the jobs are added to `events` in chunk order.
#[allow(clippy::too_many_arguments)]
pub fn movement_tick(
    world: &mut World,
    mats: &MaterialTable,
    react: &ReactTable,
    tick: u64,
    seed: u64,
    stamp: u64,
    pool: Option<&rayon::ThreadPool>,
    events: &mut Vec<SimEvent>,
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
    let mut jobs: Vec<usize> = Vec::with_capacity(work.len());
    for k in 0..4 {
        jobs.clear();
        jobs.extend((0..work.len()).filter(|&i| {
            let c = work[i].0;
            ((c.y & 1) * 2 + (c.x & 1)) as usize == k
        }));
        if jobs.is_empty() {
            continue;
        }
        let run = |&i: &usize| -> JobResult {
            let (c, rect) = work[i];
            let rng = Rng::for_chunk(seed, tick, c, k as u64);
            // SAFETY: all chunks in this pass are 2 apart; see `hood.rs`.
            let mut hood = unsafe { Hood::new(ptrs.get(i), mats, react, rng, parity, outside, c.origin()) };
            update_chunk(&mut hood, rect, left_to_right);
            (i, hood.marks, hood.changed, hood.touched, hood.events)
        };
        let results: Vec<JobResult> = match pool {
            Some(p) => p.install(|| jobs.par_iter().map(run).collect()),
            None => jobs.par_iter().map(run).collect(),
        };
        for (i, marks, changed, touched, job_events) in results {
            events.extend(job_events);
            let c = work[i].0;
            for (s, mark) in marks.iter().enumerate() {
                let p = ptrs.get(i)[s];
                if p.is_null() {
                    continue;
                }
                let pos = ChunkPos::new(c.x + (s as i32 % 3) - 1, c.y + (s as i32 / 3) - 1);
                // SAFETY: no job runs now, and `p` points to the live chunk at `pos`. The pointers
                // stay valid for the whole tick, because no chunk is added or removed during the passes.
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
                }
            }
        }
    }
    work.len() as u32
}
