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
//! Particles update on one thread, in list order, so the result is deterministic.
//! A step has three parts: move all particles (only reads cells), then turn the particles that
//! landed into cells (in list order), then remove the particles that are gone.

use crate::SimSettings;
use crate::chunk::{FLAG_PARITY, MOTION_MOMENTUM_SHIFT, MOTION_RIGHT, MOTION_SPEED};
use crate::world::{RawChunk, World};
use foundry_content::{MaterialTable, Phase};
use foundry_core::{CellPos, CellRect, ChunkPos, MaterialId, ParticleView};

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

/// A new particle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spawn {
    /// Position in cells.
    pub x: f32,
    pub y: f32,
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

/// All particles, as separate arrays.
#[derive(Default)]
pub struct Particles {
    x: Vec<f32>,
    y: Vec<f32>,
    vx: Vec<f32>,
    vy: Vec<f32>,
    mat: Vec<u16>,
    temp: Vec<i16>,
    shade: Vec<u8>,
    life: Vec<u8>,
    flags: Vec<u8>,
    /// Number of visual particles in the lists.
    visual: usize,
    /// Particles that landed in this step (indices), kept to reuse the memory.
    landing: Vec<u32>,
}

impl Particles {
    pub fn len(&self) -> usize {
        self.x.len()
    }

    pub fn is_empty(&self) -> bool {
        self.x.is_empty()
    }

    /// Number of visual particles.
    pub fn visual_count(&self) -> usize {
        self.visual
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
        self.x.push(s.x);
        self.y.push(s.y);
        self.vx.push(s.vx);
        self.vy.push(s.vy);
        self.mat.push(s.material.0);
        self.temp.push(s.temperature);
        self.shade.push(s.shade);
        self.life.push(s.life);
        self.flags.push(s.flags & PUBLIC_FLAGS);
        true
    }

    /// Number of material (not visual) particles of a material. For tests that count material.
    pub fn count_material(&self, m: MaterialId) -> usize {
        (0..self.len()).filter(|&i| self.mat[i] == m.0 && self.flags[i] & VISUAL == 0).count()
    }

    /// Move all particles one tick. Material particles that hit something become cells.
    ///
    /// A material particle is never lost and never stays stuck: if the grid filled up around it
    /// (for example a pool rose over it), it becomes a cell at the nearest free cell above it.
    /// Only if there is no air nearby at all does it stay a particle; it then moves up one cell
    /// per tick until it finds air.
    pub fn step(&mut self, world: &mut World, mats: &MaterialTable, settings: &SimSettings, tick: u64, stamp: u64) {
        let mut grid = Grid::default();
        let mut landing = std::mem::take(&mut self.landing);
        landing.clear();
        for i in 0..self.len() {
            if self.move_one(i, &mut grid, world, mats, settings) {
                landing.push(i as u32);
            }
        }
        let parity_next_free = (tick & 1) as u8; // the next tick's parity is the other value
        for &i in &landing {
            self.land(i as usize, world, mats, parity_next_free, stamp);
        }
        self.landing = landing;
        self.remove_gone();
    }

