//! Where the sky light enters the light area from above.
//!
//! The light pass works only on the view and a margin. Sky light that falls into this area from
//! above must know if a cell above the area stops it. The renderer has only the chunks near the
//! view (and older chunks it did not drop yet), so it looks up through the chunks it has:
//!
//! - A cell that blocks light (rock, sand) stops the sky light in its column.
//! - A chunk with no data counts as open air if it is above the row `open_above`, else as rock.
//!   So deep underground there is no sky light, and on the surface there is.
//! - Above the top of the world (y < 0) there is always sky.
//!
//! This part has no GPU code, so it has plain unit tests.

use foundry_core::{CHUNK_SIZE, CellTexel, ChunkPos};
use std::collections::HashMap;

/// For each column of a chunk: the first row from the top with a cell that blocks the sky light.
/// `CHUNK_SIZE` if no cell in the column blocks it.
type ColumnTops = [u8; CHUNK_SIZE as usize];

pub(crate) struct SkyColumns {
    tops: HashMap<ChunkPos, ColumnTops>,
    /// For each material: true if it blocks the sky light.
    blockers: Vec<bool>,
}

impl SkyColumns {
    pub fn new(blockers: Vec<bool>) -> Self {
        Self { tops: HashMap::new(), blockers }
    }

    /// Remember the blocking cells of a chunk (from its texels, row by row from the top).
    pub fn update(&mut self, pos: ChunkPos, texels: &[CellTexel]) {
        let n = CHUNK_SIZE as usize;
        let mut tops = [CHUNK_SIZE as u8; CHUNK_SIZE as usize];
        let mut left = n;
        'rows: for (y, row) in texels.chunks_exact(n).enumerate() {
            for (x, texel) in row.iter().enumerate() {
                if tops[x] as usize == n && self.blockers.get(texel[0] as usize).copied().unwrap_or(false) {
                    tops[x] = y as u8;
                    left -= 1;
                    if left == 0 {
                        break 'rows;
                    }
                }
            }
        }
        self.tops.insert(pos, tops);
    }

    pub fn remove(&mut self, pos: ChunkPos) {
        self.tops.remove(&pos);
    }

    pub fn clear(&mut self) {
        self.tops.clear();
    }

    /// The sky light (0 to 1) that reaches row `y_top` from above, for `count` groups of `step`
    /// columns from column `x0`: `out[i]` is for the columns `x0 + i * step .. x0 + (i + 1) * step`.
    /// It is the part of the columns of the group with no blocking cell above `y_top`.
    pub fn entry(&self, x0: i32, y_top: i32, step: i32, count: usize, open_above: i32, out: &mut Vec<f32>) {
        out.clear();
        out.resize(count, 0.0);
        if count == 0 || step <= 0 {
            return;
        }
        let x_end = x0 + step * count as i32;
        let part = 1.0 / step as f32;
        let first_row = (y_top - 1).div_euclid(CHUNK_SIZE);
        for cx in x0.div_euclid(CHUNK_SIZE)..=(x_end - 1).div_euclid(CHUNK_SIZE) {
            let left = cx * CHUNK_SIZE;
            let lo = (x0.max(left) - left) as u32;
            let hi = (x_end.min(left + CHUNK_SIZE) - left) as u32;
            // One bit for each column of this chunk column that is not decided yet.
            let mut unknown: u64 = mask(lo, hi);
            let mut open: u64 = 0;
            let mut row = first_row;
            while unknown != 0 {
                if row < 0 {
                    // Above the top of the world: the sky.
                    open |= unknown;
                    break;
                }
                match self.tops.get(&ChunkPos::new(cx, row)) {
                    Some(tops) => {
                        // Only the rows above `y_top` count.
                        let last = (y_top - 1 - row * CHUNK_SIZE).min(CHUNK_SIZE - 1);
                        let mut bits = unknown;
                        while bits != 0 {
                            let c = bits.trailing_zeros();
                            bits &= bits - 1;
                            if tops[c as usize] as i32 <= last {
                                unknown &= !(1u64 << c);
                            }
                        }
                    }
                    None if (row + 1) * CHUNK_SIZE <= open_above => {}
                    None => break,
                }
                row -= 1;
            }
            let mut bits = open;
            while bits != 0 {
                let c = bits.trailing_zeros() as i32;
                bits &= bits - 1;
                out[((left + c - x0) / step) as usize] += part;
            }
        }
    }
}

