use super::*;
use crate::{SimConfig, Simulation};
use foundry_content::Content;
use foundry_core::Rng;
use std::sync::Arc;

fn world(w: i32, h: i32) -> Simulation {
    Simulation::new(Arc::new(Content::load_default().unwrap()), SimConfig::finite(w, h, 3))
}

fn fill(s: &mut Simulation, x0: i32, y0: i32, x1: i32, y1: i32, m: MaterialId) {
    for y in y0..y1 {
        for x in x0..x1 {
            s.set_cell(CellPos::new(x, y), m, None);
        }
    }
}

fn tick_until_landed(s: &mut Simulation, max: u32) {
    for _ in 0..max {
        s.tick();
        if s.particles().is_empty() {
            return;
        }
    }
    for i in 0..s.particles().len() {
        let p = s.particles().get(i);
        println!("left: {p:?}");
        let (cx, cy) = (p.x as i32, p.y as i32);
        println!("{}", crate::ascii(s, CellRect::new(cx - 6, cy - 6, cx + 7, cy + 7)));
    }
    panic!("particles did not land in {max} ticks: {} left", s.particles().len());
}

#[test]
fn fast_particles_do_not_pass_a_wall_one_cell_thick() {
    let mut s = world(4, 4);
    let (sand, stone) = (s.content().expect_material("sand"), s.content().expect_material("stone"));
    // A wall at x = 150, one cell thick, from the ceiling to the floor.
    fill(&mut s, 150, 0, 151, 256, stone);
    let mut rng = Rng::new(5);
    let n = 2000;
    for _ in 0..n {
        let y = 10.0 + rng.unit() * 200.0;
        let x = 20.0 + rng.unit() * 100.0;
        // Top speed to the right, and a random up or down speed.
        s.spawn_particle((x, y), (12.0, (rng.unit() - 0.5) * 24.0), sand, None);
    }
    tick_until_landed(&mut s, 400);
    let all = s.count_material(CellRect::new(0, 0, 256, 256), sand);
    assert_eq!(all, n, "every particle became a cell");
    assert_eq!(s.count_material(CellRect::new(151, 0, 256, 256), sand), 0, "no sand behind the wall");
}

#[test]
fn fast_particles_do_not_pass_a_diagonal_wall() {
    let mut s = world(4, 4);
    let (sand, stone) = (s.content().expect_material("sand"), s.content().expect_material("stone"));
    // A diagonal wall with only corner contacts: cells (100 + k, 20 + k).
    for k in 0..220 {
        s.set_cell(CellPos::new(100 + k, 20 + k), stone, None);
    }
    let mut rng = Rng::new(9);
    let n = 1000;
    for _ in 0..n {
        // Particles below-left of the wall, flying up and to the right at top speed.
        let t = 30.0 + rng.unit() * 130.0;
        let (x, y) = (100.0 + t - 10.0 - rng.unit() * 20.0, 20.0 + t + 10.0);
        s.spawn_particle((x, y), (11.0 + rng.unit(), -11.0 - rng.unit()), sand, None);
    }
    tick_until_landed(&mut s, 400);
    let mut above = 0;
    for k in 0..220 {
        above += s.count_material(CellRect::new(100 + k + 1, 0, 100 + k + 2, 20 + k + 1), sand);
    }
    assert_eq!(above, 0, "no sand crossed the diagonal wall");
    assert_eq!(s.count_material(CellRect::new(0, 0, 256, 256), sand), n);
}

#[test]
fn visual_particles_fade_and_never_become_cells() {
    let mut s = world(2, 2);
    let (sand, smoke) = (s.content().expect_material("sand"), s.content().expect_material("smoke"));
    for i in 0..100 {
        assert!(s.spawn_visual((20.0 + i as f32, 30.0), (0.5, -1.0), sand, 30, false));
        assert!(s.spawn_visual((20.0 + i as f32, 90.0), (0.0, 0.0), smoke, 40, true));
    }
    assert_eq!(s.particles().visual_count(), 200);
    for _ in 0..10 {
        s.tick();
    }
    // Rising particles go up.
    let mut views = vec![];
    s.particles().views(CellRect::new(0, 0, 128, 128), &mut views);
    assert!(views.iter().filter(|v| v.material == smoke.0).all(|v| v.y < 90.0));
    for _ in 0..40 {
        s.tick();
    }
    assert!(s.particles().is_empty(), "all faded");
    assert_eq!(s.particles().visual_count(), 0);
    assert_eq!(s.count_material(CellRect::new(0, 0, 128, 128), sand), 0, "no cell was made");
    assert_eq!(s.count_material(CellRect::new(0, 0, 128, 128), smoke), 0);
}

#[test]
fn visual_particles_vanish_when_they_hit_something() {
    let mut s = world(2, 2);
    let (sand, stone) = (s.content().expect_material("sand"), s.content().expect_material("stone"));
    fill(&mut s, 60, 0, 61, 128, stone);
    for i in 0..50 {
        s.spawn_visual((20.0, 20.0 + i as f32), (8.0, 0.0), sand, 200, false);
    }
    for _ in 0..10 {
        s.tick();
    }
    assert!(s.particles().is_empty(), "they hit the wall");
}

