//! Dug material that the robot does not keep (see `foundry_factory::digging`).
//!
//! The robot throws it out behind itself: the dug cell becomes air, and one unit of the loose
//! form (for example gravel for stone) flies out of the robot's back as a particle. The particle
//! falls and lands as a cell. So the dug hole stays open, and no material is lost.
//!
//! "Behind" is the side away from the aim point. If that side is blocked, the unit flies out over
//! the robot's head. If that is blocked too, it falls where it was dug.

use crate::player::{ROBOT_W, Robot, blocks};
use foundry_core::{CellPos, MaterialId};
use foundry_sim::Simulation;
use foundry_sim::particles::Spawn;

/// Throw out one unit of `material` for the dug cell `at`. `aim` is the aim point of the dig
/// tool, `n` the number of the cell in this tick (for a small spread).
pub fn throw_out(sim: &mut Simulation, robot: &Robot, aim: CellPos, at: CellPos, material: MaterialId, n: u32) {
    let temperature = sim.cell(at).temperature;
    sim.set_cell(at, MaterialId::AIR, None);
    let content = sim.content().clone();
    let r = robot.rect();
    let (cx, _) = robot.center();
    let back: i32 = if aim.x as f32 + 0.5 >= cx { -1 } else { 1 };
    // Two numbers from 0 to 1 for the spread, from the cell and its number.
    let h = spread(at, n);
    let (j1, j2) = ((h & 0xff) as f32 / 255.0, ((h >> 8) & 0xff) as f32 / 255.0);
    let free = |p: CellPos| !blocks(&content, sim.cell(p).material);
    let behind = CellPos::new(if back < 0 { r.x0 - 2 } else { r.x0 + ROBOT_W + 1 }, r.y0 + 3);
    let above = CellPos::new(r.x0 + ROBOT_W / 2, r.y0 - 3);
    let (x, y, vx, vy) = if free(behind) {
        (behind.x as f64 + 0.5, behind.y as f64 + 0.5, back as f32 * (0.8 + 0.7 * j1), -(0.8 + 0.6 * j2))
    } else if free(above) {
        (above.x as f64 + 0.5, above.y as f64 + 0.5, back as f32 * (0.5 + 0.5 * j1), -(1.0 + 0.5 * j2))
    } else {
        (at.x as f64 + 0.5, at.y as f64 + 0.5, 0.0, 0.0)
    };
    let spawn = Spawn { x, y, vx, vy, material, temperature, shade: (h >> 16) as u8, life: 0, flags: 0 };
    let max = sim.settings().max_particles;
    sim.particles_mut().spawn(spawn, max);
}

/// A number from a cell position and a counter.
fn spread(p: CellPos, n: u32) -> u32 {
    let mut h = (p.x as u32).wrapping_mul(0x9e37_79b9) ^ (p.y as u32).wrapping_mul(0x85eb_ca6b) ^ n.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^ (h >> 12)
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundry_content::Content;
    use foundry_core::CellRect;
    use foundry_sim::SimConfig;
    use std::sync::Arc;

    #[test]
    fn a_dropped_cell_flies_out_behind_the_robot_and_lands() {
        let content = Arc::new(Content::load_default().unwrap());
        let mut sim = Simulation::new(content.clone(), SimConfig::finite(4, 4, 1));
        let (stone, gravel) = (content.expect_material("stone"), content.expect_material("gravel"));
        for y in 200..240 {
            for x in 2..254 {
                sim.set_cell(CellPos::new(x, y), stone, None);
            }
        }
        let robot = Robot::standing_at(CellPos::new(128, 200));
        // The robot digs right of itself: the gravel flies out to the left.
        let at = CellPos::new(150, 202);
        throw_out(&mut sim, &robot, CellPos::new(150, 202), at, gravel, 0);
        assert!(sim.cell(at).material.is_air(), "the dug cell is open");
        assert_eq!(sim.particles().count_material(gravel), 1);
        for _ in 0..120 {
            sim.tick();
        }
        assert_eq!(sim.particles().count_material(gravel), 0, "it landed");
        let left = CellRect::new(60, 150, 124, 200);
        assert_eq!(sim.count_material(left, gravel), 1, "it lies on the ground left of the robot");
    }
}
