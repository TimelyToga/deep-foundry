//! The cell simulation.
//!
//! `Simulation` is the public interface. The game program runs it on the simulation thread.
//! The headless program and tests call it directly.
//!
//! The world has no limit to the left and right, and a fixed height (see `world.rs`). Chunks are
//! made by a `ChunkSource` when they are first needed. Only chunks near an anchor (the view, and
//! later the player and buildings) update; see `Simulation::add_anchor`.
//!
//! Module owners (see docs/design/04-build-plan.md):
//! - `chunk`, `world`, `hood`, `schedule`, `update`, `movement`, `source`, `pack`, `lib.rs`: lead
//! - `heat`: task "heat" (heat flow and phase changes)
//! - `react`: task "reactions" (reactions and burning)
//! - `explode`, `particles`: task "explosions and particles"

pub mod chunk;
pub mod explode;
pub mod heat;
pub mod hood;
pub mod movement;
pub mod pack;
pub mod particles;
pub mod react;
pub mod save;
mod schedule;
pub mod source;
mod update;
pub mod world;

pub use pack::{ChunkStore, PackedChunk};
pub use source::{AirSource, ChunkCells, ChunkSource, LayerSource};
pub use world::MemoryStats;

/// Something that happened in a tick that needs work outside the per-cell update.
#[derive(Debug, Clone, PartialEq)]
pub enum SimEvent {
    /// An explosion at a cell. Task 1C (explosions) handles the queue after movement.
    Explosion { at: CellPos, strength: f32, heat: i16 },
    /// An explosion ran in this tick (for damage, sound and screen shake). `radius` is in cells.
    /// See `explode.rs` for the strength scale.
    Exploded { at: CellPos, radius: f32, strength: f32 },
}

use chunk::{Chunk, FLAG_PARITY};
use foundry_content::Content;
use foundry_core::{
    CHUNK_AREA, CHUNK_SIZE, CellPos, CellRect, ChunkImage, ChunkPos, Command, MaterialId, PaintMode, Rng, SimStats,
    Snapshot, local_index, pack_texel,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use world::{Areas, World};

/// Chunks of sky above the surface level in a default world.
pub const DEFAULT_SKY_CHUNKS: i32 = 16;
/// Chunks from the surface level down to the bottom of a default world (8192 cells).
pub const DEFAULT_DEPTH_CHUNKS: i32 = 128;

/// Settings for a new world.
#[derive(Clone)]
pub struct SimConfig {
    pub seed: u64,
    /// `None`: the world has no limit to the left and right (the game world).
    /// `Some(w)`: a finite box `w` chunks wide, from x = 0 to x = w × 64 (for tests and scenes).
    /// Cells outside the box act as bedrock.
    pub width_chunks: Option<i32>,
    /// Chunks of sky above the surface level. The top of the sky is y = 0, so the surface level
    /// is at y = `sky_chunks` × 64. Cells above the top act as bedrock (a closed ceiling for now).
    pub sky_chunks: i32,
    /// Chunks from the surface level down to the bottom of the world. Cells below act as bedrock.
    pub depth_chunks: i32,
    /// Finite box only: bedrock on the left, right and bottom edges (2 cells thick).
    /// An infinite world gets its bedrock from the chunk source.
    pub bedrock_border: bool,
    /// Makes the cells of new chunks. `None`: air for a finite box, and a `LayerSource` (air above
    /// the surface level, stone below, bedrock at the bottom) for an infinite world.
    pub source: Option<Arc<dyn ChunkSource>>,
}

impl SimConfig {
    /// A finite box of `width` × `height` chunks, all air, with a bedrock border.
    /// This is the world of the small tests and scenes.
    pub fn finite(width_chunks: i32, height_chunks: i32, seed: u64) -> Self {
        Self { seed, width_chunks: Some(width_chunks), sky_chunks: 0, depth_chunks: height_chunks, bedrock_border: true, source: None }
    }

    /// An infinite world with the default height and the given source (`None`: `LayerSource`).
    pub fn infinite(seed: u64, source: Option<Arc<dyn ChunkSource>>) -> Self {
        Self { seed, source, ..Self::default() }
    }

    /// Height of the world in chunks.
    pub fn height_chunks(&self) -> i32 {
        self.sky_chunks + self.depth_chunks
    }

    /// The row of cells where the surface level is.
    pub fn surface_y(&self) -> i32 {
        self.sky_chunks * CHUNK_SIZE
    }
}

impl Default for SimConfig {
    /// An infinite world: 16 chunks of sky and 128 chunks below the surface level.
    fn default() -> Self {
        Self {
            seed: 1,
            width_chunks: None,
            sky_chunks: DEFAULT_SKY_CHUNKS,
            depth_chunks: DEFAULT_DEPTH_CHUNKS,
            bedrock_border: true,
            source: None,
        }
    }
}

impl std::fmt::Debug for SimConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimConfig")
            .field("seed", &self.seed)
            .field("width_chunks", &self.width_chunks)
            .field("sky_chunks", &self.sky_chunks)
            .field("depth_chunks", &self.depth_chunks)
            .field("bedrock_border", &self.bedrock_border)
            .field("source", &self.source.as_ref().map(|s| s.name().to_string()))
            .finish()
    }
}

