//! Free-flying cells (technical design section 6.6): splash droplets, explosion debris, and
//! visual particles (sparks, dust, smoke puffs).
//!
//! # Material particles
//! A material particle is a cell that left the grid. It flies with gravity and becomes a grid cell
//! again where it lands, so no material is lost. There is no limit on their number.
//!
//! # Visual particles
//! A visual particle (flag `VISUAL`) is only for drawing. It never becomes a cell. Its `life` is
//! the number of ticks it has left; it is removed when its life ends, or when it hits a cell that
//! is not air, gas or fire. There are at most `MAX_VISUAL` of them; more are not added.
//! With the flag `RISE` it rises slowly and slows down (smoke puffs); without it, it falls with
//! gravity (sparks, dust).
//!
//! # How a particle moves
//! In each tick a particle moves along a straight line and looks at every cell that the line
//! crosses. So it cannot pass through a wall that is one cell thick, however fast it is.
//!
//! Chunks outside every simulation area, and chunks that are packed or not in memory, act as walls:
//! a material particle lands before it enters one. So particles do not carry cells out of the
//! update area. A material particle that starts a tick in a live chunk outside every simulation
//! area (a splash from a cell that moved over the edge) lands where it is. One that is in a chunk
//! that is packed or not in memory (the area moved away) waits there, with its velocity, until the
//! chunk updates again.
//!
//! # Positions
//! A particle stores its cell (`i32`) and its place inside the cell (`f32`, 0 to 1). So its
//! position stays exact far from x = 0, where an `f32` cannot hold whole cells. All line math is
//! done relative to the particle's cell, with small numbers. `Spawn` and `ParticleView` use `f64`.
//!
//! # The three parts of a step
//! 1. Move all particles. This part only reads cells, and each particle changes only itself, so
//!    many particles move in parallel (see `Table`). The result does not depend on the number of
//!    threads.
//! 2. Turn the material particles that hit something into cells, on one thread, in list order.
//! 3. Remove the particles that are gone, and keep the order of the others.

use crate::SimSettings;
use crate::chunk::{Chunk, FLAG_PARITY, MOTION_MOMENTUM_SHIFT, MOTION_RIGHT, MOTION_SPEED};
use crate::world::{RawChunk, World};
use foundry_content::{MaterialTable, Phase};
use foundry_core::{CHUNK_SHIFT, CellPos, CellRect, ChunkPos, MaterialId, ParticleView};
use rayon::prelude::*;

#[cfg(test)]
mod tests;

/// Particle flag: only for drawing. It never becomes a cell (see the module documentation).
pub const VISUAL: u8 = 1 << 0;
/// Particle flag for visual particles: it rises slowly and slows down (smoke puffs).
pub const RISE: u8 = 1 << 1;
/// Flags that a caller may set. The other bits are for `step` only.
const PUBLIC_FLAGS: u8 = VISUAL | RISE;
/// Step flag: the particle hit something and becomes a cell in the landing part of the step.
const LAND: u8 = 1 << 6;
/// Step flag: the particle is removed at the end of the step.
const GONE: u8 = 1 << 7;

/// Most visual particles at one time. More are not added.
pub const MAX_VISUAL: usize = 20_000;

/// Speed kept per tick by a material particle (air drag).
const DRAG: f32 = 0.995;
/// Speed kept per tick by a falling visual particle (sparks and dust slow down faster).
const VISUAL_DRAG: f32 = 0.97;
/// Speed kept per tick by a rising visual particle (smoke).
const RISE_DRAG: f32 = 0.9;
/// Upward pull of a rising visual particle, as a part of gravity.
const RISE_LIFT: f32 = 0.3;

/// With at least this many particles, the move part runs on several threads.
const PARALLEL_MIN: usize = 4096;
/// Particles per job in the parallel move.
const PARALLEL_PART: usize = 1024;
/// The parallel move needs a table of all chunks around the particles. If there are more chunks
/// than this (particles near anchors that are far apart), the move runs on one thread.
const MAX_TABLE_CHUNKS: i64 = 4096;

/// A new particle, or the state of one particle (see `Particles::get`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spawn {
    /// Position in cells (`f64`, so it is exact far from x = 0).
    pub x: f64,
    pub y: f64,
    /// Velocity in cells per tick.
    pub vx: f32,
    pub vy: f32,
    pub material: MaterialId,
    pub temperature: i16,
    pub shade: u8,
    /// Life of the cell when it lands (fading materials), or ticks left for a visual particle.
    pub life: u8,
    pub flags: u8,
}

