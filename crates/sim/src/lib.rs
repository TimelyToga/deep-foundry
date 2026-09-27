//! The cell simulation.
//!
//! `Simulation` is the public interface. The game program runs it on the simulation thread.
//! The headless program and tests call it directly.
//!
//! Module owners (see docs/design/04-build-plan.md):
//! - `chunk`, `world`, `lib.rs`: lead (shared interface; change through interface requests)
//! - `naive`: Milestone 0 movement. Milestone 1 replaces it.

pub mod chunk;
mod naive;
pub mod world;

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
        };
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
        naive::tick(&mut self.world, &self.content.materials, self.tick, self.seed, self.stamp);
        self.tick += 1;
        let ms = start.elapsed().as_secs_f32() * 1000.0;
        let loaded = self.world.chunks.iter().filter(|c| c.is_some()).count() as u32;
        self.stats = SimStats {
            tick: self.tick,
            tick_ms: ms,
            awake_chunks: loaded,
            loaded_chunks: loaded,
            sections: vec![("movement", ms)],
        };
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
        Snapshot {
            tick: self.tick,
            paused: self.paused,
            world_cells: self.size_cells(),
            chunks,
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
