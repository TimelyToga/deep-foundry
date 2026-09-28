//! The early metal chain uses heat from world cells, then casts cooled metal.

mod common;

use common::*;
use foundry_content::ItemRef;
use foundry_core::{CellRect, TilePos};
use foundry_factory::{Factory, GuideState};

#[test]
fn campfire_and_bellows_heat_the_crucible_for_tin_copper_and_bronze() {
    let c = content();
    let mut sim = world(&c, Some(96));
    let mut f = Factory::new(c.clone());
    research_all(&mut f);
    let campfire = f
        .place(
            kind(&c, "campfire"),
            TilePos::new(5, 11),
            0,
            false,
            &mut sim,
        )
        .unwrap();
    let crucible = f
        .place(
            kind(&c, "crucible"),
            TilePos::new(5, 10),
            0,
            false,
            &mut sim,
        )
        .unwrap();
    let bellows = f
        .place(kind(&c, "bellows"), TilePos::new(8, 11), 0, false, &mut sim)
        .unwrap();
    // Keep the fluid tap closed so the test can inspect the machine output buffer.
    fill(
        &mut sim,
        CellRect::new(48, 80, 56, 88),
        mat(&c, "stone"),
        None,
    );
    let campfire_recipe = c.factory.recipe("pit_fired_clay_brick").unwrap();
    f.set_recipe(campfire, Some(campfire_recipe)).unwrap();
    assert_eq!(f.buildings.insert(&c, campfire, item(&c, "wood"), 60), 60);

    let tin = c.factory.recipe("tin_smelting").unwrap();
    f.set_recipe(crucible, Some(tin)).unwrap();
    f.buildings
        .insert(&c, crucible, item(&c, "raw_cassiterite"), 16);
    f.buildings.insert(&c, crucible, item(&c, "charcoal"), 8);
    run(&mut f, &mut sim, 3000);
    let view = f.building_view(crucible).unwrap();
    assert_eq!(view.outputs[0].item, item(&c, "molten_tin"));
    assert_eq!(
        view.outputs[0].count, 16,
        "tin smelting reads the heated simulation cells"
    );
    assert!(view.temperature >= 900, "{view:?}");
    assert_eq!(
        f.take_outputs_to_player(crucible)[0].item,
        item(&c, "molten_tin")
    );

    let copper = c.factory.recipe("copper_smelting").unwrap();
    f.set_recipe(crucible, Some(copper)).unwrap();
    let mut reached_copper_heat = false;
    let fire_cell = TilePos::new(5, 11).origin().offset(4, 0);
    let mut peak_fire_cell = i16::MIN;
    for _ in 0..3 {
        f.buildings
            .insert(&c, crucible, item(&c, "raw_malachite"), 16);
        f.buildings.insert(&c, crucible, item(&c, "charcoal"), 8);
        for _ in 0..3000 {
            run(&mut f, &mut sim, 1);
            reached_copper_heat |= f.building_view(crucible).unwrap().temperature >= 1100;
            peak_fire_cell = peak_fire_cell.max(sim.cell(fire_cell).temperature);
        }
        assert_eq!(
            f.building_view(crucible).unwrap().outputs[0].count,
            16,
            "{:?}",
            f.building_view(crucible).unwrap()
        );
        f.take_outputs_to_player(crucible);
    }
    let view = f.building_view(crucible).unwrap();
    assert!(
        reached_copper_heat,
        "bellows should raise the real fire cells: {view:?}"
    );

    // Bellows raised the campfire's actual cells, not a recipe-only temperature value.
    assert!(
        peak_fire_cell >= 1100,
        "peak heated cell was {peak_fire_cell} °C"
    );
    assert_ne!(bellows, campfire);

    // Three copper charges and one tin charge make one real bronze alloy batch.
    let alloy = c.factory.recipe("bronze_alloy").unwrap();
    f.set_recipe(crucible, Some(alloy)).unwrap();
    assert_eq!(
        f.insert_from_player(crucible, item(&c, "molten_copper"), 48),
        48
    );
    assert_eq!(
        f.insert_from_player(crucible, item(&c, "molten_tin"), 16),
        16
    );
    run(&mut f, &mut sim, 300);
    assert_eq!(f.building_view(crucible).unwrap().outputs[0].count, 64);
    f.take_outputs_to_player(crucible);

    let mold = f
        .place(
            kind(&c, "plate_mold"),
            TilePos::new(6, 11),
            0,
            false,
            &mut sim,
        )
        .unwrap();
    f.set_recipe(mold, Some(c.factory.recipe("bronze_plate").unwrap()))
        .unwrap();
    assert_eq!(
        f.insert_from_player(mold, item(&c, "molten_bronze"), 48),
        32
    );
    run(&mut f, &mut sim, 8 * 60 * 2);
    f.take_outputs_to_player(mold);
    assert_eq!(
        f.insert_from_player(mold, item(&c, "molten_bronze"), 16),
        16
    );
    run(&mut f, &mut sim, 8 * 60);
    f.take_outputs_to_player(mold);
    assert_eq!(f.item_count(item(&c, "bronze_plate")), 3);
    assert_eq!(f.item_count(item(&c, "molten_tin")), 0);
    assert_eq!(f.item_count(item(&c, "molten_copper")), 0);
    // The alloy made four plate units of metal. The mold has room for two casts at a time;
    // three plates consume 48 units and the final 16 remain in the player's inventory.
    assert_eq!(f.item_count(item(&c, "molten_bronze")), 16);
    assert_eq!(
        f.item_count(item(&c, "bronze_plate")) * 16 + f.item_count(item(&c, "molten_bronze")),
        64
    );

    f.craft(c.factory.recipe("bronze_gear").unwrap(), 1)
        .unwrap();
    run(&mut f, &mut sim, 120);
    assert_eq!(f.item_count(item(&c, "bronze_gear")), 1);
    assert_eq!(f.item_count(item(&c, "bronze_plate")), 0);
    assert_eq!(
        f.item_count(item(&c, "bronze_gear")),
        1,
        "the 3 bronze plates were consumed by the gear recipe"
    );
}