/// The largest `f32` below 1.
const BELOW_ONE: f32 = 1.0 - f32::EPSILON / 2.0;

/// One particle in the list.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Particle {
    /// The cell that the particle is in.
    cx: i32,
    cy: i32,
    /// The place inside the cell, 0 to 1 (0.5 is the middle).
    fx: f32,
    fy: f32,
    vx: f32,
    vy: f32,
    mat: u16,
    temp: i16,
    shade: u8,
    life: u8,
    flags: u8,
}

impl Particle {
    #[inline(always)]
    fn cell(&self) -> CellPos {
        CellPos::new(self.cx, self.cy)
    }

    /// Set the position to (x, y), relative to the top-left corner of cell `c`.
    #[inline]
    fn set_near(&mut self, c: CellPos, x: f32, y: f32) {
        let (dx, dy) = (x.floor(), y.floor());
        self.cx = c.x.wrapping_add(dx as i32);
        self.cy = c.y.wrapping_add(dy as i32);
        self.fx = (x - dx).min(BELOW_ONE);
        self.fy = (y - dy).min(BELOW_ONE);
    }

    /// Position in cells.
    fn world_xy(&self) -> (f64, f64) {
        (self.cx as f64 + self.fx as f64, self.cy as f64 + self.fy as f64)
    }
}

/// Split a position in cells into the cell and the place inside the cell.
fn split(v: f64) -> (i32, f32) {
    let v = if v.is_finite() { v.clamp(i32::MIN as f64, i32::MAX as f64) } else { 0.0 };
    let c = v.floor();
    (c as i32, ((v - c) as f32).min(BELOW_ONE))
}

/// All particles.
#[derive(Default)]
pub struct Particles {
    list: Vec<Particle>,
    /// Number of visual particles in the list.
    visual: usize,
    /// Memory that the step reuses.
    search: Search,
    table: Table,
}

impl Particles {
    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Number of visual particles.
    pub fn visual_count(&self) -> usize {
        self.visual
    }

    /// The state of particle `i` (for tests and tools). `flags` has only `VISUAL` and `RISE`.
    pub fn get(&self, i: usize) -> Spawn {
        let p = &self.list[i];
        let (x, y) = p.world_xy();
        Spawn {
            x,
            y,
            vx: p.vx,
            vy: p.vy,
            material: MaterialId(p.mat),
            temperature: p.temp,
            shade: p.shade,
            life: p.life,
            flags: p.flags & PUBLIC_FLAGS,
        }
    }

    /// Add a particle. Returns false if it was not added.
    ///
    /// Material particles are always added (dropping one would lose material). A visual particle
    /// is not added when there are `MAX_VISUAL` visual particles, or `max` particles in all.
    pub fn spawn(&mut self, s: Spawn, max: usize) -> bool {
        let visual = s.flags & VISUAL != 0;
        if visual && (self.visual >= MAX_VISUAL || self.len() >= max) {
            return false;
        }
        self.visual += visual as usize;
        let ((cx, fx), (cy, fy)) = (split(s.x), split(s.y));
        self.list.push(Particle {
            cx,
            cy,
            fx,
            fy,
            vx: s.vx,
            vy: s.vy,
            mat: s.material.0,
            temp: s.temperature,
            shade: s.shade,
            life: s.life,
            flags: s.flags & PUBLIC_FLAGS,
        });
        true
    }

    /// Number of material (not visual) particles of a material. For tests that count material.
    pub fn count_material(&self, m: MaterialId) -> usize {
        self.list.iter().filter(|p| p.mat == m.0 && p.flags & VISUAL == 0).count()
    }

    /// Move all particles one tick. Material particles that hit something become cells.
    /// `pool`: the thread pool for the move part (`None`: the global pool).
    ///
    /// A material particle is never lost and never stays stuck: if the grid filled up around it
    /// (for example a pool rose over it), it becomes a cell at the nearest free cell above it, or
    /// else at the nearest air cell it can reach (see `free_spot`). Only if there is no air nearby
    /// at all does it stay a particle; it then moves up one cell per tick (if it can) until it
    /// finds air.
    pub fn step(
        &mut self,
        world: &mut World,
        mats: &MaterialTable,
        settings: &SimSettings,
        tick: u64,
        stamp: u64,
        pool: Option<&rayon::ThreadPool>,
    ) {
        // 1. Move. Both ways give the same result.
        if self.list.len() >= PARALLEL_MIN && self.table.build(world, &self.list, settings.particle_max_speed) {
            let table = &self.table;
            let run = |part: &mut [Particle]| {
                let mut cache = TableCache::default();
                for p in part {
                    move_one(p, &mut |c| table.look(&mut cache, c), mats, settings);
                }
            };
            let list = &mut self.list;
            match pool {
                Some(pool) => pool.install(|| list.par_chunks_mut(PARALLEL_PART).for_each(run)),
                None => list.par_chunks_mut(PARALLEL_PART).for_each(run),
            }
        } else {
            let mut grid = Grid::default();
            for p in &mut self.list {
                move_one(p, &mut |c| grid.look(world, c), mats, settings);
            }
        }
        // 2. Land, in list order.
        let parity_next_free = (tick & 1) as u8; // the next tick's parity is the other value
        for i in 0..self.list.len() {
            if self.list[i].flags & LAND != 0 {
                self.land(i, world, mats, parity_next_free, stamp);
            }
        }
        // 3. Remove.
        self.remove_gone();
    }

