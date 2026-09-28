//! Explosion tests: scenes made in code. Each scene checks that no material is lost (cells plus
//! particles) and checks the result.
//!
//! `cargo test -p foundry_sim --release explosion_frames -- --ignored --nocapture` writes pictures
//! of each scene to `out/explosions/` (particles are light dots).

use super::*;
use crate::{Simulation, SimConfig};
use foundry_content::Content;
use foundry_core::{CHUNK_SIZE, Command};
use std::sync::Arc;

fn content() -> Arc<Content> {
    Arc::new(Content::load_default().unwrap())
}

/// A finite box world of `w` × `h` chunks.
fn world(w: i32, h: i32) -> Simulation {
    Simulation::new(content(), SimConfig::finite(w, h, 7))
}

fn mat(s: &Simulation, name: &str) -> MaterialId {
    s.content().expect_material(name)
}

/// Fill cells with x in `x0..x1` and y in `y0..y1`.
fn fill(s: &mut Simulation, x0: i32, y0: i32, x1: i32, y1: i32, name: &str) {
    let m = mat(s, name);
    for y in y0..y1 {
        for x in x0..x1 {
            s.set_cell(CellPos::new(x, y), m, None);
        }
    }
}

/// Cells of a material in the whole finite world, plus material particles.
fn total(s: &Simulation, m: MaterialId) -> usize {
    let (w, h) = s.size_cells();
    s.count_material(CellRect::new(0, 0, w, h), m) + s.particles().count_material(m)
}

/// Tick until there are no particles and no awake chunks (or `max` ticks). Checks every tick that
/// the total of each material in `kept` does not change. Returns the ticks it took.
fn run_to_rest(s: &mut Simulation, kept: &[MaterialId], max: u32) -> u32 {
    let start: Vec<usize> = kept.iter().map(|&m| total(s, m)).collect();
    for t in 1..=max {
        s.tick();
        let now: Vec<usize> = kept.iter().map(|&m| total(s, m)).collect();
        assert_eq!(now, start, "material is kept (tick {t})");
        if s.particles().is_empty() && s.stats().awake_chunks == 0 {
            return t;
        }
    }
    max
}

/// A sand pit: sand from y = 140 to the floor, in a 4 × 4 chunk box.
fn sand_pit() -> Simulation {
    let mut s = world(4, 4);
    fill(&mut s, 2, 140, 254, 254, "sand");
    s
}

#[test]
fn radius_and_speed_grow_with_strength() {
    assert!(radius(10.0) < radius(30.0) && radius(30.0) < radius(100.0));
    assert!(radius(1e9) <= MAX_RADIUS);
    assert!(radius(-5.0) >= 1.0);
    assert!(throw_speed(10.0) < throw_speed(100.0));
}

#[test]
fn an_explosion_in_sand_throws_sand_that_lands_again() {
    let mut s = sand_pit();
    let sand = mat(&s, "sand");
    let before = total(&s, sand);
    let center = CellPos::new(128, 150);
    // No heat: hot sand takes about 40,000 ticks to cool to rest (slow heat flow in a slope of
    // 1-2 °C per cell), and this test checks that the thrown cells land and rest.
    assert!(s.explode(center, 40.0, 0));
    // The center and the cells around it flew away.
    assert!(s.cell(center).material != sand);
    let thrown = s.particles().count_material(sand);
    assert!(thrown > 100, "cells were thrown: {thrown}");
    assert_eq!(total(&s, sand), before, "thrown cells are particles");
    assert!(s.particles().visual_count() > 0, "sparks, smoke and dust");
    assert!(s.events().iter().any(|e| matches!(e, SimEvent::Exploded { at, .. } if *at == center)));
    // Some sand flies above the old surface.
    let mut highest = f32::MAX;
    for _ in 0..10 {
        s.tick();
        let mut views = vec![];
        s.particles().views(CellRect::new(0, 0, 256, 256), &mut views);
        for v in views.iter().filter(|v| v.material == sand.0) {
            highest = highest.min(v.y);
        }
    }
    assert!(highest < 130.0, "sand flies up: highest {highest}");
    let ticks = run_to_rest(&mut s, &[sand], 3000);
    assert!(ticks < 3000, "everything rests");
    assert_eq!(total(&s, sand), before);
}

#[test]
fn an_explosion_does_not_break_bedrock() {
    let mut s = world(3, 3);
    let bedrock = mat(&s, "bedrock");
    let all = CellRect::new(0, 0, 192, 192);
    fill(&mut s, 40, 40, 150, 150, "bedrock");
    let before = s.count_material(all, bedrock);
    let hash = s.world_hash();
    assert!(s.explode(CellPos::new(95, 95), 255.0, 2000));
    assert!(s.explode(CellPos::new(95, 95), 10_000.0, 0));
    assert_eq!(s.count_material(all, bedrock), before);
    assert_eq!(s.world_hash(), hash, "no cell changed");
    assert_eq!(s.particles().count_material(bedrock), 0);
}