#[test]
fn visual_particles_have_a_hard_limit_and_material_particles_have_none() {
    let mut s = world(2, 2);
    let sand = s.content().expect_material("sand");
    let mut added = 0;
    for i in 0..MAX_VISUAL + 500 {
        added += s.spawn_visual((10.0 + (i % 100) as f32, 10.0), (0.0, 0.0), sand, 100, true) as usize;
    }
    assert_eq!(added, MAX_VISUAL);
    assert_eq!(s.particles().visual_count(), MAX_VISUAL);
    let limit = s.settings().max_particles;
    for i in 0..limit {
        s.spawn_particle((10.0 + (i % 100) as f32, 20.0 + (i / 100 % 80) as f32), (0.0, 0.0), sand, None);
    }
    assert_eq!(s.particles().len(), MAX_VISUAL + limit, "material particles are always added");
    assert!(!s.spawn_visual((10.0, 10.0), (0.0, 0.0), sand, 100, true));
}

#[test]
fn save_and_load_keep_particles() {
    let mut s = world(2, 2);
    let (sand, smoke) = (s.content().expect_material("sand"), s.content().expect_material("smoke"));
    s.spawn_particle((10.0, 10.0), (1.0, -2.0), sand, Some(300));
    s.spawn_visual((20.0, 10.0), (0.0, 0.0), smoke, 50, true);
    let mut bytes = vec![];
    s.particles().write(&mut bytes).unwrap();
    let ids: Vec<MaterialId> = s.content().materials.all().collect();
    let p = Particles::read(&mut bytes.as_slice(), &ids).unwrap();
    assert_eq!(p.len(), 2);
    assert_eq!(p.visual_count(), 1);
    assert_eq!(p.count_material(sand), 1);
    assert_eq!((p.get(1).flags, p.get(1).life), (VISUAL | RISE, 50));
}

/// Speed of the particle step with 50,000 particles in the air (material particles, and a mix of
/// material and visual particles). Run in release mode:
/// `cargo test -p foundry_sim --release particle_step_speed -- --ignored --nocapture`.
#[test]
#[ignore]
fn particle_step_speed() {
    for (name, visual) in [("material", 0usize), ("mixed", MAX_VISUAL)] {
        for threads in [1, 0] {
            let mut s = world(16, 12);
            if threads > 0 {
                s.set_threads(threads);
            }
            let sand = s.content().expect_material("sand");
            let stone = s.content().expect_material("stone");
            fill(&mut s, 2, 700, 1022, 766, stone);
            let mut rng = Rng::new(1);
            let n = 50_000;
            let mut times = vec![];
            for round in 0..40 {
                // Keep 50,000 particles in the air: add new ones for those that landed.
                while s.particles().len() < n {
                    let (x, y) = (50.0 + rng.unit() * 920.0, 100.0 + rng.unit() * 400.0);
                    let v = ((rng.unit() - 0.5) * 12.0, -rng.unit() * 8.0);
                    if s.particles().visual_count() < visual {
                        s.spawn_visual((x, y), v, sand, 200, false);
                    } else {
                        s.spawn_particle((x, y), v, sand, None);
                    }
                }
                let settings = s.settings.clone();
                let start = std::time::Instant::now();
                s.particles.step(&mut s.world, &s.content.materials, &settings, round, 100 + round, s.pool.as_ref());
                times.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            times.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mean = times.iter().sum::<f64>() / times.len() as f64;
            let t = if threads == 1 { "1 thread".to_string() } else { format!("{} threads", rayon::current_num_threads()) };
            println!("{name}, {t}: 50,000 particles, step mean {mean:.3} ms, median {:.3} ms, max {:.3} ms", times[20], times[39]);
        }
    }
}

#[test]
fn a_particle_made_inside_a_solid_gets_out() {
    let mut s = world(2, 2);
    let (sand, stone) = (s.content().expect_material("sand"), s.content().expect_material("stone"));
    fill(&mut s, 40, 40, 80, 80, stone);
    s.spawn_particle((60.5, 60.5), (0.0, 0.0), sand, None);
    tick_until_landed(&mut s, 10);
    assert_eq!(s.count_material(CellRect::new(0, 0, 128, 128), sand), 1);
    assert_eq!(s.count_material(CellRect::new(40, 40, 80, 80), stone), 40 * 40);
}

#[test]
#[ignore]
fn debug_ball() {
    for cx in [192, 200] {
        let mut s = Simulation::new(Arc::new(Content::load_default().unwrap()), SimConfig::finite(6, 4, 4));
        let water = s.content().expect_material("water");
        s.paint(CellPos::new(cx, 50), 30, water, foundry_core::PaintMode::Replace, None);
        let mut rest = 0;
        for t in 0..3000 {
            s.tick();
            if s.stats().awake_chunks == 0 && s.particles().is_empty() {
                rest = t;
                break;
            }
        }
        println!("cx {cx} rest {rest}");
        for y in 240..250 {
            let xs: Vec<i32> = (0..384).filter(|&x| s.cell(CellPos::new(x, y)).material == water).collect();
            if !xs.is_empty() && xs.len() < 50 {
                println!("row {y}: {xs:?}");
            } else {
                println!("row {y}: {} water", xs.len());
            }
        }
    }
}