    /// Turn particle `i` (a material particle that hit something) into a grid cell.
    fn land(&mut self, i: usize, world: &mut World, mats: &MaterialTable, parity: u8, stamp: u64) {
        let p = &mut self.list[i];
        let at = p.cell();
        let mut grid = Grid::default();
        let liquid = mats.phase[p.mat as usize] == Phase::Liquid;
        let back = if liquid { None } else { Some((p.vx, p.vy)) };
        if let Some(spot) = free_spot(&mut grid, world, mats, at, back, &mut self.search) {
            let cell = Landing {
                material: MaterialId(p.mat),
                temp: p.temp,
                shade: p.shade,
                life: p.life,
                motion: landing_motion(p.vx, p.vy),
            };
            place(&mut grid, world, mats, spot, cell, parity, stamp);
            p.flags = GONE;
            return;
        }
        // No air nearby: stop, move up one cell, and try again next tick.
        p.flags &= !LAND;
        p.vx = 0.0;
        p.vy = 0.0;
        if let Look::Cell(m) | Look::Paused(m) = grid.look(world, at.offset(0, -1))
            && mats.phase[m.index()] != Phase::Solid
        {
            p.cy -= 1;
        }
    }

    /// Remove the particles marked `GONE`, keep the others in order, and clear the step flags.
    fn remove_gone(&mut self) {
        self.list.retain_mut(|p| {
            p.flags &= !LAND;
            p.flags & GONE == 0
        });
        self.visual = self.list.iter().filter(|p| p.flags & VISUAL != 0).count();
    }

    /// Write all particles (for saves): the count (u32), then for each particle: cell x and y
    /// (i32), place in the cell x and y, velocity x and y (f32), material (u16), temperature (i16),
    /// shade, life and flags (u8). 31 bytes for each particle.
    pub fn write(&self, w: &mut impl std::io::Write) -> std::io::Result<()> {
        w.write_all(&(self.len() as u32).to_le_bytes())?;
        for p in &self.list {
            w.write_all(&p.cx.to_le_bytes())?;
            w.write_all(&p.cy.to_le_bytes())?;
            for v in [p.fx, p.fy, p.vx, p.vy] {
                w.write_all(&v.to_le_bytes())?;
            }
            w.write_all(&p.mat.to_le_bytes())?;
            w.write_all(&p.temp.to_le_bytes())?;
            w.write_all(&[p.shade, p.life, p.flags])?;
        }
        Ok(())
    }

    /// Read particles written by `write`. `remap` turns saved material ids into current ones.
    pub fn read(r: &mut impl std::io::Read, remap: &[MaterialId]) -> std::io::Result<Particles> {
        let mut b4 = [0u8; 4];
        r.read_exact(&mut b4)?;
        let n = u32::from_le_bytes(b4) as usize;
        if n > 10_000_000 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "too many particles"));
        }
        let mut p = Particles::default();
        let mut rec = [0u8; 31];
        for _ in 0..n {
            r.read_exact(&mut rec)?;
            let b4 = |i: usize| [rec[i], rec[i + 1], rec[i + 2], rec[i + 3]];
            let f = |i: usize| f32::from_le_bytes(b4(i));
            let saved = u16::from_le_bytes([rec[24], rec[25]]) as usize;
            p.spawn(
                Spawn {
                    x: i32::from_le_bytes(b4(0)) as f64 + f(8).clamp(0.0, BELOW_ONE) as f64,
                    y: i32::from_le_bytes(b4(4)) as f64 + f(12).clamp(0.0, BELOW_ONE) as f64,
                    vx: f(16),
                    vy: f(20),
                    material: remap.get(saved).copied().unwrap_or(MaterialId::AIR),
                    temperature: i16::from_le_bytes([rec[26], rec[27]]),
                    shade: rec[28],
                    life: rec[29],
                    flags: rec[30],
                },
                usize::MAX,
            );
        }
        Ok(p)
    }

    /// Particles inside an area, for the snapshot.
    pub fn views(&self, area: CellRect, out: &mut Vec<ParticleView>) {
        for p in &self.list {
            if area.contains(p.cell()) {
                let (x, y) = p.world_xy();
                out.push(ParticleView {
                    x,
                    y,
                    vx: p.vx,
                    vy: p.vy,
                    material: p.mat,
                    temperature: p.temp,
                    shade: p.shade,
                });
            }
        }
    }
}

