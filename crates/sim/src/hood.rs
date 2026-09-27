//! `Hood`: access to the 3 × 3 chunks around the chunk that one job updates.
//!
//! Coordinates are relative to the top-left cell of the center chunk. The center chunk has
//! x and y in 0..64. The whole hood is -64..128.
//!
//! Safety rule of the parallel update: in one pass, jobs update chunks that have one chunk
//! between them. A job reads and writes only cells within `MAX_CELL_MOVE` (32) cells of its own
//! chunk, plus one more cell for dirty marks. So two jobs of one pass never touch the same cell.
//! All cell reads and writes go through raw element pointers (never `&mut Chunk`), because two
//! jobs can hold pointers to the same neighbor chunk at the same time.

use crate::SimEvent;
use crate::react::ReactTable;
use crate::chunk::{Chunk, FLAG_PARITY, LocalRect};
use foundry_content::{MaterialTable, Phase};
use foundry_core::{CHUNK_MASK, CHUNK_SHIFT, CellPos, MAX_CELL_MOVE, MaterialId, Rng};
use std::ptr::{addr_of, addr_of_mut};

/// Farthest a job may reach outside its own chunk (cell moves plus one cell for marks).
const REACH: i32 = MAX_CELL_MOVE + 1;

pub struct Hood<'a> {
    /// Row by row: index `(dy + 1) * 3 + (dx + 1)`. Null means outside the world.
    ptrs: [*mut Chunk; 9],
    pub mats: &'a MaterialTable,
    pub react: &'a ReactTable,
    pub rng: Rng,
    /// `tick & 1`.
    pub parity: u8,
    /// Cells to update in the next tick, for each of the 9 chunks.
    pub marks: [LocalRect; 9],
    /// Bit `s` is set if a cell in chunk slot `s` changed (the renderer needs a new image).
    pub changed: u16,
    /// Bit `s` is set if any cell array of chunk slot `s` was written, also without a visible
    /// change (for example the updated bit). Such a chunk is no longer pristine.
    pub touched: u16,
    /// Material of cells outside the world.
    pub outside: MaterialId,
    /// World position of the center chunk's top-left cell.
    pub origin: CellPos,
    /// Events for the simulation to handle after the pass (explosions, ...).
    pub events: Vec<SimEvent>,
}

