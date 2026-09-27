//! The parallel movement pass (technical design section 6.1).
//!
//! 1. Take the dirty rectangle of each chunk as this tick's work. Chunks with no work sleep.
//! 2. Make sure every neighbor of a working chunk exists, so jobs never need to allocate.
//! 3. Run 4 passes. Pass k updates the chunks where (x mod 2, y mod 2) is the k-th pair.
//!    The jobs of one pass run in parallel (see the safety rule in `hood.rs`).
//! 4. After each pass, merge the dirty marks and "changed" bits of all jobs, in chunk order.
//!    This keeps the result the same for any number of threads.

use crate::chunk::{Chunk, LocalRect};
use crate::hood::Hood;
use crate::update::update_chunk;
use crate::world::World;
use crate::SimEvent;
use foundry_content::MaterialTable;
use foundry_core::{ChunkPos, Rng};
use rayon::prelude::*;

/// Raw pointers to all chunks, for one pass. Null for chunks that do not exist.
struct ChunkPtrs(Vec<*mut Chunk>);

// SAFETY: jobs use the pointers only under the rule in `hood.rs`.
unsafe impl Sync for ChunkPtrs {}

impl ChunkPtrs {
    fn new(world: &mut World) -> Self {
        Self(world.chunks.iter_mut().map(|c| c.as_deref_mut().map_or(std::ptr::null_mut(), |c| c as *mut Chunk)).collect())
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

type JobResult = (usize, [LocalRect; 9], u16, Vec<SimEvent>);

/// Run the movement and reaction update for one tick. Returns the number of chunks that worked.
/// Events from the jobs are added to `events` in chunk order.
#[allow(clippy::too_many_arguments)]
pub fn movement_tick(
    world: &mut World,
    mats: &MaterialTable,
    tick: u64,
    seed: u64,
    stamp: u64,
    pool: Option<&rayon::ThreadPool>,
    events: &mut Vec<SimEvent>,
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
    for (k, pass) in passes.iter().enumerate() {
        if pass.is_empty() {
            continue;
        }
        let ptrs = ChunkPtrs::new(world);
        let run = |&(i, work): &(usize, LocalRect)| -> JobResult {
            let (cx, cy) = (i as i32 % w, i as i32 / w);
            let rng = Rng::for_chunk(seed, tick, ChunkPos::new(cx, cy), k as u64);
            // SAFETY: all chunks in this pass are 2 apart; see `hood.rs`.
            let origin = ChunkPos::new(cx, cy).origin();
            let mut hood = unsafe { Hood::new(ptrs.hood(cx, cy, w, h), mats, rng, parity, outside, origin) };
            update_chunk(&mut hood, work, left_to_right);
            (i, hood.marks, hood.changed, hood.events)
        };
        let results: Vec<JobResult> = match pool {
            Some(p) => p.install(|| pass.par_iter().map(run).collect()),
            None => pass.par_iter().map(run).collect(),
        };
        drop(ptrs);
        for (i, marks, changed, job_events) in results {
            events.extend(job_events);
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
    }
    awake as u32
}
