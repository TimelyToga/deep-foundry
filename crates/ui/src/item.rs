//! What the UI shows about items and recipes. All of it comes from `foundry_content`:
//! items are [`ItemRef`] (a bulk material counted in units, or a part counted in pieces;
//! every building is also a part), recipes are `content.factory.recipes`.
//!
//! This module only adds the things the UI needs on top: the crafting tab of a recipe,
//! where a recipe is made, the kind label, tooltip facts, and fill colors.

use foundry_content::{Content, ItemRef, Phase, Recipe};
use foundry_core::{BuildingKindId, RecipeId, TechId};
use std::borrow::Cow;

/// What kind of item it is, for the tooltip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Powder,
    Liquid,
    Gas,
    /// A solid block material, for example stone.
    Solid,
    Fire,
    Part,
    Building,
}

impl ItemKind {
    pub fn label(self) -> &'static str {
        match self {
            ItemKind::Powder => "Powder",
            ItemKind::Liquid => "Liquid",
            ItemKind::Gas => "Gas",
            ItemKind::Solid => "Solid material",
            ItemKind::Fire => "Fire",
            ItemKind::Part => "Part",
            ItemKind::Building => "Building",
        }
    }
}

/// True for bulk materials (they go into the material tank, not the part slots).
pub fn is_bulk(item: ItemRef) -> bool {
    matches!(item, ItemRef::Material(_))
}

pub fn kind(content: &Content, item: ItemRef) -> ItemKind {
    match item {
        ItemRef::Material(m) => match content.materials.phase.get(m.index()).copied().unwrap_or(Phase::Solid) {
            Phase::Powder => ItemKind::Powder,
            Phase::Liquid => ItemKind::Liquid,
            Phase::Gas => ItemKind::Gas,
            Phase::Fire => ItemKind::Fire,
            Phase::Solid | Phase::Empty => ItemKind::Solid,
        },
        ItemRef::Part(p) => match content.factory.parts.get(p.0 as usize).and_then(|x| x.building) {
            Some(_) => ItemKind::Building,
            None => ItemKind::Part,
        },
    }
}

/// The name of an item, or "Unknown item" if the id is not in the content.
pub fn name(content: &Content, item: ItemRef) -> &str {
    let ok = match item {
        ItemRef::Material(m) => m.index() < content.materials.names.len(),
        ItemRef::Part(p) => (p.0 as usize) < content.factory.parts.len(),
    };
    if ok { content.item_name(item) } else { "Unknown item" }
}

/// The building type that an item places, if any.
pub fn building_of(content: &Content, item: ItemRef) -> Option<BuildingKindId> {
    match item {
        ItemRef::Part(p) => content.factory.parts.get(p.0 as usize).and_then(|x| x.building),
        ItemRef::Material(_) => None,
    }
}

/// The item that places a building type.
pub fn building_item(content: &Content, kind: BuildingKindId) -> Option<ItemRef> {
    content.factory.buildings.get(kind.0 as usize).map(|b| ItemRef::Part(b.part))
}

/// The name of a building type.
pub fn building_name(content: &Content, kind: BuildingKindId) -> &str {
    content.factory.buildings.get(kind.0 as usize).map(|b| b.name.as_str()).unwrap_or("Unknown building")
}

/// One or two sentences about an item.
pub fn description(content: &Content, item: ItemRef) -> Cow<'_, str> {
    match item {
        ItemRef::Part(p) => match content.factory.parts.get(p.0 as usize) {
            Some(part) if !part.description.is_empty() => Cow::Borrowed(part.description.as_str()),
            Some(part) => match part.building.and_then(|b| content.factory.buildings.get(b.0 as usize)) {
                Some(b) if !b.description.is_empty() => Cow::Borrowed(b.description.as_str()),
                _ => Cow::Borrowed(""),
            },
            None => Cow::Borrowed(""),
        },
        ItemRef::Material(_) => Cow::Borrowed(match kind(content, item) {
            ItemKind::Powder => "A powder. It falls and makes piles.",
            ItemKind::Liquid => "A liquid. It flows and finds its level.",
            ItemKind::Gas => "A gas. It rises or sinks by its density and spreads out.",
            ItemKind::Fire => "Fire. It rises, heats and ignites what it touches.",
            _ => "A solid material. It does not move.",
        }),
    }
}

/// Most pieces of a part in one inventory slot. For materials: `tank_capacity`.
pub fn stack_size(content: &Content, item: ItemRef, tank_capacity: u32) -> u32 {
    match item {
        ItemRef::Part(p) => content.factory.parts.get(p.0 as usize).map(|x| x.stack.max(1) as u32).unwrap_or(50),
        ItemRef::Material(_) => tank_capacity,
    }
}

/// The main color of an item (for fill bars), as RGBA. Parts use the color of their material.
pub fn color(content: &Content, item: ItemRef) -> [u8; 4] {
    let mat = match item {
        ItemRef::Material(m) => Some(m),
        ItemRef::Part(p) => content.factory.parts.get(p.0 as usize).and_then(|x| x.material),
    };
    mat.and_then(|m| content.materials.colors.get(m.index()))
        .and_then(|c| c.first())
        .map(|c| [c[0], c[1], c[2], 255])
        .unwrap_or([150, 150, 150, 255])
}

/// One line of facts in a tooltip, for example ("Melts at", "1085 °C").
#[derive(Debug, Clone, PartialEq)]
pub struct Fact {
    pub label: String,
    pub value: String,
}

fn fact(label: &str, value: String) -> Fact {
    Fact { label: label.into(), value }
}