/// Global simulation settings. Change them with `Simulation::settings_mut`. Per-material settings
/// (flow, momentum, splash, viscosity, friction, ...) are in the data files.
#[derive(Debug, Clone, PartialEq)]
pub struct SimSettings {
    /// Chunks around each anchor area that also update. A chunk with work that is farther from
    /// every anchor waits (it keeps its work) until an anchor comes near.
    pub sim_margin_chunks: i32,
    /// Chunks around each anchor area that stay unpacked in memory. Farther chunks are packed
    /// (changed chunks) or dropped (chunks that did not change since the source made them).
    /// The world uses at least `sim_margin_chunks + 2`.
    pub keep_margin_chunks: i32,
    /// Look for far chunks to pack or drop every this many ticks. 0: never.
    pub unload_every_ticks: u32,
    /// Gravity for free-flying particles, in cells per tick².
    pub particle_gravity: f32,
    /// Top speed of particles, in cells per tick.
    pub particle_max_speed: f32,
    /// Soft limit on particles. Splashes stop above half of it; visual particles stop at it.
    pub max_particles: usize,
    /// A liquid cell must land with at least this fall speed (0 to 28) to splash.
    /// Fall speed goes up by 1 per tick of free fall; a cell falls `1 + speed / 4` cells per tick.
    pub splash_min_speed: u8,
    /// How far (cells, 1 to 31) a liquid cell looks to the side for a place to fall, and how far
    /// pressure can push it. A liquid uses at most 4 × its `flow`.
    pub liquid_look_ahead: i32,
}

impl Default for SimSettings {
    fn default() -> Self {
        Self {
            sim_margin_chunks: 4,
            keep_margin_chunks: 8,
            unload_every_ticks: 10,
            particle_gravity: 0.18,
            particle_max_speed: 12.0,
            max_particles: 50_000,
            splash_min_speed: 10,
            liquid_look_ahead: 31,
        }
    }
}

/// A number setting that the game shows as a slider (see `SimSettings::sliders`).
#[derive(Debug, Clone, PartialEq)]
pub struct SettingSlider {
    /// The key for `SimSettings::set` and `Command::SetSimSetting`.
    pub key: &'static str,
    /// The name shown to the player.
    pub label: &'static str,
    /// One short sentence about what it does.
    pub help: &'static str,
    pub value: f32,
    pub min: f32,
    pub max: f32,
    /// Round the value to steps of this size.
    pub step: f32,
}

