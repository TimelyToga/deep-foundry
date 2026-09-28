//! Tests of the world generator: the same cells every time and in any order, no seams at chunk
//! borders, the start area rule, a stable world at the start, and save and load.

use foundry_content::{Content, Phase};
use foundry_core::{CHUNK_AREA, CHUNK_SIZE, CellPos, CellRect, ChunkPos, Command, MaterialId, local_index};
use foundry_sim::{ChunkCells, ChunkSource, SimConfig, Simulation};
use foundry_worldgen::{Biome, WorldGen, WorldGenSettings};
use std::sync::{Arc, OnceLock};

fn content() -> Arc<Content> {
    static C: OnceLock<Arc<Content>> = OnceLock::new();
    C.get_or_init(|| Arc::new(Content::load_default().expect("data files"))).clone()
}

fn source() -> WorldGen {
    WorldGen::new(&content(), WorldGenSettings::default())
}

/// The cells and temperatures of one chunk.
fn chunk(wg: &WorldGen, seed: u64, pos: ChunkPos) -> (Vec<u16>, Vec<i16>) {
    let mut mat = Box::new([0u16; CHUNK_AREA]);
    let mut temp = Box::new([ChunkCells::MATERIAL_DEFAULT; CHUNK_AREA]);
    let mut cells = ChunkCells { pos, seed, mat: &mut mat, temp: &mut temp, awake: false };
    wg.generate(&mut cells);
    assert!(!cells.awake, "a generated chunk sleeps");
    (mat.to_vec(), temp.to_vec())
}

/// The cells of a rectangle of chunks, as one array (row by row).
struct Area {
    x0: i32,
    y0: i32,
    w: i32,
    h: i32,
    mat: Vec<u16>,
}

impl Area {
    fn new(wg: &WorldGen, seed: u64, cx0: i32, cy0: i32, cw: i32, ch: i32) -> Area {
        let (w, h) = (cw * CHUNK_SIZE, ch * CHUNK_SIZE);
        let mut mat = vec![0; (w * h) as usize];
        for cy in 0..ch {
            for cx in 0..cw {
                let (m, _) = chunk(wg, seed, ChunkPos::new(cx0 + cx, cy0 + cy));
                for ly in 0..CHUNK_SIZE {
                    for lx in 0..CHUNK_SIZE {
                        let (x, y) = (cx * CHUNK_SIZE + lx, cy * CHUNK_SIZE + ly);
                        mat[(y * w + x) as usize] = m[local_index(lx, ly)];
                    }
                }
            }
        }
        Area { x0: cx0 * CHUNK_SIZE, y0: cy0 * CHUNK_SIZE, w, h, mat }
    }

    /// Material at world cell (x, y).
    fn at(&self, x: i32, y: i32) -> u16 {
        self.mat[((y - self.y0) * self.w + (x - self.x0)) as usize]
    }

    fn count(&self, m: MaterialId) -> usize {
        self.mat.iter().filter(|&&v| v == m.0).count()
    }
}

/// Some chunks in the sky, at the surface in each biome, in the upper stone and deep down.
fn sample_positions() -> Vec<ChunkPos> {
    let mut v = vec![];
    for cx in [-60, -31, -12, -1, 0, 1, 9, 30, 55, 400, -900] {
        for cy in [2, 13, 15, 16, 17, 19, 25, 40, 70, 143] {
            v.push(ChunkPos::new(cx, cy));
        }
    }
    v
}

#[test]
fn the_same_chunk_twice_gives_the_same_cells() {
    let wg = source();
    for p in sample_positions() {
        assert_eq!(chunk(&wg, 7, p), chunk(&wg, 7, p), "chunk {p:?}");
    }
    // A new generator with the same settings makes the same cells.
    let other = source();
    for p in sample_positions().into_iter().step_by(7) {
        assert_eq!(chunk(&wg, 7, p), chunk(&other, 7, p));
    }
}

