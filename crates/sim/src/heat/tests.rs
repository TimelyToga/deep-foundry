//! Tests of heat flow and phase changes.

use super::*;
use crate::{ChunkCells, ChunkSource, SimConfig, Simulation};
use foundry_content::Content;
use foundry_core::{CellPos, CellRect, Command};
use std::sync::Arc;

fn content() -> Arc<Content> {
    Arc::new(Content::load_default().unwrap())
}

fn finite(w: i32, h: i32, seed: u64) -> Simulation {
    Simulation::new(content(), SimConfig::finite(w, h, seed))
}

fn id(s: &Simulation, name: &str) -> MaterialId {
    s.content().expect_material(name)
}

/// Fill the rectangle x0..x1, y0..y1 with a material.
fn fill(s: &mut Simulation, r: CellRect, m: MaterialId, t: Option<i16>) {
    for y in r.y0..r.y1 {
        for x in r.x0..r.x1 {
            s.set_cell(CellPos::new(x, y), m, t);
        }
    }
}

fn ticks(s: &mut Simulation, n: u32) {
    for _ in 0..n {
        s.tick();
    }
}

/// Mean temperature of the cells of material `m` in `r` (all cells if `m` is `None`).
fn mean_temp(s: &Simulation, r: CellRect, m: Option<MaterialId>) -> f64 {
    let (mut sum, mut n) = (0.0, 0);
    for y in r.y0..r.y1 {
        for x in r.x0..r.x1 {
            let c = s.cell(CellPos::new(x, y));
            if m.is_none_or(|m| m == c.material) {
                sum += c.temperature as f64;
                n += 1;
            }
        }
    }
    if n == 0 { f64::NAN } else { sum / n as f64 }
}

/// Heat energy `Σ C × T` of all cells in `r`.
fn energy(s: &Simulation, r: CellRect) -> f64 {
    let caps = &s.content().materials.heat_capacity;
    let mut e = 0.0;
    for y in r.y0..r.y1 {
        for x in r.x0..r.x1 {
            let c = s.cell(CellPos::new(x, y));
            e += caps[c.material.index()].max(0.01) as f64 * c.temperature as f64;
        }
    }
    e
}

fn all(s: &Simulation) -> CellRect {
    let (w, h) = s.size_cells();
    CellRect::new(0, 0, w, h)
}

#[test]
fn table_limits_the_step() {
    let c = content();
    let t = HeatTable::new(&c.materials);
    for m in c.materials.all() {
        let e = t.mats[m.index()];
        assert!(e.g >= 0.0 && e.g * e.inv_c <= MAX_STEP + 1e-6, "{}: g {} inv_c {}", c.materials.ids[m.index()], e.g, e.inv_c);
    }
    // Metal conducts better than firebrick, and firebrick better than nothing.
    let (cu, fb) = (c.expect_material("copper_block"), c.expect_material("firebrick_block"));
    let coef = |m: MaterialId| t.mats[m.index()].g * t.mats[m.index()].inv_c;
    assert!(coef(cu) > 10.0 * coef(fb) && coef(fb) > 0.0);
    // Phase change points from the data.
    let (ice, water, steam) = (c.expect_material("ice"), c.expect_material("water"), c.expect_material("steam"));
    assert_eq!(t.phase_change(ice, -1), None);
    assert_eq!(t.phase_change(ice, 0), Some(water));
    assert_eq!(t.phase_change(water, 99), None);
    assert_eq!(t.phase_change(water, 100), Some(steam));
    assert_eq!(t.phase_change(steam, 95), None);
    assert_eq!(t.phase_change(steam, 94), Some(water));
}

#[test]
fn metal_bar_warms_along_its_length_and_firebrick_stays_cooler() {
    let mut s = finite(3, 2, 1);
    let (stone, copper, firebrick) = (id(&s, "stone"), id(&s, "copper_block"), id(&s, "firebrick_block"));
    // A hot stone block with two bars going right from it: copper above, firebrick below.
    fill(&mut s, CellRect::new(10, 30, 20, 80), stone, Some(1100));
    fill(&mut s, CellRect::new(20, 36, 150, 42), copper, None);
    fill(&mut s, CellRect::new(20, 64, 150, 70), firebrick, None);
    ticks(&mut s, 600);
    let at = |s: &Simulation, x: i32, y: i32| mean_temp(s, CellRect::new(x, y, x + 4, y + 6), None);
    let cu: Vec<f64> = (0..6).map(|k| at(&s, 20 + k * 5, 36)).collect();
    let fb: Vec<f64> = (0..6).map(|k| at(&s, 20 + k * 5, 64)).collect();
    println!("every 5 cells from the block: copper {cu:.0?}, firebrick {fb:.0?}");
    assert!(cu.windows(2).all(|w| w[0] > w[1]), "copper is hotter near the block");
    assert!(cu[4] > 50.0, "copper warms 20 cells from the block: {}", cu[4]);
    assert!((0..6).all(|k| fb[k] < cu[k]), "firebrick stays cooler");
    assert!(fb[0] > 25.0 && fb[2] < 25.0, "firebrick warms only near the block");
}

