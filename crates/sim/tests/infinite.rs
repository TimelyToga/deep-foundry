//! Tests for the infinite world: chunk sources, anchors, pausing, packing, dropping, saves and
//! far coordinates.

use foundry_content::Content;
use foundry_core::{CHUNK_AREA, CHUNK_SIZE, CellPos, CellRect, ChunkPos, Command, MaterialId, PaintMode, Rng};
use foundry_sim::chunk::{Chunk, GENERATED_VERSION};
use foundry_sim::{ChunkCells, ChunkSource, ChunkStore, PackedChunk, SimConfig, Simulation};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

fn content() -> Arc<Content> {
    Arc::new(Content::load_default().unwrap())
}

/// Air above y = 1024, stone below with random holes and sand pockets from the seed, bedrock
/// at the bottom. The holes are closed (sand only below stone), so the world is stable.
struct Speckle {
    stone: u16,
    sand: u16,
    bedrock: u16,
    bottom: i32,
}

impl Speckle {
    fn new(c: &Content, bottom: i32) -> Self {
        Self {
            stone: c.expect_material("stone").0,
            sand: c.expect_material("sand").0,
            bedrock: c.expect_material("bedrock").0,
            bottom,
        }
    }
}

impl ChunkSource for Speckle {
    fn generate(&self, cells: &mut ChunkCells) {
        let mut rng = Rng::for_chunk(cells.seed, 0, cells.pos, 77);
        for ly in 0..CHUNK_SIZE {
            let y = cells.top() + ly;
            for lx in 0..CHUNK_SIZE {
                let i = foundry_core::local_index(lx, ly);
                let r = rng.below(100);
                cells.mat[i] = if y >= self.bottom - 2 {
                    self.bedrock
                } else if y < 1024 {
                    0
                } else if r < 3 && y > 1100 {
                    self.sand
                } else {
                    self.stone
                };
            }
        }
        // Some chunks are hot, to test temperatures from the source.
        if cells.pos.x.rem_euclid(7) == 3 && cells.pos.y > 20 {
            cells.temp.fill(300);
        }
    }

    fn name(&self) -> &str {
        "speckle"
    }
}

fn speckle_world(seed: u64) -> Simulation {
    let c = content();
    let source: Arc<dyn ChunkSource> = Arc::new(Speckle::new(&c, 40 * CHUNK_SIZE));
    Simulation::new(c, SimConfig { sky_chunks: 16, depth_chunks: 24, ..SimConfig::infinite(seed, Some(source)) })
}

/// A layer world (air above y = 1024, stone below), 40 chunks tall.
fn layer_world(seed: u64) -> Simulation {
    Simulation::new(content(), SimConfig { sky_chunks: 16, depth_chunks: 24, ..SimConfig::infinite(seed, None) })
}

/// A view of about 20 × 12 chunks with its top-left chunk at (cx, cy).
fn view_at(cx: i32, cy: i32) -> CellRect {
    CellRect::new(cx * CHUNK_SIZE, cy * CHUNK_SIZE, (cx + 20) * CHUNK_SIZE, (cy + 12) * CHUNK_SIZE)
}

/// All cell arrays of a live chunk, as bytes, to compare two chunks exactly.
fn cells_of(sim: &Simulation, c: ChunkPos) -> Vec<u8> {
    let ch = sim.world().chunk(c).unwrap_or_else(|| panic!("chunk {c:?} is live"));
    bytes_of(ch)
}

fn bytes_of(ch: &Chunk) -> Vec<u8> {
    let mut out = Vec::with_capacity(CHUNK_AREA * 8);
    for i in 0..CHUNK_AREA {
        out.extend(ch.mat[i].to_le_bytes());
        out.extend(ch.temp[i].to_le_bytes());
        out.extend([ch.shade[i], ch.life[i], ch.motion[i], ch.flags[i]]);
    }
    out
}

fn tick(s: &mut Simulation, n: u32) {
    for _ in 0..n {
        s.tick();
    }
}

