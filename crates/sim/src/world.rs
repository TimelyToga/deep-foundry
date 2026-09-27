//! The world: chunks stored by position in hash maps.
//!
//! The width has no limit, unless the world is a finite test box. The height is fixed: from the
//! top of the sky (y = 0) down to the bottom of the world. Cells above and below act as bedrock
//! (`World::outside`), and so do cells left and right of a finite box.
//!
//! # Where a chunk is
//!
//! | State | Stored in | Meaning |
//! |---|---|---|
//! | Live | `live` | The cells are in memory. Only live chunks update. |
//! | Air | `air` | The source made the chunk all air at the default temperature, and nothing changed it since. No cells are stored. |
//! | Packed | `packed`, or the `ChunkStore` | A changed chunk far from every anchor. Its cells are compressed with lz4. |
//! | Not in memory | nowhere | Not made yet, or made and then dropped because nothing changed it. The source makes it again when it is needed. |
//!
//! # Work
//!
//! Every live chunk with a non-empty dirty rectangle is in the awake list or in the paused set
//! (then `Chunk::queued` is true). The movement pass reads only the awake list. So the cost of a
//! tick depends on the chunks that have work, not on the number of chunks in memory.
//!
//! - The awake list holds the chunks that may have work in the next tick.
//! - The paused set holds the chunks that have work but are outside every simulation area (an
//!   anchor area plus `SimSettings::sim_margin_chunks`). They keep their dirty rectangle, and they
//!   continue when an anchor comes near. A paused chunk can be packed.
//!
//! With no anchors, the whole world is near: all chunks with work update, and nothing is packed
//! or dropped.

use crate::chunk::{Chunk, GENERATED_VERSION, LocalRect};
use crate::pack::{ChunkStore, PackedChunk};
use crate::source::{ChunkCells, ChunkSource};
use foundry_content::{Content, MaterialTable};
use foundry_core::{
    CHUNK_AREA, CHUNK_MASK, CHUNK_SHIFT, CHUNK_SIZE, CellPos, CellRect, ChunkPos, DEFAULT_TEMPERATURE, MaterialId, Rng,
};
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;

/// Salt for the random numbers of new chunks (shade and life of each cell).
const GENERATE_SALT: u64 = 0x6765_6e65_7261_7465;

/// A rectangle of chunks. `x1` and `y1` are outside (exclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkArea {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl ChunkArea {
    /// The chunks that touch `r`, plus `margin` chunks on each side.
    /// An empty rectangle gives the chunk of its corner (`x0`, `y0`).
    pub fn around(r: CellRect, margin: i32) -> Self {
        let (x0, y0) = (r.x0 >> CHUNK_SHIFT, r.y0 >> CHUNK_SHIFT);
        let (x1, y1) = if r.is_empty() {
            (x0 + 1, y0 + 1)
        } else {
            (((r.x1 - 1) >> CHUNK_SHIFT) + 1, ((r.y1 - 1) >> CHUNK_SHIFT) + 1)
        };
        let m = margin.max(0);
        Self { x0: x0.saturating_sub(m), y0: y0.saturating_sub(m), x1: x1.saturating_add(m), y1: y1.saturating_add(m) }
    }

    #[inline(always)]
    pub fn contains(&self, c: ChunkPos) -> bool {
        c.x >= self.x0 && c.x < self.x1 && c.y >= self.y0 && c.y < self.y1
    }
}

/// Where chunks update and where they stay unpacked. Made from the anchors.
#[derive(Debug, Clone, Default)]
pub struct Areas {
    /// Chunks inside one of these update.
    pub sim: Vec<ChunkArea>,
    /// Chunks outside all of these are packed or dropped.
    pub keep: Vec<ChunkArea>,
}

impl Areas {
    /// `anchors` are cell rectangles. The margins are in chunks.
    pub fn new(anchors: &[CellRect], sim_margin: i32, keep_margin: i32) -> Self {
        let keep_margin = keep_margin.max(sim_margin + 2);
        Self {
            sim: anchors.iter().map(|r| ChunkArea::around(*r, sim_margin)).collect(),
            keep: anchors.iter().map(|r| ChunkArea::around(*r, keep_margin)).collect(),
        }
    }

