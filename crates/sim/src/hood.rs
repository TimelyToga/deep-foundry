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

use crate::particles::Spawn;
use crate::react::ReactTable;
use crate::schedule::PassInput;
use crate::{SimEvent, SimSettings};
use crate::chunk::{Chunk, FLAG_BUILDING, FLAG_BURNING, FLAG_PARITY, LocalRect, MOTION_SPEED};
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
    pub settings: &'a SimSettings,
    /// False when there are too many particles: liquids do not splash.
    pub splash_ok: bool,
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
    /// New particles (cells that left the grid), added after the pass.
    pub spawns: Vec<Spawn>,
    /// Rows (bit y) of each of the 9 chunks that got a cell with a fall speed (see
    /// `Chunk::falling_rows`). Added to the chunks after the pass.
    pub falling: [u64; 9],
    /// For rows 0..64 of the center chunk: which cells from x = -64 to 127 are free (air, gas or
    /// fire), as 3 words of bits, filled when first needed (see `row_has_free`).
    free_rows: [[u64; 3]; 64],
    /// Bit y: `free_rows[y]` is filled.
    free_rows_known: u64,
    /// World positions of liquid cells for the level pass (see `movement::level`).
    pub levels: Vec<CellPos>,
    /// World positions of top liquid cells that moved away (see `movement::wake_row_ends`).
    pub opened: Vec<CellPos>,
    /// True in the fall pass. Its jobs run one per chunk column at the same time, so a job may
    /// read and write only cells of its own chunk column (hood x in 0..64). Marks can still go to
    /// the side chunks: they stay in the hood until the pass ends.
    pub fall_pass: bool,
}

impl<'a> Hood<'a> {
    /// # Safety
    /// The pointers must be valid for the whole life of the hood, and the caller must follow the
    /// safety rule in the module documentation.
    pub unsafe fn new(ptrs: [*mut Chunk; 9], input: PassInput<'a>, rng: Rng, parity: u8, outside: MaterialId, origin: CellPos) -> Self {
        Self {
            ptrs,
            mats: input.mats,
            react: input.react,
            settings: input.settings,
            splash_ok: input.splash_ok,
            rng,
            parity,
            marks: [LocalRect::EMPTY; 9],
            changed: 0,
            touched: 0,
            outside,
            origin,
            events: Vec::new(),
            spawns: Vec::new(),
            falling: [0; 9],
            free_rows: [[0; 3]; 64],
            free_rows_known: 0,
            levels: Vec::new(),
            opened: Vec::new(),
            fall_pass: false,
        }
    }

    /// World position of a hood cell.
    #[inline(always)]
    pub fn world_pos(&self, x: i32, y: i32) -> CellPos {
        self.origin.offset(x, y)
    }

    /// Take the cell out of the grid and make it a flying particle with this velocity
    /// (cells per tick). The cell becomes air. The particle is added after the pass.
    pub fn launch(&mut self, x: i32, y: i32, vx: f32, vy: f32) {
        self.launch_from(x, y, x, y, vx, vy);
    }

    /// Like `launch`, but the particle starts in the middle of cell (sx, sy) (for example the
    /// air cell above, so that a cell that moves into (x, y) later in the pass does not stop it).
    pub fn launch_from(&mut self, x: i32, y: i32, sx: i32, sy: i32, vx: f32, vy: f32) {
        let m = self.mat(x, y);
        if m.is_air() {
            return;
        }
        let p = self.world_pos(sx, sy);
        let (temp, shade, life) = (self.temp(x, y), self.read_u8(x, y, |p| unsafe { addr_of!((*p).shade).cast::<u8>() }), self.life(x, y));
        self.spawns.push(Spawn {
            x: p.x as f64 + 0.5,
            y: p.y as f64 + 0.5,
            vx,
            vy,
            material: m,
            temperature: temp,
            shade,
            life,
            flags: 0,
        });
        self.replace(x, y, MaterialId::AIR, None);
        self.set_motion(x, y, 0);
    }

    /// Send an event to the simulation. It is handled after the movement passes.
    #[inline]
    pub fn emit(&mut self, event: SimEvent) {
        self.events.push(event);
    }

    /// Slot and index of a cell that is read or written. In the fall pass the cell must be in the
    /// center chunk column (slots 1, 4 and 7).
    #[inline(always)]
    fn cell(&self, x: i32, y: i32) -> (usize, usize) {
        let (s, i) = Self::slot(x, y);
        debug_assert!(!self.fall_pass || s % 3 == 1, "fall pass reads a cell outside its chunk column: {x},{y}");
        (s, i)
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
        !self.ptrs[self.cell(x, y).0].is_null()
    }

