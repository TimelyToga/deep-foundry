//! Checks on the Tier 0-1 factory data: parts, buildings, recipes, technologies and
//! milestones in `assets/data/{parts,buildings,recipes,tech,milestones}/*.ron`.
//!
//! The loader (`FactoryContent::build`, run by `Content::load_default`) already checks that
//! every id a recipe, port filter or tech refers to actually exists, and that every recipe's
//! category matches some building's `crafts` list. These tests check the things the loader
//! does not: that the tech tree and recipes actually let a player reach milestone 2 from the
//! start of the game.

use foundry_content::{Content, ItemRef};
use std::collections::HashSet;

/// Items the player starts with or gets some other way, so they never need a recipe:
/// the Hub is the landing pod the player begins next to, not something they craft.
const EXEMPT_ITEMS: &[&str] = &["hub"];

/// The ids of every technology reachable from the start: technologies with no `requires`,
/// plus any technology whose `requires` are all already reachable. This ignores kit and
/// discovery point costs (those are gameplay pacing, not data completeness) and only checks
/// the shape of the `requires` graph.
fn reachable_techs(content: &Content) -> HashSet<String> {
    let fc = &content.factory;
    let mut reachable: HashSet<String> = HashSet::new();
    loop {
        let mut added = false;
        for tech in &fc.techs {
            if reachable.contains(&tech.id) {
                continue;
            }
            if tech.requires.iter().all(|r| reachable.contains(&fc.tech_def(*r).id)) {
                reachable.insert(tech.id.clone());
                added = true;
            }
        }
        if !added {
            break;
        }
    }
    reachable
}

/// The ids of every recipe a player can eventually use: known from the start, or unlocked by
/// a reachable technology.
fn reachable_recipe_ids(content: &Content, reachable_techs: &HashSet<String>) -> HashSet<String> {
    content
        .factory
        .recipes
        .iter()
        .filter(|r| match r.unlocked_by {
            None => true,
            Some(t) => reachable_techs.contains(&content.factory.tech_def(t).id),
        })
        .map(|r| r.id.clone())
        .collect()
}

/// Every item a player can eventually make, starting from nothing but materials (which come
/// from the world: digging, scanning and cell reactions) and the exempt items above.
/// An item is makeable once every reachable recipe that outputs it has all its part inputs
/// already makeable (material inputs are always assumed available).
fn makeable_items(content: &Content, reachable_recipes: &HashSet<String>) -> HashSet<ItemRef> {
    let fc = &content.factory;
    let mut known: HashSet<ItemRef> = HashSet::new();
    for id in EXEMPT_ITEMS {
        if let Some(item) = content.item(id) {
            known.insert(item);
        }
    }
    loop {
        let mut added = false;
        for recipe in &fc.recipes {
            if !reachable_recipes.contains(&recipe.id) {
                continue;
            }
            let inputs_ready = recipe
                .inputs
                .iter()
                .all(|s| matches!(s.item, ItemRef::Material(_)) || known.contains(&s.item));
            if !inputs_ready {
                continue;
            }
            for out in recipe.outputs.iter().chain(recipe.byproducts.iter().map(|(s, _)| s)) {
                if known.insert(out.item) {
                    added = true;
                }
            }
        }
        if !added {
            break;
        }
    }
    known
}

fn item_label(content: &Content, item: ItemRef) -> String {
    content.item_name(item).to_string()
}

#[test]
fn default_factory_data_loads() {
    let content = Content::load_default().expect("the default factory data must load and pass all checks");
    assert!(content.factory.parts.len() >= 10, "expected at least 10 parts");
    assert!(content.factory.buildings.len() >= 15, "expected at least 15 buildings");
    assert!(content.factory.recipes.len() >= 30, "expected at least 30 recipes");
    assert!(content.factory.techs.len() >= 5, "expected at least 5 technologies");
    assert_eq!(content.factory.milestones.len(), 2, "expected Hub repair stages 1 and 2");
}