#[test]
fn a_wall_that_does_not_break_shields_the_cells_behind_it() {
    let mut s = world(3, 3);
    let sand = mat(&s, "sand");
    // Sand from x = 20 to 170. A bedrock wall at x = 100..103 cuts it.
    fill(&mut s, 20, 100, 170, 180, "sand");
    fill(&mut s, 100, 60, 103, 190, "bedrock");
    let right = CellRect::new(103, 0, 192, 190);
    let before = s.count_material(right, sand);
    assert!(s.explode(CellPos::new(96, 120), 60.0, 0));
    assert_eq!(s.count_material(right, sand), before, "no sand behind the wall was thrown");
    assert!(s.count_material(CellRect::new(20, 100, 100, 180), sand) < 80 * 80, "sand before the wall was thrown");
}

#[test]
fn stone_breaks_into_gravel_only_near_a_strong_explosion() {
    let mut s = world(3, 3);
    let (stone, gravel) = (mat(&s, "stone"), mat(&s, "gravel"));
    fill(&mut s, 20, 60, 170, 180, "stone");
    let stone_before = total(&s, stone);
    // Weak: stone (hardness 40) does not break.
    assert!(s.explode(CellPos::new(95, 120), 30.0, 0));
    assert_eq!(total(&s, stone), stone_before);
    assert_eq!(total(&s, gravel), 0);
    // Strong: a hole, and the stone flies as gravel.
    assert!(s.explode(CellPos::new(95, 120), 150.0, 0));
    let broken = stone_before - total(&s, stone);
    assert!(broken > 20, "some stone broke: {broken}");
    assert_eq!(total(&s, gravel), broken, "each broken stone cell is one gravel cell");
    let r = radius(150.0);
    assert!((broken as f32) < 3.2 * r * r / 2.0, "only stone near the center broke: {broken}");
    run_to_rest(&mut s, &[stone, gravel], 3000);
}

#[test]
fn heat_makes_fire_and_hot_air_near_the_center() {
    let mut s = world(3, 3);
    let fire = mat(&s, "fire");
    let center = CellPos::new(96, 96);
    assert!(s.explode(center, 60.0, 1500));
    let area = CellRect::around(center, radius(60.0) as i32);
    assert!(s.count_material(area, fire) > 0, "some fire");
    assert!(s.cell(center.offset(1, 1)).temperature > 500 || s.cell(center.offset(1, 1)).material == fire);
    // Far from the center, nothing changed.
    assert_eq!(s.cell(CellPos::new(10, 10)).temperature, foundry_core::DEFAULT_TEMPERATURE);
    // No heat: no fire, and no visual sparks.
    let mut s = world(3, 3);
    assert!(s.explode(center, 60.0, 0));
    assert_eq!(s.count_material(area, fire), 0);
    assert_eq!(s.world_hash(), world(3, 3).world_hash(), "a blast in air with no heat changes no cell");
}

#[test]
fn flammable_cells_near_the_center_burn() {
    let mut s = world(3, 3);
    let (wood, fire) = (mat(&s, "wood"), mat(&s, "fire"));
    fill(&mut s, 60, 60, 130, 130, "wood");
    let before = total(&s, wood);
    assert!(s.explode(CellPos::new(95, 95), 60.0, 1200));
    let area = CellRect::new(0, 0, 192, 192);
    assert!(s.count_material(area, fire) > 5, "wood near the center burns");
    assert!(total(&s, wood) < before);
}

#[test]
fn same_result_with_1_and_6_threads() {
    let make = |threads: usize| {
        let mut s = world(8, 6);
        s.set_threads(threads);
        fill(&mut s, 2, 250, 510, 382, "sand");
        fill(&mut s, 100, 200, 200, 250, "water");
        fill(&mut s, 300, 220, 420, 250, "stone");
        s
    };
    let (mut a, mut b) = (make(1), make(6));
    for t in 0..400 {
        if t % 60 == 0 {
            let at = CellPos::new(60 + t * 3 / 2, 240);
            assert_eq!(a.explode(at, 70.0, 900), b.explode(at, 70.0, 900));
        }
        a.tick();
        b.tick();
        if t % 20 == 0 {
            assert_eq!(a.world_hash(), b.world_hash(), "tick {t}");
            assert_eq!(a.particles().len(), b.particles().len(), "tick {t}");
        }
    }
    assert_eq!(a.world_hash(), b.world_hash());
}

