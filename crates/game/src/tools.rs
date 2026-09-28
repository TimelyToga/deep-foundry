//! The multi-tool of the robot (game design section 6.3): dig, spray and scan.
//!
//! - **Dig** removes cells in a circle at the aim point. Each dug cell becomes one unit of its
//!   broken form (stone becomes gravel, an ore vein becomes raw ore). The robot keeps useful
//!   materials in its tanks and throws out the others (`foundry_factory::digging`, `spoil.rs`).
//!   A thrown out cell costs the same dig points. Each
//!   tick has `DIG_POWER` dig points; a cell costs `1 + hardness / 10` points. Cells harder than
//!   the drill head limit stay. Building body cells and bedrock stay. A cell whose material does
//!   not fit into the tanks stays too. The first dig of a material discovers it, like a scan.
//! - **Spray** puts material from a tank back into the world, into empty cells near the aim point.
//! - **Scan** discovers the material under the aim point (`Factory::scan`).
//!
//! The aim point is moved toward the robot when it is farther than `REACH` cells.

use crate::player::Robot;
use foundry_content::{Content, ItemRef, Layer, Phase};
use foundry_core::{CellPos, MaterialId};
use foundry_factory::{Dug, Factory};
use foundry_sim::Simulation;
use std::sync::OnceLock;

/// How far the tools reach from the middle of the robot, in cells.
pub const REACH: f32 = 80.0;
/// Radius of the dig circle in cells.
pub const DIG_RADIUS: i32 = 5;
/// Dig points per tick.
pub const DIG_POWER: u32 = 12;
/// Radius of the spray area in cells.
pub const SPRAY_RADIUS: i32 = 3;
/// Cells sprayed per tick.
pub const SPRAY_RATE: u32 = 4;
/// Liquids hotter than this (°C) do not go into the tank (no heat-proof tank yet).
pub const MAX_TANK_TEMPERATURE: i16 = 300;
/// The hardest material each drill head level can dig. Level 0 digs soft materials (sand, dirt,
/// clay, wood, surface ores). The technology effect `dig_hardness` adds levels.
pub const DIG_HARDNESS: [u8; 5] = [30, 60, 100, 150, 250];

/// What one tick of digging did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DigResult {
    /// Cells dug.
    pub dug: u32,
    /// The material of the first dug cell (for the picture: dug cells fly to the tool).
    pub material: Option<MaterialId>,
    /// A material that was too hard for the drill head.
    pub too_hard: Option<MaterialId>,
    /// A material that did not fit into the tanks.
    pub tank_full: Option<MaterialId>,
    /// A liquid that was too hot for the tank.
    pub too_hot: Option<MaterialId>,
}

/// The hardest material the robot can dig now.
pub fn dig_limit(factory: &Factory) -> u8 {
    let level = factory.progress.effect("dig_hardness").max(0.0).round() as usize;
    DIG_HARDNESS[level.min(DIG_HARDNESS.len() - 1)]
}

/// The aim point, moved toward the robot so that it is at most `REACH` cells away.
pub fn clamp_aim(robot: &Robot, aim: CellPos) -> CellPos {
    let (cx, cy) = robot.center();
    let (dx, dy) = (aim.x as f32 + 0.5 - cx, aim.y as f32 + 0.5 - cy);
    let d = (dx * dx + dy * dy).sqrt();
    if d <= REACH {
        return aim;
    }
    let k = REACH / d;
    CellPos::new((cx + dx * k).floor() as i32, (cy + dy * k).floor() as i32)
}

/// Offsets in a circle, nearest to the middle first.
fn circle(radius: i32) -> Vec<(i32, i32)> {
    let mut v: Vec<(i32, i32)> = (-radius..=radius)
        .flat_map(|y| (-radius..=radius).map(move |x| (x, y)))
        .filter(|(x, y)| x * x + y * y <= radius * radius)
        .collect();
    v.sort_by_key(|(x, y)| (x * x + y * y, *y, *x));
    v
}

fn dig_circle() -> &'static [(i32, i32)] {
    static C: OnceLock<Vec<(i32, i32)>> = OnceLock::new();
    C.get_or_init(|| circle(DIG_RADIUS))
}

fn spray_circle() -> &'static [(i32, i32)] {
    static C: OnceLock<Vec<(i32, i32)>> = OnceLock::new();
    C.get_or_init(|| circle(SPRAY_RADIUS))
}

/// True if a building body is on this cell.
fn is_building(factory: &Factory, p: CellPos) -> bool {
    factory.buildings.at_tile(p.tile(), Layer::Front).is_some()
}

/// One tick of digging at `aim`.
pub fn dig(factory: &mut Factory, sim: &mut Simulation, robot: &Robot, aim: CellPos) -> DigResult {
    let aim = clamp_aim(robot, aim);
    let content = factory.content.clone();
    let c: &Content = &content;
    let limit = dig_limit(factory);
    let mut budget = DIG_POWER;
    let mut out = DigResult::default();
    for &(dx, dy) in dig_circle() {
        let p = aim.offset(dx, dy);
        let cell = sim.cell(p);
        let m = cell.material;
        if m.is_air() || matches!(c.materials.phase[m.index()], Phase::Gas | Phase::Fire | Phase::Empty) {
            continue;
        }
        let hardness = c.materials.hardness[m.index()];
        if hardness == u8::MAX || is_building(factory, p) {
            continue;
        }
        if hardness > limit {
            out.too_hard.get_or_insert(m);
            continue;
        }
        if c.materials.phase[m.index()] == Phase::Liquid && cell.temperature > MAX_TANK_TEMPERATURE {
            out.too_hot.get_or_insert(m);
            continue;
        }
        let cost = 1 + hardness as u32 / 10;
        if cost > budget {
            break;
        }
        // A kept unit goes into the tanks; a dropped one flies out behind the robot (spoil.rs).
        // The first dig of a material discovers it.
        match factory.take_dug_cell(m) {
            Dug::Kept => sim.set_cell(p, MaterialId::AIR, None),
            Dug::Dropped(loose) => crate::spoil::throw_out(sim, robot, aim, p, loose, out.dug),
            Dug::NoRoom(b) => {
                out.tank_full.get_or_insert(b);
                continue;
            }
        }
        budget -= cost;
        out.dug += 1;
        out.material.get_or_insert(m);
    }
    out
}

