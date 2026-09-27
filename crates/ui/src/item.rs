//! Items and recipes as the UI sees them.
//!
//! An item is anything that can be in a slot: a bulk material (counted in units),
//! a part (counted in pieces), or a building in item form (counted in pieces).
//!
//! The real item and recipe data does not exist yet (Milestone 3-4). Until then, the
//! game (or `crate::mock`) fills a [`Catalog`] with [`ItemInfo`] and [`RecipeView`] values.

use foundry_core::{BuildingKindId, MaterialId, RecipeId};
use std::collections::HashMap;

/// A part type (gear, plate, circuit, kit). The number comes from the part data files
/// when they exist. Until then the game or the mock gives the numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PartId(pub u16);

/// Anything that can be in a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ItemId {
    /// Bulk material. Counts are in units (one unit is one cell).
    Material(MaterialId),
    /// A discrete part. Counts are in pieces.
    Part(PartId),
    /// A building in item form. Counts are in pieces.
    Building(BuildingKindId),
}

impl ItemId {
    /// True for bulk materials. Bulk materials go into the material tank, not the part slots.
    pub fn is_bulk(self) -> bool {
        matches!(self, ItemId::Material(_))
    }
}

/// A number of one item. For materials the count is in units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ItemStack {
    pub item: ItemId,
    pub count: u32,
}

impl ItemStack {
    pub fn new(item: ItemId, count: u32) -> Self {
        Self { item, count }
    }
}

/// An amount of an item in a recipe. For materials the amount is in units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemAmount {
    pub item: ItemId,
    pub amount: u32,
    /// Chance to get this output (1.0 = always). Only used for recipe outputs.
    pub chance: f32,
}

impl ItemAmount {
    pub fn new(item: ItemId, amount: u32) -> Self {
        Self { item, amount, chance: 1.0 }
    }
}

/// What kind of item it is, for the tooltip and for the slot rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    /// A powder, for example sand or crushed ore.
    Powder,
    /// A liquid, for example water or molten copper.
    Liquid,
    /// A gas, for example steam.
    Gas,
    /// A solid block material, for example stone or clay brick blocks.
    Solid,
    /// Fire and other short-lived materials.
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

/// The shape that the icon generator draws. Real art replaces the generator later
/// (see `crate::icons`). Each shape uses the colors in [`IconSpec`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconShape {
    // Bulk materials
    Pile,
    Drop,
    Gas,
    Block,
    Flame,
    // Parts
    Gear,
    Plate,
    Ingot,
    Rod,
    WireCoil,
    Pipe,
    Brick,
    Circuit,
    Kit,
    Vial,
    Pane,
    Sheet,
    Bolt,
    VacuumTube,
    // Buildings
    Machine(MachineGlyph),
    Belt,
    Crate,
    Barrel,
    Wall,
    Ladder,
    Campfire,
    Workbench,
    Hopper,
    Mold,
    Crucible,
    Tank,
    Cable,
    SolarPanel,
    Battery,
}

/// The small symbol on a machine icon. It tells the machine family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MachineGlyph {
    None,
    Gear,
    Flame,
    Hammer,
    Crusher,
    Drop,
    Flask,
    Arrow,
    Lightning,
    Fan,
    Magnet,
    Drill,
    Wire,
    Plus,
}

/// How to draw the icon of one item.
#[derive(Debug, Clone, PartialEq)]
pub struct IconSpec {
    pub shape: IconShape,
    /// The main colors, as RGBA. Materials use their material colors. At least one color.
    pub colors: Vec<[u8; 4]>,
    /// Tier of the item (0 to 5). Machines use it for the colored band.
    pub tier: u8,
}

impl IconSpec {
    pub fn new(shape: IconShape, color: [u8; 4]) -> Self {
        Self { shape, colors: vec![color], tier: 0 }
    }

    pub fn with_tier(mut self, tier: u8) -> Self {
        self.tier = tier;
        self
    }

    /// The first color, or gray.
    pub fn main_color(&self) -> [u8; 4] {
        self.colors.first().copied().unwrap_or([128, 128, 128, 255])
    }
}

/// One line of facts in a tooltip, for example ("Melts at", "1085 °C").
#[derive(Debug, Clone, PartialEq)]
pub struct Fact {
    pub label: String,
    pub value: String,
}

/// Everything the UI shows about one item.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemInfo {
    pub id: ItemId,
    /// The string id from the data files, for example "bronze_gear". Real art uses it as the file name.
    pub key: String,
    pub name: String,
    /// One or two short sentences.
    pub description: String,
    pub kind: ItemKind,
    pub icon: IconSpec,
    /// Most pieces in one part slot. Not used for bulk materials (the tank slot capacity counts).
    pub stack_size: u32,
    pub tier: u8,
    /// Extra lines for the tooltip.
    pub facts: Vec<Fact>,
}