#[test]
fn generation_is_deterministic_and_does_not_depend_on_order() {
    let mut a = speckle_world(5);
    let mut b = speckle_world(5);
    let target = ChunkPos::new(-37, 20);
    // World a makes the target chunk first. World b makes many other chunks first.
    a.apply(Command::SetView { area: view_at(-40, 18) });
    b.apply(Command::SetView { area: view_at(-60, 10) });
    b.take_snapshot();
    b.apply(Command::SetView { area: view_at(-40, 18) });
    a.take_snapshot();
    b.take_snapshot();
    assert_eq!(cells_of(&a, target), cells_of(&b, target));
    let ch = a.world().chunk(target).unwrap();
    assert!(ch.pristine);
    assert_eq!(ch.version, GENERATED_VERSION);
    // The source's temperatures are used; other cells get their material's default.
    let hot = a.world().chunk(ChunkPos::new(-39, 21)).unwrap();
    assert!(hot.temp.iter().all(|&t| t == 300));
    let stone = a.content().expect_material("stone");
    let i = (0..CHUNK_AREA).find(|&i| ch.mat[i] == stone.0).unwrap();
    assert_eq!(ch.temp[i], a.content().materials.temperature[stone.index()]);
    // Another seed gives other cells.
    let mut c = speckle_world(6);
    c.apply(Command::SetView { area: view_at(-40, 18) });
    c.take_snapshot();
    assert_ne!(cells_of(&a, target), cells_of(&c, target));
    // Sky chunks are not stored as cells.
    assert!(b.world().chunk(ChunkPos::new(-50, 12)).is_none());
    assert!(b.memory().air_chunks > 0);
}

#[test]
fn result_does_not_depend_on_thread_count_in_an_infinite_world() {
    let run = |threads: usize| {
        let mut s = speckle_world(9);
        s.set_threads(threads);
        let c = s.content().clone();
        let (sand, water, oil, air) =
            (c.expect_material("sand"), c.expect_material("water"), c.expect_material("oil"), MaterialId::AIR);
        let mut hashes = vec![];
        for t in 0..600 {
            // The view walks to the left over the whole run, so chunks are made, packed and dropped.
            let x = -t * 12;
            s.apply(Command::SetView { area: CellRect::new(x - 600, 700, x + 600, 1400) });
            if t % 40 == 0 {
                s.paint(CellPos::new(x - 300, 800), 20, sand, PaintMode::Replace, None);
                s.paint(CellPos::new(x - 200, 900), 15, water, PaintMode::Replace, None);
                s.paint(CellPos::new(x - 100, 1040), 12, air, PaintMode::Replace, None);
                s.paint(CellPos::new(x - 400, 850), 10, oil, PaintMode::Replace, None);
            }
            s.tick();
            if t % 50 == 0 {
                hashes.push(s.world_hash());
            }
        }
        hashes.push(s.world_hash());
        let m = s.memory();
        assert!(m.packed_chunks > 0, "some chunks were packed: {m:?}");
        (hashes, m.generated_total)
    };
    let one = run(1);
    let six = run(6);
    assert_eq!(one, six);
}

#[test]
fn material_is_kept_and_chunks_sleep_far_to_the_left() {
    let mut s = layer_world(3);
    let c = s.content().clone();
    let (sand, water) = (c.expect_material("sand"), c.expect_material("water"));
    let x0 = -1_000 * CHUNK_SIZE;
    s.apply(Command::SetView { area: CellRect::new(x0, 0, x0 + 1024, 1100) });
    // A box of stone walls on the ground, to keep the water in a small area.
    let stone = c.expect_material("stone");
    for y in 700..1024 {
        for x in [x0 + 100, x0 + 101, x0 + 900, x0 + 901] {
            s.set_cell(CellPos::new(x, y), stone, None);
        }
    }
    s.paint(CellPos::new(x0 + 300, 800), 40, sand, PaintMode::Replace, None);
    s.paint(CellPos::new(x0 + 600, 750), 50, water, PaintMode::Replace, None);
    let area = CellRect::new(x0, 0, x0 + 1024, 1100);
    let before = (s.count_material(area, sand), s.count_material(area, water));
    // Water on sand in a wide box takes about 7000 ticks to come to rest.
    tick(&mut s, 8000);
    assert_eq!((s.count_material(area, sand), s.count_material(area, water)), before);
    assert!(s.stats().awake_chunks <= 2, "awake chunks at rest: {}", s.stats().awake_chunks);
    // Everything is on the ground (stone starts at y = 1024).
    assert_eq!(s.count_material(CellRect::new(x0, 0, x0 + 1024, 900), sand), 0);
}

