//! The types in the factory data files: parts, buildings, recipes, technologies and milestones.
//!
//! Files: `assets/data/parts/*.ron`, `buildings/*.ron`, `recipes/*.ron`, `tech/*.ron`,
//! `milestones/*.ron`. Each file is a list of one entry type.
//!
//! Item ids: a recipe input or output, a port filter and a delivery name an item by its string id.
//! An item is either a material (bulk, counted in units = cells) or a part (counted in pieces).
//! Material ids and part ids share one name space: an id can not be both.
//! Every building is also a part with the same id (the item that places it).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A discrete item, for example a gear, a plate, a circuit or a research kit. RON name `Part(...)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename = "Part", deny_unknown_fields)]
pub struct PartDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Crafting menu tab: "logistics", "production", "intermediate", "power" or "research".
    pub category: String,
    /// Pieces in one inventory slot.
    #[serde(default = "default_stack")]
    pub stack: u16,
    /// What the part is made of. When it melts, burns or breaks, `units` cells of this material
    /// go back into the world.
    #[serde(default)]
    pub material: Option<String>,
    #[serde(default)]
    pub units: u16,
    /// Shape for the generated icon: "gear", "plate", "ingot", "rod", "wire", "pipe", "brick",
    /// "circuit", "kit", "tube", "vial", "sheet", "bolt", "machine", ...
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// The layer a building is in (game design section 10.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Layer {
    /// Solid buildings: cells cannot move through them.
    #[default]
    Front,
    /// Pipes, cables, tubes and signal wires behind the cells.
    Back,
}

/// A side of a tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    Up,
    Down,
    Left,
    Right,
}

/// What a port does (game design section 10.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PortKind {
    /// Takes powder cells that fall or slide into it.
    BulkIn,
    /// Puts powder cells into the world.
    BulkOut,
    /// Takes liquid or gas cells from the world.
    FluidIn,
    /// Puts liquid or gas cells into the world.
    FluidOut,
    /// Connects to a pipe in the back layer.
    Pipe,
    /// Takes parts.
    PartIn,
    /// Gives parts.
    PartOut,
    /// Connects to an item tube in the back layer.
    Tube,
    /// Connects to a power cable.
    Power,
    /// Takes heat from, or gives heat to, the cells on this side.
    Heat,
    /// Releases waste gas into the world.
    Exhaust,
    /// Connects to a signal wire.
    Signal,
}

/// One port of a building.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortDef {
    pub kind: PortKind,
    /// The tile of the building that has the port. (0, 0) is the top-left tile, before rotation.
    pub tile: (u8, u8),
    /// The side of that tile the port faces.
    pub side: Side,
    /// Only these item ids pass. Empty: the building decides (for example by its recipe).
    #[serde(default)]
    pub filter: Vec<String>,
    /// Name shown in the building window, for example "Iron tap".
    #[serde(default)]
    pub name: Option<String>,
}

/// Power use or production of a building.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerDef {
    /// Voltage tier: 0 = no electric power (fire, steam), 1 = LV, 2 = MV, 3 = HV, 4 = EV.
    pub tier: u8,
    /// Watts used while working.
    #[serde(default)]
    pub use_w: f32,
    /// Watts used while idle.
    #[serde(default)]
    pub idle_w: f32,
    /// Watts produced (generators).
    #[serde(default)]
    pub produce_w: f32,
    /// Joules stored (batteries).
    #[serde(default)]
    pub store_j: f32,
    /// Steam machines: steam units used per second while working.
    #[serde(default)]
    pub steam_per_s: f32,
    /// Fuel burners: heat released per second while burning (W).
    #[serde(default)]
    pub burn_w: f32,
}

