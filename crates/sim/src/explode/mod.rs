//! Explosions (technical design section 6.7).
//!
//! An explosion has a center, a strength and a heat value.
//!
//! - **Strength** uses the same scale as material hardness (0 to 255). A cell breaks if the power
//!   that reaches it is above its hardness. Hardness 255 (bedrock) never breaks. Examples:
//!   10 = a small gas pop, 30 = a small explosion (throws sand and dirt, does not break stone),
//!   60 = breaks a little stone near the center, 150 = a large explosion.
//! - **Radius** grows with the strength: see `radius`. It is at most `MAX_RADIUS` cells.
//! - **Heat** (°C) heats the air, gas and debris near the center and makes a few fire cells there.
//!   0 means a blast with no heat.
//!
//! # What an explosion does
//! 1. Rays go out from the center in all directions. The power on a ray is the strength at the
//!    center and falls to 0 at the radius (slowly near the center, fast near the radius). Each cell that breaks takes some power from the ray
//!    (a harder cell takes more). A cell that does not break stops the ray. So hard walls shield
//!    the cells behind them. Each cell gets the highest power of all rays that reach it.
//! 2. Each cell that breaks:
//!    - near the center (the core: `CORE` × radius), a flammable cell turns into its fire;
//!    - every other cell flies outward as a material particle. The speed falls with the distance
//!      from the center. The direction turns toward the open side (where the rays found air), so
//!      a buried explosion throws its cells out of the hole. A solid flies as its broken form
//!      (stone as gravel). The particles land again as cells, so no material is lost.
//! 3. In the core, air and gas get the heat, and some air cells become fire or smoke. Flammable
//!    gas then ignites through the normal reactions in the next ticks, so chain reactions spread
//!    over ticks.
//! 4. Visual particles: sparks (if there is heat), smoke puffs and dust.
//!
//! Cells of buildings (`FLAG_BUILDING`) do not change and stop rays. Each explosion that runs
//! sends `SimEvent::Exploded`, so the factory can damage buildings and the player, and the game
//! can play a sound and shake the screen.
//!
//! # Queue and limits
//! `SimEvent::Explosion` events of one tick run in the next tick, after the movement passes (the
//! reactions of a tick send them). `Explosions::process` runs queued explosions in order until
//! `WORK_PER_TICK` cells were looked at, but always at least one. The others wait in the queue
//! for the next tick. So a large chain of explosions never stalls a tick.
//!
//! An explosion never changes cells in chunks that do not update:
//! - An explosion whose center is in a chunk outside every simulation area waits in the queue
//!   until an anchor comes near.
//! - Rays stop at chunks outside every simulation area. The cells there do not change.
//! - Before an explosion runs, the chunks around it that update are made live (as the movement
//!   pass does for the neighbors of working chunks).
//!
//! Explosions run on one thread, with random numbers from the world seed, the tick and the
//! explosion. So the result does not depend on the number of threads.

use crate::chunk::{FLAG_BUILDING, FLAG_PARITY};
use crate::particles::{Particles, RISE, Spawn, VISUAL};
use crate::world::World;
use crate::{SimEvent, SimSettings};
use foundry_content::{MaterialTable, Phase};
use foundry_core::{CellPos, CellRect, ChunkPos, DEFAULT_TEMPERATURE, MaterialId, Rng};
use std::collections::VecDeque;
use std::f32::consts::TAU;

#[cfg(test)]
mod tests;

/// Largest radius of an explosion, in cells.
pub const MAX_RADIUS: f32 = 48.0;
/// Cells that explosions may look at in one tick (ray steps plus cells in the square around each
/// explosion). The explosion that goes over it still runs; the next ones wait.
pub const WORK_PER_TICK: usize = 60_000;
/// Most explosions in the queue. Explosion events that come when the queue is full are dropped.
pub const MAX_QUEUED: usize = 4096;
/// The core is the part of the radius where heat, fire and burning happen.
pub const CORE: f32 = 0.45;