#[test]
fn far_chunks_pause_outside_anchors_and_continue() {
    let mut s = layer_world(4);
    let c = s.content().clone();
    let water = c.expect_material("water");
    s.apply(Command::SetView { area: view_at(0, 10) });
    // A ball of water in the air, 200 chunks to the right: far outside the view.
    let far = CellPos::new(200 * CHUNK_SIZE + 32, 700);
    s.paint(far, 30, water, PaintMode::Replace, None);
    let area = CellRect::around(far, 3000);
    let total = s.count_material(area, water);
    let hash = s.world_hash();
    tick(&mut s, 200);
    // Nothing moved: the chunks wait with their work.
    assert_eq!(s.world_hash(), hash, "far water did not move");
    assert_eq!(s.cell(far.offset(0, -30)).material, water, "the top of the ball is still there");
    assert!(s.memory().paused_chunks > 0);
    // After some ticks far from the view, the waiting chunks are packed, with their work.
    assert!(s.world().chunk(far.chunk()).is_none(), "the far chunk is packed");
    assert!(s.memory().packed_chunks > 0);
    // The view comes near: the water falls and lands on the ground, and none is lost.
    s.apply(Command::SetView { area: view_at(195, 5) });
    tick(&mut s, 600);
    assert_eq!(s.count_material(area, water), total);
    assert!(s.cell(far.offset(0, -30)).material.is_air(), "the ball fell");
    assert!(s.count_material(CellRect::new(far.x - 3000, 990, far.x + 3000, 1024), water) > total * 9 / 10);
}

#[test]
fn a_flood_stops_at_the_edge_of_the_simulation_area_and_keeps_its_water() {
    let mut s = layer_world(8);
    let c = s.content().clone();
    let water = c.expect_material("water");
    // A small anchor with the default margin of 4 chunks.
    let id = s.add_anchor(CellRect::new(0, 900, 64, 1000));
    s.paint(CellPos::new(32, 800), 60, water, PaintMode::Replace, None);
    let area = CellRect::new(-4000, 0, 4000, 1100);
    let total = s.count_material(area, water);
    tick(&mut s, 1500);
    assert_eq!(s.count_material(area, water), total);
    // No water beyond the simulation area (anchor chunk 0 plus 4 chunks, plus one chunk that
    // the edge chunks can write into).
    let limit = 6 * CHUNK_SIZE;
    assert_eq!(s.count_material(CellRect::new(limit, 0, 4000, 1100), water), 0);
    assert_eq!(s.count_material(CellRect::new(-4000, 0, -limit, 1100), water), 0);
    // Remove the anchor: with no anchors, the whole world updates again, and the flood spreads out.
    s.remove_anchor(id);
    tick(&mut s, 3000);
    assert_eq!(s.count_material(area, water), total);
    assert!(s.count_material(CellRect::new(limit, 0, 4000, 1100), water) > 0);
}

#[test]
fn pristine_chunks_are_dropped_and_come_back_identical() {
    let mut s = speckle_world(12);
    s.apply(Command::SetView { area: view_at(0, 14) });
    s.tick();
    let target = ChunkPos::new(5, 20);
    let before = cells_of(&s, target);
    assert!(s.world().chunk(target).unwrap().pristine);
    // Go far away. The chunks near the start are dropped (not packed: they did not change).
    s.apply(Command::SetView { area: view_at(500, 14) });
    tick(&mut s, 30);
    assert!(s.world().chunk(target).is_none(), "dropped");
    let m = s.memory();
    assert_eq!(m.packed_chunks, 0, "nothing changed, so nothing is packed: {m:?}");
    // Come back: the chunk is made again, with the same cells.
    s.apply(Command::SetView { area: view_at(0, 14) });
    s.tick();
    assert_eq!(cells_of(&s, target), before);
    assert!(s.world().chunk(target).unwrap().pristine);
}