/// Facts for the tooltip of an item.
pub fn facts(content: &Content, item: ItemRef) -> Vec<Fact> {
    let mut v = vec![];
    match item {
        ItemRef::Material(m) => {
            let t = &content.materials;
            let i = m.index();
            if i >= t.len() {
                return v;
            }
            if t.density[i] > 0.0 {
                v.push(fact("Density", format!("{} kg/m³", t.density[i].round())));
            }
            if let Some(c) = t.melt[i] {
                v.push(fact("Melts at", format!("{} °C", c.at)));
            }
            if let Some(c) = t.freeze[i] {
                v.push(fact("Freezes below", format!("{} °C", c.at)));
            }
            if let Some(c) = t.boil[i] {
                v.push(fact("Boils at", format!("{} °C", c.at)));
            }
            if let Some(b) = t.burn[i] {
                v.push(fact("Burns at", format!("{} °C", b.ignite_at)));
            }
        }
        ItemRef::Part(p) => {
            let Some(part) = content.factory.parts.get(p.0 as usize) else { return v };
            if let Some(b) = part.building.and_then(|b| content.factory.buildings.get(b.0 as usize)) {
                v.push(fact("Size", format!("{} × {} tiles", b.size.0, b.size.1)));
                v.push(fact("Tier", b.tier.to_string()));
                if let Some(pw) = &b.power {
                    if pw.produce_w > 0.0 {
                        v.push(fact("Makes", crate::format::watts(pw.produce_w as f64)));
                    }
                    if pw.use_w > 0.0 {
                        v.push(fact("Power use", format!("{} ({})", crate::format::watts(pw.use_w as f64), crate::model::Voltage::from_tier(pw.tier).map(|x| x.label()).unwrap_or("-"))));
                    }
                    if pw.store_j > 0.0 {
                        v.push(fact("Stores", crate::format::joules(pw.store_j as f64)));
                    }
                    if pw.steam_per_s > 0.0 {
                        v.push(fact("Steam use", format!("{} units per second", pw.steam_per_s)));
                    }
                }
                if b.speed != 1.0 && !b.crafts.is_empty() {
                    v.push(fact("Speed", format!("× {}", b.speed)));
                }
                v.push(fact("Max temperature", format!("{} °C", b.max_temp)));
            } else if let Some(m) = part.material
                && part.units > 0
            {
                v.push(fact("Made of", format!("{} units of {}", part.units, content.materials.names[m.index()])));
            }
            v.push(fact("Stack size", part.stack.to_string()));
        }
    }
    v
}

/// The tab of the crafting menu, as in Factorio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CraftGroup {
    Logistics,
    Production,
    Intermediate,
    Power,
    Research,
}

impl CraftGroup {
    pub const ALL: [CraftGroup; 5] =
        [CraftGroup::Logistics, CraftGroup::Production, CraftGroup::Intermediate, CraftGroup::Power, CraftGroup::Research];

    /// From the `group` string of a recipe ("logistics", "production", ...). Unknown: intermediate.
    pub fn parse(s: &str) -> CraftGroup {
        match s {
            "logistics" => CraftGroup::Logistics,
            "production" => CraftGroup::Production,
            "power" => CraftGroup::Power,
            "research" => CraftGroup::Research,
            _ => CraftGroup::Intermediate,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CraftGroup::Logistics => "Logistics",
            CraftGroup::Production => "Production",
            CraftGroup::Intermediate => "Intermediate products",
            CraftGroup::Power => "Power",
            CraftGroup::Research => "Research",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            CraftGroup::Intermediate => "Intermediate",
            g => g.label(),
        }
    }
}

/// Where a recipe can be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Maker {
    Hand,
    Building(BuildingKindId),
}

/// Where a recipe can be made: by hand (if `hand`), every building that crafts its category, and
/// for hand recipes also the buildings that craft "hand" (the workbench).
pub fn makers(content: &Content, recipe: &Recipe) -> Vec<Maker> {
    let mut v = vec![];
    if recipe.hand {
        v.push(Maker::Hand);
    }
    for (i, b) in content.factory.buildings.iter().enumerate() {
        let hand_helper = recipe.hand && b.crafts.iter().any(|c| c == "hand");
        if b.crafts.contains(&recipe.category) || hand_helper {
            v.push(Maker::Building(BuildingKindId(i as u16)));
        }
    }
    v
}

/// True if the player knows the recipe: no technology unlocks it, or the technology is done.
pub fn recipe_known(recipe: &Recipe, finished: &[TechId]) -> bool {
    recipe.unlocked_by.is_none_or(|t| finished.contains(&t))
}

/// The recipes a building type can make now (known recipes of the categories it crafts).
pub fn building_recipes(content: &Content, kind: BuildingKindId, finished: &[TechId]) -> Vec<RecipeId> {
    let Some(b) = content.factory.buildings.get(kind.0 as usize) else { return vec![] };
    content
        .factory
        .recipes
        .iter()
        .enumerate()
        .filter(|(_, r)| b.crafts.contains(&r.category) && recipe_known(r, finished))
        .map(|(i, _)| RecipeId(i as u16))
        .collect()
}

/// A recipe by id, if it exists.
pub fn recipe(content: &Content, id: RecipeId) -> Option<&Recipe> {
    content.factory.recipes.get(id.0 as usize)
}

/// The item whose icon shows a recipe: its first output.
pub fn recipe_item(recipe: &Recipe) -> Option<ItemRef> {
    recipe.outputs.first().map(|s| s.item)
}