#[test]
fn ice_next_to_lava_melts_into_water() {
    let mut s = finite(2, 2, 2);
    let (lava, ice, water, steam) = (id(&s, "lava"), id(&s, "ice"), id(&s, "water"), id(&s, "steam"));
    fill(&mut s, CellRect::new(2, 110, 126, 126), lava, None);
    fill(&mut s, CellRect::new(40, 90, 60, 110), ice, Some(-10));
    let ice_before = s.count_material(all(&s), ice);
    ticks(&mut s, 20);
    let (ice_after, water_n, steam_n) = (s.count_material(all(&s), ice), s.count_material(all(&s), water), s.count_material(all(&s), steam));
    println!("ice {ice_before} -> {ice_after}, water {water_n}, steam {steam_n}");
    assert!(ice_after < ice_before, "some ice melts");
    assert!(water_n > 0, "the ice becomes water");
}

#[test]
fn water_next_to_lava_boils_into_steam() {
    let mut s = finite(2, 2, 3);
    let (lava, water, steam) = (id(&s, "lava"), id(&s, "water"), id(&s, "steam"));
    fill(&mut s, CellRect::new(2, 110, 126, 126), lava, None);
    fill(&mut s, CellRect::new(2, 100, 126, 110), water, None);
    ticks(&mut s, 30);
    let n = s.count_material(all(&s), steam);
    println!("steam {n}");
    assert!(n > 20, "water on lava boils: {n} steam cells");
}

#[test]
fn steam_in_a_cold_box_condenses_into_water() {
    let mut s = finite(2, 2, 4);
    let (stone, steam, water) = (id(&s, "stone"), id(&s, "steam"), id(&s, "water"));
    // A closed stone box at 5 °C, 4 cells thick, with steam inside.
    fill(&mut s, CellRect::new(20, 20, 108, 108), stone, Some(5));
    fill(&mut s, CellRect::new(24, 24, 104, 104), steam, None);
    let before = s.count_material(all(&s), steam);
    ticks(&mut s, 900);
    let (after, w) = (s.count_material(all(&s), steam), s.count_material(all(&s), water));
    println!("steam {before} -> {after}, water {w}");
    assert!(w > before / 4 && after < before * 3 / 4, "much of the steam condenses");
}

#[test]
fn lava_cools_into_stone() {
    let mut s = finite(2, 2, 5);
    let (lava, stone) = (id(&s, "lava"), id(&s, "stone"));
    fill(&mut s, CellRect::new(2, 100, 126, 126), stone, None);
    fill(&mut s, CellRect::new(44, 80, 84, 100), lava, None);
    let (lava_before, stone_before) = (s.count_material(all(&s), lava), s.count_material(all(&s), stone));
    ticks(&mut s, 3000);
    let (lava_after, stone_after) = (s.count_material(all(&s), lava), s.count_material(all(&s), stone));
    println!("lava {lava_before} -> {lava_after}, stone {stone_before} -> {stone_after}");
    assert!(lava_after < lava_before / 2, "most lava freezes");
    assert_eq!(stone_after + lava_after, stone_before + lava_before, "lava becomes stone");
}

#[test]
fn heat_energy_is_kept_in_a_closed_box() {
    // A world full of material (no air, which moves toward the air temperature): the edges of the
    // world do not take or give heat. No phase changes happen at these temperatures.
    let mut s = finite(3, 2, 6);
    let r = all(&s);
    let (stone, copper, firebrick, water) = (id(&s, "stone"), id(&s, "copper_block"), id(&s, "firebrick_block"), id(&s, "water"));
    fill(&mut s, CellRect::new(2, 0, r.x1 - 2, r.y1 - 2), stone, Some(20));
    fill(&mut s, CellRect::new(20, 20, 60, 60), copper, Some(600));
    fill(&mut s, CellRect::new(60, 20, 70, 100), firebrick, Some(300));
    fill(&mut s, CellRect::new(100, 30, 150, 80), water, Some(80));
    fill(&mut s, CellRect::new(62, 90, 130, 94), copper, Some(20));
    let before = energy(&s, r);
    let spread = |s: &Simulation| mean_temp(s, CellRect::new(20, 20, 60, 60), None);
    ticks(&mut s, 2000);
    let after = energy(&s, r);
    println!("energy {before:.0} -> {after:.0} ({:+.3}%), copper block {:.0}", (after - before) / before * 100.0, spread(&s));
    assert!(spread(&s) < 300.0, "heat spreads out");
    assert!(((after - before) / before).abs() < 0.003, "energy {before} -> {after}");
    assert_eq!(s.count_material(r, water), 50 * 50, "no phase change");
}