    /// True if there are no anchors. Then the whole world is near.
    pub fn is_unlimited(&self) -> bool {
        self.sim.is_empty()
    }

    /// True if a chunk with work at `c` updates.
    #[inline]
    pub fn simulates(&self, c: ChunkPos) -> bool {
        self.sim.is_empty() || self.sim.iter().any(|a| a.contains(c))
    }

    /// True if a chunk at `c` stays unpacked in memory.
    #[inline]
    pub fn keeps(&self, c: ChunkPos) -> bool {
        self.keep.is_empty() || self.keep.iter().any(|a| a.contains(c))
    }
}

/// How much chunk data the world holds. See `World::memory`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemoryStats {
    /// Chunks with their cells in memory.
    pub live_chunks: usize,
    /// Chunks that are known to be all air. No cells are stored for them.
    pub air_chunks: usize,
    /// Changed chunks that are packed in memory.
    pub packed_chunks: usize,
    /// Bytes of the packed cell data in memory.
    pub packed_bytes: usize,
    /// Chunks in the `ChunkStore` (0 without a store).
    pub stored_chunks: usize,
    /// Entries in the awake list, and chunks in the paused set.
    pub awake_list: usize,
    pub paused_chunks: usize,
    /// Chunks that the source made since the world was made (also chunks made again after a drop).
    pub generated_total: u64,
    /// An estimate of the memory for all chunk data, in bytes (cells, packed data, hash maps).
    pub bytes: usize,
}

/// A new chunk from the source.
enum Made {
    /// All air at the default temperature, asleep. No cells need to be stored.
    Air,
    Cells(Box<Chunk>),
}

pub struct World {
    live: FxHashMap<ChunkPos, Box<Chunk>>,
    air: FxHashSet<ChunkPos>,
    packed: FxHashMap<ChunkPos, PackedChunk>,
    packed_bytes: usize,
    store: Option<Box<dyn ChunkStore>>,
    /// With a store: move packed chunks into it when `packed_bytes` is above this.
    packed_limit: usize,
    awake: Vec<ChunkPos>,
    paused: FxHashSet<ChunkPos>,
    /// Chunks that worked in the last movement tick, sorted by (y, x).
    worked: Vec<ChunkPos>,
    areas: Areas,
    source: Arc<dyn ChunkSource>,
    content: Arc<Content>,
    seed: u64,
    /// Height in chunks. Chunk rows 0 to `height_chunks - 1` exist.
    height_chunks: i32,
    /// Width in chunks of a finite box (columns 0 to `width - 1`). `None`: no limit.
    width_chunks: Option<i32>,
    /// The material that cells outside the world act as (bedrock).
    pub outside: MaterialId,
    generated: u64,
}

impl World {
    pub fn new(
        content: Arc<Content>,
        source: Arc<dyn ChunkSource>,
        seed: u64,
        height_chunks: i32,
        width_chunks: Option<i32>,
        outside: MaterialId,
    ) -> Self {
        Self {
            live: FxHashMap::default(),
            air: FxHashSet::default(),
            packed: FxHashMap::default(),
            packed_bytes: 0,
            store: None,
            packed_limit: usize::MAX,
            awake: Vec::new(),
            paused: FxHashSet::default(),
            worked: Vec::new(),
            areas: Areas::default(),
            source,
            content,
            seed,
            height_chunks: height_chunks.max(1),
            width_chunks: width_chunks.map(|w| w.max(1)),
            outside,
            generated: 0,
        }
    }

    // ---- Size ----

    /// Height of the world in cells. Rows 0 to `height_cells() - 1` exist.
    pub fn height_cells(&self) -> i32 {
        self.height_chunks * CHUNK_SIZE
    }