/// Bits `lo..hi` set.
fn mask(lo: u32, hi: u32) -> u64 {
    let upto = |n: u32| if n >= 64 { u64::MAX } else { (1u64 << n) - 1 };
    upto(hi) & !upto(lo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundry_core::{CHUNK_AREA, local_index};

    const ROCK: u16 = 1;

    fn sky() -> SkyColumns {
        SkyColumns::new(vec![false, true])
    }

    /// A chunk of air with rock at the given local cells.
    fn chunk(rock: &[(i32, i32)]) -> Vec<CellTexel> {
        let mut t = vec![[0u16; 4]; CHUNK_AREA];
        for &(x, y) in rock {
            t[local_index(x, y)][0] = ROCK;
        }
        t
    }

    #[test]
    fn mask_bits() {
        assert_eq!(mask(0, 64), u64::MAX);
        assert_eq!(mask(0, 3), 0b111);
        assert_eq!(mask(2, 4), 0b1100);
        assert_eq!(mask(5, 5), 0);
    }

    #[test]
    fn unknown_chunks_are_air_above_the_level_and_rock_below() {
        let s = sky();
        let mut out = vec![];
        s.entry(0, 500, 4, 8, 1000, &mut out);
        assert_eq!(out, vec![1.0; 8]);
        s.entry(0, 500, 4, 8, 0, &mut out);
        assert_eq!(out, vec![0.0; 8]);
        // A light area that starts above the world always has sky.
        s.entry(-100, -20, 4, 8, 0, &mut out);
        assert_eq!(out, vec![1.0; 8]);
    }

    #[test]
    fn a_rock_above_the_area_blocks_its_column() {
        let mut s = sky();
        // Chunk row 7 covers y = 448..512. Rock in column 3 at y = 458.
        s.update(ChunkPos::new(0, 7), &chunk(&[(3, 10)]));
        let mut out = vec![];
        s.entry(0, 500, 4, 4, 10_000, &mut out);
        assert_eq!(out, vec![0.75, 1.0, 1.0, 1.0]);
        // The same rock below the top of the light area does not count.
        s.entry(0, 450, 4, 4, 10_000, &mut out);
        assert_eq!(out, vec![1.0; 4]);
        // Single columns, from x = 2.
        s.entry(2, 500, 1, 3, 10_000, &mut out);
        assert_eq!(out, vec![1.0, 0.0, 1.0]);
    }

    #[test]
    fn known_air_chunks_let_the_sky_through_below_the_level() {
        let mut s = sky();
        // Deep underground: a shaft of known air chunks from y = 0 down to the area.
        for row in 0..10 {
            s.update(ChunkPos::new(-1, row), &chunk(&[]));
        }
        let mut out = vec![];
        s.entry(-64, 640, 64, 1, 0, &mut out);
        assert_eq!(out, vec![1.0]);
        // One chunk of the shaft is missing: below the level it counts as rock.
        s.remove(ChunkPos::new(-1, 4));
        s.entry(-64, 640, 64, 1, 0, &mut out);
        assert_eq!(out, vec![0.0]);
        s.clear();
        s.entry(-64, 640, 64, 1, 10_000, &mut out);
        assert_eq!(out, vec![1.0]);
    }

    #[test]
    fn groups_across_chunk_borders() {
        let mut s = sky();
        // Rock over columns 62, 63 of chunk 0 and column 0 of chunk 1.
        s.update(ChunkPos::new(0, 0), &chunk(&[(62, 5), (63, 5)]));
        s.update(ChunkPos::new(1, 0), &chunk(&[(0, 5)]));
        let mut out = vec![];
        s.entry(60, 40, 4, 2, 0, &mut out);
        assert_eq!(out, vec![0.5, 0.75]);
    }
}