#[test]
fn chunks_go_to_heat_sleep_when_temperatures_are_equal() {
    let mut s = finite(2, 1, 7);
    let r = all(&s);
    let (bedrock, copper) = (id(&s, "bedrock"), id(&s, "copper_block"));
    // Everything at 100 °C: the bedrock border too.
    for y in r.y0..r.y1 {
        for x in r.x0..r.x1 {
            let edge = x < 2 || x >= r.x1 - 2 || y >= r.y1 - 2;
            s.set_cell(CellPos::new(x, y), if edge { bedrock } else { copper }, Some(100));
        }
    }
    ticks(&mut s, 3);
    assert_eq!(s.stats().awake_chunks, 0, "equal temperatures: no work");

    // A hot spot wakes the chunk. It spreads out, and then the chunks sleep again.
    fill(&mut s, CellRect::new(60, 28, 68, 36), copper, Some(180));
    s.tick();
    assert!(s.stats().awake_chunks > 0);
    let mut slept = None;
    for t in 0..4000 {
        s.tick();
        if s.stats().awake_chunks == 0 {
            slept = Some(t);
            break;
        }
    }
    let spot = mean_temp(&s, CellRect::new(60, 28, 68, 36), None);
    println!("asleep after {slept:?} ticks, hot spot {spot:.1}");
    assert!(slept.is_some(), "the chunks go to heat-sleep");
    assert!(spot < 130.0, "the hot spot spread out: {spot}");
    // At rest, no two neighbors differ by `MIN_DIFF` or more.
    for y in 1..r.y1 - 1 {
        for x in 1..r.x1 - 1 {
            let t = |dx, dy| s.cell(CellPos::new(x + dx, y + dy)).temperature;
            assert!(!differs(t(0, 0), t(1, 0)) && !differs(t(0, 0), t(0, 1)), "{x},{y}");
        }
    }
    let h = s.world_hash();
    ticks(&mut s, 50);
    assert_eq!(h, s.world_hash(), "nothing changes while asleep");
}

/// A world with many heat effects at once, for the thread test.
fn heat_mix(threads: usize) -> Simulation {
    let mut s = finite(8, 4, 11);
    s.set_threads(threads);
    let (stone, copper, lava, water, ice, steam, firebrick) = (
        id(&s, "stone"),
        id(&s, "copper_block"),
        id(&s, "lava"),
        id(&s, "water"),
        id(&s, "ice"),
        id(&s, "steam"),
        id(&s, "firebrick_block"),
    );
    fill(&mut s, CellRect::new(2, 200, 510, 254), stone, None);
    fill(&mut s, CellRect::new(40, 150, 200, 200), lava, None);
    fill(&mut s, CellRect::new(60, 100, 180, 150), water, None);
    fill(&mut s, CellRect::new(250, 120, 300, 200), ice, Some(-20));
    fill(&mut s, CellRect::new(300, 150, 360, 200), lava, None);
    fill(&mut s, CellRect::new(380, 60, 440, 120), steam, Some(130));
    fill(&mut s, CellRect::new(60, 60, 500, 64), copper, Some(900));
    fill(&mut s, CellRect::new(450, 130, 470, 200), firebrick, Some(700));
    s
}

#[test]
fn result_does_not_depend_on_thread_count() {
    let mut a = heat_mix(1);
    let mut b = heat_mix(6);
    for t in 0..400 {
        a.tick();
        b.tick();
        if t % 50 == 0 {
            assert_eq!(a.world_hash(), b.world_hash(), "tick {t}");
        }
    }
    assert_eq!(a.world_hash(), b.world_hash());
    // The world did heat things: some water boiled and some ice melted.
    let r = all(&a);
    assert!(a.count_material(r, id(&a, "ice")) < 50 * 80);
}

/// All air, and every chunk works at once (so the heat pass sees all of them).
struct AwakeAir {
    temperature: Option<i16>,
}

impl ChunkSource for AwakeAir {
    fn generate(&self, cells: &mut ChunkCells) {
        cells.awake = true;
        if let Some(t) = self.temperature {
            cells.temp.fill(t);
        }
    }

    fn name(&self) -> &str {
        "awake_air"
    }
}

fn awake_air_world(source_temp: Option<i16>, air: i16) -> Simulation {
    let mut s = Simulation::new(content(), SimConfig::infinite(3, Some(Arc::new(AwakeAir { temperature: source_temp }))));
    s.set_air_temperature(&[air]);
    s.apply(Command::SetView { area: CellRect::new(-200, 1000, 200, 1200) });
    s
}

