//! The cell simulation.
//!
//! `Simulation` is the public interface. The game program runs it on the simulation thread.
//! The headless program and tests call it directly.
//!
//! Module owners (see docs/design/04-build-plan.md):
//! - `chunk`, `world`, `hood`, `schedule`, `update`, `movement`, `lib.rs`: lead
//! - `heat`: task "heat" (heat flow and phase changes)
//! - `react`: task "reactions" (reactions and burning)
//! - `explode`, `particles`: task "explosions and particles"

pub mod chunk;
pub mod explode;
pub mod heat;
pub mod hood;
pub mod movement;
pub mod particles;
pub mod react;
mod schedule;
mod update;
pub mod world;

/// Something that happened in a tick that needs work outside the per-cell update.
#[derive(Debug, Clone, PartialEq)]
pub enum SimEvent {
    /// An explosion at a cell. Task 1C (explosions) handles the queue after movement.
    Explosion { at: CellPos, strength: f32, heat: i16 },
}

use chunk::{Chunk, FLAG_PARITY};
use foundry_content::Content;
use foundry_core::{
    CHUNK_AREA, CellPos, CellRect, ChunkImage, ChunkPos, Command, MaterialId, PaintMode, Rng, SimStats, Snapshot,
    pack_texel,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use world::World;

/// Settings for a new world.
#[derive(Debug, Clone)]
pub struct SimConfig {
    pub width_chunks: i32,
    pub height_chunks: i32,
    pub seed: u64,
    /// Put bedrock on the left, right and bottom edges (2 cells thick).
    pub bedrock_border: bool,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self { width_chunks: 32, height_chunks: 16, seed: 1, bedrock_border: true }
    }
}

/// The state of one cell, for tools and tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub material: MaterialId,
    pub temperature: i16,
}

pub struct Simulation {
    content: Arc<Content>,
    world: World,
    seed: u64,
    tick: u64,
    /// Goes up at each tick and each command that changes cells. Chunks store it as their version.
    stamp: u64,
    paused: bool,
    step_requested: bool,
    view: Option<CellRect>,
    /// Chunk versions that the renderer has.
    sent: HashMap<ChunkPos, u64>,
    stats: SimStats,
    paint_rng: Rng,
    pool: Option<rayon::ThreadPool>,
    /// Events of the last tick. See `events`.
    events: Vec<SimEvent>,
    react: react::ReactTable,
    particles: particles::Particles,
    debug: bool,
    /// Air temperature for each row of cells (°C). Heat moves air cells toward it.
    air_temperature: Vec<i16>,
}

impl Simulation {
    pub fn new(content: Arc<Content>, config: SimConfig) -> Self {
        let bedrock = content.material("bedrock").unwrap_or(MaterialId::AIR);
        let world = World::new(config.width_chunks, config.height_chunks, bedrock);
        let mut sim = Simulation {
            content,
            world,
            seed: config.seed,
            tick: 0,
            stamp: 1,
            paused: false,
            step_requested: false,
            view: None,
            sent: HashMap::new(),
            stats: SimStats::default(),
            paint_rng: Rng::new(config.seed ^ 0x70_6169_6e74),
            pool: None,
            events: Vec::new(),
            react: react::ReactTable::default(),
            particles: particles::Particles::default(),
            debug: false,
            air_temperature: vec![],
        };
        sim.react = react::ReactTable::new(&sim.content);
        sim.air_temperature = vec![foundry_core::DEFAULT_TEMPERATURE; sim.world.height_cells() as usize];
        if config.bedrock_border && !bedrock.is_air() {
            let (w, h) = sim.size_cells();
            for y in 0..h {
                for x in [0, 1, w - 2, w - 1] {
                    sim.set_cell(CellPos::new(x, y), bedrock, None);
                }
            }
            for x in 0..w {
                for y in [h - 2, h - 1] {
                    sim.set_cell(CellPos::new(x, y), bedrock, None);
                }
            }
        }
        sim
    }