/// Move one particle one tick. Only reads cells (through `look`). Sets `LAND` on a material
/// particle that must become a cell (the landing part of the step does that), and `GONE` on a
/// particle that is removed.
#[inline]
fn move_one(p: &mut Particle, look: &mut impl FnMut(CellPos) -> Look, mats: &MaterialTable, settings: &SimSettings) {
    let visual = p.flags & VISUAL != 0;
    let g = settings.particle_gravity;
    let (mut vx, mut vy) = (p.vx, p.vy);
    if visual {
        if p.life <= 1 {
            p.flags |= GONE;
            return;
        }
        p.life -= 1;
        if p.flags & RISE != 0 {
            vx *= RISE_DRAG;
            vy = vy * RISE_DRAG - g * RISE_LIFT;
        } else {
            vx *= VISUAL_DRAG;
            vy += g;
        }
    } else {
        vx *= DRAG;
        vy += g;
    }
    let max_v = settings.particle_max_speed;
    vx = vx.clamp(-max_v, max_v);
    vy = vy.clamp(-max_v, max_v);
    // The position relative to the top-left corner of the particle's cell.
    let (mut here, mut px, mut py) = (p.cell(), p.fx, p.fy);
    match look(here) {
        Look::Cell(m) if passable(m, mats) => {}
        Look::Cell(_) | Look::Paused(_) if visual => {
            p.flags |= GONE;
            return;
        }
        // The grid changed under a material particle, or it is in a live chunk that does not
        // update: it lands where it is.
        Look::Cell(_) | Look::Paused(_) => {
            p.flags |= LAND;
            (p.vx, p.vy) = (vx, vy);
            return;
        }
        // In a chunk that is packed or not in memory: a material particle waits (it keeps its
        // velocity).
        Look::Wall if visual => {
            p.flags |= GONE;
            return;
        }
        Look::Wall => return,
        // Above the top of the world: come back down.
        Look::Outside if here.y < 0 => {
            here = CellPos::new(here.x, 0);
            py = 0.5;
            vy = vy.max(0.0);
        }
        // Out through a side or the bottom (only if there is no bedrock border).
        Look::Outside => {
            p.flags |= GONE;
            return;
        }
    }
    match fly(look, mats, here, px, py, vx, vy) {
        Flight::Free => {
            px += vx;
            py += vy;
        }
        Flight::Hit { last, t } => {
            if visual {
                p.flags |= GONE;
                return;
            }
            (px, py) = point_in_cell(here, px, py, vx, vy, t, last);
            p.flags |= LAND;
        }
        Flight::Ceiling { last, t } => {
            (px, py) = point_in_cell(here, px, py, vx, vy, t, last);
            vy = vy.max(0.0);
        }
        Flight::Lost => {
            p.flags |= GONE;
            return;
        }
    }
    p.set_near(here, px, py);
    (p.vx, p.vy) = (vx, vy);
}

/// What a particle finds at a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Look {
    /// A cell in a chunk that updates.
    Cell(MaterialId),
    /// A cell in a live chunk outside every simulation area. Particles do not fly into it, but a
    /// landing particle may use a free cell there.
    Paused(MaterialId),
    /// A cell in a chunk that is packed or not in memory (or all air and outside every simulation
    /// area).
    Wall,
    /// A cell outside the world.
    Outside,
}

/// What a chunk is, for particles.
#[derive(Debug, Clone, Copy)]
enum Kind {
    /// A live chunk in a simulation area.
    Live(*const Chunk),
    /// A live chunk outside every simulation area.
    Paused(*const Chunk),
    /// All air (no cells in memory), in a simulation area.
    Air,
    /// Packed, not in memory, or all air outside every simulation area.
    Wall,
    /// Outside the world.
    Outside,
}