/// Power that a breaking cell takes from a ray: this, plus `ABSORB_PER_HARDNESS` × its hardness.
const ABSORB: f32 = 0.25;
const ABSORB_PER_HARDNESS: f32 = 0.15;
/// Rays per cell of circumference.
const RAYS_PER_CELL: f32 = 1.5;
/// Chance that an air cell at the center becomes fire (it falls to 0 at the edge of the core).
const FIRE_CHANCE: f32 = 0.35;
/// Chance that an air cell at the center becomes smoke (it falls to 0 at the edge of the core).
const SMOKE_CHANCE: f32 = 0.2;
/// Chance that a flammable cell in the core turns into fire (else it is thrown).
const BURN_CHANCE: f32 = 0.5;
/// Thrown cells get this part of their speed as an extra upward speed.
const UP_BIAS: f32 = 0.3;
/// How much thrown cells turn toward the open side of the explosion (where the free cells are).
/// With all free cells on one side, a cell thrown straight away from it goes nowhere.
const OPEN_PULL: f32 = 1.0;
/// Chance that a thrown cell also makes a dust particle.
const DUST_CHANCE: f32 = 0.25;

/// Radius in cells of an explosion with this strength.
pub fn radius(strength: f32) -> f32 {
    (1.0 + 1.6 * strength.max(0.0).sqrt()).min(MAX_RADIUS)
}

/// Speed (cells per tick) of cells thrown from the center of an explosion with this strength.
pub fn throw_speed(strength: f32) -> f32 {
    2.0 + 0.6 * strength.max(0.0).sqrt()
}

/// One explosion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Blast {
    pub at: CellPos,
    pub strength: f32,
    pub heat: i16,
}

impl Blast {
    /// Rays and ray steps of this explosion.
    fn rays(&self) -> (usize, i32) {
        let r = radius(self.strength);
        (((TAU * r * RAYS_PER_CELL).ceil() as usize).max(16), (r * 2.0).ceil() as i32)
    }

    /// The cells this explosion looks at: ray steps and the cells of its square.
    fn work(&self) -> usize {
        let (rays, steps) = self.rays();
        let w = 2 * radius(self.strength).ceil() as usize + 1;
        rays * (steps as usize + 1) + w * w
    }
}

/// What the tick passes to the explosion code.
pub struct Ctx<'a> {
    pub mats: &'a MaterialTable,
    pub settings: &'a SimSettings,
    pub seed: u64,
    pub tick: u64,
    pub stamp: u64,
    pub pool: Option<&'a rayon::ThreadPool>,
}

/// A change to one cell of a chunk.
#[derive(Debug, Clone, Copy)]
enum Edit {
    /// The cell flew away: it becomes air with this temperature.
    Air { i: u16, temp: i16 },
    /// The cell becomes another material (fire).
    Cell { i: u16, mat: MaterialId, temp: i16, life: u8 },
    /// The cell gets this temperature.
    Temp { i: u16, temp: i16 },
}

/// The explosion queue and the memory that explosions reuse.
#[derive(Default)]
pub struct Explosions {
    queue: VecDeque<Blast>,
    /// Highest ray power for each cell of the square around the running explosion.
    power: Vec<f32>,
    edits: Vec<Edit>,
    marks: Vec<CellPos>,
    chunks: Vec<ChunkPos>,
    fire: Option<MaterialId>,
    smoke: Option<MaterialId>,
    /// Explosions that ran so far. Part of the seed of each explosion's random numbers.
    count: u64,
}

impl Explosions {
    pub fn new(mats: &MaterialTable) -> Self {
        Self {
            queue: VecDeque::new(),
            power: Vec::new(),
            edits: Vec::new(),
            marks: Vec::new(),
            chunks: Vec::new(),
            fire: mats.find("fire"),
            smoke: mats.find("smoke"),
            count: 0,
        }
    }

    /// Number of explosions that wait in the queue.
    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// Add an explosion to the end of the queue. Returns false if the queue is full.
    pub fn push(&mut self, b: Blast) -> bool {
        if self.queue.len() >= MAX_QUEUED {
            return false;
        }
        self.queue.push_back(b);
        true
    }

    /// Queue the explosion events of the last tick, then run queued explosions in order until
    /// `WORK_PER_TICK` cells were looked at. Explosions whose center does not update wait.
    /// Sends `SimEvent::Exploded` into `out` for each explosion that ran.
    pub fn process(&mut self, world: &mut World, ctx: &Ctx, events: &[SimEvent], particles: &mut Particles, out: &mut Vec<SimEvent>) {
        for e in events {
            if let SimEvent::Explosion { at, strength, heat } = *e {
                self.push(Blast { at, strength, heat });
            }
        }
        let mut work = 0;
        let mut full = false;
        for _ in 0..self.queue.len() {
            let Some(b) = self.queue.pop_front() else { break };
            if !world.in_bounds(b.at) {
                continue;
            }
            if full || !world.areas().simulates(b.at.chunk()) {
                self.queue.push_back(b);
                continue;
            }
            let cost = b.work();
            if work > 0 && work + cost > WORK_PER_TICK {
                full = true;
                self.queue.push_back(b);
                continue;
            }
            work += cost;
            self.run(world, ctx, b, particles, out);
        }
    }

