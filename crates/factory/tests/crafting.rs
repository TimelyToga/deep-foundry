//! Hand crafting with intermediates, the workbench speed and known recipes.

mod common;

use common::*;
use foundry_core::{CellPos, RecipeId, TilePos};
use foundry_factory::{CraftError, Factory, Progress};

/// A factory with all technologies done and a few items.
fn stocked_factory() -> Factory {
    let c = content();
    let mut f = Factory::new(c.clone());
    research_all(&mut f);
    f.player.insert(&c, item(&c, "bronze_plate"), 2);
    f.player.insert(&c, item(&c, "tin_plate"), 1);
    f.player.insert(&c, item(&c, "clay"), 16);
    f
}

#[test]
fn missing_ingredients_are_crafted_first() {
    let c = content();
    let mut sim = world(&c, Some(96));
    let mut f = stocked_factory();
    let kit_recipe = c.factory.recipe("bronze_kit").unwrap();
    // The kit needs a bronze gear and a raw clay brick. The player has neither, but can make both.
    f.craft(kit_recipe, 1).expect("queued");
    let names: Vec<(String, bool)> = f.crafting_view().into_iter().map(|j| (j.name, j.intermediate)).collect();
    assert_eq!(
        names,
        vec![("Bronze gear".into(), true), ("Raw clay brick".into(), true), ("Bronze research kit".into(), false)]
    );
    assert!(f.player.is_empty(), "all ingredients are held by the queue");
    // 2 s + 1 s + 5 s at speed 1 = 480 ticks.
    let kit = item(&c, "bronze_kit");
    run(&mut f, &mut sim, 479);
    assert_eq!(f.player.count(kit), 0);
    run(&mut f, &mut sim, 1);
    assert_eq!(f.player.count(kit), 2, "one craft makes 2 kits");
    assert_eq!(f.player.contents().len(), 1, "no gear or brick is left over");
    assert!(f.hand.is_idle());
}

#[test]
fn a_workbench_nearby_doubles_the_speed() {
    let c = content();
    let mut sim = world(&c, Some(96));
    let mut f = stocked_factory();
    f.place(kind(&c, "workbench"), TilePos::new(4, 10), 0, false, &mut sim).unwrap();
    f.player_pos = Some(CellPos::new(60, 90)); // 12 cells right of the workbench, reach is 48
    f.craft(c.factory.recipe("bronze_kit").unwrap(), 1).unwrap();
    run(&mut f, &mut sim, 239);
    assert_eq!(f.player.count(item(&c, "bronze_kit")), 0);
    run(&mut f, &mut sim, 1);
    assert_eq!(f.player.count(item(&c, "bronze_kit")), 2);
    // Far away the speed is 1 again.
    assert_eq!(f.buildings.hand_speed(&c, CellPos::new(120, 10)), 1.0);
}

#[test]
fn only_known_recipes_are_crafted() {
    let c = content();
    let mut f = stocked_factory();
    let kit_recipe = c.factory.recipe("bronze_kit").unwrap();
    let gear_recipe = c.factory.recipe("bronze_gear").unwrap();
    // With no research done, only recipes that no technology unlocks are known. The kit needs
    // "Research".
    f.progress = Progress::new(&c);
    assert!(!f.is_recipe_known(kit_recipe));
    assert_eq!(f.craft(kit_recipe, 1), Err(CraftError::NotKnown));
    let research = c.factory.recipe_def(kit_recipe).unlocked_by.expect("a tech unlocks the kit");
    f.progress.debug_complete(&c, research);
    assert!(f.is_recipe_known(kit_recipe));
    assert_eq!(f.can_craft(kit_recipe, 1), Ok(()));
    // The kit is known but the gear is not: the gear cannot be made on the way.
    let no_gear = |r: RecipeId| r != gear_recipe;
    match f.hand.craft(&c, &mut f.player, kit_recipe, 1, &no_gear) {
        Err(CraftError::Missing { name, count, .. }) => assert_eq!((name.as_str(), count), ("Bronze gear", 1)),
        other => panic!("{other:?}"),
    }
    assert_eq!(f.player.count(item(&c, "bronze_plate")), 2, "nothing was taken");
}

#[test]
fn cancel_gives_back_everything_including_made_intermediates() {
    let c = content();
    let mut sim = world(&c, Some(96));
    let mut f = stocked_factory();
    let req = f.craft(c.factory.recipe("bronze_kit").unwrap(), 1).unwrap();
    // After 130 ticks the gear is made (120 ticks) and the brick is on its way.
    run(&mut f, &mut sim, 130);
    assert!(f.cancel_craft(req).is_empty());
    assert!(f.hand.is_idle());
    assert_eq!(f.player.count(item(&c, "bronze_gear")), 1);
    assert_eq!(f.player.count(item(&c, "bronze_plate")), 0);
    assert_eq!(f.player.count(item(&c, "tin_plate")), 1);
    assert_eq!(f.player.count(item(&c, "clay")), 16);
}
