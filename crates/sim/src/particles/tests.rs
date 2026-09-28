use super::*;
use crate::{SimConfig, Simulation};
use foundry_content::Content;
use foundry_core::PaintMode;
use std::sync::Arc;

#[test]
fn debug_flood() {
    let content = Arc::new(Content::load_default().unwrap());
    let mut s = Simulation::new(content, SimConfig { sky_chunks: 16, depth_chunks: 24, ..SimConfig::infinite(8, None) });
    let water = s.content().expect_material("water");
    s.add_anchor(CellRect::new(0, 900, 64, 1000));
    s.paint(CellPos::new(32, 800), 60, water, PaintMode::Replace, None);
    let area = CellRect::new(-4000, 0, 4000, 1100);
    let total = s.count_material(area, water);
    for _ in 0..1500 {
        s.tick();
    }
    let now = s.count_material(area, water);
    println!("end: {now} vs {total}, particles {}", s.particles().len());
    let p = s.particles();
    for i in 0..p.len() {
        println!("p {} {} {} {} {:?} {:?}", p.x[i], p.y[i], p.vx[i], p.vy[i], p.flags_and_life(i), s.world().areas().simulates(CellPos::new(p.x[i] as i32, p.y[i] as i32).chunk()));
    }
}