#[test]
fn packed_chunks_unpack_identical() {
    let mut s = speckle_world(13);
    let c = s.content().clone();
    s.apply(Command::SetView { area: view_at(0, 10) });
    // Dig a hole and drop sand into it, then wait until all is still.
    s.paint(CellPos::new(300, 1030), 20, MaterialId::AIR, PaintMode::Replace, None);
    s.paint(CellPos::new(300, 950), 10, c.expect_material("sand"), PaintMode::Replace, None);
    tick(&mut s, 800);
    assert_eq!(s.stats().awake_chunks, 0, "all is still");
    let changed: Vec<ChunkPos> = s.world().changed_positions();
    assert!(!changed.is_empty());
    let before: Vec<Vec<u8>> = changed.iter().map(|p| cells_of(&s, *p)).collect();
    let hash = s.world_hash();
    // Go far away: the changed chunks are packed.
    s.apply(Command::SetView { area: view_at(-800, 10) });
    tick(&mut s, 30);
    for p in &changed {
        assert!(s.world().chunk(*p).is_none(), "{p:?} is packed");
    }
    assert_eq!(s.memory().packed_chunks, changed.len());
    assert_eq!(s.world_hash(), hash, "the hash reads packed chunks too");
    // Come back: the same cells.
    s.apply(Command::SetView { area: view_at(0, 10) });
    s.tick();
    let after: Vec<Vec<u8>> = changed.iter().map(|p| cells_of(&s, *p)).collect();
    assert!(before == after, "unpacked chunks are identical");
}

#[test]
fn save_and_load_of_a_sparse_world_continue_identically() {
    let mut a = speckle_world(21);
    let c = a.content().clone();
    let (sand, water) = (c.expect_material("sand"), c.expect_material("water"));
    // Explore about 300 chunks to the right, and change the world in a few places on the way.
    for t in 0..300 {
        let x = t * CHUNK_SIZE;
        a.apply(Command::SetView { area: CellRect::new(x, 640, x + 1280, 1400) });
        if t % 100 == 0 {
            a.paint(CellPos::new(x + 200, 1040), 25, MaterialId::AIR, PaintMode::Replace, None);
            a.paint(CellPos::new(x + 200, 900), 15, water, PaintMode::Replace, None);
            a.paint(CellPos::new(x + 400, 800), 10, sand, PaintMode::Replace, None);
        }
        a.tick();
    }
    let m = a.memory();
    assert!(m.generated_total > 3000, "{m:?}");
    let mut bytes = vec![];
    a.save(&mut bytes).unwrap();
    let saved = a.world().saved_positions().len();
    assert!(saved < 200, "only changed chunks are saved: {saved}");
    assert!(bytes.len() < 1_500_000, "the save is small: {} bytes", bytes.len());

    let source: Arc<dyn ChunkSource> = Arc::new(Speckle::new(&c, 40 * CHUNK_SIZE));
    let (mut b, report) = Simulation::load_with_source(c.clone(), Some(source), &mut bytes.as_slice()).unwrap();
    assert_eq!(report.chunks, saved);
    assert_eq!(a.world_hash(), b.world_hash());
    // Both continue the same way, also when the view moves back over the changed places.
    for t in 0..400 {
        let x = (300 - t) * CHUNK_SIZE;
        for s in [&mut a, &mut b] {
            s.apply(Command::SetView { area: CellRect::new(x, 640, x + 1280, 1400) });
            if t == 150 {
                s.paint(CellPos::new(x + 300, 900), 12, sand, PaintMode::Replace, None);
            }
            s.tick();
        }
        if t % 50 == 0 {
            assert_eq!(a.world_hash(), b.world_hash(), "tick {t}");
        }
    }
    assert_eq!(a.world_hash(), b.world_hash());
    let p = CellPos::new(100 * CHUNK_SIZE + 200, 1040);
    assert_eq!(a.cell(p), b.cell(p));
}