    pub fn height_chunks(&self) -> i32 {
        self.height_chunks
    }

    /// Width in chunks of a finite box. `None` for a world with no limit to the left and right.
    pub fn width_chunks(&self) -> Option<i32> {
        self.width_chunks
    }

    /// True if the chunk is inside the world: inside the rows of the world, and inside the box
    /// for a finite world.
    #[inline(always)]
    pub fn chunk_in_bounds(&self, c: ChunkPos) -> bool {
        c.y >= 0 && c.y < self.height_chunks && self.width_chunks.is_none_or(|w| c.x >= 0 && c.x < w)
    }

    /// True if the cell is inside the world (see `chunk_in_bounds`).
    #[inline(always)]
    pub fn in_bounds(&self, p: CellPos) -> bool {
        self.chunk_in_bounds(p.chunk())
    }

    /// The part of `r` that is inside the world.
    pub fn clip(&self, r: CellRect) -> CellRect {
        let mut out = r.intersect(&CellRect::new(r.x0, 0, r.x1, self.height_cells()));
        if let Some(w) = self.width_chunks {
            out = out.intersect(&CellRect::new(0, out.y0, w * CHUNK_SIZE, out.y1));
        }
        out
    }

    pub fn source(&self) -> &Arc<dyn ChunkSource> {
        &self.source
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    // ---- Reading cells ----

    /// Material at a cell. Outside the world: bedrock. Fast for live chunks. For a packed chunk or
    /// a chunk that is not in memory it unpacks or makes the chunk to read one cell (slow).
    #[inline]
    pub fn mat(&self, p: CellPos) -> MaterialId {
        if !self.in_bounds(p) {
            return self.outside;
        }
        let c = p.chunk();
        if let Some(ch) = self.live.get(&c) {
            return MaterialId(ch.mat[p.local_index()]);
        }
        if self.air.contains(&c) {
            return MaterialId::AIR;
        }
        self.cell_slow(p).0
    }

    /// Material and temperature at a cell. Outside the world: bedrock at the default temperature.
    pub fn cell(&self, p: CellPos) -> (MaterialId, i16) {
        if !self.in_bounds(p) {
            return (self.outside, DEFAULT_TEMPERATURE);
        }
        match self.live.get(&p.chunk()) {
            Some(ch) => (MaterialId(ch.mat[p.local_index()]), ch.temp[p.local_index()]),
            None => self.cell_slow(p),
        }
    }

    fn cell_slow(&self, p: CellPos) -> (MaterialId, i16) {
        let i = p.local_index();
        self.with_cells(p.chunk(), |c| match c {
            Some(c) => (MaterialId(c.mat[i]), c.temp[i]),
            None => (MaterialId::AIR, DEFAULT_TEMPERATURE),
        })
    }

    /// Call `f` with the cells of a chunk inside the world, wherever the chunk is. `None` means all
    /// air at the default temperature. A packed chunk is unpacked into a copy, and a chunk that is
    /// not in memory is made into a copy; the world does not change. For tools and tests.
    pub fn with_cells<R>(&self, c: ChunkPos, f: impl FnOnce(Option<&Chunk>) -> R) -> R {
        debug_assert!(self.chunk_in_bounds(c));
        if let Some(ch) = self.live.get(&c) {
            return f(Some(ch));
        }
        if self.air.contains(&c) {
            return f(None);
        }
        let packed = self.packed.get(&c).cloned().or_else(|| self.store.as_ref().and_then(|s| s.read(c)));
        if let Some(p) = packed {
            let ch = p.unpack(None).expect("a packed chunk in memory is valid");
            return f(Some(&ch));
        }
        match make_chunk(self.source.as_ref(), &self.content.materials, self.seed, c) {
            Made::Air => f(None),
            Made::Cells(ch) => f(Some(&ch)),
        }
    }

    /// The chunk if its cells are in memory (not packed). Never makes, unpacks or changes a chunk.
    pub fn chunk(&self, c: ChunkPos) -> Option<&Chunk> {
        self.live.get(&c).map(|b| &**b)
    }

    /// Positions of the chunks that have their cells in memory, sorted by (y, x).
    pub fn loaded_chunks(&self) -> impl Iterator<Item = ChunkPos> + use<> {
        let mut v: Vec<ChunkPos> = self.live.keys().copied().collect();
        v.sort_unstable_by_key(|c| (c.y, c.x));
        v.into_iter()
    }

    /// Number of chunks with their cells in memory.
    pub fn live_count(&self) -> usize {
        self.live.len()
    }

    /// Number of packed chunks in memory.
    pub fn packed_count(&self) -> usize {
        self.packed.len()
    }

    /// Chunks that the source made since the world was made.
    pub fn generated_total(&self) -> u64 {
        self.generated
    }

    /// Chunks that worked in the last movement tick, sorted by (y, x). Other passes (for example
    /// heat) can use this list.
    pub fn worked_chunks(&self) -> &[ChunkPos] {
        &self.worked
    }

    /// How much chunk data the world holds.
    pub fn memory(&self) -> MemoryStats {
        let chunk_bytes = std::mem::size_of::<Chunk>();
        // Hash map entries: key, value and control bytes, with about 1/8 free space.
        let entry = |value: usize| (std::mem::size_of::<ChunkPos>() + value + 1) * 8 / 7;
        let bytes = self.live.len() * (chunk_bytes + entry(8))
            + self.air.len() * entry(0)
            + self.packed.len() * entry(std::mem::size_of::<PackedChunk>())
            + self.packed_bytes
            + self.paused.len() * entry(0)
            + self.awake.capacity() * std::mem::size_of::<ChunkPos>();
        MemoryStats {
            live_chunks: self.live.len(),
            air_chunks: self.air.len(),
            packed_chunks: self.packed.len(),
            packed_bytes: self.packed_bytes,
            stored_chunks: self.store.as_ref().map_or(0, |s| s.positions().len()),
            awake_list: self.awake.len(),
            paused_chunks: self.paused.len(),
            generated_total: self.generated,
            bytes,
        }
    }

    // ---- Writing cells ----

    /// The chunk, made, unpacked or taken from the store if needed. `None` only outside the world.
    ///
    /// The chunk counts as changed from now on (it is no longer pristine), because the caller can
    /// write its cells. It is also put in the awake list, so a dirty rectangle that the caller sets
    /// is seen in the next tick.
    pub fn chunk_mut(&mut self, c: ChunkPos) -> Option<&mut Chunk> {
        if !self.chunk_in_bounds(c) {
            return None;
        }
        if !self.live.contains_key(&c) {
            self.load(&[c], true, None);
        }
        let ch = self.live.get_mut(&c).expect("loaded");
        ch.pristine = false;
        if !ch.queued {
            ch.queued = true;
            self.awake.push(c);
        }
        Some(ch)
    }

    /// The chunk if the world has its cells (live or packed; a packed chunk is unpacked). Never
    /// makes a new chunk: `None` for a chunk that is not in memory, a chunk that is all air, and a
    /// chunk outside the world. Like `chunk_mut`, the chunk counts as changed.
    pub fn chunk_mut_if_exists(&mut self, c: ChunkPos) -> Option<&mut Chunk> {
        if !self.chunk_in_bounds(c) || self.air.contains(&c) {
            return None;
        }
        if !self.live.contains_key(&c) {
            if !self.packed.contains_key(&c) && !self.store.as_ref().is_some_and(|s| s.contains(c)) {
                return None;
            }
            self.load(&[c], true, None);
        }
        self.chunk_mut(c)
    }

    /// Update this cell and its 8 neighbors in the next tick. Chunks that are all air are skipped
    /// (air never needs an update). A neighbor chunk that is not in memory is made first, because
    /// its cells next to `p` may need to move (for example sand above a hole that was just dug).
    pub fn mark_dirty_around(&mut self, p: CellPos) {
        let (lx, ly) = (p.x & CHUNK_MASK, p.y & CHUNK_MASK);
        if lx > 0 && lx < CHUNK_MASK && ly > 0 && ly < CHUNK_MASK {
            if let Some(c) = self.chunk_for_marks(p.chunk()) {
                c.dirty.add_rect(LocalRect { x0: lx - 1, y0: ly - 1, x1: lx + 2, y1: ly + 2 });
            }
            return;
        }
        for dy in -1..=1 {
            for dx in -1..=1 {
                let q = p.offset(dx, dy);
                if let Some(c) = self.chunk_for_marks(q.chunk()) {
                    c.dirty.add_point(q.x & CHUNK_MASK, q.y & CHUNK_MASK);
                }
            }
        }
    }

    /// A chunk that is about to get dirty marks: live and queued. `None` outside the world and for
    /// chunks that are all air. Does not change `pristine` (a mark is not a change of a cell).
    fn chunk_for_marks(&mut self, c: ChunkPos) -> Option<&mut Chunk> {
        if !self.chunk_in_bounds(c) || self.air.contains(&c) {
            return None;
        }
        if !self.live.contains_key(&c) {
            self.load(&[c], false, None);
        }
        let ch = self.live.get_mut(&c)?;
        if !ch.queued {
            ch.queued = true;
            self.awake.push(c);
        }
        Some(ch)
    }

    // ---- Loading and making chunks ----

    /// Make sure that the chunks at `positions` are in memory. Chunks outside the world are skipped.
    /// Packed chunks are unpacked. Chunks that are not in memory are made by the source.
    /// `need_cells`: also make chunks that are all air live (the movement pass needs their cells);
    /// otherwise they are only noted as air.
    ///
    /// Unpacking and making run in parallel. The result does not depend on the order or on the
    /// number of threads. Returns the number of chunks the source made.
    pub fn load(&mut self, positions: &[ChunkPos], need_cells: bool, pool: Option<&rayon::ThreadPool>) -> usize {
        // Lists of neighbors repeat positions. Sort them, so each position is handled once.
        let sorted: Vec<ChunkPos>;
        let positions = if positions.len() > 1 {
            let mut v = positions.to_vec();
            v.sort_unstable_by_key(|c| (c.y, c.x));
            v.dedup();
            sorted = v;
            &sorted[..]
        } else {
            positions
        };
        let mut jobs: Vec<(ChunkPos, Option<PackedChunk>)> = Vec::new();
        for &c in positions {
            if !self.chunk_in_bounds(c) || self.live.contains_key(&c) {
                continue;
            }
            if self.air.contains(&c) {
                if need_cells {
                    self.air.remove(&c);
                    self.insert_live(c, Chunk::new_air());
                }
                continue;
            }
            let packed = self.take_packed(c);
            jobs.push((c, packed));
        }
        if jobs.is_empty() {
            return 0;
        }
        let (source, mats, seed) = (self.source.as_ref(), &self.content.materials, self.seed);
        let run = |(c, packed): &(ChunkPos, Option<PackedChunk>)| match packed {
            Some(p) => Made::Cells(p.unpack(None).expect("a packed chunk in memory is valid")),
            None => make_chunk(source, mats, seed, *c),
        };
        let made: Vec<Made> = if jobs.len() == 1 {
            vec![run(&jobs[0])]
        } else {
            match pool {
                Some(p) => p.install(|| jobs.par_iter().map(run).collect()),
                None => jobs.par_iter().map(run).collect(),
            }
        };
        let mut generated = 0;
        for ((c, packed), m) in jobs.into_iter().zip(made) {
            generated += packed.is_none() as usize;
            match m {
                Made::Air if !need_cells => {
                    self.air.insert(c);
                }
                Made::Air => self.insert_live(c, Chunk::new_air()),
                Made::Cells(ch) => self.insert_live(c, ch),
            }
        }
        self.generated += generated as u64;
        generated
    }

    /// Take the packed chunk at `c` from memory or from the store.
    fn take_packed(&mut self, c: ChunkPos) -> Option<PackedChunk> {
        if let Some(p) = self.packed.remove(&c) {
            self.packed_bytes -= p.bytes.len();
            return Some(p);
        }
        self.store.as_mut().and_then(|s| s.take(c))
    }

    /// Put a chunk into the live map and keep the work lists right.
    fn insert_live(&mut self, c: ChunkPos, mut ch: Box<Chunk>) {
        ch.queued = false;
        if self.paused.contains(&c) {
            ch.queued = true;
        } else if !ch.dirty.is_empty() {
            ch.queued = true;
            self.awake.push(c);
        }
        self.live.insert(c, ch);
    }

    /// Put a packed chunk into the world (for loading). Replaces any other data at `c`.
    /// A chunk with work goes to the paused set; the next tick moves it to the awake list if an
    /// anchor is near.
    pub fn insert_packed(&mut self, c: ChunkPos, p: PackedChunk) {
        if let Some(old) = self.live.remove(&c)
            && old.queued
        {
            self.awake.retain(|a| *a != c);
        }
        self.air.remove(&c);
        if !p.dirty.is_empty() {
            self.paused.insert(c);
        }
        self.packed_bytes += p.bytes.len();
        if let Some(old) = self.packed.insert(c, p) {
            self.packed_bytes -= old.bytes.len();
        }
    }

    // ---- Anchors and memory ----

    /// Set the areas where chunks update and stay unpacked. Paused chunks that are now inside a
    /// simulation area go back to the awake list (packed ones are unpacked).
    pub fn set_areas(&mut self, areas: Areas, pool: Option<&rayon::ThreadPool>) {
        self.areas = areas;
        if self.paused.is_empty() {
            return;
        }
        let mut back: Vec<ChunkPos> = self.paused.iter().copied().filter(|c| self.areas.simulates(*c)).collect();
        back.sort_unstable_by_key(|c| (c.y, c.x));
        let mut unpack = Vec::new();
        for c in back {
            self.paused.remove(&c);
            if self.live.contains_key(&c) {
                // Still queued; now in the awake list instead of the paused set.
                self.awake.push(c);
            } else {
                unpack.push(c);
            }
        }
        self.load(&unpack, true, pool);
    }

    pub fn areas(&self) -> &Areas {
        &self.areas
    }

    /// Pack or drop the live chunks outside every keep area, and forget far air chunks.
    /// - A pristine chunk with no work is dropped. The source can make it again.
    /// - A changed chunk, or a paused chunk, is packed.
    /// - Chunks in the awake list stay; the next tick decides where they go.
    ///
    /// Does nothing when there are no anchors. Packing runs in parallel.
    pub fn unload_far(&mut self, pool: Option<&rayon::ThreadPool>) {
        if self.areas.is_unlimited() {
            return;
        }
        let mut drop = Vec::new();
        let mut pack = Vec::new();
        for (c, ch) in &self.live {
            if self.areas.keeps(*c) {
                continue;
            }
            let paused = ch.queued && self.paused.contains(c);
            if ch.queued && !paused {
                continue;
            }
            if ch.pristine && !paused && ch.dirty.is_empty() {
                drop.push(*c);
            } else {
                pack.push(*c);
            }
        }
        for c in &drop {
            self.live.remove(c);
        }
        if !pack.is_empty() {
            let chunks: Vec<(ChunkPos, Box<Chunk>)> =
                pack.iter().map(|c| (*c, self.live.remove(c).expect("live"))).collect();
            let run = |(_, ch): &(ChunkPos, Box<Chunk>)| PackedChunk::pack(ch);
            let packed: Vec<PackedChunk> = match pool {
                Some(p) => p.install(|| chunks.par_iter().map(run).collect()),
                None => chunks.par_iter().map(run).collect(),
            };
            for ((c, _), p) in chunks.into_iter().zip(packed) {
                self.packed_bytes += p.bytes.len();
                self.packed.insert(c, p);
            }
        }
        let areas = &self.areas;
        self.air.retain(|c| areas.keeps(*c));
        if self.store.is_some() && self.packed_bytes > self.packed_limit {
            self.move_to_store();
        }
    }

    /// Use a store for packed chunks (see `ChunkStore`). When the packed chunks in memory use more
    /// than `limit_bytes`, the farthest ones move into the store.
    pub fn set_store(&mut self, store: Box<dyn ChunkStore>, limit_bytes: usize) {
        self.store = Some(store);
        self.packed_limit = limit_bytes;
    }

    /// Move packed chunks into the store, farthest from the anchors first, until they use at most
    /// 3/4 of the limit.
    fn move_to_store(&mut self) {
        let Some(store) = self.store.as_mut() else { return };
        let centers: Vec<(i64, i64)> =
            self.areas.keep.iter().map(|a| ((a.x0 as i64 + a.x1 as i64) / 2, (a.y0 as i64 + a.y1 as i64) / 2)).collect();
        let distance = |c: &ChunkPos| {
            centers.iter().map(|&(x, y)| (c.x as i64 - x).abs() + (c.y as i64 - y).abs()).min().unwrap_or(0)
        };
        // Only changed chunks go to the store (a pristine packed chunk is rare: it has work).
        let mut order: Vec<(i64, ChunkPos)> =
            self.packed.iter().filter(|(_, p)| !p.pristine).map(|(c, _)| (distance(c), *c)).collect();
        order.sort_unstable_by(|a, b| b.0.cmp(&a.0).then((a.1.y, a.1.x).cmp(&(b.1.y, b.1.x))));
        let target = self.packed_limit / 4 * 3;
        for (_, c) in order {
            if self.packed_bytes <= target {
                break;
            }
            let p = self.packed.remove(&c).expect("packed");
            self.packed_bytes -= p.bytes.len();
            store.put(c, p);
        }
    }

    // ---- For the movement pass and saves ----

    /// Take this tick's work: the dirty rectangle of each chunk in the awake list that is inside
    /// a simulation area, sorted by (y, x). Chunks with no work leave the list. Chunks outside every
    /// simulation area go to the paused set and keep their dirty rectangle.
    pub(crate) fn take_work(&mut self, out: &mut Vec<(ChunkPos, LocalRect)>) {
        out.clear();
        let mut list = std::mem::take(&mut self.awake);
        for c in list.drain(..) {
            let Some(ch) = self.live.get_mut(&c) else { continue };
            if ch.dirty.is_empty() {
                ch.queued = false;
                continue;
            }
            if !self.areas.simulates(c) {
                self.paused.insert(c);
                continue;
            }
            out.push((c, std::mem::replace(&mut ch.dirty, LocalRect::EMPTY)));
            ch.queued = false;
        }
        // Keep the allocation of the list. Nothing was added to `self.awake` in the loop.
        self.awake = list;
        out.sort_unstable_by_key(|w| (w.0.y, w.0.x));
        self.worked.clear();
        self.worked.extend(out.iter().map(|w| w.0));
    }

    /// Raw pointers to the 3 × 3 chunks around `c` (row by row), for the movement pass. Null for
    /// chunks outside the world. Live chunks that are missing are added to `missing` and the
    /// result is false.
    pub(crate) fn hood_ptrs(&mut self, c: ChunkPos, missing: &mut Vec<ChunkPos>) -> ([*mut Chunk; 9], bool) {
        let mut out = [std::ptr::null_mut(); 9];
        let mut complete = true;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let q = ChunkPos::new(c.x + dx, c.y + dy);
                if !self.chunk_in_bounds(q) {
                    continue;
                }
                match self.live.get_mut(&q) {
                    // `Box::as_mut_ptr` makes no reference to the chunk, so pointers made by several
                    // calls for the same chunk stay valid together.
                    Some(b) => out[((dy + 1) * 3 + dx + 1) as usize] = Box::as_mut_ptr(b),
                    None => {
                        missing.push(q);
                        complete = false;
                    }
                }
            }
        }
        (out, complete)
    }

    /// Put a chunk into the awake list if it is not queued yet. For the movement pass, which holds
    /// raw pointers to the chunks.
    ///
    /// # Safety
    /// `ch` must point to the live chunk at `c`, and no other code may use that chunk now.
    pub(crate) unsafe fn queue_raw(&mut self, c: ChunkPos, ch: *mut Chunk) {
        // SAFETY: see the function documentation.
        unsafe {
            if !(*ch).queued {
                (*ch).queued = true;
                self.awake.push(c);
            }
        }
    }

    /// The positions of all chunks that are not pristine (live, packed and stored), sorted by (y, x).
    /// The world hash covers these chunks.
    pub fn changed_positions(&self) -> Vec<ChunkPos> {
        let mut v: Vec<ChunkPos> = self.live.iter().filter(|(_, c)| !c.pristine).map(|(p, _)| *p).collect();
        v.extend(self.packed.iter().filter(|(_, p)| !p.pristine).map(|(c, _)| *c));
        if let Some(s) = &self.store {
            v.extend(s.positions());
        }
        v.sort_unstable_by_key(|c| (c.y, c.x));
        v
    }

    /// The positions of the chunks that a save must write, sorted by (y, x): the changed chunks,
    /// and pristine chunks with work (their dirty rectangle cannot be made again by the source).
    pub fn saved_positions(&self) -> Vec<ChunkPos> {
        let mut v: Vec<ChunkPos> =
            self.live.iter().filter(|(_, c)| !c.pristine || !c.dirty.is_empty()).map(|(p, _)| *p).collect();
        v.extend(self.packed.keys().copied());
        if let Some(s) = &self.store {
            v.extend(s.positions());
        }
        v.sort_unstable_by_key(|c| (c.y, c.x));
        v
    }

    /// The packed form of a chunk (for saves): packs a live chunk, or copies a packed one.
    pub fn packed_copy(&self, c: ChunkPos) -> Option<PackedChunk> {
        if let Some(ch) = self.live.get(&c) {
            return Some(PackedChunk::pack(ch));
        }
        self.packed.get(&c).cloned().or_else(|| self.store.as_ref().and_then(|s| s.read(c)))
    }
}