    /// Run one explosion now if its center updates, else queue it. Returns true if it ran.
    pub fn run_now(&mut self, world: &mut World, ctx: &Ctx, b: Blast, particles: &mut Particles, out: &mut Vec<SimEvent>) -> bool {
        if !world.in_bounds(b.at) {
            return false;
        }
        if !world.areas().simulates(b.at.chunk()) {
            self.push(b);
            return false;
        }
        self.run(world, ctx, b, particles, out);
        true
    }

    /// Run one explosion (see the module documentation).
    fn run(&mut self, world: &mut World, ctx: &Ctx, b: Blast, particles: &mut Particles, out: &mut Vec<SimEvent>) {
        let mats = ctx.mats;
        let r = radius(b.strength);
        let reach = r.ceil() as i32;
        let square = CellRect::around(b.at, reach);
        let w = square.width();
        self.count += 1;
        let salt = self.count ^ ((b.at.local_index() as u64) << 32);
        let mut rng = Rng::for_chunk(ctx.seed, ctx.tick, b.at.chunk(), 0x626c_6173_7400 ^ salt);

        // Make the chunks that update live, so that the rays and cells below can read them.
        self.chunks.clear();
        self.chunks.extend(world.clip(square).chunks().filter(|c| world.areas().simulates(*c)));
        world.load(&self.chunks, true, ctx.pool);

        // 1. Rays.
        self.power.clear();
        self.power.resize((w * w) as usize, 0.0);
        let (rays, steps) = b.rays();
        let turn = rng.unit() * TAU / rays as f32;
        // The middle of the explosion cell, for particles (`f64`: exact far from x = 0).
        let (ox, oy) = (b.at.x as f64 + 0.5, b.at.y as f64 + 0.5);
        let mut reader = Reader::default();
        // The open side: the sum of the ray directions, each times the free cells (air, gas, fire)
        // on the ray. `free` is the number of free cells on all rays.
        let (mut open_x, mut open_y, mut free) = (0.0f32, 0.0f32, 0.0f32);
        for k in 0..rays {
            let a = turn + TAU * k as f32 / rays as f32;
            let (dx, dy) = (a.cos(), a.sin());
            let mut absorbed = 0.0;
            let mut last = CellPos::new(i32::MIN, i32::MIN);
            for s in 0..=steps {
                let d = s as f32 * 0.5;
                let f = d / r;
                let power = b.strength * (1.0 - f * f) - absorbed;
                if power <= 0.0 {
                    break;
                }
                // Relative to the explosion cell, so the `f32` numbers stay small.
                let p = b.at.offset((0.5 + dx * d).floor() as i32, (0.5 + dy * d).floor() as i32);
                if p == last {
                    continue;
                }
                last = p;
                let Some((m, flags)) = reader.read(world, p) else { break };
                let slot = &mut self.power[((p.y - square.y0) * w + p.x - square.x0) as usize];
                *slot = slot.max(power);
                if m.is_air() || matches!(mats.phase[m.index()], Phase::Gas | Phase::Fire) {
                    open_x += dx;
                    open_y += dy;
                    free += 1.0;
                    continue;
                }
                let h = mats.hardness[m.index()];
                if h == 255 || h as f32 >= power || flags & FLAG_BUILDING != 0 {
                    break;
                }
                absorbed += ABSORB + ABSORB_PER_HARDNESS * h as f32;
            }
        }

        // Thrown cells turn toward the open side, more when the free cells are all on one side
        // (a buried explosion throws its cells out of the hole, not into the ground).
        let open_len = (open_x * open_x + open_y * open_y).sqrt();
        let (open_x, open_y) = if free > 0.0 && open_len > 0.0 {
            let k = OPEN_PULL * open_len / free / open_len;
            (open_x * k, open_y * k)
        } else {
            (0.0, 0.0)
        };

        // 2. and 3. The cells, chunk by chunk in (y, x) order.
        let core = r * CORE;
        let speed0 = throw_speed(b.strength).min(ctx.settings.particle_max_speed);
        let parity = ((ctx.tick & 1) as u8) ^ 1;
        let max_particles = ctx.settings.max_particles;
        for ci in 0..self.chunks.len() {
            let c = self.chunks[ci];
            let Some(ch) = world.chunk(c) else { continue };
            let o = c.origin();
            let rect = square.intersect(&c.cell_rect());
            self.edits.clear();
            for y in rect.y0..rect.y1 {
                for x in rect.x0..rect.x1 {
                    let power = self.power[((y - square.y0) * w + x - square.x0) as usize];
                    if power <= 0.0 {
                        continue;
                    }
                    let i = CellPos::new(x, y).local_index();
                    if ch.flags[i] & FLAG_BUILDING != 0 {
                        continue;
                    }
                    let m = MaterialId(ch.mat[i]);
                    let (fx, fy) = ((x - b.at.x) as f32, (y - b.at.y) as f32);
                    let d = (fx * fx + fy * fy).sqrt();
                    let in_core = b.heat > 0 && d <= core;
                    let heat = if in_core { (b.heat as f32 * (1.0 - 0.5 * d / core.max(1.0))) as i16 } else { i16::MIN };
                    let i16x = i as u16;
                    match mats.phase[m.index()] {
                        Phase::Empty => {
                            if !in_core {
                                continue;
                            }
                            let near = 1.0 - d / core.max(1.0);
                            let roll = rng.unit();
                            if let Some(fire) = self.fire
                                && roll < FIRE_CHANCE * near
                            {
                                let temp = heat.max(mats.temperature[fire.index()]);
                                self.edits.push(Edit::Cell { i: i16x, mat: fire, temp, life: new_life(mats, fire, &mut rng) });
                            } else if let Some(smoke) = self.smoke
                                && roll > 1.0 - SMOKE_CHANCE * near
                            {
                                self.edits.push(Edit::Cell { i: i16x, mat: smoke, temp: heat, life: new_life(mats, smoke, &mut rng) });
                            } else if heat > ch.temp[i] {
                                self.edits.push(Edit::Temp { i: i16x, temp: heat });
                            }
                        }
                        Phase::Gas => {
                            if in_core && heat > ch.temp[i] {
                                self.edits.push(Edit::Temp { i: i16x, temp: heat });
                            }
                        }
                        Phase::Fire => {}
                        phase => {
                            let h = mats.hardness[m.index()];
                            if h == 255 || h as f32 >= power {
                                continue;
                            }
                            if in_core
                                && let Some(burn) = mats.burn[m.index()]
                                && rng.chance(BURN_CHANCE)
                            {
                                let temp = heat.max(burn.fire_temp);
                                self.edits.push(Edit::Cell { i: i16x, mat: burn.fire, temp, life: new_life(mats, burn.fire, &mut rng) });
                                continue;
                            }
                            // Throw the cell outward.
                            let into = if phase == Phase::Solid { mats.broken_into[m.index()] } else { m };
                            let (mut ux, mut uy) = if d < 0.5 { (rng.unit() - 0.5, -1.0) } else { (fx / d, fy / d) };
                            (ux, uy) = (ux + open_x, uy + open_y);
                            let len = (ux * ux + uy * uy).sqrt().max(0.2);
                            (ux, uy) = (ux / len, uy / len);
                            let spin = (rng.unit() - 0.5) * 0.6;
                            (ux, uy) = (ux * spin.cos() - uy * spin.sin(), ux * spin.sin() + uy * spin.cos());
                            let speed = speed0 * (1.0 - 0.6 * d / r) * (0.75 + 0.5 * rng.unit());
                            let (vx, vy) = (ux * speed, uy * speed - speed * UP_BIAS);
                            let temp = ch.temp[i].max(heat);
                            let life = if into == m { ch.life[i] } else { new_life(mats, into, &mut rng) };
                            let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                            let spawn = Spawn { x: px, y: py, vx, vy, material: into, temperature: temp, shade: ch.shade[i], life, flags: 0 };
                            particles.spawn(spawn, usize::MAX);
                            if rng.chance(DUST_CHANCE) {
                                let dust = Spawn {
                                    vx: vx * 0.5 + rng.unit() - 0.5,
                                    vy: vy * 0.5 - rng.unit() * 0.5,
                                    life: 12 + rng.below(20) as u8,
                                    flags: VISUAL,
                                    ..spawn
                                };
                                particles.spawn(dust, max_particles);
                            }
                            self.edits.push(Edit::Air { i: i16x, temp: heat.max(DEFAULT_TEMPERATURE) });
                        }
                    }
                }
            }
            if self.edits.is_empty() {
                continue;
            }
            let Some(ch) = world.chunk_mut(c) else { continue };
            self.marks.clear();
            for e in &self.edits {
                let i = match *e {
                    Edit::Air { i, temp } => {
                        let i = i as usize;
                        (ch.mat[i], ch.temp[i], ch.life[i], ch.motion[i]) = (0, temp, 0, 0);
                        i
                    }
                    Edit::Cell { i, mat, temp, life } => {
                        let i = i as usize;
                        (ch.mat[i], ch.temp[i], ch.life[i], ch.motion[i]) = (mat.0, temp, life, 0);
                        i
                    }
                    Edit::Temp { i, temp } => {
                        let i = i as usize;
                        ch.temp[i] = temp;
                        i
                    }
                };
                ch.flags[i] = (ch.flags[i] & !FLAG_PARITY) | parity;
                let (lx, ly) = foundry_core::local_xy(i);
                self.marks.push(o.offset(lx, ly));
            }
            ch.version = ctx.stamp;
            for &p in &self.marks {
                world.mark_dirty_around(p);
            }
        }

        // 4. Visual particles: sparks and smoke puffs.
        if let Some(fire) = self.fire
            && b.heat > 0
        {
            for _ in 0..(r * 1.5) as usize {
                let a = rng.unit() * TAU;
                let speed = speed0 * (1.0 + rng.unit());
                let spark = Spawn {
                    x: ox,
                    y: oy,
                    vx: a.cos() * speed,
                    vy: a.sin() * speed - speed * UP_BIAS,
                    material: fire,
                    temperature: b.heat.max(mats.temperature[fire.index()]),
                    shade: rng.next_u32() as u8,
                    life: 8 + rng.below(16) as u8,
                    flags: VISUAL,
                };
                particles.spawn(spark, max_particles);
            }
        }
        if let Some(smoke) = self.smoke {
            for _ in 0..(r * 0.8) as usize {
                let a = rng.unit() * TAU;
                let d = rng.unit() * core.max(1.0);
                let speed = 0.3 + rng.unit() * 0.6;
                let puff = Spawn {
                    x: ox + (a.cos() * d) as f64,
                    y: oy + (a.sin() * d) as f64,
                    vx: a.cos() * speed,
                    vy: a.sin() * speed - 0.3,
                    material: smoke,
                    temperature: mats.temperature[smoke.index()],
                    shade: rng.next_u32() as u8,
                    life: 40 + rng.below(70) as u8,
                    flags: VISUAL | RISE,
                };
                particles.spawn(puff, max_particles);
            }
        }
        out.push(SimEvent::Exploded { at: b.at, radius: r, strength: b.strength });
    }
}

/// A new life value for a cell of material `m` (fading materials), else 0.
fn new_life(mats: &MaterialTable, m: MaterialId, rng: &mut Rng) -> u8 {
    match mats.life[m.index()] {
        Some((lo, hi)) => lo + rng.below((hi - lo) as u32 + 1) as u8,
        None => 0,
    }
}

/// Reads cells for the rays. It keeps the last chunk. Chunks outside the world, outside every
/// simulation area or not live give `None`: the ray stops there.
#[derive(Default)]
struct Reader<'w> {
    pos: Option<ChunkPos>,
    chunk: Option<&'w crate::chunk::Chunk>,
}

impl<'w> Reader<'w> {
    #[inline]
    fn read(&mut self, world: &'w World, p: CellPos) -> Option<(MaterialId, u8)> {
        let c = p.chunk();
        if self.pos != Some(c) {
            self.pos = Some(c);
            self.chunk = if world.chunk_in_bounds(c) && world.areas().simulates(c) { world.chunk(c) } else { None };
        }
        let ch = self.chunk?;
        let i = p.local_index();
        Some((MaterialId(ch.mat[i]), ch.flags[i]))
    }
}