#[test]
fn explosion_events_run_in_the_next_tick_and_work_is_limited() {
    let mut s = sand_pit();
    let sand = mat(&s, "sand");
    let before = total(&s, sand);
    // Many large explosions at once: more work than one tick allows.
    let big = Blast { at: CellPos::new(128, 180), strength: 200.0, heat: 0 };
    let n = WORK_PER_TICK / big.work() * 3 + 3;
    for i in 0..n {
        s.events.push(SimEvent::Explosion { at: CellPos::new(40 + (i as i32 * 17) % 176, 180), strength: 200.0, heat: 0 });
    }
    s.tick();
    let ran = s.events().iter().filter(|e| matches!(e, SimEvent::Exploded { .. })).count();
    assert!(ran >= 1 && ran < n, "some ran in this tick: {ran} of {n}");
    assert_eq!(s.queued_explosions(), n - ran);
    let mut ticks = 1;
    while s.queued_explosions() > 0 {
        s.tick();
        ticks += 1;
        assert!(ticks < 20);
    }
    assert!(ticks >= 3, "the work was spread over {ticks} ticks");
    run_to_rest(&mut s, &[sand], 4000);
    assert_eq!(total(&s, sand), before);
}

/// An infinite world: air above y = 1024, stone below (the default `LayerSource`).
fn layer_world() -> Simulation {
    Simulation::new(content(), SimConfig { sky_chunks: 16, depth_chunks: 24, ..SimConfig::infinite(5, None) })
}

#[test]
fn an_explosion_outside_the_update_area_waits() {
    let mut s = layer_world();
    let stone = mat(&s, "stone");
    // The view is around chunk 0; the update area reaches 4 chunks farther.
    s.apply(Command::SetView { area: CellRect::new(0, 900, 64, 1100) });
    s.tick();
    let far = CellPos::new(40 * CHUNK_SIZE + 32, 1030);
    let area = CellRect::around(far, 40);
    let before = s.count_material(area, stone);
    assert!(!s.explode(far, 150.0, 800));
    assert_eq!(s.queued_explosions(), 1);
    for _ in 0..5 {
        s.tick();
    }
    assert_eq!(s.queued_explosions(), 1, "it still waits");
    assert_eq!(s.count_material(area, stone), before, "nothing changed there");
    assert!(s.world().chunk(far.chunk()).is_none_or(|c| c.pristine), "the chunk was not written");
    // The view comes near: the explosion runs.
    s.apply(Command::SetView { area: CellRect::new(far.x - 200, 900, far.x + 200, 1100) });
    s.tick();
    assert_eq!(s.queued_explosions(), 0);
    assert!(s.count_material(area, stone) < before, "stone broke");
}

#[test]
fn an_explosion_at_the_edge_of_the_update_area_is_clipped() {
    let mut s = layer_world();
    s.settings_mut().sim_margin_chunks = 1;
    let stone = mat(&s, "stone");
    // View: chunk column 0. Update area: columns -1 to 1.
    s.apply(Command::SetView { area: CellRect::new(0, 960, 64, 1100) });
    s.tick();
    let edge = CellPos::new(2 * CHUNK_SIZE - 4, 1030);
    let outside = CellRect::new(2 * CHUNK_SIZE, 900, 3 * CHUNK_SIZE, 1100);
    let before = s.count_material(outside, stone);
    assert!(s.explode(edge, 200.0, 800));
    assert!(s.count_material(CellRect::new(edge.x - 20, 1024, edge.x, 1060), stone) < 20 * 36, "stone inside broke");
    assert_eq!(s.count_material(outside, stone), before, "no stone outside the update area broke");
    for c in outside.chunks() {
        assert!(s.world().chunk(c).is_none_or(|ch| ch.pristine), "chunk {c:?} was not written by the explosion");
    }
    // Every thrown cell lands in the update area (or next to its edge), none is lost.
    for _ in 0..600 {
        s.tick();
    }
    assert!(s.particles().is_empty());
}

#[test]
fn building_cells_do_not_change() {
    let mut s = world(3, 3);
    fill(&mut s, 80, 80, 110, 110, "stone");
    for y in 80..110 {
        for x in 80..110 {
            let c = s.world.chunk_mut(CellPos::new(x, y).chunk()).unwrap();
            c.flags[CellPos::new(x, y).local_index()] |= FLAG_BUILDING;
        }
    }
    let hash = s.world_hash();
    assert!(s.explode(CellPos::new(95, 95), 250.0, 0));
    assert_eq!(s.world_hash(), hash);
}