    pub fn content(&self) -> &Arc<Content> {
        &self.content
    }

    /// World size in cells (width, height).
    pub fn size_cells(&self) -> (i32, i32) {
        (self.world.width_cells(), self.world.height_cells())
    }

    pub fn tick_count(&self) -> u64 {
        self.tick
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn stats(&self) -> &SimStats {
        &self.stats
    }

    /// Apply one command from the main thread.
    pub fn apply(&mut self, cmd: Command) {
        match cmd {
            Command::Paint { center, radius, material, mode, temperature } => {
                self.paint(center, radius as i32, material, mode, temperature)
            }
            Command::SetView { area } => self.view = Some(area),
            Command::ForgetChunks(list) => {
                for c in list {
                    self.sent.remove(&c);
                }
            }
            Command::ResendAll => self.sent.clear(),
            Command::SetPaused(p) => self.paused = p,
            Command::Step => self.step_requested = true,
            Command::SetDebug(on) => self.debug = on,
        }
    }

    /// Run one tick unless the simulation is paused. A `Step` command allows one tick while paused.
    /// Returns true if a tick ran.
    pub fn advance(&mut self) -> bool {
        if self.paused && !self.step_requested {
            return false;
        }
        self.step_requested = false;
        self.tick();
        true
    }

    /// Run one tick, also when paused.
    pub fn tick(&mut self) {
        let start = Instant::now();
        self.stamp += 1;
        let mats = &self.content.materials;
        // Events of the last tick (explosions) are handled first, so chains spread over ticks.
        let previous = std::mem::take(&mut self.events);
        explode::process(&mut self.world, mats, &previous, &mut self.particles, self.tick, self.seed, self.stamp);
        let t_explode = start.elapsed().as_secs_f32() * 1000.0;
        let awake = schedule::movement_tick(
            &mut self.world,
            mats,
            &self.react,
            self.tick,
            self.seed,
            self.stamp,
            self.pool.as_ref(),
            &mut self.events,
        );
        let t_move = start.elapsed().as_secs_f32() * 1000.0;
        self.particles.step(&mut self.world, mats, self.tick, self.seed, self.stamp);
        let t_particles = start.elapsed().as_secs_f32() * 1000.0;
        heat::step(&mut self.world, mats, &self.air_temperature, self.tick, self.seed, self.stamp, self.pool.as_ref());
        let ms = start.elapsed().as_secs_f32() * 1000.0;
        self.tick += 1;
        let loaded = self.world.chunks.iter().filter(|c| c.is_some()).count() as u32;
        self.stats = SimStats {
            tick: self.tick,
            tick_ms: ms,
            awake_chunks: awake,
            loaded_chunks: loaded,
            sections: vec![
                ("explosions", t_explode),
                ("movement", t_move - t_explode),
                ("particles", t_particles - t_move),
                ("heat", ms - t_particles),
            ],
        };
    }

    /// Set the air temperature for each row of cells (°C), from the top of the world down.
    /// Heat moves air cells toward it. Rows past the end of the list use the last value.
    pub fn set_air_temperature(&mut self, by_row: &[i16]) {
        let h = self.world.height_cells() as usize;
        let last = by_row.last().copied().unwrap_or(foundry_core::DEFAULT_TEMPERATURE);
        self.air_temperature = (0..h).map(|y| by_row.get(y).copied().unwrap_or(last)).collect();
    }

    /// Mutable access to the particles, for tools that spawn them.
    pub fn particles_mut(&mut self) -> &mut particles::Particles {
        &mut self.particles
    }

    /// The events of the last tick (explosions, ...), in a fixed order.
    pub fn events(&self) -> &[SimEvent] {
        &self.events
    }

    /// Use a private thread pool with `n` threads (1 = one thread). By default the simulation uses
    /// the global rayon pool. The result of a tick is the same for any number of threads.
    pub fn set_threads(&mut self, n: usize) {
        self.pool = rayon::ThreadPoolBuilder::new().num_threads(n.max(1)).build().ok();
    }

    /// The cell at a position. Outside the world: bedrock.
    pub fn cell(&self, p: CellPos) -> Cell {
        if !self.world.in_bounds(p) {
            return Cell { material: self.world.outside, temperature: foundry_core::DEFAULT_TEMPERATURE };
        }
        match self.world.chunk(p.chunk()) {
            Some(c) => Cell { material: MaterialId(c.mat[p.local_index()]), temperature: c.temp[p.local_index()] },
            None => Cell { material: MaterialId::AIR, temperature: foundry_core::DEFAULT_TEMPERATURE },
        }
    }

    /// Write one cell. `temperature: None` uses the material's default temperature.
    /// Does nothing outside the world.
    pub fn set_cell(&mut self, p: CellPos, material: MaterialId, temperature: Option<i16>) {
        let mats = &self.content.materials;
        let temp = temperature.unwrap_or(mats.temperature[material.index()]);
        let life = match mats.life[material.index()] {
            Some((lo, hi)) => lo + self.paint_rng.below((hi - lo) as u32 + 1) as u8,
            None => 0,
        };
        let shade = self.paint_rng.next_u32() as u8;
        // A new cell may move in the next tick: give it the parity of the tick before.
        let parity = ((self.tick & 1) as u8) ^ 1;
        let stamp = self.stamp;
        let Some(c) = self.world.chunk_mut(p.chunk()) else { return };
        let i = p.local_index();
        c.mat[i] = material.0;
        c.temp[i] = temp;
        c.shade[i] = shade;
        c.life[i] = life;
        c.motion[i] = 0;
        c.flags[i] = (c.flags[i] & !FLAG_PARITY) | parity;
        c.version = stamp;
        self.world.mark_dirty_around(p);
    }

    /// Write a whole chunk at once (for world generation and loading). `materials` and
    /// `temperatures` have `CHUNK_AREA` entries, row by row from the top. `temperatures: None` uses
    /// each material's default temperature. The whole chunk is updated in the next tick.
    pub fn fill_chunk(&mut self, pos: ChunkPos, materials: &[u16], temperatures: Option<&[i16]>) {
        assert_eq!(materials.len(), CHUNK_AREA);
        if let Some(t) = temperatures {
            assert_eq!(t.len(), CHUNK_AREA);
        }
        let mats = &self.content.materials;
        let parity = ((self.tick & 1) as u8) ^ 1;
        let stamp = self.stamp;
        let mut rng = Rng::for_chunk(self.seed, self.tick, pos, 0x66696c6c);
        let Some(c) = self.world.chunk_mut(pos) else { return };
        for i in 0..CHUNK_AREA {
            let m = materials[i] as usize;
            c.mat[i] = materials[i];
            c.temp[i] = temperatures.map_or(mats.temperature[m], |t| t[i]);
            c.shade[i] = rng.next_u32() as u8;
            c.life[i] = match mats.life[m] {
                Some((lo, hi)) => lo + rng.below((hi - lo) as u32 + 1) as u8,
                None => 0,
            };
            c.motion[i] = 0;
            c.flags[i] = parity;
        }
        c.version = stamp;
        c.dirty = chunk::LocalRect::FULL;
    }

    /// Fill a circle. Bedrock is never replaced, except by painting bedrock.
    pub fn paint(&mut self, center: CellPos, radius: i32, material: MaterialId, mode: PaintMode, temperature: Option<i16>) {
        self.stamp += 1;
        let bedrock = self.world.outside;
        let r2 = radius * radius;
        for y in center.y - radius..=center.y + radius {
            for x in center.x - radius..=center.x + radius {
                let (dx, dy) = (x - center.x, y - center.y);
                if dx * dx + dy * dy > r2 {
                    continue;
                }
                let p = CellPos::new(x, y);
                if !self.world.in_bounds(p) {
                    continue;
                }
                let old = self.world.mat(p);
                if old == bedrock && material != bedrock {
                    continue;
                }
                if mode == PaintMode::OnlyAir && !old.is_air() {
                    continue;
                }
                self.set_cell(p, material, temperature);
            }
        }
    }

    /// The chunks in the view that changed since the last snapshot, and the current numbers.
    pub fn take_snapshot(&mut self) -> Snapshot {
        let mut chunks = vec![];
        if let Some(view) = self.view {
            let bounds = CellRect::new(0, 0, self.world.width_cells(), self.world.height_cells());
            let area = view.intersect(&bounds);
            let in_view: Vec<ChunkPos> = area.chunks().collect();
            self.sent.retain(|c, _| {
                let r = c.cell_rect();
                !r.intersect(&area).is_empty()
            });
            for pos in in_view {
                let Some(ch) = self.world.chunk(pos) else { continue };
                if self.sent.get(&pos) == Some(&ch.version) {
                    continue;
                }
                self.sent.insert(pos, ch.version);
                chunks.push(pack_chunk(pos, ch));
            }
        }
        let mut particles = vec![];
        let mut debug_chunks = vec![];
        if let Some(view) = self.view {
            self.particles.views(view, &mut particles);
            if self.debug {
                for pos in view.chunks() {
                    if let Some(ch) = self.world.chunk(pos)
                        && !ch.dirty.is_empty()
                    {
                        let o = pos.origin();
                        let d = ch.dirty;
                        debug_chunks.push(foundry_core::DebugChunk {
                            pos,
                            updated: CellRect::new(o.x + d.x0, o.y + d.y0, o.x + d.x1, o.y + d.y1),
                        });
                    }
                }
            }
        }
        Snapshot {
            tick: self.tick,
            paused: self.paused,
            world_cells: self.size_cells(),
            chunks,
            particles,
            debug_chunks,
            stats: self.stats.clone(),
        }
    }

    /// A hash of all cell materials and temperatures. Equal worlds give equal hashes.
    pub fn world_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for pos in self.world.loaded_chunks() {
            let c = self.world.chunk(pos).unwrap();
            if c.is_all_air() {
                continue;
            }
            h = fnv(h, &pos.x.to_le_bytes());
            h = fnv(h, &pos.y.to_le_bytes());
            for i in 0..CHUNK_AREA {
                h = fnv(h, &c.mat[i].to_le_bytes());
                h = fnv(h, &c.temp[i].to_le_bytes());
            }
        }
        h
    }