impl SimSettings {
    /// The settings that a player can change with a slider: the liquid and droplet settings.
    /// (The per-material liquid values are in the data files; see `assets/data/README.md`.)
    pub fn sliders(&self) -> Vec<SettingSlider> {
        vec![
            SettingSlider {
                key: "splash_min_speed",
                label: "Splash speed",
                help: "Liquid that lands faster than this splashes droplets. Lower: more splashes.",
                value: self.splash_min_speed as f32,
                min: 0.0,
                max: 28.0,
                step: 1.0,
            },
            SettingSlider {
                key: "liquid_look_ahead",
                label: "Liquid reach",
                help: "How far liquids look and push to the side. Lower: slower leveling, steeper heaps.",
                value: self.liquid_look_ahead as f32,
                min: 1.0,
                max: 31.0,
                step: 1.0,
            },
            SettingSlider {
                key: "particle_gravity",
                label: "Droplet gravity",
                help: "Gravity for droplets in the air. Higher: lower, shorter splashes.",
                value: self.particle_gravity,
                min: 0.05,
                max: 0.5,
                step: 0.01,
            },
        ]
    }

    /// Change a setting by its slider key. The value is clamped to the slider range.
    /// Returns false for an unknown key.
    pub fn set(&mut self, key: &str, value: f32) -> bool {
        let Some(s) = self.sliders().into_iter().find(|s| s.key == key) else { return false };
        let v = value.clamp(s.min, s.max);
        match key {
            "splash_min_speed" => self.splash_min_speed = v.round() as u8,
            "liquid_look_ahead" => self.liquid_look_ahead = v.round() as i32,
            "particle_gravity" => self.particle_gravity = v,
            _ => return false,
        }
        true
    }
}

/// Identifies an anchor. See `Simulation::add_anchor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AnchorId(pub u64);

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
    /// The area the renderer shows (from `Command::SetView`). It is also an anchor.
    view: Option<CellRect>,
    /// Other anchors: areas that must update and stay in memory.
    anchors: Vec<(AnchorId, CellRect)>,
    next_anchor: u64,
    /// The anchors or settings changed since the world last got its areas.
    anchors_changed: bool,
    /// Chunk versions that the renderer has. A chunk that is not here is air for the renderer.
    sent: HashMap<ChunkPos, u64>,
    stats: SimStats,
    paint_rng: Rng,
    pool: Option<rayon::ThreadPool>,
    /// Events of the last tick. See `events`.
    events: Vec<SimEvent>,
    react: react::ReactTable,
    particles: particles::Particles,
    explosions: explode::Explosions,
    debug: bool,
    /// Air temperature for each row of cells (°C). Heat moves air cells toward it.
    air_temperature: Vec<i16>,
    settings: SimSettings,
}