/// Time of explosions in a world of stone, sand and water. Run in release mode:
/// `cargo test -p foundry_sim --release explosion_speed -- --ignored --nocapture`.
#[test]
#[ignore]
fn explosion_speed() {
    let make = || {
        let mut s = world(8, 8);
        fill(&mut s, 2, 200, 510, 510, "stone");
        fill(&mut s, 2, 150, 510, 200, "sand");
        fill(&mut s, 100, 120, 400, 150, "water");
        s
    };
    for strength in [10.0, 40.0, 150.0, 255.0, 1000.0] {
        let mut s = make();
        let start = std::time::Instant::now();
        s.explode(CellPos::new(256, 190), strength, 1200);
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        println!("strength {strength}: radius {:.1}, {ms:.3} ms, {} particles", radius(strength), s.particles().len());
    }
    // A chain: 300 small explosions in one tick's events (as a burning gas pocket sends them).
    let mut s = make();
    s.tick();
    for i in 0..300 {
        s.events.push(SimEvent::Explosion { at: CellPos::new(60 + i * 13 % 400, 140 + i % 20), strength: 12.0, heat: 900 });
    }
    let mut ticks = vec![];
    while ticks.is_empty() || s.queued_explosions() > 0 {
        let start = std::time::Instant::now();
        s.tick();
        let explode_ms = s.stats().sections.iter().find(|x| x.0 == "explosions").map_or(0.0, |x| x.1);
        ticks.push((start.elapsed().as_secs_f64() * 1000.0, explode_ms));
    }
    println!("chain of 300 (strength 12): {} ticks, (tick ms, explosion ms) {ticks:.2?}", ticks.len());
}

/// Pictures of the scenes: `out/explosions/<scene>_<tick>.png`.
#[test]
#[ignore]
fn explosion_frames() {
    let dir = format!("{}/../../out/explosions", env!("CARGO_MANIFEST_DIR"));
    let frames = [0u64, 2, 6, 12, 25, 50, 100, 400];
    // 1. Sand pit with a hot explosion.
    let mut s = sand_pit();
    fill(&mut s, 60, 120, 64, 140, "stone");
    s.explode(CellPos::new(128, 150), 40.0, 800);
    shoot(&mut s, &frames, &format!("{dir}/sand"), CellRect::new(0, 0, 256, 256), 2);
    // 1b. The same pit, a stronger explosion just below the surface.
    let mut s = sand_pit();
    s.explode(CellPos::new(128, 143), 80.0, 800);
    shoot(&mut s, &frames, &format!("{dir}/sand_strong"), CellRect::new(0, 0, 256, 256), 2);
    // 2. A cave in stone with dirt and water, a strong explosion.
    let mut s = world(4, 4);
    fill(&mut s, 2, 100, 254, 254, "stone");
    fill(&mut s, 60, 120, 200, 180, "air");
    fill(&mut s, 60, 165, 200, 180, "water");
    fill(&mut s, 60, 150, 110, 165, "dirt");
    fill(&mut s, 150, 150, 170, 165, "wood");
    s.explode(CellPos::new(112, 166), 150.0, 1500);
    shoot(&mut s, &frames, &format!("{dir}/cave"), CellRect::new(0, 0, 256, 256), 2);
    // 3. Bedrock wall shields sand.
    let mut s = world(3, 3);
    fill(&mut s, 20, 100, 170, 190, "sand");
    fill(&mut s, 100, 60, 103, 190, "bedrock");
    s.explode(CellPos::new(96, 120), 60.0, 600);
    shoot(&mut s, &frames, &format!("{dir}/wall"), CellRect::new(0, 0, 192, 192), 2);
}

fn shoot(s: &mut Simulation, frames: &[u64], prefix: &str, area: CellRect, scale: u32) {
    // One contact sheet with all frames, 4 in a row, and one picture of the last frame.
    let last = *frames.last().unwrap();
    let (w, h) = (area.width() as u32 * scale, area.height() as u32 * scale);
    let rows = frames.len().div_ceil(4) as u32;
    let mut sheet = image::RgbImage::from_pixel((w + 4) * 4, (h + 4) * rows, image::Rgb([90, 90, 90]));
    for t in 0..=last {
        if let Some(k) = frames.iter().position(|&f| f == t) {
            let img = crate::render(s, area, scale);
            image::imageops::replace(&mut sheet, &img, ((k % 4) as u32 * (w + 4)) as i64, ((k / 4) as u32 * (h + 4)) as i64);
        }
        s.tick();
    }
    crate::dump_png(s, area, scale, &format!("{prefix}_last.png"));
    sheet.save(format!("{prefix}_sheet.png")).unwrap();
}

#[test]
fn the_explode_command_runs_an_explosion() {
    let mut s = sand_pit();
    let sand = mat(&s, "sand");
    let center = CellPos::new(128, 150);
    s.apply(Command::Explode { center, strength: 40.0, heat: 0 });
    assert!(s.cell(center).material != sand, "the center flew away");
    assert!(s.particles().count_material(sand) > 100, "cells were thrown");
    assert!(s.events().iter().any(|e| matches!(e, SimEvent::Exploded { at, .. } if *at == center)));
}