impl ItemInfo {
    /// The main color of the item, for fill bars and graph lines.
    pub fn color(&self) -> [u8; 4] {
        self.icon.main_color()
    }
}

/// The tab of the crafting menu that a recipe is in. Same groups as Factorio.
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

    pub fn label(self) -> &'static str {
        match self {
            CraftGroup::Logistics => "Logistics",
            CraftGroup::Production => "Production",
            CraftGroup::Intermediate => "Intermediate products",
            CraftGroup::Power => "Power",
            CraftGroup::Research => "Research",
        }
    }
}

/// Where a recipe can be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Maker {
    /// In the robot's hands (hand crafting).
    Hand,
    /// In a building of this type.
    Building(BuildingKindId),
}

/// One recipe.
#[derive(Debug, Clone, PartialEq)]
pub struct RecipeView {
    pub id: RecipeId,
    pub name: String,
    pub group: CraftGroup,
    /// Row inside the crafting tab. Recipes with the same row number are on one line.
    pub row: u8,
    pub ingredients: Vec<ItemAmount>,
    pub results: Vec<ItemAmount>,
    /// Crafting time in seconds at speed 1.
    pub time: f32,
    /// Where it can be made. `Maker::Hand` means the player can craft it in the character screen.
    pub made_in: Vec<Maker>,
    /// False if research has not unlocked it yet. Locked recipes are not shown.
    pub unlocked: bool,
    /// Needs this temperature (°C) or more, for heat recipes. Shown in the tooltip.
    pub min_temperature: Option<f32>,
}

impl RecipeView {
    pub fn hand_craftable(&self) -> bool {
        self.made_in.contains(&Maker::Hand)
    }

    /// The item whose icon shows the recipe: the first result.
    pub fn main_item(&self) -> Option<ItemId> {
        self.results.first().map(|r| r.item)
    }
}

/// All items and recipes. The game builds it once and changes `revision` when the data changes
/// (for example after a data reload or a discovery). The UI rebuilds the icon atlas when
/// `revision` changes.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub revision: u64,
    items: Vec<ItemInfo>,
    item_index: HashMap<ItemId, usize>,
    recipes: Vec<RecipeView>,
    recipe_index: HashMap<RecipeId, usize>,
    used_in: HashMap<ItemId, Vec<RecipeId>>,
    made_by: HashMap<ItemId, Vec<RecipeId>>,
}

impl Catalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or replace an item.
    pub fn add_item(&mut self, info: ItemInfo) {
        if let Some(&i) = self.item_index.get(&info.id) {
            self.items[i] = info;
        } else {
            self.item_index.insert(info.id, self.items.len());
            self.items.push(info);
        }
    }

    /// Add or replace a recipe.
    pub fn add_recipe(&mut self, recipe: RecipeView) {
        if let Some(&i) = self.recipe_index.get(&recipe.id) {
            self.recipes[i] = recipe;
            self.rebuild_uses();
        } else {
            for ing in &recipe.ingredients {
                self.used_in.entry(ing.item).or_default().push(recipe.id);
            }
            for res in &recipe.results {
                self.made_by.entry(res.item).or_default().push(recipe.id);
            }
            self.recipe_index.insert(recipe.id, self.recipes.len());
            self.recipes.push(recipe);
        }
    }

    fn rebuild_uses(&mut self) {
        self.used_in.clear();
        self.made_by.clear();
        for r in &self.recipes {
            for ing in &r.ingredients {
                self.used_in.entry(ing.item).or_default().push(r.id);
            }
            for res in &r.results {
                self.made_by.entry(res.item).or_default().push(r.id);
            }
        }
    }

    pub fn item(&self, id: ItemId) -> Option<&ItemInfo> {
        self.item_index.get(&id).map(|&i| &self.items[i])
    }

    /// The name of an item, or "Unknown item".
    pub fn name(&self, id: ItemId) -> &str {
        self.item(id).map(|i| i.name.as_str()).unwrap_or("Unknown item")
    }

    /// The most pieces of this item in one part slot.
    pub fn stack_size(&self, id: ItemId) -> u32 {
        self.item(id).map(|i| i.stack_size.max(1)).unwrap_or(50)
    }

    pub fn items(&self) -> &[ItemInfo] {
        &self.items
    }

    pub fn recipe(&self, id: RecipeId) -> Option<&RecipeView> {
        self.recipe_index.get(&id).map(|&i| &self.recipes[i])
    }

    pub fn recipes(&self) -> &[RecipeView] {
        &self.recipes
    }

    /// Recipes that use this item as an ingredient.
    pub fn used_in(&self, id: ItemId) -> &[RecipeId] {
        self.used_in.get(&id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Recipes that make this item.
    pub fn made_by(&self, id: ItemId) -> &[RecipeId] {
        self.made_by.get(&id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// The item form of a building type, if the catalog has one.
    pub fn building_item(&self, kind: BuildingKindId) -> Option<&ItemInfo> {
        self.item(ItemId::Building(kind))
    }
}