/// One tick of spraying `material` from the tanks at `aim`. Returns the cells sprayed.
pub fn spray(factory: &mut Factory, sim: &mut Simulation, robot: &Robot, aim: CellPos, material: MaterialId) -> u32 {
    let aim = clamp_aim(robot, aim);
    let item = ItemRef::Material(material);
    let body = robot.rect();
    let mut n = 0;
    for &(dx, dy) in spray_circle() {
        if n >= SPRAY_RATE || factory.player.count(item) == 0 {
            break;
        }
        let p = aim.offset(dx, dy);
        if body.contains(p) || !sim.cell(p).material.is_air() || is_building(factory, p) {
            continue;
        }
        let (w, h) = sim.size_cells();
        if (w > 0 && (p.x < 0 || p.x >= w)) || p.y < 0 || p.y >= h {
            continue;
        }
        if factory.player.remove(item, 1) == 1 {
            sim.set_cell(p, material, None);
            n += 1;
        }
    }
    n
}

/// Scan the cell at `aim`. Returns the material there (not air) and the materials this scan
/// discovered.
pub fn scan(factory: &mut Factory, sim: &Simulation, robot: &Robot, aim: CellPos) -> Option<(MaterialId, Vec<MaterialId>)> {
    let aim = clamp_aim(robot, aim);
    let m = sim.cell(aim).material;
    if m.is_air() {
        return None;
    }
    Some((m, factory.scan(m)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundry_core::CellRect;
    use foundry_sim::SimConfig;
    use std::sync::Arc;

    fn setup() -> (Factory, Simulation, Robot) {
        let content = Arc::new(Content::load_default().unwrap());
        let mut sim = Simulation::new(content.clone(), SimConfig::finite(4, 4, 1));
        let (clay, stone) = (content.expect_material("clay"), content.expect_material("stone"));
        for y in 200..240 {
            for x in 2..254 {
                sim.set_cell(CellPos::new(x, y), if x < 128 { clay } else { stone }, None);
            }
        }
        let factory = Factory::new(content);
        let robot = Robot::standing_at(CellPos::new(128, 200));
        (factory, sim, robot)
    }

    #[test]
    fn dig_puts_cells_into_the_tank() {
        let (mut f, mut sim, robot) = setup();
        let clay = sim.content().expect_material("clay");
        let mut total = 0;
        for _ in 0..10 {
            total += dig(&mut f, &mut sim, &robot, CellPos::new(110, 205)).dug;
        }
        assert!(total >= 40, "dug {total}");
        assert_eq!(f.player.count(ItemRef::Material(clay)), total);
        let area = CellRect::around(CellPos::new(110, 205), DIG_RADIUS);
        assert_eq!(sim.count_material(area, clay) + total as usize, area_cells_of(&area));
    }

    fn area_cells_of(area: &CellRect) -> usize {
        // Every cell of the circle's box was clay (the box is inside the clay area).
        (area.width() * area.height()) as usize
    }

    #[test]
    fn stone_is_too_hard_at_the_start() {
        let (mut f, mut sim, robot) = setup();
        let r = dig(&mut f, &mut sim, &robot, CellPos::new(150, 205));
        assert_eq!(r.dug, 0);
        assert_eq!(r.too_hard, sim.content().material("stone"));
    }

    #[test]
    fn spray_puts_material_back() {
        let (mut f, mut sim, robot) = setup();
        let clay = sim.content().expect_material("clay");
        f.player.insert(&f.content.clone(), ItemRef::Material(clay), 10);
        let aim = CellPos::new(100, 150);
        let mut n = 0;
        for _ in 0..5 {
            n += spray(&mut f, &mut sim, &robot, aim, clay);
        }
        assert_eq!(n, 10);
        assert_eq!(f.player.count(ItemRef::Material(clay)), 0);
        assert_eq!(sim.count_material(CellRect::around(aim, SPRAY_RADIUS), clay), 10);
    }

    #[test]
    fn far_aim_is_moved_into_reach() {
        let (_, _, robot) = setup();
        let p = clamp_aim(&robot, CellPos::new(1000, 192));
        let (cx, cy) = robot.center();
        let d = ((p.x as f32 - cx).powi(2) + (p.y as f32 - cy).powi(2)).sqrt();
        assert!(d <= REACH + 1.0, "{d}");
    }

    #[test]
    fn scan_discovers_once() {
        let (mut f, sim, robot) = setup();
        let clay = sim.content().expect_material("clay");
        let (m, found) = scan(&mut f, &sim, &robot, CellPos::new(120, 205)).unwrap();
        assert_eq!(m, clay);
        assert_eq!(found, vec![clay]);
        let (_, again) = scan(&mut f, &sim, &robot, CellPos::new(120, 205)).unwrap();
        assert!(again.is_empty());
        assert!(scan(&mut f, &sim, &robot, CellPos::new(120, 100)).is_none(), "air is not scanned");
    }
}

#[cfg(test)]
#[path = "generated_tools_tests.rs"]
mod generated_tools_tests;