    #[inline(always)]
    pub fn mat(&self, x: i32, y: i32) -> MaterialId {
        let (s, i) = self.cell(x, y);
        let p = self.ptrs[s];
        if p.is_null() {
            return self.outside;
        }
        // SAFETY: valid pointer (see `new`); element access only.
        MaterialId(unsafe { *addr_of!((*p).mat).cast::<u16>().add(i) })
    }

    /// The cells in row `y` of the center chunk, with x in `x0..x1`, that are not air and have a
    /// fall speed, as bits (bit x). A fast check for the fall pass: it reads the motion bytes 8 at
    /// a time and looks at single cells only where a fall speed is set.
    #[inline]
    pub fn center_row_falling(&self, y: i32, x0: i32, x1: i32) -> u64 {
        debug_assert!((0..64).contains(&y) && 0 <= x0 && x1 <= 64);
        let p = self.ptrs[4];
        let start = (y << CHUNK_SHIFT) as usize;
        // SAFETY: the center chunk exists (see `new`); the row is in the job's own chunk.
        let (mat, motion) = unsafe {
            (
                std::slice::from_raw_parts(addr_of!((*p).mat).cast::<u16>().add(start), 64),
                std::slice::from_raw_parts(addr_of!((*p).motion).cast::<u8>().add(start), 64),
            )
        };
        const SPEED8: u64 = 0x1f1f_1f1f_1f1f_1f1f;
        let mut bits = 0u64;
        for (w, bytes) in motion.as_chunks::<8>().0.iter().enumerate() {
            let word = u64::from_le_bytes(*bytes);
            if word & SPEED8 == 0 {
                continue;
            }
            for (k, &b) in bytes.iter().enumerate() {
                let x = w * 8 + k;
                if b & MOTION_SPEED != 0 && mat[x] != 0 {
                    bits |= 1 << x;
                }
            }
        }
        let range = if x1 - x0 >= 64 { u64::MAX } else { ((1u64 << (x1 - x0)) - 1) << x0 };
        bits & range
    }

    /// True if a free cell (air, gas or fire) may be in row `y` (0..64) with x in `x0..x1`
    /// (`-64 <= x0`, `x1 <= 128`). The rows are read once per job, so a cell that became free
    /// later in this job may be missed (the move that made it free wakes the row for the next tick);
    /// a false true only costs a scan.
    #[inline]
    pub fn row_has_free(&mut self, y: i32, x0: i32, x1: i32) -> bool {
        debug_assert!((0..64).contains(&y));
        if self.free_rows_known & (1 << y) == 0 {
            let mut words = [0u64; 3];
            for (k, word) in words.iter_mut().enumerate() {
                let p = self.ptrs[3 + k];
                if p.is_null() {
                    continue;
                }
                // SAFETY: as in `mat`; one row of one chunk of the hood.
                let row = unsafe { std::slice::from_raw_parts(addr_of!((*p).mat).cast::<u16>().add((y << CHUNK_SHIFT) as usize), 64) };
                for (x, &m) in row.iter().enumerate() {
                    let free = m == 0 || matches!(self.mats.phase[m as usize], Phase::Gas | Phase::Fire);
                    *word |= (free as u64) << x;
                }
            }
            self.free_rows[y as usize] = words;
            self.free_rows_known |= 1 << y;
        }
        let words = &self.free_rows[y as usize];
        let (a, b) = ((x0.max(-64) + 64) as u32, (x1.min(128) + 64) as u32);
        // Bits a..b of the 192-bit row.
        (0..3u32).any(|k| {
            let (lo, hi) = (a.max(k * 64), b.min(k * 64 + 64));
            if lo >= hi {
                return false;
            }
            let n = hi - lo;
            let mask = if n == 64 { u64::MAX } else { ((1u64 << n) - 1) << (lo - k * 64) };
            words[k as usize] & mask != 0
        })
    }