    /// Number of cells of a material in an area.
    pub fn count_material(&self, area: CellRect, material: MaterialId) -> usize {
        let mut n = 0;
        for y in area.y0..area.y1 {
            for x in area.x0..area.x1 {
                if self.cell(CellPos::new(x, y)).material == material {
                    n += 1;
                }
            }
        }
        n
    }

    /// Read access to the world for tools and tests.
    pub fn world(&self) -> &World {
        &self.world
    }
}

fn pack_chunk(pos: ChunkPos, c: &Chunk) -> ChunkImage {
    let mut texels = Vec::with_capacity(CHUNK_AREA);
    for i in 0..CHUNK_AREA {
        texels.push(pack_texel(c.mat[i], c.temp[i], c.shade[i], c.life[i], c.flags[i]));
    }
    ChunkImage { pos, texels: texels.into_boxed_slice() }
}

#[inline]
fn fnv(mut h: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sim() -> Simulation {
        let content = Arc::new(Content::load_default().unwrap());
        Simulation::new(content, SimConfig { width_chunks: 4, height_chunks: 4, seed: 3, bedrock_border: true })
    }

    #[test]
    fn sand_falls_to_the_floor() {
        let mut s = sim();
        let sand = s.content().expect_material("sand");
        s.set_cell(CellPos::new(100, 10), sand, None);
        for _ in 0..400 {
            s.tick();
        }
        // The floor is bedrock at y = 254 and 255. Sand rests on it.
        assert_eq!(s.cell(CellPos::new(100, 253)).material, sand);
        assert_eq!(s.count_material(CellRect::new(0, 0, 256, 256), sand), 1);
    }

    #[test]
    fn water_spreads_flat() {
        let mut s = sim();
        let water = s.content().expect_material("water");
        s.paint(CellPos::new(128, 100), 10, water, PaintMode::Replace, None);
        let total = s.count_material(CellRect::new(0, 0, 256, 256), water);
        for _ in 0..2000 {
            s.tick();
        }
        assert_eq!(s.count_material(CellRect::new(0, 0, 256, 256), water), total, "water is kept");
        // It should cover the floor: the bottom free row is full from wall to wall.
        assert_eq!(s.count_material(CellRect::new(2, 253, 254, 254), water), 252);
    }

    #[test]
    fn snapshot_sends_changed_chunks_once() {
        let mut s = sim();
        s.apply(Command::SetView { area: CellRect::new(0, 0, 256, 256) });
        let first = s.take_snapshot();
        assert!(!first.chunks.is_empty());
        assert!(s.take_snapshot().chunks.is_empty(), "nothing changed");
        s.apply(Command::Paint {
            center: CellPos::new(70, 70),
            radius: 1,
            material: s.content().expect_material("stone"),
            mode: PaintMode::Replace,
            temperature: None,
        });
        let snap = s.take_snapshot();
        assert_eq!(snap.chunks.len(), 1);
        assert_eq!(snap.chunks[0].pos, ChunkPos::new(1, 1));
    }

    #[test]
    fn same_seed_same_world() {
        let run = || {
            let mut s = sim();
            let sand = s.content().expect_material("sand");
            s.paint(CellPos::new(120, 40), 20, sand, PaintMode::Replace, None);
            for _ in 0..300 {
                s.tick();
            }
            s.world_hash()
        };
        assert_eq!(run(), run());
    }
}