#[test]
fn chunks_at_rest_at_their_source_temperatures_stay_pristine() {
    // Air at 20 °C and an air temperature of 20 °C: in balance.
    let mut s = awake_air_world(None, 20);
    ticks(&mut s, 40);
    assert!(s.world().live_count() > 20);
    assert!(s.world().changed_positions().is_empty(), "no chunk is written");
    // Warm air from the source, with the same air temperature: also in balance.
    let mut s = awake_air_world(Some(35), 35);
    ticks(&mut s, 40);
    assert!(s.world().changed_positions().is_empty(), "no chunk is written");
    // Not in balance: the air moves toward the air temperature, so the chunks change.
    let mut s = awake_air_world(Some(35), 20);
    ticks(&mut s, 40);
    assert!(!s.world().changed_positions().is_empty(), "the test sees writes");
}

#[test]
fn air_moves_toward_the_air_temperature() {
    let mut s = finite(1, 1, 8);
    s.set_air_temperature(&[60]);
    // A write wakes the chunk; then the air follows the air temperature.
    s.set_cell(CellPos::new(30, 30), id(&s, "stone"), Some(60));
    ticks(&mut s, 1500);
    let air = mean_temp(&s, CellRect::new(4, 4, 60, 60), Some(MaterialId::AIR));
    println!("air {air:.1}");
    assert!(air > 55.0, "air warms toward 60: {air}");
}

#[test]
fn a_hot_chunk_outside_the_update_area_waits() {
    let mut s = Simulation::new(content(), SimConfig::infinite(5, None));
    let copper = id(&s, "copper_block");
    let view = CellRect::new(-300, 700, 300, 1000);
    s.apply(Command::SetView { area: view });
    // A hot copper block in the sky, cooling in the air.
    fill(&mut s, CellRect::new(0, 800, 20, 820), copper, Some(900));
    ticks(&mut s, 20);
    let p = CellPos::new(0, 800);
    assert!(s.cell(p).temperature < 900, "the corner cools");
    // The view goes far away: the block stops cooling, and waits.
    s.apply(Command::SetView { area: CellRect::new(100_000, 700, 100_600, 1000) });
    ticks(&mut s, 30);
    let waiting = s.cell(p).temperature;
    ticks(&mut s, 100);
    assert_eq!(s.cell(p).temperature, waiting, "no heat work far away");
    assert!(s.memory().paused_chunks > 0, "the hot chunk waits in the paused set");
    // The view comes back: it continues.
    s.apply(Command::SetView { area: view });
    ticks(&mut s, 30);
    assert!(s.cell(p).temperature < waiting, "cooling continues");
}

#[test]
fn glowing_cells_get_new_versions_but_not_in_every_tick() {
    let mut s = finite(1, 1, 9);
    let (copper, firebrick) = (id(&s, "copper_block"), id(&s, "firebrick_block"));
    fill(&mut s, CellRect::new(10, 10, 54, 54), firebrick, None);
    fill(&mut s, CellRect::new(20, 20, 44, 44), copper, Some(900));
    s.tick();
    let pos = ChunkPos::new(0, 0);
    let temps = |s: &Simulation| s.world().chunk(pos).unwrap().temp;
    let mut seen = temps(&s);
    let mut version = s.world().chunk(pos).unwrap().version;
    let (mut new_versions, mut changed_ticks) = (0, 0);
    for _ in 0..300 {
        let before = temps(&s);
        s.tick();
        let now = temps(&s);
        changed_ticks += (before != now) as u32;
        let v = s.world().chunk(pos).unwrap().version;
        if v != version {
            version = v;
            new_versions += 1;
            seen = now;
            continue;
        }
        // The renderer has `seen`: glowing cells are at most `GLOW_STEP` away from it.
        for i in 0..CHUNK_AREA {
            if seen[i] >= 450 && now[i] >= 450 {
                assert!((seen[i] - now[i]).abs() < GLOW_STEP as i16, "cell {i}: seen {} now {}", seen[i], now[i]);
            }
        }
    }
    println!("{new_versions} new versions in {changed_ticks} ticks with changes");
    assert!(new_versions > 5, "the renderer sees the cooling");
    assert!(new_versions < changed_ticks, "not a new version for every small change");

    // Warm but not glowing: temperatures change, the version does not.
    let mut s = finite(1, 1, 9);
    fill(&mut s, CellRect::new(20, 20, 44, 44), copper, Some(200));
    s.tick();
    let v = s.world().chunk(pos).unwrap().version;
    let t = temps(&s);
    ticks(&mut s, 20);
    assert_ne!(temps(&s), t);
    assert_eq!(s.world().chunk(pos).unwrap().version, v);
}