impl Kind {
    /// The kind of chunk `c`. The pointers are only used to read cells.
    fn of(world: &mut World, c: ChunkPos) -> Kind {
        if !world.chunk_in_bounds(c) {
            return Kind::Outside;
        }
        match world.raw_chunk(c) {
            RawChunk::Live(ch) => Kind::Live(ch.cast_const()),
            RawChunk::Air => Kind::Air,
            RawChunk::Unknown => match world.chunk(c) {
                Some(ch) => Kind::Paused(std::ptr::from_ref(ch)),
                None => Kind::Wall,
            },
        }
    }

    /// The cell at `p` of a chunk of this kind.
    #[inline(always)]
    fn look(self, p: CellPos) -> Look {
        match self {
            // SAFETY (both arms): the pointer points to a live chunk of the world (see `Kind::of`).
            // No chunk leaves the world and no cell is written while a `Kind` is used: `Grid` and
            // `Table` are used in the move part of a step (which only reads cells), and in the
            // landing part a new `Grid` is made for each particle and is not used after the
            // particle's cell is written. This reads one element.
            Kind::Live(ch) => Look::Cell(MaterialId(unsafe { (*ch).mat[p.local_index()] })),
            Kind::Paused(ch) => Look::Paused(MaterialId(unsafe { (*ch).mat[p.local_index()] })),
            Kind::Air => Look::Cell(MaterialId::AIR),
            Kind::Wall => Look::Wall,
            Kind::Outside => Look::Outside,
        }
    }
}

/// Reads cells for particles on one thread. It keeps the last chunk it looked at, because the
/// cells that one particle looks at in one tick are mostly in one chunk.
struct Grid {
    pos: ChunkPos,
    kind: Kind,
    valid: bool,
}

impl Default for Grid {
    fn default() -> Self {
        Self { pos: ChunkPos::new(0, 0), kind: Kind::Wall, valid: false }
    }
}

impl Grid {
    #[inline(always)]
    fn look(&mut self, world: &mut World, p: CellPos) -> Look {
        let c = p.chunk();
        if !self.valid || c != self.pos {
            self.pos = c;
            self.kind = Kind::of(world, c);
            self.valid = true;
        }
        self.kind.look(p)
    }
}

/// The kinds of all chunks around the particles, for the parallel move. It covers every chunk
/// that a particle can reach in one tick.
#[derive(Default)]
struct Table {
    x0: i32,
    y0: i32,
    w: i32,
    h: i32,
    kinds: Vec<Kind>,
}

// SAFETY: the table holds pointers to chunks only to read cells, and it is used only while no
// thread writes to the world (the move part of a step).
unsafe impl Sync for Table {}
unsafe impl Send for Table {}

/// The last chunk that one job of the parallel move looked at.
#[derive(Default)]
struct TableCache {
    last: Option<(ChunkPos, Kind)>,
}

impl Table {
    /// Fill the table for these particles. Returns false if it would have more than
    /// `MAX_TABLE_CHUNKS` chunks.
    fn build(&mut self, world: &mut World, list: &[Particle], max_speed: f32) -> bool {
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for p in list {
            x0 = x0.min(p.cx);
            y0 = y0.min(p.cy);
            x1 = x1.max(p.cx);
            y1 = y1.max(p.cy);
        }
        // A particle moves at most `max_speed` cells in x and in y in one tick. One above the top
        // of the world first moves to y = 0.5.
        let m = max_speed.clamp(0.0, 1e6).ceil() as i32 + 2;
        let chunk = |v: i32| v >> CHUNK_SHIFT;
        let (cx0, cy0) = (chunk(x0.saturating_sub(m)), chunk(y0.min(0).saturating_sub(m)));
        let (cx1, cy1) = (chunk(x1.saturating_add(m)), chunk(y1.max(0).saturating_add(m)));
        if (cx1 as i64 - cx0 as i64 + 1) * (cy1 as i64 - cy0 as i64 + 1) > MAX_TABLE_CHUNKS {
            return false;
        }
        let (w, h) = (cx1 - cx0 + 1, cy1 - cy0 + 1);
        (self.x0, self.y0, self.w, self.h) = (cx0, cy0, w, h);
        self.kinds.clear();
        for cy in cy0..=cy1 {
            for cx in cx0..=cx1 {
                self.kinds.push(Kind::of(world, ChunkPos::new(cx, cy)));
            }
        }
        true
    }

    #[inline(always)]
    fn look(&self, cache: &mut TableCache, p: CellPos) -> Look {
        let c = p.chunk();
        let kind = match cache.last {
            Some((pos, kind)) if pos == c => kind,
            _ => {
                let (tx, ty) = (c.x - self.x0, c.y - self.y0);
                // Every chunk that a particle can reach is in the table; this is only a guard.
                let kind = if (0..self.w).contains(&tx) && (0..self.h).contains(&ty) {
                    self.kinds[(ty * self.w + tx) as usize]
                } else {
                    Kind::Wall
                };
                cache.last = Some((c, kind));
                kind
            }
        };
        kind.look(p)
    }
}

