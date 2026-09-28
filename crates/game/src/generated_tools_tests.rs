//! Tools must support both sides of an unbounded generated world.
use super::*;
use foundry_core::CellRect;
use foundry_sim::SimConfig;
use std::sync::Arc;

#[test]
fn spray_works_at_positive_and_negative_positions_in_infinite_worlds() {
    let content = Arc::new(Content::load_default().unwrap());
    for x in [-128, 128] {
        let mut sim = Simulation::new(content.clone(), SimConfig::infinite(3, None));
        let mut factory = Factory::new(content.clone());
        let water = content.expect_material("water");
        let item = ItemRef::Material(water);
        factory.player.insert(&content, item, 16);
        let robot = Robot::standing_at(CellPos::new(x, 100));
        let aim = CellPos::new(x + 20, 85);
        assert_eq!(spray(&mut factory, &mut sim, &robot, aim, water), SPRAY_RATE);
        assert_eq!(factory.player.count(item), 16 - SPRAY_RATE);
        assert_eq!(sim.count_material(CellRect::around(aim, SPRAY_RADIUS), water), SPRAY_RATE as usize);
    }
}