#[test]
fn the_order_of_calls_does_not_change_the_result() {
    let wg = source();
    let list = sample_positions();
    let forward: Vec<_> = list.iter().map(|&p| chunk(&wg, 3, p)).collect();
    let backward: Vec<_> = list.iter().rev().map(|&p| chunk(&wg, 3, p)).collect();
    assert!(forward.iter().eq(backward.iter().rev()));
    // On four threads at once, each taking every fourth chunk.
    let parallel: Vec<Vec<(usize, (Vec<u16>, Vec<i16>))>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..4)
            .map(|t| {
                let (wg, list) = (&wg, &list);
                s.spawn(move || list.iter().enumerate().skip(t).step_by(4).map(|(i, &p)| (i, chunk(wg, 3, p))).collect())
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for (i, cells) in parallel.into_iter().flatten() {
        assert_eq!(cells, forward[i], "chunk {:?}", list[i]);
    }
}

#[test]
fn different_seeds_make_different_worlds() {
    let wg = source();
    let p = ChunkPos::new(3, 16);
    assert_ne!(chunk(&wg, 1, p).0, chunk(&wg, 2, p).0);
}

#[test]
fn sky_is_air_and_the_bottom_is_bedrock() {
    let wg = source();
    let s = *wg.world_settings();
    let (sky, _) = chunk(&wg, 1, ChunkPos::new(5, 1));
    assert!(sky.iter().all(|&v| v == 0));
    let bedrock = content().expect_material("bedrock").0;
    let bottom = ChunkPos::new(5, s.bottom_y / CHUNK_SIZE - 1);
    let (cells, _) = chunk(&wg, 1, bottom);
    for lx in 0..CHUNK_SIZE {
        assert_eq!(cells[local_index(lx, CHUNK_SIZE - 1)], bedrock);
        assert_eq!(cells[local_index(lx, CHUNK_SIZE - 2)], bedrock);
    }
}

/// Every powder cell has support: no air, gas or liquid below it or diagonally below it.
fn check_support(a: &Area, name: &str) {
    let c = content();
    let phase = |v: u16| c.materials.phase[v as usize];
    let open = |v: u16| !matches!(phase(v), Phase::Solid | Phase::Powder);
    let mut powder = 0;
    for y in a.y0..a.y0 + a.h - 1 {
        for x in a.x0 + 1..a.x0 + a.w - 1 {
            let v = a.at(x, y);
            if phase(v) != Phase::Powder {
                continue;
            }
            powder += 1;
            for dx in -1..=1 {
                let b = a.at(x + dx, y + 1);
                assert!(!open(b), "{name}: {} at ({x}, {y}) has {} at dx {dx} below it", c.materials.ids[v as usize], c.materials.ids[b as usize]);
            }
        }
    }
    assert!(powder > 1000, "{name}: only {powder} powder cells");
}

#[test]
fn powder_has_support_across_chunk_borders() {
    let wg = source();
    let s = *wg.world_settings();
    let sy = s.surface_y / CHUNK_SIZE;
    // The start area with the lake and the river, a desert, a tundra, and the upper stone.
    check_support(&Area::new(&wg, 1, -16, sy - 3, 32, 8), "start");
    check_support(&Area::new(&wg, 1, 40, sy - 3, 24, 8), "desert");
    check_support(&Area::new(&wg, 1, -64, sy - 3, 24, 8), "tundra");
    check_support(&Area::new(&wg, 1, -8, sy + 10, 16, 16), "upper stone");
}

#[test]
fn no_seams_at_chunk_borders() {
    let wg = source();
    let seed = 11;
    let s = *wg.world_settings();
    let sy = s.surface_y / CHUNK_SIZE;
    for (name, a) in [
        ("surface", Area::new(&wg, seed, -24, sy - 3, 48, 6)),
        ("upper stone", Area::new(&wg, seed, -10, sy + 12, 20, 16)),
    ] {
        // Neighbor cells differ about as often across a chunk border as inside a chunk.
        let (mut border, mut border_n, mut inside, mut inside_n) = (0usize, 0usize, 0usize, 0usize);
        for y in a.y0..a.y0 + a.h - 1 {
            for x in a.x0..a.x0 + a.w - 1 {
                let v = a.at(x, y);
                let diff = (v != a.at(x + 1, y)) as usize + (v != a.at(x, y + 1)) as usize;
                if (x + 1) % CHUNK_SIZE == 0 || (y + 1) % CHUNK_SIZE == 0 {
                    border += diff;
                    border_n += 2;
                } else {
                    inside += diff;
                    inside_n += 2;
                }
            }
        }
        let (b, i) = (border as f64 / border_n as f64, inside as f64 / inside_n as f64);
        assert!(b < i * 1.3 + 0.002, "{name}: {b:.4} of cell pairs differ across chunk borders, {i:.4} inside");
    }

    // The ground in the cells matches `ground_y` in every column, also at chunk borders.
    let c = content();
    let a = Area::new(&wg, seed, -24, sy - 5, 48, 9);
    // Trees, bushes and boulders stand on the ground.
    let (wood, leaves, stone) = (c.expect_material("wood").0, c.expect_material("leaves").0, c.expect_material("stone").0);
    for x in a.x0..a.x0 + a.w {
        let g = wg.ground_y(seed, x);
        let v = a.at(x, g);
        assert!(v != 0 && c.materials.phase[v as usize] != Phase::Liquid, "column {x}: no ground at row {g}");
        let above = a.at(x, g - 1);
        let top = wg.water_y(seed, x).map_or(g, |w| w);
        assert!(
            above == 0 || above == wood || above == leaves || above == stone || top < g,
            "column {x}: {} above the ground",
            c.materials.ids[above as usize]
        );
    }

    // The lake crosses chunk borders: its water top is one flat row.
    let lake: Vec<i32> = (-900..-400).filter_map(|x| wg.water_y(seed, x)).collect();
    assert!(lake.len() > 100, "the start lake has water");
    assert!(lake.iter().all(|&w| w == lake[0]));
    for x in -900..-400 {
        if let Some(w) = wg.water_y(seed, x) {
            let water = c.expect_material("water").0;
            assert_eq!(a.at(x, w), water, "column {x}");
            assert_eq!(a.at(x, w - 1), 0, "column {x}: air above the water");
        }
    }
}

#[test]
fn the_start_area_has_everything() {
    let c = content();
    let wg = source();
    let s = *wg.world_settings();
    for seed in [1, 2, 99] {
        // 150 tiles = 1200 cells left and right of x = 0.
        let a = Area::new(&wg, seed, -1200 / CHUNK_SIZE, s.surface_y / CHUNK_SIZE - 6, 2400 / CHUNK_SIZE, 12);
        for id in ["wood", "leaves", "clay", "water", "sand", "malachite", "cassiterite", "gravel", "coal", "grass", "dirt"] {
            let n = a.count(c.expect_material(id));
            assert!(n > 40, "seed {seed}: only {n} cells of {id} near the start");
        }
        // A flat place for the Hub at x = 0, on a tile line.
        let (x, g) = wg.start(seed);
        assert_eq!(x, 0);
        assert_eq!(g % 8, 0, "seed {seed}: the flat ground is on a tile line");
        for x in -60..60 {
            assert_eq!(wg.ground_y(seed, x), g, "seed {seed}: ground at x = {x}");
            assert_eq!(a.at(x, g - 1), 0, "seed {seed}: air above the flat place at x = {x}");
        }
        // The cassiterite lies at the bottom of the lake and the river, under the water.
        let tin = c.expect_material("cassiterite").0;
        let under_water = (-1200..1200).any(|x| wg.water_y(seed, x).is_some() && (0..12).any(|d| a.at(x, wg.ground_y(seed, x) + d) == tin));
        assert!(under_water, "seed {seed}: cassiterite in a river or lake bed");
    }
}

#[test]
fn biomes_follow_the_plan() {
    let wg = source();
    assert_eq!(wg.biome_at(1, 0), Biome::Temperate);
    assert_eq!(wg.biome_at(1, 1100), Biome::Temperate);
    assert_eq!(wg.biome_at(1, -1100), Biome::Temperate);
    assert_eq!(wg.biome_at(1, 2600), Biome::Desert);
    assert_eq!(wg.biome_at(1, -2600), Biome::Tundra);
    // Further out, all three biomes come again.
    let mut seen = [false; 3];
    for i in -40..40 {
        seen[wg.biome_at(1, i * 3200) as usize] = true;
    }
    assert_eq!(seen, [true; 3]);
    // The tundra is cold, the air below is warmer than at the surface.
    let s = *wg.world_settings();
    assert!(wg.air_temperature_at(1, -2600, s.surface_y) < 0);
    assert!(wg.air_temperature_at(1, 0, s.surface_y) > 10);
    assert!(wg.air_temperature_rows()[(s.surface_y + 2500) as usize] > 40);
}

/// A generator whose chunks are all awake, so every cell gets a chance to move.
struct Awake(WorldGen);

impl ChunkSource for Awake {
    fn generate(&self, cells: &mut ChunkCells) {
        self.0.generate(cells);
        cells.awake = true;
    }

    fn name(&self) -> &str {
        "worldgen"
    }

    fn settings(&self) -> String {
        ChunkSource::settings(&self.0)
    }
}

#[test]
fn the_world_is_stable_at_the_start() {
    let c = content();
    let seed = 5;
    let config = SimConfig { depth_chunks: 20, ..SimConfig::infinite(seed, None) };
    let wg = WorldGen::new(&c, WorldGenSettings::for_world(config.sky_chunks, config.depth_chunks));
    let s = *wg.world_settings();
    let fresh = WorldGen::new(&c, s);
    let config = SimConfig { source: Some(Arc::new(Awake(wg))), ..config };
    let mut sim = Simulation::new(c.clone(), config);
    // The view of the game at the start, a little wider: the lake, the Hub place and the river.
    let view = CellRect::new(-1000, s.surface_y - 300, 800, s.surface_y + 300);
    sim.apply(Command::SetView { area: view });
    sim.tick();
    let awake = sim.stats().awake_chunks;
    assert!(awake > 100, "only {awake} chunks updated in the first tick");
    for _ in 1..200 {
        sim.tick();
    }
    let area = CellRect::new(view.x0, view.y0, view.x1, view.y1);
    let mut moved = 0;
    let mut solid = 0;
    for cp in area.chunks() {
        let (m, _) = chunk(&fresh, seed, cp);
        for ly in 0..CHUNK_SIZE {
            for lx in 0..CHUNK_SIZE {
                let p = CellPos::new(cp.x * CHUNK_SIZE + lx, cp.y * CHUNK_SIZE + ly);
                let now = sim.cell(p).material.0;
                let was = m[local_index(lx, ly)];
                solid += (was != 0) as usize;
                moved += (now != was) as usize;
            }
        }
    }
    assert!(solid > 100_000, "{solid} cells of material");
    println!("stability: {moved} of {solid} cells changed in 200 ticks ({awake} chunks awake in the first tick)");
    assert!(moved * 2000 < solid, "{moved} of {solid} cells changed in 200 ticks");
}

#[test]
fn a_generated_world_saves_and_loads() {
    let c = content();
    let seed = 8;
    let config = SimConfig { depth_chunks: 16, ..SimConfig::infinite(seed, None) };
    let wg = WorldGen::new(&c, WorldGenSettings::for_world(config.sky_chunks, config.depth_chunks));
    let s = *wg.world_settings();
    let air = wg.air_temperature_rows().to_vec();
    let config = SimConfig { source: Some(Arc::new(wg)), ..config };
    let mut sim = Simulation::new(c.clone(), config);
    sim.set_air_temperature(&air);
    sim.apply(Command::SetView { area: CellRect::new(-300, s.surface_y - 200, 300, s.surface_y + 200) });
    // Dig a hole next to the start, so there is a changed chunk to save.
    for x in 20..40 {
        for y in s.surface_y - 2..s.surface_y + 30 {
            sim.set_cell(CellPos::new(x, y), MaterialId::AIR, None);
        }
    }
    for _ in 0..30 {
        sim.tick();
    }
    let mut bytes = vec![];
    sim.save(&mut bytes).unwrap();
    let resolve = |content: &Content, name: &str, settings: &str| -> Option<Arc<dyn ChunkSource>> {
        (name == "worldgen").then(|| WorldGen::from_settings(content, settings)).flatten().map(|g| Arc::new(g) as Arc<dyn ChunkSource>)
    };
    let (mut loaded, _) = Simulation::load_with_resolver(c.clone(), &resolve, &mut bytes.as_slice()).unwrap();
    assert_eq!(loaded.world().source().name(), "worldgen");
    assert_eq!(loaded.world_hash(), sim.world_hash());
    // Chunks that were not saved are made again with the same cells.
    for p in [CellPos::new(-250, s.surface_y + 10), CellPos::new(900, s.surface_y + 400), CellPos::new(0, s.surface_y - 50)] {
        assert_eq!(loaded.cell(p).material, sim.cell(p).material);
    }
    for _ in 0..10 {
        sim.tick();
        loaded.tick();
    }
    assert_eq!(loaded.world_hash(), sim.world_hash());
    // Another depth is another world: the settings do not match.
    let other = |content: &Content, _: &str, _: &str| -> Option<Arc<dyn ChunkSource>> {
        Some(Arc::new(WorldGen::new(content, WorldGenSettings::for_world(16, 40))))
    };
    assert!(Simulation::load_with_resolver(c, &other, &mut bytes.as_slice()).is_err());
}