#[test]
fn coordinates_near_one_million_chunks_work() {
    for cx in [1_000_000, -1_000_000] {
        let mut s = layer_world(31);
        let sand = s.content().expect_material("sand");
        let x = cx * CHUNK_SIZE + 10;
        s.apply(Command::SetView { area: CellRect::new(x - 640, 640, x + 640, 1280) });
        let first = s.take_snapshot();
        assert!(!first.chunks.is_empty());
        assert!(first.chunks.iter().all(|c| (c.pos.x - cx).abs() <= 11), "chunk positions near {cx}");
        s.paint(CellPos::new(x, 800), 8, sand, PaintMode::Replace, None);
        let n = s.count_material(CellRect::around(CellPos::new(x, 900), 300), sand);
        tick(&mut s, 400);
        // The sand fell onto the stone at y = 1024 and none was lost.
        assert_eq!(s.count_material(CellRect::around(CellPos::new(x, 900), 300), sand), n);
        assert_eq!(s.cell(CellPos::new(x, 1023)).material, sand);
        assert!(s.cell(CellPos::new(x, 800)).material.is_air());
        // The snapshot sends the changed chunks with their positions.
        let snap = s.take_snapshot();
        let with_sand = snap.chunks.iter().filter(|c| c.texels.iter().any(|t| t[0] == sand.0)).count();
        assert!(with_sand > 0);
        assert!(snap.chunks.iter().all(|c| (c.pos.x - cx).abs() <= 11));
        assert_eq!(snap.world_cells.0, 0, "no width limit");
    }
}

#[test]
fn a_tick_touches_only_chunks_with_work() {
    let mut s = layer_world(40);
    // Make about 3000 chunks by moving a large view over them.
    for i in 0..10 {
        s.apply(Command::SetView { area: CellRect::new(i * 1280, 0, i * 1280 + 1280, 2560) });
        s.tick();
    }
    let sand = s.content().expect_material("sand");
    s.apply(Command::SetView { area: view_at(0, 10) });
    s.set_cell(CellPos::new(100, 1000), sand, None);
    s.tick();
    assert!(s.stats().awake_chunks <= 2);
    assert!(s.memory().awake_list <= 4, "{:?}", s.memory());
}

/// A store in memory, for the test of the disk store hook.
#[derive(Default, Clone)]
struct TestStore(Arc<Mutex<HashMap<ChunkPos, PackedChunk>>>);

impl ChunkStore for TestStore {
    fn put(&mut self, pos: ChunkPos, chunk: PackedChunk) {
        self.0.lock().unwrap().insert(pos, chunk);
    }
    fn take(&mut self, pos: ChunkPos) -> Option<PackedChunk> {
        self.0.lock().unwrap().remove(&pos)
    }
    fn read(&self, pos: ChunkPos) -> Option<PackedChunk> {
        self.0.lock().unwrap().get(&pos).cloned()
    }
    fn contains(&self, pos: ChunkPos) -> bool {
        self.0.lock().unwrap().contains_key(&pos)
    }
    fn positions(&self) -> Vec<ChunkPos> {
        self.0.lock().unwrap().keys().copied().collect()
    }
}

#[test]
fn packed_chunks_move_to_the_store_and_come_back() {
    let mut s = speckle_world(50);
    let store = TestStore::default();
    s.set_chunk_store(Box::new(store.clone()), 20_000);
    let sand = s.content().expect_material("sand");
    for t in 0..120 {
        let x = t * CHUNK_SIZE;
        s.apply(Command::SetView { area: CellRect::new(x, 700, x + 640, 1200) });
        if t % 20 == 0 {
            s.paint(CellPos::new(x + 300, 1040), 10, MaterialId::AIR, PaintMode::Replace, None);
            s.paint(CellPos::new(x + 300, 1000), 6, sand, PaintMode::Replace, None);
        }
        s.tick();
    }
    tick(&mut s, 20);
    let hash = s.world_hash();
    let m = s.memory();
    assert!(m.stored_chunks > 0, "{m:?}");
    assert!(m.packed_bytes <= 20_000, "{m:?}");
    // Saves include the stored chunks.
    let mut bytes = vec![];
    s.save(&mut bytes).unwrap();
    let source: Arc<dyn ChunkSource> = Arc::new(Speckle::new(s.content(), 40 * CHUNK_SIZE));
    let (b, _) = Simulation::load_with_source(s.content().clone(), Some(source), &mut bytes.as_slice()).unwrap();
    assert_eq!(b.world_hash(), hash);
    // Going back takes the chunks out of the store.
    s.apply(Command::SetView { area: CellRect::new(0, 700, 640, 1200) });
    s.tick();
    assert_eq!(s.world_hash(), hash);
    assert!(s.world().chunk(ChunkPos::new(4, 16)).is_some());
}