#[cfg(test)]
mod schedule_tests {
    use super::*;

    fn big(threads: usize) -> Simulation {
        let content = Arc::new(Content::load_default().unwrap());
        let mut s = Simulation::new(content, SimConfig { width_chunks: 12, height_chunks: 8, seed: 9, bedrock_border: true });
        s.set_threads(threads);
        let c = s.content().clone();
        let (sand, water, oil, stone, smoke) = (
            c.expect_material("sand"),
            c.expect_material("water"),
            c.expect_material("oil"),
            c.expect_material("stone"),
            c.expect_material("smoke"),
        );
        for x in (40..740).step_by(90) {
            s.paint(CellPos::new(x, 300), 12, stone, PaintMode::Replace, None);
        }
        s.paint(CellPos::new(150, 80), 40, sand, PaintMode::Replace, None);
        s.paint(CellPos::new(400, 60), 50, water, PaintMode::Replace, None);
        s.paint(CellPos::new(600, 100), 30, oil, PaintMode::Replace, None);
        s.paint(CellPos::new(300, 400), 20, smoke, PaintMode::Replace, None);
        s
    }

    #[test]
    fn result_does_not_depend_on_thread_count() {
        let mut a = big(1);
        let mut b = big(6);
        for t in 0..400 {
            a.tick();
            b.tick();
            if t % 50 == 0 {
                assert_eq!(a.world_hash(), b.world_hash(), "tick {t}");
            }
        }
        assert_eq!(a.world_hash(), b.world_hash());
    }