impl<'a> Hood<'a> {
    /// # Safety
    /// The pointers must be valid for the whole life of the hood, and the caller must follow the
    /// safety rule in the module documentation.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn new(
        ptrs: [*mut Chunk; 9],
        mats: &'a MaterialTable,
        react: &'a ReactTable,
        rng: Rng,
        parity: u8,
        outside: MaterialId,
        origin: CellPos,
    ) -> Self {
        Self { ptrs, mats, react, rng, parity, marks: [LocalRect::EMPTY; 9], changed: 0, touched: 0, outside, origin, events: Vec::new() }
    }

    /// World position of a hood cell.
    #[inline(always)]
    pub fn world_pos(&self, x: i32, y: i32) -> CellPos {
        self.origin.offset(x, y)
    }

    /// Send an event to the simulation. It is handled after the movement passes.
    #[inline]
    pub fn emit(&mut self, event: SimEvent) {
        self.events.push(event);
    }

    #[inline(always)]
    fn slot(x: i32, y: i32) -> (usize, usize) {
        debug_assert!((-REACH..64 + REACH).contains(&x) && (-REACH..64 + REACH).contains(&y), "hood access out of reach: {x},{y}");
        let s = (((y >> CHUNK_SHIFT) + 1) * 3 + (x >> CHUNK_SHIFT) + 1) as usize;
        let i = (((y & CHUNK_MASK) << CHUNK_SHIFT) | (x & CHUNK_MASK)) as usize;
        (s, i)
    }

    /// True if the cell is inside the world.
    #[inline(always)]
    pub fn inside(&self, x: i32, y: i32) -> bool {
        !self.ptrs[Self::slot(x, y).0].is_null()
    }

    #[inline(always)]
    pub fn mat(&self, x: i32, y: i32) -> MaterialId {
        let (s, i) = Self::slot(x, y);
        let p = self.ptrs[s];
        if p.is_null() {
            return self.outside;
        }
        // SAFETY: valid pointer (see `new`); element access only.
        MaterialId(unsafe { *addr_of!((*p).mat).cast::<u16>().add(i) })
    }

    #[inline(always)]
    pub fn phase(&self, x: i32, y: i32) -> Phase {
        self.mats.phase[self.mat(x, y).index()]
    }

    #[inline(always)]
    pub fn temp(&self, x: i32, y: i32) -> i16 {
        let (s, i) = Self::slot(x, y);
        let p = self.ptrs[s];
        if p.is_null() {
            return foundry_core::DEFAULT_TEMPERATURE;
        }
        // SAFETY: as in `mat`.
        unsafe { *addr_of!((*p).temp).cast::<i16>().add(i) }
    }

    /// Set the temperature. Does not mark the cell; call `mark_changed` or `keep_awake` if needed.
    #[inline(always)]
    pub fn set_temp(&mut self, x: i32, y: i32, t: i16) {
        let (s, i) = Self::slot(x, y);
        let p = self.ptrs[s];
        if !p.is_null() {
            self.touched |= 1 << s;
            // SAFETY: as in `mat`.
            unsafe { *addr_of_mut!((*p).temp).cast::<i16>().add(i) = t }
        }
    }

    #[inline(always)]
    pub fn life(&self, x: i32, y: i32) -> u8 {
        self.read_u8(x, y, |p| unsafe { addr_of!((*p).life).cast::<u8>() })
    }

    #[inline(always)]
    pub fn set_life(&mut self, x: i32, y: i32, v: u8) {
        self.write_u8(x, y, v, |p| unsafe { addr_of_mut!((*p).life).cast::<u8>() })
    }

    #[inline(always)]
    pub fn motion(&self, x: i32, y: i32) -> u8 {
        self.read_u8(x, y, |p| unsafe { addr_of!((*p).motion).cast::<u8>() })
    }

    #[inline(always)]
    pub fn set_motion(&mut self, x: i32, y: i32, v: u8) {
        self.write_u8(x, y, v, |p| unsafe { addr_of_mut!((*p).motion).cast::<u8>() })
    }

    #[inline(always)]
    pub fn flags(&self, x: i32, y: i32) -> u8 {
        self.read_u8(x, y, |p| unsafe { addr_of!((*p).flags).cast::<u8>() })
    }

    #[inline(always)]
    fn set_flags(&mut self, x: i32, y: i32, v: u8) {
        self.write_u8(x, y, v, |p| unsafe { addr_of_mut!((*p).flags).cast::<u8>() })
    }

    #[inline(always)]
    fn read_u8(&self, x: i32, y: i32, field: impl Fn(*mut Chunk) -> *const u8) -> u8 {
        let (s, i) = Self::slot(x, y);
        let p = self.ptrs[s];
        if p.is_null() {
            return 0;
        }
        // SAFETY: as in `mat`.
        unsafe { *field(p).add(i) }
    }

    #[inline(always)]
    fn write_u8(&mut self, x: i32, y: i32, v: u8, field: impl Fn(*mut Chunk) -> *mut u8) {
        let (s, i) = Self::slot(x, y);
        let p = self.ptrs[s];
        if !p.is_null() {
            self.touched |= 1 << s;
            // SAFETY: as in `mat`.
            unsafe { *field(p).add(i) = v }
        }
    }

    /// True if the cell already moved or changed in this tick.
    #[inline(always)]
    pub fn is_updated(&self, x: i32, y: i32) -> bool {
        self.flags(x, y) & FLAG_PARITY == self.parity
    }

    /// Mark the cell as updated in this tick.
    #[inline(always)]
    pub fn set_updated(&mut self, x: i32, y: i32) {
        let f = self.flags(x, y);
        self.set_flags(x, y, (f & !FLAG_PARITY) | self.parity);
    }

    /// Swap two cells completely (material, temperature, shade, life, motion). Marks both as
    /// updated and changed. Both cells must be inside the world.
    #[inline]
    pub fn swap(&mut self, ax: i32, ay: i32, bx: i32, by: i32) {
        let (sa, ia) = Self::slot(ax, ay);
        let (sb, ib) = Self::slot(bx, by);
        let (pa, pb) = (self.ptrs[sa], self.ptrs[sb]);
        debug_assert!(!pa.is_null() && !pb.is_null());
        self.touched |= (1 << sa) | (1 << sb);
        // SAFETY: as in `mat`. The two cells are different, so the element pointers do not alias.
        unsafe {
            std::ptr::swap(addr_of_mut!((*pa).mat).cast::<u16>().add(ia), addr_of_mut!((*pb).mat).cast::<u16>().add(ib));
            std::ptr::swap(addr_of_mut!((*pa).temp).cast::<i16>().add(ia), addr_of_mut!((*pb).temp).cast::<i16>().add(ib));
            std::ptr::swap(addr_of_mut!((*pa).shade).cast::<u8>().add(ia), addr_of_mut!((*pb).shade).cast::<u8>().add(ib));
            std::ptr::swap(addr_of_mut!((*pa).life).cast::<u8>().add(ia), addr_of_mut!((*pb).life).cast::<u8>().add(ib));
            std::ptr::swap(addr_of_mut!((*pa).motion).cast::<u8>().add(ia), addr_of_mut!((*pb).motion).cast::<u8>().add(ib));
        }
        self.set_updated(ax, ay);
        self.set_updated(bx, by);
        self.mark_changed(ax, ay);
        self.mark_changed(bx, by);
    }

    /// Turn a cell into another material in place. Keeps shade and motion.
    /// `temp: None` keeps the temperature. Sets a new life from the material's life range.
    /// Marks the cell as updated and changed.
    #[inline]
    pub fn replace(&mut self, x: i32, y: i32, m: MaterialId, temp: Option<i16>) {
        let (s, i) = Self::slot(x, y);
        let p = self.ptrs[s];
        if p.is_null() {
            return;
        }
        let life = match self.mats.life[m.index()] {
            Some((lo, hi)) => lo + self.rng.below((hi - lo) as u32 + 1) as u8,
            None => 0,
        };
        self.touched |= 1 << s;
        // SAFETY: as in `mat`.
        unsafe {
            *addr_of_mut!((*p).mat).cast::<u16>().add(i) = m.0;
            *addr_of_mut!((*p).life).cast::<u8>().add(i) = life;
            if let Some(t) = temp {
                *addr_of_mut!((*p).temp).cast::<i16>().add(i) = t;
            }
        }
        self.set_updated(x, y);
        self.mark_changed(x, y);
    }

    /// The cell changed: update it and its 8 neighbors in the next tick, and send a new image.
    #[inline]
    pub fn mark_changed(&mut self, x: i32, y: i32) {
        let (s, _) = Self::slot(x, y);
        self.changed |= 1 << s;
        let (lx, ly) = (x & CHUNK_MASK, y & CHUNK_MASK);
        if lx > 0 && lx < CHUNK_MASK && ly > 0 && ly < CHUNK_MASK {
            self.marks[s].add_rect(LocalRect { x0: lx - 1, y0: ly - 1, x1: lx + 2, y1: ly + 2 });
        } else {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    self.keep_awake(x + dx, y + dy);
                }
            }
        }
    }

    /// Update all cells in a rectangle (hood coordinates, `x1`/`y1` exclusive) in the next tick,
    /// without a visible change. The rectangle must stay within the reach rule.
    pub fn keep_awake_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        let (x0, y0) = (x0.max(-REACH), y0.max(-REACH));
        let (x1, y1) = (x1.min(64 + REACH), y1.min(64 + REACH));
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        for sy in (y0 >> CHUNK_SHIFT)..=((y1 - 1) >> CHUNK_SHIFT) {
            for sx in (x0 >> CHUNK_SHIFT)..=((x1 - 1) >> CHUNK_SHIFT) {
                let (ox, oy) = (sx << CHUNK_SHIFT, sy << CHUNK_SHIFT);
                let s = ((sy + 1) * 3 + sx + 1) as usize;
                self.marks[s].add_rect(LocalRect { x0: x0 - ox, y0: y0 - oy, x1: x1 - ox, y1: y1 - oy });
            }
        }
    }

    /// Update this cell again in the next tick, without a visible change.
    #[inline(always)]
    pub fn keep_awake(&mut self, x: i32, y: i32) {
        let (s, _) = Self::slot(x, y);
        self.marks[s].add_point(x & CHUNK_MASK, y & CHUNK_MASK);
    }
}