impl Simulation {
    pub fn new(content: Arc<Content>, config: SimConfig) -> Self {
        let bedrock = content.material("bedrock").unwrap_or(MaterialId::AIR);
        let height_chunks = config.height_chunks().max(1);
        let source: Arc<dyn ChunkSource> = match (&config.source, config.width_chunks) {
            (Some(s), _) => s.clone(),
            (None, Some(_)) => Arc::new(AirSource),
            (None, None) => Arc::new(LayerSource::new(&content, config.surface_y(), height_chunks * CHUNK_SIZE)),
        };
        let world = World::new(content.clone(), source, config.seed, height_chunks, config.width_chunks, bedrock);
        let mut sim = Simulation {
            content,
            world,
            seed: config.seed,
            tick: 0,
            // Version 1 means "as the source made it" (`chunk::GENERATED_VERSION`).
            stamp: chunk::GENERATED_VERSION + 1,
            paused: false,
            step_requested: false,
            view: None,
            anchors: Vec::new(),
            next_anchor: 1,
            anchors_changed: true,
            sent: HashMap::new(),
            stats: SimStats::default(),
            paint_rng: Rng::new(config.seed ^ 0x70_6169_6e74),
            pool: None,
            events: Vec::new(),
            react: react::ReactTable::default(),
            particles: particles::Particles::default(),
            explosions: explode::Explosions::default(),
            debug: false,
            air_temperature: vec![],
            settings: SimSettings::default(),
        };
        sim.react = react::ReactTable::new(&sim.content);
        sim.explosions = explode::Explosions::new(&sim.content.materials);
        sim.air_temperature = vec![foundry_core::DEFAULT_TEMPERATURE; sim.world.height_cells() as usize];
        if config.bedrock_border && !bedrock.is_air() && config.width_chunks.is_some() {
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

    /// World size in cells (width, height). The world goes from y = 0 down to y = height.
    ///
    /// The width is 0 for a world with no limit to the left and right (the normal game world).
    /// For a finite box, x goes from 0 to width.
    pub fn size_cells(&self) -> (i32, i32) {
        (self.world.width_chunks().map_or(0, |w| w * CHUNK_SIZE), self.world.height_cells())
    }

    pub fn tick_count(&self) -> u64 {
        self.tick
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn stats(&self) -> &SimStats {
        &self.stats
    }

    /// How much chunk data the world holds.
    pub fn memory(&self) -> MemoryStats {
        self.world.memory()
    }

    /// Apply one command from the main thread.
    pub fn apply(&mut self, cmd: Command) {
        match cmd {
            Command::Paint { center, radius, material, mode, temperature } => {
                self.paint(center, radius as i32, material, mode, temperature)
            }
            Command::SetView { area } => {
                if self.view != Some(area) {
                    self.view = Some(area);
                    self.anchors_changed = true;
                }
            }
            Command::ForgetChunks(list) => {
                for c in list {
                    self.sent.remove(&c);
                }
            }
            Command::ResendAll => self.sent.clear(),
            Command::SetPaused(p) => self.paused = p,
            Command::Step => self.step_requested = true,
            Command::SetDebug(on) => self.debug = on,
            Command::SetSimSetting { key, value } => {
                self.settings.set(&key, value);
            }
            // Handled by the thread that owns the simulation.
            Command::SaveWorld { .. } | Command::LoadWorld { .. } => {}
        }
    }

    // ---- Anchors ----

    /// Add an anchor: an area of cells that must update and stay in memory, for example the
    /// player or a factory building. Chunks within `SimSettings::sim_margin_chunks` of any anchor
    /// (or of the view) update; farther chunks wait until an anchor comes near. The chunks inside
    /// the area are made at the start of the next tick.
    ///
    /// With no anchors and no view, the whole world updates.
    pub fn add_anchor(&mut self, area: CellRect) -> AnchorId {
        let id = AnchorId(self.next_anchor);
        self.next_anchor += 1;
        self.anchors.push((id, area));
        self.anchors_changed = true;
        id
    }

    /// Move an anchor to a new area. Returns false if there is no anchor with this id.
    pub fn move_anchor(&mut self, id: AnchorId, area: CellRect) -> bool {
        match self.anchors.iter_mut().find(|a| a.0 == id) {
            Some(a) => {
                if a.1 != area {
                    a.1 = area;
                    self.anchors_changed = true;
                }
                true
            }
            None => false,
        }
    }

    /// Remove an anchor. Returns false if there is no anchor with this id.
    pub fn remove_anchor(&mut self, id: AnchorId) -> bool {
        let before = self.anchors.len();
        self.anchors.retain(|a| a.0 != id);
        self.anchors_changed |= self.anchors.len() != before;
        self.anchors.len() != before
    }

    /// The anchors (without the view), in the order they were added.
    pub fn anchors(&self) -> &[(AnchorId, CellRect)] {
        &self.anchors
    }

    /// The area of the last `Command::SetView`.
    pub fn view(&self) -> Option<CellRect> {
        self.view
    }

    /// Give the world its update and keep areas, and make the chunks inside the anchor areas.
    fn sync_anchors(&mut self) {
        if !self.anchors_changed {
            return;
        }
        self.anchors_changed = false;
        let rects: Vec<CellRect> = self.view.iter().copied().chain(self.anchors.iter().map(|a| a.1)).collect();
        let s = &self.settings;
        let areas = Areas::new(&rects, s.sim_margin_chunks, s.keep_margin_chunks);
        self.world.set_areas(areas, self.pool.as_ref());
        let mut need = Vec::new();
        for r in &rects {
            need.extend(self.world.clip(*r).chunks());
        }
        self.world.load(&need, false, self.pool.as_ref());
    }

    // ---- Ticks ----

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
        let ms = |start: Instant| start.elapsed().as_secs_f32() * 1000.0;
        self.stamp += 1;
        let generated_before = self.world.generated_total();
        self.sync_anchors();
        let t_anchors = ms(start);
        let mats = &self.content.materials;
        // Events of the last tick (explosions) run after this tick's movement, so chains spread
        // over ticks.
        let previous = std::mem::take(&mut self.events);
        let mut spawns = vec![];
        let splash_ok = self.particles.len() < self.settings.max_particles / 2;
        let awake = schedule::movement_tick(
            &mut self.world,
            schedule::PassInput { mats, react: &self.react, settings: &self.settings, splash_ok },
            self.tick,
            self.seed,
            self.stamp,
            self.pool.as_ref(),
            &mut self.events,
            &mut spawns,
        );
        for sp in spawns {
            self.particles.spawn(sp, self.settings.max_particles);
        }
        let t_move = ms(start);
        let ctx = explode::Ctx { mats, settings: &self.settings, seed: self.seed, tick: self.tick, stamp: self.stamp, pool: self.pool.as_ref() };
        self.explosions.process(&mut self.world, &ctx, &previous, &mut self.particles, &mut self.events);
        let t_explode = ms(start);
        self.particles.step(&mut self.world, mats, &self.settings, self.tick, self.stamp);
        let t_particles = ms(start);
        heat::step(&mut self.world, mats, &self.air_temperature, self.tick, self.seed, self.stamp, self.pool.as_ref());
        let t_heat = ms(start);
        let every = self.settings.unload_every_ticks as u64;
        if every > 0 && self.tick.is_multiple_of(every) {
            self.world.unload_far(self.pool.as_ref());
        }
        let total = ms(start);
        self.tick += 1;
        self.stats = SimStats {
            tick: self.tick,
            tick_ms: total,
            awake_chunks: awake,
            loaded_chunks: self.world.live_count() as u32,
            packed_chunks: self.world.packed_count() as u32,
            generated_chunks: (self.world.generated_total() - generated_before) as u32,
            sections: vec![
                ("anchors", t_anchors),
                ("movement", t_move - t_anchors),
                ("explosions", t_explode - t_move),
                ("particles", t_particles - t_explode),
                ("heat", t_heat - t_particles),
                ("memory", total - t_heat),
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

    pub fn settings(&self) -> &SimSettings {
        &self.settings
    }

    /// Change global settings. They apply from the next tick.
    pub fn settings_mut(&mut self) -> &mut SimSettings {
        self.anchors_changed = true;
        &mut self.settings
    }

    /// The particles (free-flying cells).
    pub fn particles(&self) -> &particles::Particles {
        &self.particles
    }

    /// Mutable access to the particles, for tools that spawn them.
    pub fn particles_mut(&mut self) -> &mut particles::Particles {
        &mut self.particles
    }

    /// Run an explosion now (for tools, tests and the debug brush). See `explode.rs` for the
    /// strength scale; `heat` is in °C (0: no heat). The thrown cells fly from the next tick on.
    /// If the center is in a chunk that does not update, the explosion waits in the queue until
    /// an anchor comes near, and this returns false.
    pub fn explode(&mut self, center: CellPos, strength: f32, heat: i16) -> bool {
        self.sync_anchors();
        self.stamp += 1;
        let ctx = explode::Ctx {
            mats: &self.content.materials,
            settings: &self.settings,
            seed: self.seed,
            tick: self.tick,
            stamp: self.stamp,
            pool: self.pool.as_ref(),
        };
        let blast = explode::Blast { at: center, strength, heat };
        self.explosions.run_now(&mut self.world, &ctx, blast, &mut self.particles, &mut self.events)
    }

    /// Number of explosions that wait in the queue.
    pub fn queued_explosions(&self) -> usize {
        self.explosions.queued()
    }

    /// Add a material particle: a cell that flies from `pos` with `velocity` (cells per tick) and
    /// becomes a cell where it lands. `temperature: None` uses the material's default temperature.
    pub fn spawn_particle(&mut self, pos: (f32, f32), velocity: (f32, f32), material: MaterialId, temperature: Option<i16>) {
        let mats = &self.content.materials;
        let life = match mats.life[material.index()] {
            Some((lo, hi)) => lo + self.paint_rng.below((hi - lo) as u32 + 1) as u8,
            None => 0,
        };
        let spawn = particles::Spawn {
            x: pos.0,
            y: pos.1,
            vx: velocity.0,
            vy: velocity.1,
            material,
            temperature: temperature.unwrap_or(mats.temperature[material.index()]),
            shade: self.paint_rng.next_u32() as u8,
            life,
            flags: 0,
        };
        self.particles.spawn(spawn, usize::MAX);
    }

    /// Add a visual particle (only drawn, never a cell) that lives `life` ticks. `rise`: it rises
    /// slowly like smoke; else it falls like a spark. Returns false if there are too many.
    pub fn spawn_visual(&mut self, pos: (f32, f32), velocity: (f32, f32), material: MaterialId, life: u8, rise: bool) -> bool {
        let spawn = particles::Spawn {
            x: pos.0,
            y: pos.1,
            vx: velocity.0,
            vy: velocity.1,
            material,
            temperature: self.content.materials.temperature[material.index()],
            shade: self.paint_rng.next_u32() as u8,
            life,
            flags: particles::VISUAL | if rise { particles::RISE } else { 0 },
        };
        self.particles.spawn(spawn, self.settings.max_particles)
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

    // ---- Cells ----

    /// The cell at a position. Outside the world: bedrock. A cell in a chunk that is not in
    /// memory is read from the chunk source (slow, but it does not change the world).
    pub fn cell(&self, p: CellPos) -> Cell {
        let (material, temperature) = self.world.cell(p);
        Cell { material, temperature }
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
        // Make the chunks first (in parallel), so that reading the old cells below is fast.
        let area = self.world.clip(CellRect::around(center, radius));
        let chunks: Vec<ChunkPos> = area.chunks().collect();
        self.world.load(&chunks, false, self.pool.as_ref());
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

    // ---- Snapshots and tools ----

    /// The chunks in the view that changed since the last snapshot, and the current numbers.
    pub fn take_snapshot(&mut self) -> Snapshot {
        // Make the chunks of a new view now, if no tick ran since the view changed.
        self.sync_anchors();
        let mut chunks = vec![];
        if let Some(view) = self.view {
            let area = self.world.clip(view);
            self.sent.retain(|c, _| !c.cell_rect().intersect(&area).is_empty());
            for pos in area.chunks() {
                // A chunk that is not live here is all air (sync_anchors made the view).
                let chunk = self.world.chunk(pos);
                let version = chunk.map_or(0, |c| c.version);
                if self.sent.get(&pos).copied().unwrap_or(0) == version {
                    continue;
                }
                self.sent.insert(pos, version);
                chunks.push(match chunk {
                    Some(ch) => pack_chunk(pos, ch),
                    None => ChunkImage::new_air(pos),
                });
            }
        }
        let mut particles = vec![];
        let mut debug_chunks = vec![];
        if let Some(view) = self.view {
            self.particles.views(view, &mut particles);
            if self.debug {
                for pos in self.world.clip(view).chunks() {
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
            notices: vec![],
            stats: self.stats.clone(),
        }
    }

    /// A hash of the materials and temperatures of all changed chunks (chunks that are not
    /// pristine), in (y, x) order. Unchanged chunks come from the seed and the chunk source, so equal
    /// worlds give equal hashes. Chunks that are all air are skipped.
    pub fn world_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for pos in self.world.changed_positions() {
            self.world.with_cells(pos, |c| {
                let Some(c) = c else { return };
                if c.is_all_air() {
                    return;
                }
                h = fnv(h, &pos.x.to_le_bytes());
                h = fnv(h, &pos.y.to_le_bytes());
                for i in 0..CHUNK_AREA {
                    h = fnv(h, &c.mat[i].to_le_bytes());
                    h = fnv(h, &c.temp[i].to_le_bytes());
                }
            });
        }
        h
    }

    /// Number of cells of a material in an area. Cells outside the world count as bedrock.
    /// Chunks that are not in memory are read from the chunk source; the world does not change.
    pub fn count_material(&self, area: CellRect, material: MaterialId) -> usize {
        let mut n = 0;
        for c in area.chunks() {
            let r = area.intersect(&c.cell_rect());
            if r.is_empty() {
                continue;
            }
            if !self.world.chunk_in_bounds(c) {
                if material == self.world.outside {
                    n += (r.width() * r.height()) as usize;
                }
                continue;
            }
            let o = c.origin();
            n += self.world.with_cells(c, |cells| match cells {
                None if material.is_air() => (r.width() * r.height()) as usize,
                None => 0,
                Some(ch) => {
                    let mut k = 0;
                    for y in r.y0..r.y1 {
                        for x in r.x0..r.x1 {
                            k += (ch.mat[local_index(x - o.x, y - o.y)] == material.0) as usize;
                        }
                    }
                    k
                }
            });
        }
        n
    }

    /// Read access to the world for tools and tests.
    pub fn world(&self) -> &World {
        &self.world
    }

    /// Use a store for packed chunks, for example region files on disk (see `ChunkStore`).
    /// When the packed chunks in memory use more than `limit_bytes`, the farthest move into it.
    pub fn set_chunk_store(&mut self, store: Box<dyn ChunkStore>, limit_bytes: usize) {
        self.world.set_store(store, limit_bytes);
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
        Simulation::new(content, SimConfig::finite(4, 4, 3))
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
        let mut s = Simulation::new(content, SimConfig::finite(12, 8, 9));
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
        let mut s = Simulation::new(content, SimConfig::finite(2, 2, 2));
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
        let mut s = Simulation::new(content, SimConfig::finite(12, 8, 9));
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
    let img = render(s, r, scale);
    std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap()).ok();
    img.save(path).unwrap();
}

/// A picture of the cells in `r` (each cell `scale` × `scale` pixels). Particles are light dots.
#[cfg(test)]
pub(crate) fn render(s: &Simulation, r: CellRect, scale: u32) -> image::RgbImage {
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
    let mut views = vec![];
    s.particles().views(r, &mut views);
    for v in views {
        let (px, py) = ((v.x - r.x0 as f32) as u32, (v.y - r.y0 as f32) as u32);
        if px < w && py < h {
            let col = c.materials.colors[v.material as usize][0];
            for dy in 0..scale {
                for dx in 0..scale {
                    let lighter = |c: u8| c.saturating_add(70);
                    img.put_pixel(px * scale + dx, py * scale + dy, image::Rgb([lighter(col[0]), lighter(col[1]), lighter(col[2])]));
                }
            }
        }
    }
    img
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
mod liquid_tests;

#[cfg(test)]
mod debug_view {
    use super::*;
    #[test]
    #[ignore]
    fn show_water() {
        let content = Arc::new(Content::load_default().unwrap());
        let mut s = Simulation::new(content, SimConfig::finite(4, 4, 3));
        let water = s.content().expect_material("water");
        s.paint(CellPos::new(128, 100), 10, water, PaintMode::Replace, None);
        for _ in 0..2000 {
            s.tick();
        }
        println!("{}", ascii(&s, CellRect::new(0, 244, 256, 256)));
        println!("awake {}", s.stats().awake_chunks);
        for pos in s.world.loaded_chunks().collect::<Vec<_>>() {
            s.world.chunk_mut(pos).unwrap().dirty = chunk::LocalRect::FULL;
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
        let mut s = Simulation::new(content, SimConfig::finite(4, 4, 3));
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