    #[test]
    fn material_is_kept_and_chunks_sleep() {
        let mut s = big(4);
        let all = CellRect::new(0, 0, 768, 512);
        let c = s.content().clone();
        let count = |s: &Simulation| {
            ["sand", "water", "oil"].map(|n| s.count_material(all, c.expect_material(n)))
        };
        let before = count(&s);
        // Oil is viscous: a wide oil slope takes about 9000 ticks to become flat.
        for _ in 0..10_000 {
            s.tick();
        }
        assert_eq!(count(&s), before, "no powder or liquid is lost");
        // Smoke is gone after its life; sand, water and oil are at rest.
        assert_eq!(s.count_material(all, c.expect_material("smoke")), 0);
        assert!(s.stats().awake_chunks <= 2, "awake chunks at rest: {}", s.stats().awake_chunks);
    }

    #[test]
    fn oil_floats_on_water() {
        let content = Arc::new(Content::load_default().unwrap());
        let mut s = Simulation::new(content, SimConfig { width_chunks: 2, height_chunks: 2, seed: 2, bedrock_border: true });
        let c = s.content().clone();
        let (water, oil) = (c.expect_material("water"), c.expect_material("oil"));
        // Oil below, water above, in a narrow box.
        let stone = c.expect_material("stone");
        for y in 20..126 {
            for x in [40, 61] {
                s.set_cell(CellPos::new(x, y), stone, None);
            }
        }
        for y in 60..126 {
            for x in 41..61 {
                s.set_cell(CellPos::new(x, y), if y >= 93 { oil } else { water }, None);
            }
        }
        for _ in 0..4000 {
            s.tick();
        }
        let top_oil = s.count_material(CellRect::new(41, 60, 61, 93), oil);
        assert!(top_oil > 600, "most oil is on top: {top_oil} of 660");
    }
}

