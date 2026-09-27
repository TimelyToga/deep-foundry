//! Rotation and flip of a building, and the cells next to a port.
//!
//! Rules (y goes down):
//! - `flip` mirrors the building left to right. It is done first.
//! - `rotation` turns the building 90 degrees clockwise, `rotation` times (0 to 3).
//! - A building of `size` (w, h) tiles has size (h, w) after a rotation of 1 or 3.

use foundry_content::Side;
use foundry_core::{CellPos, CellRect, TILE_SIZE, TilePos};
use serde::{Deserialize, Serialize};

/// How a building is turned and mirrored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Transform {
    /// Clockwise quarter turns, 0 to 3.
    pub rotation: u8,
    /// Mirror left to right (before the rotation).
    pub flip: bool,
}

impl Transform {
    pub const IDENTITY: Transform = Transform { rotation: 0, flip: false };

    /// `rotation` is taken modulo 4.
    pub const fn new(rotation: u8, flip: bool) -> Self {
        Self { rotation: rotation & 3, flip }
    }

    /// The size in tiles after the transform.
    pub const fn size(self, size: (u8, u8)) -> (u8, u8) {
        if self.rotation & 1 == 1 { (size.1, size.0) } else { size }
    }

    /// Where a tile of the building (in data coordinates) is after the transform.
    /// The result is an offset from the top-left tile of the placed footprint.
    pub fn tile(self, size: (u8, u8), tile: (u8, u8)) -> (u8, u8) {
        let (mut w, mut h) = size;
        let (mut x, mut y) = tile;
        if self.flip {
            x = w - 1 - x;
        }
        for _ in 0..self.rotation {
            // One clockwise quarter turn of a w × h grid: (x, y) -> (h - 1 - y, x).
            let nx = h - 1 - y;
            let ny = x;
            x = nx;
            y = ny;
            std::mem::swap(&mut w, &mut h);
        }
        (x, y)
    }

    /// Where a side (in data coordinates) points after the transform.
    pub fn side(self, side: Side) -> Side {
        let mut s = if self.flip { mirror(side) } else { side };
        for _ in 0..self.rotation {
            s = clockwise(s);
        }
        s
    }
}

/// The side after a clockwise quarter turn.
pub const fn clockwise(s: Side) -> Side {
    match s {
        Side::Up => Side::Right,
        Side::Right => Side::Down,
        Side::Down => Side::Left,
        Side::Left => Side::Up,
    }
}

/// The side after a left-right mirror.
pub const fn mirror(s: Side) -> Side {
    match s {
        Side::Left => Side::Right,
        Side::Right => Side::Left,
        other => other,
    }
}

pub const fn opposite(s: Side) -> Side {
    match s {
        Side::Up => Side::Down,
        Side::Down => Side::Up,
        Side::Left => Side::Right,
        Side::Right => Side::Left,
    }
}

/// One step toward a side, as (dx, dy).
pub const fn side_step(s: Side) -> (i32, i32) {
    match s {
        Side::Up => (0, -1),
        Side::Down => (0, 1),
        Side::Left => (-1, 0),
        Side::Right => (1, 0),
    }
}

/// The tile next to `tile` on a side.
pub const fn neighbor_tile(tile: TilePos, s: Side) -> TilePos {
    let (dx, dy) = side_step(s);
    TilePos { x: tile.x + dx, y: tile.y + dy }
}

/// The cells of a rectangle of tiles.
pub const fn tiles_to_cells(at: TilePos, size: (u8, u8)) -> CellRect {
    let o = at.origin();
    CellRect { x0: o.x, y0: o.y, x1: o.x + size.0 as i32 * TILE_SIZE, y1: o.y + size.1 as i32 * TILE_SIZE }
}

/// Positions along one tile side, from the middle outward: 3, 4, 2, 5, 1, 6, 0, 7.
/// A port puts cells near its middle first, so the output makes one stream.
pub const MIDDLE_OUT: [i32; TILE_SIZE as usize] = [3, 4, 2, 5, 1, 6, 0, 7];

/// The cell outside a tile side: `row` 0 is the row (or column) that touches the side, row 1 the
/// next one out, and so on. `along` is 0 to 7 along the side.
pub const fn outside_cell(tile: TilePos, s: Side, row: i32, along: i32) -> CellPos {
    let o = tile.origin();
    match s {
        Side::Up => CellPos { x: o.x + along, y: o.y - 1 - row },
        Side::Down => CellPos { x: o.x + along, y: o.y + TILE_SIZE + row },
        Side::Left => CellPos { x: o.x - 1 - row, y: o.y + along },
        Side::Right => CellPos { x: o.x + TILE_SIZE + row, y: o.y + along },
    }
}