    /// How many cells next to (x, y) in direction `dir` (1 or -1), one after another, have
    /// material `m`, at most `max`. Reads the rows of the chunks directly (faster than `mat`
    /// for each cell). The cells read must stay within the reach rule.
    #[inline]
    pub fn run_of(&self, x: i32, y: i32, dir: i32, m: MaterialId, max: i32) -> i32 {
        let mut n = 0;
        let mut cx = x + dir;
        while n < max {
            let (s, i) = self.cell(cx, y);
            let p = self.ptrs[s];
            if p.is_null() {
                return n;
            }
            // Cells of this chunk on the row, in direction `dir`.
            let lx = cx & CHUNK_MASK;
            let left = if dir > 0 { CHUNK_MASK - lx } else { lx } + 1;
            let count = left.min(max - n);
            // SAFETY: as in `mat`; the cells are in one row of one chunk.
            let row = unsafe { addr_of!((*p).mat).cast::<u16>().add(i) };
            for k in 0..count {
                // SAFETY: `k` steps stay inside the chunk row (see `left`).
                let v = unsafe { *row.offset((k * dir) as isize) };
                if v != m.0 {
                    return n + k;
                }
            }
            n += count;
            cx += dir * count;
        }
        n
    }

    #[inline(always)]
    pub fn phase(&self, x: i32, y: i32) -> Phase {
        self.mats.phase[self.mat(x, y).index()]
    }

    #[inline(always)]
    pub fn temp(&self, x: i32, y: i32) -> i16 {
        let (s, i) = self.cell(x, y);
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
        let (s, i) = self.cell(x, y);
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
        self.write_u8(x, y, v, |p| unsafe { addr_of_mut!((*p).motion).cast::<u8>() });
        if v & MOTION_SPEED != 0 {
            self.note_falling(x, y);
        }
    }

    /// The cell has a fall speed: the fall pass must look at its row.
    #[inline(always)]
    fn note_falling(&mut self, x: i32, y: i32) {
        let (s, _) = Self::slot(x, y);
        self.falling[s] |= 1 << (y & CHUNK_MASK);
    }

    /// `Chunk::falling_rows` of the center chunk.
    #[inline]
    pub fn center_falling_rows(&self) -> u64 {
        // SAFETY: the center chunk exists (see `new`); only the job of the center chunk writes this
        // field during a pass.
        unsafe { *addr_of!((*self.ptrs[4]).falling_rows) }
    }

    /// Clear row `y` in `Chunk::falling_rows` of the center chunk (no falling cell is left there).
    #[inline]
    pub fn clear_center_falling_row(&mut self, y: i32) {
        // SAFETY: as in `center_falling_rows`.
        unsafe { *addr_of_mut!((*self.ptrs[4]).falling_rows) &= !(1u64 << y) }
    }

    #[inline(always)]
    pub fn flags(&self, x: i32, y: i32) -> u8 {
        self.read_u8(x, y, |p| unsafe { addr_of!((*p).flags).cast::<u8>() })
    }

    /// Set all flag bits of a cell. Keep `FLAG_PARITY` as it is (use `set_updated` for it).
    #[inline(always)]
    pub fn set_flags(&mut self, x: i32, y: i32, v: u8) {
        self.write_u8(x, y, v, |p| unsafe { addr_of_mut!((*p).flags).cast::<u8>() })
    }

    #[inline(always)]
    fn read_u8(&self, x: i32, y: i32, field: impl Fn(*mut Chunk) -> *const u8) -> u8 {
        let (s, i) = self.cell(x, y);
        let p = self.ptrs[s];
        if p.is_null() {
            return 0;
        }
        // SAFETY: as in `mat`.
        unsafe { *field(p).add(i) }
    }

    #[inline(always)]
    fn write_u8(&mut self, x: i32, y: i32, v: u8, field: impl Fn(*mut Chunk) -> *mut u8) {
        let (s, i) = self.cell(x, y);
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
        let (sa, ia) = self.cell(ax, ay);
        let (sb, ib) = self.cell(bx, by);
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
            if *addr_of!((*pa).motion).cast::<u8>().add(ia) & MOTION_SPEED != 0 {
                self.falling[sa] |= 1 << (ay & CHUNK_MASK);
            }
            if *addr_of!((*pb).motion).cast::<u8>().add(ib) & MOTION_SPEED != 0 {
                self.falling[sb] |= 1 << (by & CHUNK_MASK);
            }
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
        let (s, i) = self.cell(x, y);
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
            // The new cell does not burn (the burning state is in `life`), and it is not part of
            // a building.
            *addr_of_mut!((*p).flags).cast::<u8>().add(i) &= !(FLAG_BURNING | FLAG_BUILDING);
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