#[cfg(test)]
mod debug_awake {
    use super::*;
    #[test]
    #[ignore]
    fn print_awake() {
        let content = Arc::new(Content::load_default().unwrap());
        let mut s = Simulation::new(content, SimConfig { width_chunks: 12, height_chunks: 8, seed: 9, bedrock_border: true });
        let c = s.content().clone();
        let (sand, water, oil, stone) = (c.expect_material("sand"), c.expect_material("water"), c.expect_material("oil"), c.expect_material("stone"));
        for x in (40..740).step_by(90) {
            s.paint(CellPos::new(x, 300), 12, stone, PaintMode::Replace, None);
        }
        s.paint(CellPos::new(150, 80), 40, sand, PaintMode::Replace, None);
        s.paint(CellPos::new(400, 60), 50, water, PaintMode::Replace, None);
        s.paint(CellPos::new(600, 100), 30, oil, PaintMode::Replace, None);
        for t in 0..12000 {
            s.tick();
            if t % 1000 == 999 {
                println!("tick {} awake {}", t + 1, s.stats().awake_chunks);
            }
            if [200, 1000, 3000, 6000].contains(&(t + 1)) {
                dump_png(&s, CellRect::new(0, 256, 768, 512), 2, &format!("{}/../../out/mixed_{}.png", env!("CARGO_MANIFEST_DIR"), t + 1));
            }
        }
        for pos in s.world.loaded_chunks().collect::<Vec<_>>() {
            let ch = s.world.chunk(pos).unwrap();
            if ch.dirty.is_empty() { continue; }
            let d = ch.dirty;
            let mut mats = std::collections::BTreeMap::new();
            for y in d.y0..d.y1 { for x in d.x0..d.x1 { *mats.entry(c.materials.ids[ch.mat[foundry_core::local_index(x,y)] as usize].clone()).or_insert(0) += 1; } }
            println!("{pos:?} dirty {d:?} {mats:?}");
        }
        let before = s.world_hash();
        let r = CellRect::new(200, 480, 768, 512);
        let a = ascii(&s, r);
        s.tick();
        let b = ascii(&s, r);
        s.tick();
        let c2 = ascii(&s, r);
        for (i, ((la, lb), lc)) in a.lines().zip(b.lines()).zip(c2.lines()).enumerate() {
            if la != lb || lb != lc {
                let cols: Vec<usize> = la.chars().zip(lb.chars()).enumerate().filter(|(_, (p, q))| p != q).map(|(k, _)| k + 200).collect();
                println!("row {} changed at x {:?}\n{la}\n{lb}\n{lc}", r.y0 + i as i32, cols);
            }
        }
        println!("hash changed in one tick: {}", before != s.world_hash());
    }
}