/// Make the chunk at `c` with the source, and set shade, life, motion and flags.
/// The result is the same for the same source, seed and position.
fn make_chunk(source: &dyn ChunkSource, mats: &MaterialTable, seed: u64, c: ChunkPos) -> Made {
    let mut ch = Chunk::new_air();
    ch.temp.fill(ChunkCells::MATERIAL_DEFAULT);
    let awake = {
        let Chunk { mat, temp, .. } = &mut *ch;
        let mut cells = ChunkCells { pos: c, seed, mat, temp, awake: false };
        source.generate(&mut cells);
        cells.awake
    };
    let mut all_air = !awake;
    let mut rng = Rng::for_chunk(seed, 0, c, GENERATE_SALT);
    for i in 0..CHUNK_AREA {
        let m = ch.mat[i] as usize;
        if ch.temp[i] == ChunkCells::MATERIAL_DEFAULT {
            ch.temp[i] = mats.temperature[m];
        }
        let r = rng.next_u64();
        if m == 0 {
            all_air &= ch.temp[i] == DEFAULT_TEMPERATURE;
            continue;
        }
        all_air = false;
        ch.shade[i] = r as u8;
        if let Some((lo, hi)) = mats.life[m] {
            // A number in lo..=hi from the high 32 bits of `r`.
            ch.life[i] = lo + (((r >> 32) * (hi - lo) as u64 + (r >> 32)) >> 32) as u8;
        }
    }
    if all_air {
        return Made::Air;
    }
    ch.version = GENERATED_VERSION;
    ch.dirty = if awake { LocalRect::FULL } else { LocalRect::EMPTY };
    ch.pristine = true;
    Made::Cells(ch)
}