    /// Move particle `i` one tick. Only reads cells. Returns true if it is a material particle that
    /// must become a cell (the landing part of the step does that).
    #[inline]
    fn move_one(&mut self, i: usize, grid: &mut Grid, world: &mut World, mats: &MaterialTable, settings: &SimSettings) -> bool {
        let flags = self.flags[i];
        let visual = flags & VISUAL != 0;
        let g = settings.particle_gravity;
        let (mut vx, mut vy) = (self.vx[i], self.vy[i]);
        if visual {
            if self.life[i] <= 1 {
                self.flags[i] = flags | GONE;
                return false;
            }
            self.life[i] -= 1;
            if flags & RISE != 0 {
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
        let (mut px, mut py) = (self.x[i], self.y[i]);
        let here = CellPos::new(px.floor() as i32, py.floor() as i32);
        match grid.look(world, here) {
            Look::Cell(m) if passable(m, mats) => {}
            Look::Cell(_) | Look::Paused(_) if visual => {
                self.flags[i] = flags | GONE;
                return false;
            }
            // The grid changed under a material particle, or it is in a live chunk that does not
            // update: it lands where it is.
            Look::Cell(_) | Look::Paused(_) => {
                self.flags[i] = flags | LAND;
                (self.vx[i], self.vy[i]) = (vx, vy);
                return true;
            }
            // In a chunk that is packed or not in memory: a material particle waits (it keeps its
            // velocity).
            Look::Wall if visual => {
                self.flags[i] = flags | GONE;
                return false;
            }
            Look::Wall => return false,
            // Above the top of the world: come back down.
            Look::Outside if here.y < 0 => {
                py = 0.5;
                vy = vy.max(0.0);
            }
            // Out through a side or the bottom (only if there is no bedrock border).
            Look::Outside => {
                self.flags[i] = flags | GONE;
                return false;
            }
        }
        let mut land = false;
        match fly(grid, world, mats, px, py, vx, vy) {
            Flight::Free => {
                px += vx;
                py += vy;
            }
            Flight::Hit { last, t } => {
                if visual {
                    self.flags[i] = flags | GONE;
                    return false;
                }
                (px, py) = point_in_cell(px, py, vx, vy, t, last);
                land = true;
            }
            Flight::Ceiling { last, t } => {
                (px, py) = point_in_cell(px, py, vx, vy, t, last);
                vy = vy.max(0.0);
            }
            Flight::Lost => {
                self.flags[i] = flags | GONE;
                return false;
            }
        }
        (self.x[i], self.y[i], self.vx[i], self.vy[i]) = (px, py, vx, vy);
        if land {
            self.flags[i] = flags | LAND;
        }
        land
    }

    /// Turn particle `i` (a material particle that hit something) into a grid cell.
    fn land(&mut self, i: usize, world: &mut World, mats: &MaterialTable, parity: u8, stamp: u64) {
        let at = CellPos::new(self.x[i].floor() as i32, self.y[i].floor() as i32);
        let mut grid = Grid::default();
        if let Some(spot) = free_spot(&mut grid, world, mats, at) {
            let cell = Landing {
                material: MaterialId(self.mat[i]),
                temp: self.temp[i],
                shade: self.shade[i],
                life: self.life[i],
                motion: landing_motion(self.vx[i], self.vy[i]),
            };
            place(&mut grid, world, mats, spot, cell, parity, stamp);
            self.flags[i] = GONE;
            return;
        }
        // No air nearby: stop, move up one cell, and try again next tick.
        self.flags[i] &= !LAND;
        self.vx[i] = 0.0;
        self.vy[i] = 0.0;
        if let Look::Cell(m) | Look::Paused(m) = grid.look(world, at.offset(0, -1))
            && mats.phase[m.index()] != Phase::Solid
        {
            self.y[i] -= 1.0;
        }
    }

    /// Remove the particles marked `GONE`, keep the others in order, and clear the step flags.
    fn remove_gone(&mut self) {
        let mut keep = 0;
        let mut visual = 0;
        for i in 0..self.len() {
            let f = self.flags[i];
            if f & GONE != 0 {
                continue;
            }
            visual += (f & VISUAL != 0) as usize;
            self.x[keep] = self.x[i];
            self.y[keep] = self.y[i];
            self.vx[keep] = self.vx[i];
            self.vy[keep] = self.vy[i];
            self.mat[keep] = self.mat[i];
            self.temp[keep] = self.temp[i];
            self.shade[keep] = self.shade[i];
            self.life[keep] = self.life[i];
            self.flags[keep] = f & PUBLIC_FLAGS;
            keep += 1;
        }
        self.visual = visual;
        self.x.truncate(keep);
        self.y.truncate(keep);
        self.vx.truncate(keep);
        self.vy.truncate(keep);
        self.mat.truncate(keep);
        self.temp.truncate(keep);
        self.shade.truncate(keep);
        self.life.truncate(keep);
        self.flags.truncate(keep);
    }

    /// Write all particles (for saves).
    pub fn write(&self, w: &mut impl std::io::Write) -> std::io::Result<()> {
        w.write_all(&(self.len() as u32).to_le_bytes())?;
        for i in 0..self.len() {
            for v in [self.x[i], self.y[i], self.vx[i], self.vy[i]] {
                w.write_all(&v.to_le_bytes())?;
            }
            w.write_all(&self.mat[i].to_le_bytes())?;
            w.write_all(&self.temp[i].to_le_bytes())?;
            w.write_all(&[self.shade[i], self.life[i], self.flags[i]])?;
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
        let mut rec = [0u8; 23];
        for _ in 0..n {
            r.read_exact(&mut rec)?;
            let f = |i: usize| f32::from_le_bytes([rec[i], rec[i + 1], rec[i + 2], rec[i + 3]]);
            let saved = u16::from_le_bytes([rec[16], rec[17]]) as usize;
            p.spawn(
                Spawn {
                    x: f(0),
                    y: f(4),
                    vx: f(8),
                    vy: f(12),
                    material: remap.get(saved).copied().unwrap_or(MaterialId::AIR),
                    temperature: i16::from_le_bytes([rec[18], rec[19]]),
                    shade: rec[20],
                    life: rec[21],
                    flags: rec[22],
                },
                usize::MAX,
            );
        }
        Ok(p)
    }

    /// Particles inside an area, for the snapshot.
    pub fn views(&self, area: CellRect, out: &mut Vec<ParticleView>) {
        for i in 0..self.len() {
            let (x, y) = (self.x[i], self.y[i]);
            if area.contains(CellPos::new(x as i32, y as i32)) {
                out.push(ParticleView {
                    x,
                    y,
                    vx: self.vx[i],
                    vy: self.vy[i],
                    material: self.mat[i],
                    temperature: self.temp[i],
                    shade: self.shade[i],
                });
            }
        }
    }

    /// Flags and life of particle `i` (for tests and tools). See `VISUAL` and `RISE`.
    pub fn flags_and_life(&self, i: usize) -> (u8, u8) {
        (self.flags[i], self.life[i])
    }
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

/// Reads cells for particles. It keeps the last chunk it looked at, because the cells that one
/// particle looks at in one tick are mostly in one chunk.
struct Grid {
    pos: ChunkPos,
    kind: RawChunk,
    /// The chunk is live but outside every simulation area.
    paused: bool,
    valid: bool,
}

impl Default for Grid {
    fn default() -> Self {
        Self { pos: ChunkPos::new(0, 0), kind: RawChunk::Unknown, paused: false, valid: false }
    }
}

impl Grid {
    #[inline(always)]
    fn look(&mut self, world: &mut World, p: CellPos) -> Look {
        let c = p.chunk();
        if !self.valid || c != self.pos {
            if !world.chunk_in_bounds(c) {
                return Look::Outside;
            }
            self.pos = c;
            self.kind = world.raw_chunk(c);
            self.paused = false;
            if let RawChunk::Unknown = self.kind
                && let Some(ch) = world.chunk(c)
            {
                // Only read through this pointer.
                self.kind = RawChunk::Live(std::ptr::from_ref(ch).cast_mut());
                self.paused = true;
            }
            self.valid = true;
        }
        match self.kind {
            RawChunk::Live(ch) => {
                // SAFETY: the pointer points to a live chunk (from `raw_chunk`, or from `chunk` for a
                // live chunk outside every simulation area; that one is only read). No chunk leaves
                // the world while a `Grid` is used: in the move part of a step nothing writes to the
                // world, and in the landing part a new `Grid` is made for each particle and is not
                // used after the particle's cell is written. Only one thread runs. This reads one
                // element.
                let m = MaterialId(unsafe { (*ch).mat[p.local_index()] });
                if self.paused { Look::Paused(m) } else { Look::Cell(m) }
            }
            RawChunk::Air => Look::Cell(MaterialId::AIR),
            RawChunk::Unknown => Look::Wall,
        }
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
/// The cells are visited in the order the line crosses them, so the line never skips a cell.
fn fly(grid: &mut Grid, world: &mut World, mats: &MaterialTable, x: f32, y: f32, vx: f32, vy: f32) -> Flight {
    let (mut cx, mut cy) = (x.floor() as i32, y.floor() as i32);
    let (ex, ey) = ((x + vx).floor() as i32, (y + vy).floor() as i32);
    let (sx, sy) = ((ex - cx).signum(), (ey - cy).signum());
    let (mut nx, mut ny) = ((ex - cx).abs(), (ey - cy).abs());
    // Line time between two x borders (and y borders), and the time of the next border.
    let dtx = if vx != 0.0 { 1.0 / vx.abs() } else { f32::INFINITY };
    let dty = if vy != 0.0 { 1.0 / vy.abs() } else { f32::INFINITY };
    let mut tx = if sx > 0 { (cx as f32 + 1.0 - x) * dtx } else { (x - cx as f32) * dtx };
    let mut ty = if sy > 0 { (cy as f32 + 1.0 - y) * dty } else { (y - cy as f32) * dty };
    while nx + ny > 0 {
        let last = CellPos::new(cx, cy);
        // Step in x or y, whichever border comes first. The counts make sure that the line ends in
        // the right cell also with rounding errors.
        let t;
        if ny == 0 || (nx > 0 && tx < ty) {
            t = tx;
            cx += sx;
            nx -= 1;
            tx += dtx;
        } else {
            t = ty;
            cy += sy;
            ny -= 1;
            ty += dty;
        }
        let c = CellPos::new(cx, cy);
        match grid.look(world, c) {
            Look::Cell(m) if passable(m, mats) => {}
            Look::Cell(_) | Look::Paused(_) | Look::Wall => return Flight::Hit { last, t },
            Look::Outside if c.y < 0 => return Flight::Ceiling { last, t },
            Look::Outside => return Flight::Lost,
        }
    }
    Flight::Free
}

/// The point on the line at time `t`, moved inside cell `c` (so rounding cannot put it into the
/// next cell).
fn point_in_cell(x: f32, y: f32, vx: f32, vy: f32, t: f32, c: CellPos) -> (f32, f32) {
    let inside = |v: f32, lo: i32| v.clamp(lo as f32 + 0.01, lo as f32 + 0.99);
    (inside(x + vx * t, c.x), inside(y + vy * t, c.y))
}

/// Air, gas and fire do not stop a particle.
#[inline(always)]
fn passable(m: MaterialId, mats: &MaterialTable) -> bool {
    m.is_air() || matches!(mats.phase[m.index()], Phase::Gas | Phase::Fire)
}

/// How far up a landing particle looks for air (through liquid, powder and gas).
const SEARCH_UP: i32 = 64;
/// How far to the sides a landing particle looks for air when the way up is closed.
const SEARCH_AROUND: i32 = 8;

/// The cell where a landing particle becomes a grid cell: `at` if it is air or gas; else the
/// first air cell straight up (through liquid, powder and gas, not through solids); else the
/// nearest air cell around (upper cells first). Only cells in live chunks count.
fn free_spot(grid: &mut Grid, world: &mut World, mats: &MaterialTable, at: CellPos) -> Option<CellPos> {
    if let Look::Cell(m) | Look::Paused(m) = grid.look(world, at)
        && passable(m, mats)
    {
        return Some(at);
    }
    for k in 1..=SEARCH_UP {
        let p = at.offset(0, -k);
        match grid.look(world, p) {
            Look::Cell(m) | Look::Paused(m) if m.is_air() => return Some(p),
            Look::Cell(m) | Look::Paused(m) if mats.phase[m.index()] != Phase::Solid => {}
            _ => break,
        }
    }
    for r in 1..=SEARCH_AROUND {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dy.abs()) != r {
                    continue;
                }
                let p = at.offset(dx, dy);
                if let Look::Cell(m) | Look::Paused(m) = grid.look(world, p)
                    && m.is_air()
                {
                    return Some(p);
                }
            }
        }
    }
    None
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