/// How a flight ended.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Flight {
    /// Nothing was in the way.
    Free,
    /// The line hit a cell that stops the particle, at line time `t` (0 to 1). `last` is the
    /// last free cell before it.
    Hit { last: CellPos, t: f32 },
    /// The line went above the top of the world at time `t`.
    Ceiling { last: CellPos, t: f32 },
    /// The line left the world through a side or the bottom.
    Lost,
}

/// Follow the line from (x, y) to (x + vx, y + vy) through the grid, cell by cell, and stop at
/// the first cell that is not air, gas or fire (or that is in a chunk that does not update).
/// (x, y) is relative to the top-left corner of cell `origin`.
#[inline]
fn fly(look: &mut impl FnMut(CellPos) -> Look, mats: &MaterialTable, origin: CellPos, x: f32, y: f32, vx: f32, vy: f32) -> Flight {
    for (last, c, t) in LineCells::new(origin, x, y, vx, vy) {
        match look(c) {
            Look::Cell(m) if passable(m, mats) => {}
            Look::Cell(_) | Look::Paused(_) | Look::Wall => return Flight::Hit { last, t },
            Look::Outside if c.y < 0 => return Flight::Ceiling { last, t },
            Look::Outside => return Flight::Lost,
        }
    }
    Flight::Free
}

/// The cells that the line from (x, y) to (x + vx, y + vy) crosses, in the order it crosses them,
/// without the start cell. Each item is (the cell before, the cell, the line time 0 to 1 at which
/// the line enters the cell). Two cells in a row always share a side, so the line never skips a
/// cell, also not at a corner. (x, y) is relative to the top-left corner of cell `origin`, so the
/// `f32` numbers stay small.
struct LineCells {
    cx: i32,
    cy: i32,
    sx: i32,
    sy: i32,
    /// Steps left in x and in y.
    nx: i32,
    ny: i32,
    /// Line time between two x borders (and y borders), and the time of the next border.
    dtx: f32,
    dty: f32,
    tx: f32,
    ty: f32,
}

impl LineCells {
    #[inline]
    fn new(origin: CellPos, x: f32, y: f32, vx: f32, vy: f32) -> Self {
        let (cx, cy) = (x.floor() as i32, y.floor() as i32);
        let (ex, ey) = ((x + vx).floor() as i32, (y + vy).floor() as i32);
        let (sx, sy) = ((ex - cx).signum(), (ey - cy).signum());
        let dtx = if vx != 0.0 { 1.0 / vx.abs() } else { f32::INFINITY };
        let dty = if vy != 0.0 { 1.0 / vy.abs() } else { f32::INFINITY };
        let tx = if sx > 0 { (cx as f32 + 1.0 - x) * dtx } else { (x - cx as f32) * dtx };
        let ty = if sy > 0 { (cy as f32 + 1.0 - y) * dty } else { (y - cy as f32) * dty };
        let (nx, ny) = ((ex - cx).abs(), (ey - cy).abs());
        let (cx, cy) = (origin.x.wrapping_add(cx), origin.y.wrapping_add(cy));
        Self { cx, cy, sx, sy, nx, ny, dtx, dty, tx, ty }
    }
}

impl Iterator for LineCells {
    type Item = (CellPos, CellPos, f32);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.nx + self.ny == 0 {
            return None;
        }
        let last = CellPos::new(self.cx, self.cy);
        // Step in x or y, whichever border comes first. The step counts make sure that the line
        // ends in the right cell also with rounding errors. (Written without branches: the choice
        // changes often and cannot be predicted well.)
        let x = (self.ny == 0) | ((self.nx > 0) & (self.tx < self.ty));
        let t = if x { self.tx } else { self.ty };
        self.cx += if x { self.sx } else { 0 };
        self.cy += if x { 0 } else { self.sy };
        self.nx -= x as i32;
        self.ny -= !x as i32;
        self.tx += if x { self.dtx } else { 0.0 };
        self.ty += if x { 0.0 } else { self.dty };
        Some((last, CellPos::new(self.cx, self.cy), t))
    }
}