/// The cells outside a tile side, `depth` rows deep. The nearest row comes first; each row goes
/// from the middle outward.
pub fn outside_cells(tile: TilePos, s: Side, depth: i32) -> impl Iterator<Item = CellPos> {
    (0..depth).flat_map(move |row| MIDDLE_OUT.iter().map(move |&along| outside_cell(tile, s, row, along)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Side; 4] = [Side::Up, Side::Right, Side::Down, Side::Left];

    #[test]
    fn four_turns_and_two_flips_change_nothing() {
        let size = (3, 2);
        for x in 0..3 {
            for y in 0..2 {
                let mut t = (x, y);
                let mut sz = size;
                for _ in 0..4 {
                    t = Transform::new(1, false).tile(sz, t);
                    sz = Transform::new(1, false).size(sz);
                }
                assert_eq!(t, (x, y));
                let f = Transform::new(0, true);
                assert_eq!(f.tile(size, f.tile(size, (x, y))), (x, y));
            }
        }
        for s in ALL {
            assert_eq!(Transform::new(4, false).side(s), s);
            let f = Transform::new(0, true);
            assert_eq!(f.side(f.side(s)), s);
        }
    }

    #[test]
    fn quarter_turn_moves_corners_clockwise() {
        // A 3 × 2 building. After one turn it is 2 × 3.
        let t = Transform::new(1, false);
        assert_eq!(t.size((3, 2)), (2, 3));
        assert_eq!(t.tile((3, 2), (0, 0)), (1, 0)); // top-left -> top-right
        assert_eq!(t.tile((3, 2), (2, 0)), (1, 2)); // top-right -> bottom-right
        assert_eq!(t.tile((3, 2), (2, 1)), (0, 2)); // bottom-right -> bottom-left
        assert_eq!(t.tile((3, 2), (0, 1)), (0, 0)); // bottom-left -> top-left
        assert_eq!(t.side(Side::Up), Side::Right);
        assert_eq!(t.side(Side::Left), Side::Up);
    }

    #[test]
    fn half_turn_and_flip() {
        let t = Transform::new(2, false);
        assert_eq!(t.size((3, 2)), (3, 2));
        assert_eq!(t.tile((3, 2), (0, 0)), (2, 1));
        assert_eq!(t.side(Side::Down), Side::Up);
        let f = Transform::new(0, true);
        assert_eq!(f.tile((3, 2), (0, 1)), (2, 1));
        assert_eq!(f.side(Side::Left), Side::Right);
        assert_eq!(f.side(Side::Up), Side::Up);
        // Flip first, then turn: the port on the left of the top-left tile...
        let ft = Transform::new(1, true);
        // flip: (0,0) Left -> (2,0) Right; turn: (2,0) in 3x2 -> (1,2), Right -> Down.
        assert_eq!(ft.tile((3, 2), (0, 0)), (1, 2));
        assert_eq!(ft.side(Side::Left), Side::Down);
    }

    #[test]
    fn a_port_stays_on_the_outside_edge() {
        // For every transform, a port on an outside edge stays on an outside edge and faces out.
        let size = (3, 2);
        for rot in 0..4 {
            for flip in [false, true] {
                let t = Transform::new(rot, flip);
                let (w, h) = t.size(size);
                for (tile, side) in [((0u8, 0u8), Side::Left), ((2, 1), Side::Down), ((1, 0), Side::Up), ((2, 0), Side::Right)] {
                    let (x, y) = t.tile(size, tile);
                    let s = t.side(side);
                    let (dx, dy) = side_step(s);
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    let outside = nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32;
                    assert!(outside, "rot {rot} flip {flip} tile {tile:?} side {side:?}");
                }
            }
        }
    }

    #[test]
    fn outside_cells_start_next_to_the_side() {
        let tile = TilePos::new(2, 3); // cells x 16..24, y 24..32
        let first: Vec<_> = outside_cells(tile, Side::Up, 2).collect();
        assert_eq!(first.len(), 16);
        assert_eq!(first[0], CellPos::new(19, 23));
        assert!(first[..8].iter().all(|p| p.y == 23 && (16..24).contains(&p.x)));
        assert!(first[8..].iter().all(|p| p.y == 22));
        assert_eq!(outside_cell(tile, Side::Right, 0, 0), CellPos::new(24, 24));
        assert_eq!(outside_cell(tile, Side::Down, 1, 7), CellPos::new(23, 33));
        assert_eq!(outside_cell(tile, Side::Left, 0, 0), CellPos::new(15, 24));
    }
}