#[test]
fn casting_and_gears_complete_the_bronze_chain() {
    let c = content();
    let mut sim = world(&c, Some(96));
    let mut f = Factory::new(c.clone());
    research_all(&mut f);
    let mold = f
        .place(
            kind(&c, "plate_mold"),
            TilePos::new(5, 10),
            0,
            false,
            &mut sim,
        )
        .unwrap();
    f.set_recipe(mold, Some(c.factory.recipe("bronze_plate").unwrap()))
        .unwrap();
    f.buildings.insert(&c, mold, item(&c, "molten_bronze"), 48);
    run(&mut f, &mut sim, 8 * 60 * 2);
    assert_eq!(
        f.take_outputs_to_player(mold)
            .iter()
            .map(|s| s.count)
            .sum::<u32>(),
        2
    );
    assert_eq!(
        f.buildings.insert(&c, mold, item(&c, "molten_bronze"), 16),
        16
    );
    run(&mut f, &mut sim, 8 * 60);
    assert_eq!(
        f.take_outputs_to_player(mold)
            .iter()
            .map(|s| s.count)
            .sum::<u32>(),
        1
    );
    assert_eq!(f.item_count(item(&c, "bronze_plate")), 3);

    let gears = c.factory.recipe("bronze_gear").unwrap();
    f.craft(gears, 1).unwrap();
    run(&mut f, &mut sim, 120);
    assert_eq!(f.item_count(item(&c, "bronze_gear")), 1);
    assert!(matches!(item(&c, "molten_bronze"), ItemRef::Material(_)));
}

#[test]
fn a_mold_waits_for_hot_metal_to_cool_before_casting() {
    let c = content();
    let mut sim = world(&c, Some(96));
    let mut f = Factory::new(c.clone());
    let mold = f
        .place(
            kind(&c, "plate_mold"),
            TilePos::new(5, 10),
            0,
            false,
            &mut sim,
        )
        .unwrap();
    f.set_recipe(mold, Some(c.factory.recipe("tin_plate").unwrap()))
        .unwrap();
    fill(
        &mut sim,
        CellRect::new(40, 75, 48, 79),
        mat(&c, "molten_tin"),
        Some(1200),
    );

    run(&mut f, &mut sim, 20);
    let hot = f.building_view(mold).unwrap();
    assert_eq!(hot.status, foundry_factory::Status::TooHot, "{hot:?}");
    assert!(hot.reason.starts_with("Cooling:"), "{hot:?}");
    assert_eq!(hot.outputs[0].count, 0);

    run(&mut f, &mut sim, 12_000);
    let cooled = f.building_view(mold).unwrap();
    assert_eq!(cooled.outputs[0].count, 1, "{cooled:?}");
}