/// The point on the line at time `t`, moved inside cell `c` (so rounding cannot put it into the
/// next cell). The line and the result are relative to the top-left corner of cell `origin`.
fn point_in_cell(origin: CellPos, x: f32, y: f32, vx: f32, vy: f32, t: f32, c: CellPos) -> (f32, f32) {
    let inside = |v: f32, lo: i32| v.clamp(lo as f32 + 0.01, lo as f32 + 0.99);
    (inside(x + vx * t, c.x.wrapping_sub(origin.x)), inside(y + vy * t, c.y.wrapping_sub(origin.y)))
}

/// Air, gas and fire do not stop a particle.
#[inline(always)]
fn passable(m: MaterialId, mats: &MaterialTable) -> bool {
    m.is_air() || matches!(mats.phase[m.index()], Phase::Gas | Phase::Fire)
}

/// How far up a landing particle looks for air (through liquid, powder and gas).
const SEARCH_UP: i32 = 64;
/// A landing particle that finds no air straight up looks for the nearest air cell with at most
/// this distance in x and y (the ring search).
const SEARCH_RING: i32 = 8;
/// If the ring search finds nothing, it looks for air in the square of cells with this distance
/// around it (the reach search).
const SEARCH_AROUND: i32 = 24;
const SEARCH_SIDE: i32 = 2 * SEARCH_AROUND + 1;

/// Memory for the search of `free_spot`, kept to reuse it.
#[derive(Default)]
struct Search {
    /// One bit for each cell of the search square: the cell was seen.
    seen: Vec<u64>,
    queue: Vec<CellPos>,
}

/// The cell where a landing particle becomes a grid cell:
/// 1. `at` if it is air or gas;
/// 2. for a particle that is not a liquid (`back` is its velocity): else the first air cell back
///    along the line it came from, at most `|velocity| + 2` cells (so debris piles up where it
///    hit something, and does not appear on top of the material it hit);
/// 3. else the first air cell straight up (through liquid, powder and gas, not through solids);
/// 4. else the nearest air cell around, at most `SEARCH_RING` cells away (upper cells first), with
///    no solid cell on the line between it and `at`;
/// 5. else the nearest air cell that it can reach through cells that are not solid, within
///    `SEARCH_AROUND` cells. A particle inside a solid searches through solids too.
///
/// So a particle never lands on the other side of a wall. Only cells in live chunks count.
fn free_spot(
    grid: &mut Grid,
    world: &mut World,
    mats: &MaterialTable,
    at: CellPos,
    back: Option<(f32, f32)>,
    search: &mut Search,
) -> Option<CellPos> {
    let here = grid.look(world, at);
    if let Look::Cell(m) | Look::Paused(m) = here
        && passable(m, mats)
    {
        return Some(at);
    }
    if let Some((vx, vy)) = back {
        let len = (vx * vx + vy * vy).sqrt();
        if len > 0.01 {
            let k = (len + 2.0) / len;
            for (_, c, _) in LineCells::new(at, 0.5, 0.5, -vx * k, -vy * k) {
                match grid.look(world, c) {
                    Look::Cell(m) | Look::Paused(m) if m.is_air() => return Some(c),
                    Look::Cell(m) | Look::Paused(m) if mats.phase[m.index()] != Phase::Solid => {}
                    _ => break,
                }
            }
        }
    }
    // A particle inside a solid (for example one made inside a wall) may search through solids,
    // so that it gets out.
    let in_solid = matches!(here, Look::Cell(m) | Look::Paused(m) if mats.phase[m.index()] == Phase::Solid);
    for k in 1..=SEARCH_UP {
        let p = at.offset(0, -k);
        match grid.look(world, p) {
            Look::Cell(m) | Look::Paused(m) if m.is_air() => return Some(p),
            Look::Cell(m) | Look::Paused(m) if mats.phase[m.index()] != Phase::Solid => {}
            _ => break,
        }
    }
    for r in 1..=SEARCH_RING {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dy.abs()) != r {
                    continue;
                }
                let p = at.offset(dx, dy);
                if let Look::Cell(m) | Look::Paused(m) = grid.look(world, p)
                    && m.is_air()
                    && clear_path(grid, world, mats, at, p)
                {
                    return Some(p);
                }
            }
        }
    }
    // Nearest first (by steps to a side neighbor); upper cells first at the same distance.
    search.seen.clear();
    search.seen.resize((SEARCH_SIDE * SEARCH_SIDE) as usize / 64 + 1, 0);
    search.queue.clear();
    search.queue.push(at);
    let center = (SEARCH_AROUND * SEARCH_SIDE + SEARCH_AROUND) as usize;
    search.seen[center / 64] |= 1 << (center % 64);
    let mut head = 0;
    while head < search.queue.len() {
        let p = search.queue[head];
        head += 1;
        for (dx, dy) in [(0, -1), (-1, 0), (1, 0), (0, 1)] {
            let q = p.offset(dx, dy);
            let (sx, sy) = (q.x - at.x + SEARCH_AROUND, q.y - at.y + SEARCH_AROUND);
            if !(0..SEARCH_SIDE).contains(&sx) || !(0..SEARCH_SIDE).contains(&sy) {
                continue;
            }
            let bit = (sy * SEARCH_SIDE + sx) as usize;
            if search.seen[bit / 64] & (1 << (bit % 64)) != 0 {
                continue;
            }
            search.seen[bit / 64] |= 1 << (bit % 64);
            match grid.look(world, q) {
                Look::Cell(m) | Look::Paused(m) if m.is_air() => return Some(q),
                Look::Cell(m) | Look::Paused(m) if in_solid || mats.phase[m.index()] != Phase::Solid => search.queue.push(q),
                _ => {}
            }
        }
    }
    None
}