#[test]
fn every_part_and_building_is_makeable_or_exempt() {
    let content = Content::load_default().expect("assets load");
    let techs = reachable_techs(&content);
    let recipes = reachable_recipe_ids(&content, &techs);
    let known = makeable_items(&content, &recipes);

    let mut missing = vec![];
    for part in &content.factory.parts {
        if EXEMPT_ITEMS.contains(&part.id.as_str()) {
            continue;
        }
        let item = content.item(&part.id).unwrap();
        if !known.contains(&item) {
            missing.push(part.id.clone());
        }
    }
    assert!(missing.is_empty(), "these parts or buildings have no reachable recipe: {missing:?}");
}

#[test]
fn every_technology_is_reachable_from_the_start() {
    let content = Content::load_default().expect("assets load");
    let techs = reachable_techs(&content);
    let missing: Vec<&str> =
        content.factory.techs.iter().map(|t| t.id.as_str()).filter(|id| !techs.contains(*id)).collect();
    assert!(missing.is_empty(), "these technologies are never reachable (a `requires` cycle or a missing base): {missing:?}");
}

#[test]
fn milestone_1_is_reachable_from_the_start() {
    let content = Content::load_default().expect("assets load");
    let techs = reachable_techs(&content);
    let recipes = reachable_recipe_ids(&content, &techs);
    let known = makeable_items(&content, &recipes);

    let stage1 = content.factory.milestones.iter().find(|m| m.stage == 1).expect("milestone stage 1 exists");
    let mut missing = vec![];
    for stack in &stage1.deliver {
        if !matches!(stack.item, ItemRef::Material(_)) && !known.contains(&stack.item) {
            missing.push(item_label(&content, stack.item));
        }
    }
    assert!(missing.is_empty(), "milestone 1 needs items with no reachable recipe: {missing:?}");
}

#[test]
fn milestone_2_is_reachable_from_the_start() {
    let content = Content::load_default().expect("assets load");
    let techs = reachable_techs(&content);
    let recipes = reachable_recipe_ids(&content, &techs);
    let known = makeable_items(&content, &recipes);

    let stage2 = content.factory.milestones.iter().find(|m| m.stage == 2).expect("milestone stage 2 exists");
    let mut missing = vec![];
    for stack in &stage2.deliver {
        if !matches!(stack.item, ItemRef::Material(_)) && !known.contains(&stack.item) {
            missing.push(item_label(&content, stack.item));
        }
    }
    assert!(missing.is_empty(), "milestone 2 needs items with no reachable recipe: {missing:?}");
}

/// Every guide goal (Build/HaveItem/Research/Discover/Stage) names a building, part,
/// technology or material that the data actually has. The guide loader lives in
/// `crates/factory/src/progress/guide.rs`; this test reads the same `.ron` files with a
/// small local parser so `foundry_content` does not need to depend on `foundry_factory`.
#[test]
fn guide_goals_name_real_ids() {
    let content = Content::load_default().expect("assets load");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/data/guide");
    let mut checked_any = false;
    let mut missing = vec![];
    for entry in std::fs::read_dir(&dir).expect("assets/data/guide exists") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("ron") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read guide file");
        checked_any = true;
        // A small, forgiving scan: pull every quoted string that follows one of the
        // condition keywords. This does not parse RON; it only extracts ids to check.
        for keyword in ["HaveItem(\"", "Build(\"", "Research(\"", "Discover(\""] {
            let mut rest = text.as_str();
            while let Some(pos) = rest.find(keyword) {
                rest = &rest[pos + keyword.len()..];
                let end = rest.find('"').expect("closing quote");
                let id = &rest[..end];
                let ok = match keyword {
                    "HaveItem(\"" => content.item(id).is_some(),
                    "Build(\"" => content.factory.building(id).is_some(),
                    "Research(\"" => content.factory.tech(id).is_some(),
                    "Discover(\"" => content.item(id).is_some() || content.material(id).is_some(),
                    _ => unreachable!(),
                };
                if !ok {
                    missing.push(format!("{}: {keyword}{id}\")", path.display()));
                }
                rest = &rest[end..];
            }
        }
    }
    assert!(checked_any, "no guide files found at {}", dir.display());
    assert!(missing.is_empty(), "guide goals name ids the data does not have: {missing:#?}");
}
