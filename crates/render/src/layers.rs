//! Which chunk is in which layer of the cell texture array.
//!
//! This part has no GPU code, so it has plain unit tests.

use foundry_core::{CHUNK_SIZE, CellRect, ChunkPos};
use std::collections::HashMap;

pub(crate) struct LayerMap {
    capacity: u32,
    layer_of: HashMap<ChunkPos, u32>,
    /// The chunk in each layer.
    owner: Vec<Option<ChunkPos>>,
    free: Vec<u32>,
    /// Reused list for eviction: (touches the view, squared distance, chunk).
    scratch: Vec<(bool, i64, ChunkPos)>,
}

impl LayerMap {
    pub fn new(capacity: u32) -> Self {
        Self {
            capacity,
            layer_of: HashMap::with_capacity(capacity as usize),
            owner: vec![None; capacity as usize],
            free: (0..capacity).rev().collect(),
            scratch: Vec::with_capacity(capacity as usize),
        }
    }

    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    /// Number of chunks that have a layer.
    pub fn len(&self) -> usize {
        self.layer_of.len()
    }

    #[inline]
    pub fn get(&self, pos: ChunkPos) -> Option<u32> {
        self.layer_of.get(&pos).copied()
    }

    /// Forget all chunks.
    pub fn clear(&mut self) {
        self.layer_of.clear();
        self.owner.iter_mut().for_each(|o| *o = None);
        self.free.clear();
        self.free.extend((0..self.capacity).rev());
    }

    /// The layer for a chunk. If the chunk has none, give it a free layer.
    ///
    /// If no layer is free, drop a group of chunks first: the chunks farthest from the center of `view`.
    /// Chunks that touch `view` are dropped only when there is no other choice.
    /// The dropped chunks are added to `evicted`. Returns `None` only if the capacity is 0.
    pub fn assign(&mut self, pos: ChunkPos, view: CellRect, evicted: &mut Vec<ChunkPos>) -> Option<u32> {
        if let Some(layer) = self.get(pos) {
            return Some(layer);
        }
        if self.free.is_empty() {
            self.evict_far(view, evicted);
        }
        let layer = self.free.pop()?;
        self.layer_of.insert(pos, layer);
        self.owner[layer as usize] = Some(pos);
        Some(layer)
    }

    /// Free about 1/16 of all layers (at least one), farthest from the view first.
    fn evict_far(&mut self, view: CellRect, evicted: &mut Vec<ChunkPos>) {
        let want = (self.capacity as usize / 16).max(1);
        let cx = (view.x0 as i64 + view.x1 as i64) / 2;
        let cy = (view.y0 as i64 + view.y1 as i64) / 2;
        let half = CHUNK_SIZE as i64 / 2;
        self.scratch.clear();
        self.scratch.extend(self.layer_of.keys().map(|&p| {
            let dx = p.x as i64 * CHUNK_SIZE as i64 + half - cx;
            let dy = p.y as i64 * CHUNK_SIZE as i64 + half - cy;
            let d = dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy));
            let visible = !p.cell_rect().intersect(&view).is_empty();
            (visible, d, p)
        }));
        // Chunks outside the view first, then the farthest first.
        // Ties are broken by position, so the result does not depend on the hash order.
        self.scratch.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)).then(b.2.cmp(&a.2)));
        for i in 0..want.min(self.scratch.len()) {
            let p = self.scratch[i].2;
            if let Some(layer) = self.layer_of.remove(&p) {
                self.owner[layer as usize] = None;
                self.free.push(layer);
                evicted.push(p);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assign_reuses_layers() {
        let mut m = LayerMap::new(4);
        let view = CellRect::new(0, 0, 64, 64);
        let mut ev = vec![];
        let a = m.assign(ChunkPos::new(0, 0), view, &mut ev).unwrap();
        assert_eq!(m.assign(ChunkPos::new(0, 0), view, &mut ev), Some(a));
        assert_eq!(m.len(), 1);
        assert!(ev.is_empty());
    }

    #[test]
    fn full_map_evicts_farthest_and_keeps_visible() {
        let mut m = LayerMap::new(4);
        let view = CellRect::new(0, 0, 128, 64); // chunks (0,0) and (1,0)
        let mut ev = vec![];
        for p in [ChunkPos::new(0, 0), ChunkPos::new(1, 0), ChunkPos::new(5, 0), ChunkPos::new(-9, 3)] {
            m.assign(p, view, &mut ev).unwrap();
        }
        assert!(ev.is_empty());
        let layer = m.assign(ChunkPos::new(2, 0), view, &mut ev).unwrap();
        assert_eq!(ev, vec![ChunkPos::new(-9, 3)], "the farthest chunk goes first");
        assert_eq!(m.get(ChunkPos::new(2, 0)), Some(layer));
        assert_eq!(m.get(ChunkPos::new(-9, 3)), None);
        assert_eq!(m.len(), 4);
        // Fill again: now (5,0) is the farthest.
        ev.clear();
        m.assign(ChunkPos::new(0, 1), view, &mut ev).unwrap();
        assert_eq!(ev, vec![ChunkPos::new(5, 0)]);
        // The visible chunks are still there.
        assert!(m.get(ChunkPos::new(0, 0)).is_some() && m.get(ChunkPos::new(1, 0)).is_some());
    }

    #[test]
    fn clear_frees_all() {
        let mut m = LayerMap::new(2);
        let mut ev = vec![];
        m.assign(ChunkPos::new(0, 0), CellRect::EMPTY, &mut ev);
        m.clear();
        assert_eq!(m.len(), 0);
        assert!(m.assign(ChunkPos::new(3, 3), CellRect::EMPTY, &mut ev).is_some());
        assert!(m.assign(ChunkPos::new(4, 3), CellRect::EMPTY, &mut ev).is_some());
        assert!(ev.is_empty());
    }
}
