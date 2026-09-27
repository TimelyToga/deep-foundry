//! Plain data for the interface: the building window, ports on a ghost, buffers.
//! The UI copies these into its own model. Nothing here changes the game.

use crate::buildings::PlacedPort;
use crate::inventory::InventoryView;
use crate::machines::Status;
use foundry_content::{Building as BuildingDef, Content, ItemRef, PortKind, Side};
use foundry_core::{BuildingId, BuildingKindId, RecipeId, TilePos};

/// One buffer of a machine (or one kind of item in a hopper or lab).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferView {
    pub item: ItemRef,
    pub name: String,
    pub count: u32,
    pub capacity: u32,
    /// The count one craft uses or makes (0 if it does not apply).
    pub per_craft: u32,
}

impl BufferView {
    pub fn new(content: &Content, item: ItemRef, count: u32, capacity: u32, per_craft: u32) -> Self {
        Self { item, name: content.item_name(item).to_string(), count, capacity, per_craft }
    }
}

/// A port as the player sees it: the world tile it is on and the side it faces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortView {
    pub kind: PortKind,
    pub tile: TilePos,
    pub side: Side,
    /// The name from the data, for example "Tap".
    pub name: Option<String>,
    /// Only these items pass. Empty: the building decides.
    pub filter: Vec<ItemRef>,
}

impl PortView {
    pub fn new(def: &BuildingDef, p: &PlacedPort) -> Self {
        let d = p.def.map(|i| &def.ports[i as usize]);
        Self {
            kind: p.kind,
            tile: p.tile,
            side: p.side,
            name: d.and_then(|d| d.name.clone()),
            filter: d.map(|d| d.filter.clone()).unwrap_or_default(),
        }
    }
}

/// Everything the building window shows.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildingView {
    pub id: BuildingId,
    pub kind: BuildingKindId,
    pub name: String,
    /// The logic kind from the data, for example "crafter", "belt" or "storage".
    pub logic: String,
    /// Top-left tile.
    pub at: TilePos,
    /// Size in tiles after rotation.
    pub size: (u8, u8),
    pub rotation: u8,
    pub flip: bool,
    pub status: Status,
    /// A sentence for the player, for example "Needs 3 more Bronze plate".
    pub reason: String,
    pub recipe: Option<RecipeId>,
    pub recipe_name: Option<String>,
    /// Recipes this building can run (its categories, up to its tier). The UI hides the recipes
    /// that the player does not know yet.
    pub recipes: Vec<RecipeId>,
    /// Machine input buffers, hopper contents or lab kits.
    pub inputs: Vec<BufferView>,
    /// Machine output buffers (outputs, then byproducts).
    pub outputs: Vec<BufferView>,
    /// Slots and tanks of a storage building or the Hub.
    pub inventory: Option<InventoryView>,
    /// Progress of the current craft, 0 to 1.
    pub progress: f32,
    /// Power use in the last tick (W).
    pub power_w: f32,
    /// Average body temperature (°C).
    pub temperature: i16,
    pub max_temp: i16,
    pub hit_points: u32,
    pub max_hit_points: u32,
    pub ports: Vec<PortView>,
}