/// A building type. RON name `Building(...)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename = "Building", deny_unknown_fields)]
pub struct BuildingDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// The logic of the building. The factory code has one system for each kind, for example
    /// "workbench", "crafter", "furnace", "boiler", "crucible", "mold", "hopper", "chute", "belt",
    /// "storage", "tank", "pump", "outlet", "drain", "valve", "pipe", "cable", "lab", "drill",
    /// "generator", "battery", "wall", "room_wall", "room_controller", "room_port", "sensor", "arm",
    /// "splitter", "sorter", "screen", "magnet", "fan", "sluice", "stamp_mill", "bellows", "hub".
    pub kind: String,
    /// Size in tiles (width, height).
    pub size: (u8, u8),
    #[serde(default)]
    pub layer: Layer,
    pub tier: u8,
    /// Material of the body cells.
    pub body: String,
    #[serde(default = "default_hit_points")]
    pub hit_points: u32,
    /// Above this temperature (°C) the building stops; far above it, it takes damage.
    #[serde(default = "default_max_temp")]
    pub max_temp: i16,
    #[serde(default)]
    pub ports: Vec<PortDef>,
    #[serde(default)]
    pub power: Option<PowerDef>,
    /// Recipe categories this building can make, for example ["smelting"].
    #[serde(default)]
    pub crafts: Vec<String>,
    /// Recipe speed factor (1.0 = the recipe time).
    #[serde(default = "one")]
    pub speed: f32,
    /// Numbers for the building's kind, for example "belt_speed", "slots", "capacity", "reach".
    #[serde(default)]
    pub params: BTreeMap<String, f32>,
    /// Crafting menu tab of the building's item. Default: "production".
    #[serde(default)]
    pub category: Option<String>,
    /// Pieces of the building's item in one inventory slot.
    #[serde(default = "default_building_stack")]
    pub stack: u16,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// A byproduct with a chance. RON: `(item: "iron_dust", count: 1, chance: 0.1)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChanceDef {
    pub item: String,
    pub count: u32,
    pub chance: f32,
}

/// A recipe. RON name `Recipe(...)`. Items are `(id, count)` pairs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename = "Recipe", deny_unknown_fields)]
pub struct RecipeDef {
    pub id: String,
    /// Default: the name of the first output.
    #[serde(default)]
    pub name: Option<String>,
    /// The building crafting category that makes it, for example "smelting", "crushing",
    /// "assembling", "casting", "kiln", "alloying". "hand" means only the player makes it.
    pub category: String,
    /// The player can also make it by hand (and faster at a workbench).
    #[serde(default)]
    pub hand: bool,
    pub inputs: Vec<(String, u32)>,
    pub outputs: Vec<(String, u32)>,
    #[serde(default)]
    pub byproducts: Vec<ChanceDef>,
    /// Seconds at speed 1.
    pub time: f32,
    /// Lowest machine tier that can run it (for overclocking, game design section 12.2).
    #[serde(default)]
    pub tier: u8,
    /// The machine or room must be at least this hot (°C).
    #[serde(default)]
    pub min_temp: Option<i16>,
    /// Crafting menu tab. Default: the tab of the first output.
    #[serde(default)]
    pub group: Option<String>,
}

/// A technology. RON name `Tech(...)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename = "Tech", deny_unknown_fields)]
pub struct TechDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub tier: u8,
    /// Technologies that must be done first.
    #[serde(default)]
    pub requires: Vec<String>,
    /// Research kits for one unit, for example [("bronze_kit", 1)].
    #[serde(default)]
    pub kits: Vec<(String, u32)>,
    /// Number of units. Total kits = kits × units.
    #[serde(default = "one_u32")]
    pub units: u32,
    /// Seconds for one unit in a lab at speed 1.
    #[serde(default = "default_unit_time")]
    pub unit_time: f32,
    /// Discoveries needed: material ids to scan, or "reaction:<a>+<b>" to observe.
    #[serde(default)]
    pub discoveries: Vec<String>,
    /// Discovery points needed (points come from scans and first reactions).
    #[serde(default)]
    pub discovery_points: u32,
    /// Recipes this technology unlocks. Recipes that no technology unlocks are known from the start.
    #[serde(default)]
    pub unlocks: Vec<String>,
    /// Other effects, for example ("dig_hardness", 1.0), ("belt_speed", 0.1).
    #[serde(default)]
    pub effects: Vec<(String, f32)>,
}

/// A repair stage of the Hub (game design section 15.1). RON name `Milestone(...)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename = "Milestone", deny_unknown_fields)]
pub struct MilestoneDef {
    pub stage: u8,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Items to deliver to the Hub.
    pub deliver: Vec<(String, u32)>,
    /// The research tier this stage unlocks.
    pub unlocks_tier: u8,
}

fn default_stack() -> u16 {
    100
}

fn default_building_stack() -> u16 {
    50
}

fn default_hit_points() -> u32 {
    100
}

fn default_max_temp() -> i16 {
    200
}

fn default_unit_time() -> f32 {
    10.0
}

fn one() -> f32 {
    1.0
}

fn one_u32() -> u32 {
    1
}
