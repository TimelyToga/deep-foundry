//! A real hopper, stamp mill, belt and water-fed sluice move ore into a crate unattended.

mod common;

use common::*;
use foundry_core::{CellRect, TilePos};
use foundry_factory::Factory;

#[test]
fn hopper_stamp_mill_belt_sluice_and_crate_run_without_player_actions() {
    let c = content();
    let mut sim = world(&c, Some(112));
    let mut f = Factory::new(c.clone());
    research_all(&mut f);

    let hopper = f
        .place(kind(&c, "hopper"), TilePos::new(3, 7), 0, false, &mut sim)
        .unwrap();
    let stamp = f
        .place(
            kind(&c, "stamp_mill"),
            TilePos::new(3, 9),
            0,
            false,
            &mut sim,
        )
        .unwrap();
    let mut belts = Vec::new();
    for x in 5..=9 {
        belts.push(
            f.place(
                kind(&c, "wood_belt"),
                TilePos::new(x, 13),
                0,
                false,
                &mut sim,
            )
            .unwrap(),
        );
    }
    let sluice = f
        .place(kind(&c, "sluice"), TilePos::new(10, 12), 0, false, &mut sim)
        .unwrap();
    let crate_ = f
        .place(kind(&c, "crate"), TilePos::new(13, 12), 0, false, &mut sim)
        .unwrap();
    f.set_recipe(stamp, Some(c.factory.recipe("crushed_malachite").unwrap()))
        .unwrap();
    f.set_recipe(sluice, Some(c.factory.recipe("washed_malachite").unwrap()))
        .unwrap();

    let raw = mat(&c, "raw_malachite");
    let water = mat(&c, "water");
    let mut belt_moved_ore = false;
    fill(&mut sim, tile_cells(TilePos::new(3, 4), 1, 2), raw, None);
    fill(&mut sim, tile_cells(TilePos::new(10, 8), 1, 1), water, None);

    let washed = item(&c, "washed_malachite");
    for _ in 0..2400 {
        f.tick(&mut sim);
        sim.tick();
        belt_moved_ore |= belts.iter().any(|id| {
            f.building_view(*id)
                .is_some_and(|v| v.status == foundry_factory::Status::Working)
        });
        if f.buildings
            .inventory(crate_)
            .is_some_and(|inv| inv.count(washed) >= 3)
        {
            break;
        }
    }

    let inv = f.buildings.inventory(crate_).unwrap();
    assert!(
        inv.count(washed) >= 3,
        "washed ore reaches the crate; crate={:?}, hopper={:?}, stamp={:?}, sluice={:?}",
        f.building_view(crate_),
        f.building_view(hopper),
        f.building_view(stamp),
        f.building_view(sluice)
    );
    let silt = mat(&c, "silt");
    assert_eq!(inv.count(item(&c, "silt")), 0, "tailings do not enter the product crate");
    assert!(count_all(&sim, silt) > 0, "the sluice carries light tailings into the flowing water");
    assert!(sim.count_material(CellRect::new(96, 104, 128, 112), silt) > 0, "tailings leave the lower waste outlet downstream of the sluice");
    assert!(belt_moved_ore, "crushed ore travels across the wood belt");
    let hopper_view = f.building_view(hopper).unwrap();
    assert!(hopper_view.inputs.iter().map(|b| b.count).sum::<u32>() < 16);
}