/// True if no solid cell is on the line between the centers of cells `a` and `b` (without `a`
/// and `b`). A landing particle can move through powder and liquid, but not through a wall.
fn clear_path(grid: &mut Grid, world: &mut World, mats: &MaterialTable, a: CellPos, b: CellPos) -> bool {
    let (vx, vy) = ((b.x - a.x) as f32, (b.y - a.y) as f32);
    for (_, c, _) in LineCells::new(a, 0.5, 0.5, vx, vy) {
        if c == b {
            break;
        }
        match grid.look(world, c) {
            Look::Cell(m) | Look::Paused(m) if mats.phase[m.index()] != Phase::Solid => {}
            _ => return false,
        }
    }
    true
}

/// A particle that becomes a grid cell.
struct Landing {
    material: MaterialId,
    temp: i16,
    shade: u8,
    life: u8,
    motion: u8,
}

/// Motion bits for a landing particle: its fall speed, its side, and (for liquids) a momentum
/// level from its sideways speed. A fast droplet can so splash again or run on along the surface.
fn landing_motion(vx: f32, vy: f32) -> u8 {
    let speed = ((vy - 1.0) * 4.0).clamp(0.0, 28.0) as u8;
    let side = if vx > 0.0 { MOTION_RIGHT } else { 0 };
    let level = ((vx.abs() * 1.5) as u8).min(3);
    speed | side | (level << MOTION_MOMENTUM_SHIFT)
}

/// Write a landing particle into the grid at `p` (a cell in a live chunk). If `p` holds a
/// gas, the gas moves to the first air cell above it (or is lost if there is none within
/// `SEARCH_UP` cells; this is rare).
fn place(grid: &mut Grid, world: &mut World, mats: &MaterialTable, p: CellPos, cell: Landing, parity: u8, stamp: u64) {
    let old = world.mat(p);
    if !old.is_air() {
        let mut up = None;
        for k in 1..=SEARCH_UP {
            let q = p.offset(0, -k);
            match grid.look(world, q) {
                Look::Cell(m) | Look::Paused(m) if m.is_air() => {
                    up = Some(q);
                    break;
                }
                Look::Cell(m) | Look::Paused(m) if passable(m, mats) => {}
                _ => break,
            }
        }
        if let Some(q) = up {
            let (i, j) = (p.local_index(), q.local_index());
            let Some(src) = world.chunk(p.chunk()) else { return };
            let gas = (src.mat[i], src.temp[i], src.shade[i], src.life[i]);
            let Some(dst) = world.chunk_mut(q.chunk()) else { return };
            (dst.mat[j], dst.temp[j], dst.shade[j], dst.life[j]) = gas;
            dst.motion[j] = 0;
            dst.flags[j] = (dst.flags[j] & !FLAG_PARITY) | parity;
            dst.version = stamp;
            world.mark_dirty_around(q);
        }
    }
    let liquid = mats.phase[cell.material.index()] == Phase::Liquid;
    let Some(c) = world.chunk_mut(p.chunk()) else { return };
    let i = p.local_index();
    c.mat[i] = cell.material.0;
    c.temp[i] = cell.temp;
    c.shade[i] = cell.shade;
    c.life[i] = cell.life;
    c.motion[i] = if liquid { cell.motion } else { cell.motion & (MOTION_SPEED | MOTION_RIGHT) };
    if cell.motion & MOTION_SPEED != 0 {
        c.falling_rows |= 1 << (p.y & 63);
    }
    c.flags[i] = (c.flags[i] & !FLAG_PARITY) | parity;
    c.version = stamp;
    world.mark_dirty_around(p);
}
