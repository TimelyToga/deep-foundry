//! Positions. x goes right. y goes down. y = 0 is the top of the world.

use crate::consts::*;
use serde::{Deserialize, Serialize};

/// A cell position in the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct CellPos {
    pub x: i32,
    pub y: i32,
}

impl CellPos {
    #[inline(always)]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    #[inline(always)]
    pub const fn chunk(self) -> ChunkPos {
        ChunkPos { x: self.x >> CHUNK_SHIFT, y: self.y >> CHUNK_SHIFT }
    }

    #[inline(always)]
    pub const fn tile(self) -> TilePos {
        TilePos { x: self.x >> TILE_SHIFT, y: self.y >> TILE_SHIFT }
    }

    /// Index of this cell inside its chunk's arrays.
    #[inline(always)]
    pub const fn local_index(self) -> usize {
        local_index(self.x & CHUNK_MASK, self.y & CHUNK_MASK)
    }

    #[inline(always)]
    pub const fn offset(self, dx: i32, dy: i32) -> Self {
        Self { x: self.x + dx, y: self.y + dy }
    }
}

/// Index into a chunk array from local coordinates (0..64). Rows are stored top to bottom.
#[inline(always)]
pub const fn local_index(lx: i32, ly: i32) -> usize {
    ((ly as usize) << CHUNK_SHIFT) | lx as usize
}

/// Local coordinates from an index into a chunk array.
#[inline(always)]
pub const fn local_xy(index: usize) -> (i32, i32) {
    ((index as i32) & CHUNK_MASK, (index as i32) >> CHUNK_SHIFT)
}

/// A tile position (8 × 8 cells).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct TilePos {
    pub x: i32,
    pub y: i32,
}

impl TilePos {
    #[inline(always)]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// The top-left cell of this tile.
    #[inline(always)]
    pub const fn origin(self) -> CellPos {
        CellPos { x: self.x << TILE_SHIFT, y: self.y << TILE_SHIFT }
    }
}

/// A chunk position (64 × 64 cells).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ChunkPos {
    pub x: i32,
    pub y: i32,
}

impl ChunkPos {
    #[inline(always)]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// The top-left cell of this chunk.
    #[inline(always)]
    pub const fn origin(self) -> CellPos {
        CellPos { x: self.x << CHUNK_SHIFT, y: self.y << CHUNK_SHIFT }
    }

    /// The cells of this chunk as a rectangle.
    pub const fn cell_rect(self) -> CellRect {
        let o = self.origin();
        CellRect { x0: o.x, y0: o.y, x1: o.x + CHUNK_SIZE, y1: o.y + CHUNK_SIZE }
    }
}

/// A rectangle of cells. `x0`, `y0` are inside. `x1`, `y1` are outside (exclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct CellRect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl CellRect {
    pub const EMPTY: CellRect = CellRect { x0: 0, y0: 0, x1: 0, y1: 0 };

    pub const fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        Self { x0, y0, x1, y1 }
    }

    /// The square that contains the circle with this center and radius.
    pub const fn around(center: CellPos, radius: i32) -> Self {
        Self { x0: center.x - radius, y0: center.y - radius, x1: center.x + radius + 1, y1: center.y + radius + 1 }
    }

    pub const fn width(&self) -> i32 {
        self.x1 - self.x0
    }

    pub const fn height(&self) -> i32 {
        self.y1 - self.y0
    }

    pub const fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    pub const fn contains(&self, p: CellPos) -> bool {
        p.x >= self.x0 && p.x < self.x1 && p.y >= self.y0 && p.y < self.y1
    }

    pub fn intersect(&self, o: &CellRect) -> CellRect {
        CellRect { x0: self.x0.max(o.x0), y0: self.y0.max(o.y0), x1: self.x1.min(o.x1), y1: self.y1.min(o.y1) }
    }

    /// The smallest rectangle that contains both. An empty rectangle adds nothing.
    pub fn union(&self, o: &CellRect) -> CellRect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        CellRect { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }

    /// Grow on all sides.
    pub const fn expand(&self, by: i32) -> CellRect {
        CellRect { x0: self.x0 - by, y0: self.y0 - by, x1: self.x1 + by, y1: self.y1 + by }
    }

    /// All chunks that touch this rectangle, row by row.
    pub fn chunks(&self) -> impl Iterator<Item = ChunkPos> + use<> {
        let (cx0, cy0) = (self.x0 >> CHUNK_SHIFT, self.y0 >> CHUNK_SHIFT);
        let (cx1, cy1) = if self.is_empty() {
            (cx0 - 1, cy0 - 1)
        } else {
            ((self.x1 - 1) >> CHUNK_SHIFT, (self.y1 - 1) >> CHUNK_SHIFT)
        };
        (cy0..=cy1).flat_map(move |y| (cx0..=cx1).map(move |x| ChunkPos { x, y }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_cells_use_floor_division() {
        assert_eq!(CellPos::new(-1, -1).chunk(), ChunkPos::new(-1, -1));
        assert_eq!(CellPos::new(-64, 0).chunk(), ChunkPos::new(-1, 0));
        assert_eq!(CellPos::new(-65, 0).chunk(), ChunkPos::new(-2, 0));
        assert_eq!(CellPos::new(-1, 0).local_index(), 63);
        assert_eq!(CellPos::new(-1, -8).tile(), TilePos::new(-1, -1));
    }

    #[test]
    fn local_index_round_trip() {
        for i in [0usize, 1, 63, 64, 4095] {
            let (x, y) = local_xy(i);
            assert_eq!(local_index(x, y), i);
        }
    }

    #[test]
    fn rect_chunks() {
        let r = CellRect::new(0, 0, 65, 64);
        let c: Vec<_> = r.chunks().collect();
        assert_eq!(c, vec![ChunkPos::new(0, 0), ChunkPos::new(1, 0)]);
        assert_eq!(CellRect::EMPTY.chunks().count(), 0);
    }
}