#[cfg(test)]
pub(crate) fn dump_png(s: &Simulation, r: CellRect, scale: u32, path: &str) {
    let c = s.content();
    let (w, h) = (r.width() as u32, r.height() as u32);
    let mut img = image::RgbImage::new(w * scale, h * scale);
    for y in 0..h {
        for x in 0..w {
            let m = s.cell(CellPos::new(r.x0 + x as i32, r.y0 + y as i32)).material;
            let col = c.materials.colors[m.index()][0];
            let a = col[3] as u32;
            let px = if m.is_air() { [16, 18, 24] } else { [(col[0] as u32 * a / 255) as u8, (col[1] as u32 * a / 255) as u8, (col[2] as u32 * a / 255) as u8] };
            for dy in 0..scale {
                for dx in 0..scale {
                    img.put_pixel(x * scale + dx, y * scale + dy, image::Rgb(px));
                }
            }
        }
    }
    std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap()).ok();
    img.save(path).unwrap();
}

#[cfg(test)]
pub(crate) fn ascii(s: &Simulation, r: CellRect) -> String {
    let c = s.content();
    let mut out = String::new();
    for y in r.y0..r.y1 {
        for x in r.x0..r.x1 {
            let id = &c.materials.ids[s.cell(CellPos::new(x, y)).material.index()];
            out.push(match id.as_str() {
                "air" => '.',
                "bedrock" => '#',
                "stone" => 'S',
                "sand" => 's',
                "water" => 'w',
                "oil" => 'o',
                _ => '?',
            });
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod debug_view {
    use super::*;
    #[test]
    #[ignore]
    fn show_water() {
        let content = Arc::new(Content::load_default().unwrap());
        let mut s = Simulation::new(content, SimConfig { width_chunks: 4, height_chunks: 4, seed: 3, bedrock_border: true });
        let water = s.content().expect_material("water");
        s.paint(CellPos::new(128, 100), 10, water, PaintMode::Replace, None);
        for _ in 0..2000 {
            s.tick();
        }
        println!("{}", ascii(&s, CellRect::new(0, 244, 256, 256)));
        println!("awake {}", s.stats().awake_chunks);
        for c in s.world.chunks.iter_mut().flatten() {
            c.dirty = chunk::LocalRect::FULL;
        }
        let h = s.world_hash();
        s.tick();
        println!("after waking all: changed {} awake {}", h != s.world_hash(), s.stats().awake_chunks);
        for _ in 0..500 {
            s.tick();
        }
        println!("{}", ascii(&s, CellRect::new(0, 244, 256, 256)));
    }
    #[test]
    #[ignore]
    fn show_sand_changes() {
        let content = Arc::new(Content::load_default().unwrap());
        let mut s = Simulation::new(content, SimConfig { width_chunks: 4, height_chunks: 4, seed: 3, bedrock_border: true });
        let sand = s.content().expect_material("sand");
        s.paint(CellPos::new(128, 150), 30, sand, PaintMode::Replace, None);
        for _ in 0..3000 {
            s.tick();
        }
        let r = CellRect::new(60, 200, 200, 256);
        let a = ascii(&s, r);
        s.tick();
        let b = ascii(&s, r);
        for (i, (la, lb)) in a.lines().zip(b.lines()).enumerate() {
            if la != lb {
                println!("row {} changed:\n{la}\n{lb}", r.y0 + i as i32);
            }
        }
        println!("awake {}", s.stats().awake_chunks);
        for pos in s.world.loaded_chunks().collect::<Vec<_>>() {
            let ch = s.world.chunk(pos).unwrap();
            if !ch.dirty.is_empty() { println!("{pos:?} {:?}", ch.dirty); }
        }
    }
}
