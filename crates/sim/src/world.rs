//! The grid of chunks. Chunks full of air are not stored until something is written into them.

use crate::chunk::{Chunk, LocalRect};
use foundry_core::{CHUNK_MASK, CHUNK_SIZE, CellPos, ChunkPos, MaterialId};

pub struct World {
    pub width_chunks: i32,
    pub height_chunks: i32,
    /// Row by row. `None` means the chunk is all air at the default temperature.
    pub chunks: Vec<Option<Box<Chunk>>>,
    /// The material that cells outside the world act as (bedrock).
    pub outside: MaterialId,
}

impl World {
    pub fn new(width_chunks: i32, height_chunks: i32, outside: MaterialId) -> Self {
        let n = (width_chunks * height_chunks) as usize;
        Self { width_chunks, height_chunks, chunks: (0..n).map(|_| None).collect(), outside }
    }

    pub fn width_cells(&self) -> i32 {
        self.width_chunks * CHUNK_SIZE
    }

    pub fn height_cells(&self) -> i32 {
        self.height_chunks * CHUNK_SIZE
    }

    #[inline(always)]
    pub fn chunk_in_bounds(&self, c: ChunkPos) -> bool {
        c.x >= 0 && c.y >= 0 && c.x < self.width_chunks && c.y < self.height_chunks
    }

    #[inline(always)]
    pub fn in_bounds(&self, p: CellPos) -> bool {
        p.x >= 0 && p.y >= 0 && p.x < self.width_cells() && p.y < self.height_cells()
    }

    #[inline(always)]
    pub fn chunk_index(&self, c: ChunkPos) -> usize {
        (c.y * self.width_chunks + c.x) as usize
    }

    pub fn chunk(&self, c: ChunkPos) -> Option<&Chunk> {
        if !self.chunk_in_bounds(c) {
            return None;
        }
        self.chunks[self.chunk_index(c)].as_deref()
    }

    /// The chunk, made if it does not exist yet. `None` only outside the world.
    pub fn chunk_mut(&mut self, c: ChunkPos) -> Option<&mut Chunk> {
        if !self.chunk_in_bounds(c) {
            return None;
        }
        let i = self.chunk_index(c);
        Some(self.chunks[i].get_or_insert_with(Chunk::new_air))
    }

    /// Material at a cell. Outside the world: bedrock.
    #[inline]
    pub fn mat(&self, p: CellPos) -> MaterialId {
        if !self.in_bounds(p) {
            return self.outside;
        }
        match &self.chunks[self.chunk_index(p.chunk())] {
            Some(ch) => MaterialId(ch.mat[p.local_index()]),
            None => MaterialId::AIR,
        }
    }

    /// Iterate over the positions of chunks that exist.
    pub fn loaded_chunks(&self) -> impl Iterator<Item = ChunkPos> + '_ {
        let w = self.width_chunks;
        self.chunks.iter().enumerate().filter(|(_, c)| c.is_some()).map(move |(i, _)| ChunkPos::new(i as i32 % w, i as i32 / w))
    }

    /// Update this cell and its 8 neighbors in the next tick. Chunks that do not exist are skipped
    /// (they are all air, and air never needs an update).
    pub fn mark_dirty_around(&mut self, p: CellPos) {
        let (lx, ly) = (p.x & CHUNK_MASK, p.y & CHUNK_MASK);
        if lx > 0 && lx < CHUNK_MASK && ly > 0 && ly < CHUNK_MASK {
            if let Some(c) = self.chunk_mut_if_exists(p.chunk()) {
                c.dirty.add_rect(LocalRect { x0: lx - 1, y0: ly - 1, x1: lx + 2, y1: ly + 2 });
            }
            return;
        }
        for dy in -1..=1 {
            for dx in -1..=1 {
                let q = p.offset(dx, dy);
                if let Some(c) = self.chunk_mut_if_exists(q.chunk()) {
                    c.dirty.add_point(q.x & CHUNK_MASK, q.y & CHUNK_MASK);
                }
            }
        }
    }

    /// The chunk if it exists. Never makes a new chunk.
    pub fn chunk_mut_if_exists(&mut self, c: ChunkPos) -> Option<&mut Chunk> {
        if !self.chunk_in_bounds(c) {
            return None;
        }
        let i = self.chunk_index(c);
        self.chunks[i].as_deref_mut()
    }
}
