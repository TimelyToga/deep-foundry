//! Free-flying cells (technical design section 6.6): splash droplets now, explosion debris later.
//!
//! A particle is a cell that left the grid. It flies with gravity and becomes a grid cell again
//! where it lands, so no material is lost. Visual particles (flag `VISUAL`) never land; they fade.
//! Particles update on one thread, in list order, so the result is deterministic.

use crate::SimSettings;
use crate::chunk::{FLAG_PARITY, MOTION_MOMENTUM_SHIFT, MOTION_RIGHT, MOTION_SPEED};
use crate::world::World;
use foundry_content::{MaterialTable, Phase};
use foundry_core::{CellPos, CellRect, MaterialId, ParticleView};

/// Particle flag: only for drawing. It never becomes a cell.
pub const VISUAL: u8 = 1 << 0;

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
}

impl Particles {
    pub fn len(&self) -> usize {
        self.x.len()
    }

    pub fn is_empty(&self) -> bool {
        self.x.is_empty()
    }

    /// Add a particle. Material particles are always added (dropping one would lose material).
    /// Visual particles are not added when the list is at `max`.
    pub fn spawn(&mut self, s: Spawn, max: usize) {
        if s.flags & VISUAL != 0 && self.len() >= max {
            return;
        }
        self.x.push(s.x);
        self.y.push(s.y);
        self.vx.push(s.vx);
        self.vy.push(s.vy);
        self.mat.push(s.material.0);
        self.temp.push(s.temperature);
        self.shade.push(s.shade);
        self.life.push(s.life);
        self.flags.push(s.flags);
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
        let parity_next_free = (tick & 1) as u8; // the next tick's parity is the other value
        let g = settings.particle_gravity;
        let max_v = settings.particle_max_speed;
        let mut keep = 0;
        for i in 0..self.len() {
            let visual = self.flags[i] & VISUAL != 0;
            if visual {
                if self.life[i] <= 1 {
                    continue;
                }
                self.life[i] -= 1;
            }
            let mut vx = (self.vx[i] * 0.995).clamp(-max_v, max_v);
            let mut vy = (self.vy[i] + g).clamp(-max_v, max_v);
            let (mut px, mut py) = (self.x[i], self.y[i]);
            let here = CellPos::new(px.floor() as i32, py.floor() as i32);
            // The grid changed under a material particle: it lands where it is.
            let mut landed = !visual && world.in_bounds(here) && !passable(world.mat(here), mats);
            let mut lost = false;
            if !landed {
                let steps = vx.abs().max(vy.abs()).ceil().max(1.0) as i32;
                let (dx, dy) = (vx / steps as f32, vy / steps as f32);
                for _ in 0..steps {
                    let (nx, ny) = (px + dx, py + dy);
                    let c = CellPos::new(nx.floor() as i32, ny.floor() as i32);
                    if !world.in_bounds(c) {
                        if c.y < 0 {
                            // Above the top of the world: stop rising; it falls back.
                            vy = vy.max(0.0);
                        } else {
                            // Out through a side or the bottom (only if there is no bedrock border).
                            lost = true;
                        }
                        break;
                    }
                    if !visual && !passable(world.mat(c), mats) {
                        landed = true;
                        break;
                    }
                    px = nx;
                    py = ny;
                }
            }
            if lost {
                continue;
            }
            if landed {
                let at = CellPos::new(px.floor() as i32, py.floor() as i32);
                if let Some(spot) = free_spot(world, mats, at) {
                    let cell = Landing {
                        material: MaterialId(self.mat[i]),
                        temp: self.temp[i],
                        shade: self.shade[i],
                        life: self.life[i],
                        motion: landing_motion(vx, vy),
                    };
                    place(world, mats, spot, cell, parity_next_free, stamp);
                    continue;
                }
                // No air nearby: stop, move up one cell, and try again next tick.
                vx = 0.0;
                vy = 0.0;
                let up = CellPos::new(at.x, at.y - 1);
                if world.in_bounds(up) && mats.phase[world.mat(up).index()] != Phase::Solid {
                    py -= 1.0;
                }
            }
            // Keep this particle (compact the arrays in order).
            self.x[keep] = px;
            self.y[keep] = py;
            self.vx[keep] = vx;
            self.vy[keep] = vy;
            self.mat[keep] = self.mat[i];
            self.temp[keep] = self.temp[i];
            self.shade[keep] = self.shade[i];
            self.life[keep] = self.life[i];
            self.flags[keep] = self.flags[i];
            keep += 1;
        }
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
}

/// Air and gas do not stop a particle.
#[inline]
fn passable(m: MaterialId, mats: &MaterialTable) -> bool {
    m.is_air() || matches!(mats.phase[m.index()], Phase::Gas | Phase::Fire)
}

/// How far up a landing particle looks for air (through liquid, powder and gas).
const SEARCH_UP: i32 = 64;
/// How far to the sides a landing particle looks for air when the way up is closed.
const SEARCH_AROUND: i32 = 8;

/// The cell where a landing particle becomes a grid cell: `at` if it is air or gas; else the
/// first air cell straight up (through liquid, powder and gas, not through solids); else the
/// nearest air cell around (upper cells first).
fn free_spot(world: &World, mats: &MaterialTable, at: CellPos) -> Option<CellPos> {
    if world.in_bounds(at) && passable(world.mat(at), mats) {
        return Some(at);
    }
    for k in 1..=SEARCH_UP {
        let p = at.offset(0, -k);
        if !world.in_bounds(p) {
            break;
        }
        let m = world.mat(p);
        if m.is_air() {
            return Some(p);
        }
        if mats.phase[m.index()] == Phase::Solid {
            break;
        }
    }
    for r in 1..=SEARCH_AROUND {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dy.abs()) != r {
                    continue;
                }
                let p = at.offset(dx, dy);
                if world.in_bounds(p) && world.mat(p).is_air() {
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

/// Write a landing particle into the grid at `p`. If `p` holds a gas, the gas moves to the first
/// air cell above it (or is lost if there is none within `SEARCH_UP` cells; this is rare).
fn place(world: &mut World, mats: &MaterialTable, p: CellPos, cell: Landing, parity: u8, stamp: u64) {
    let old = world.mat(p);
    if !old.is_air() {
        let up = (1..=SEARCH_UP)
            .map(|k| p.offset(0, -k))
            .take_while(|q| world.in_bounds(*q) && passable(world.mat(*q), mats))
            .find(|q| world.mat(*q).is_air());
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
    c.flags[i] = (c.flags[i] & !FLAG_PARITY) | parity;
    c.version = stamp;
    world.mark_dirty_around(p);
}
