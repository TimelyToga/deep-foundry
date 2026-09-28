//! Game content from data files in `assets/data/`.
//!
//! - `defs`: the types in the RON files (string ids, optional fields).
//! - `table`: flat lookup arrays indexed by `MaterialId`. Hot code reads only these.
//! - `load`: reads the files, checks them, and builds the tables.
//!
//! Rules for changes: you may add new optional fields (with `#[serde(default)]`).
//! Do not rename or remove fields without an entry in `docs/design/interface-requests.md`.

pub mod defs;
pub mod factory;
pub mod factory_defs;
pub mod load;
pub mod table;

pub use defs::{AltDef, BurnDef, Grain, MaterialDef, Phase, PhaseChange, REACTION_EVENTS, ReactionDef, TimerDef};
pub use factory::{Building, FactoryContent, ItemRef, Milestone, Part, Port, Recipe, Stack, Tech};
pub use factory_defs::{Layer, PortKind, PowerDef, Side};
pub use load::{ContentError, default_assets_dir};
pub use table::{Alt, Burn, Change, MAX_BURN_GASES, Matcher, MaterialTable, OwnChange, Reaction, TagTable, Timer};

use foundry_core::MaterialId;

/// All loaded content. Share it between threads with `Arc<Content>`.
#[derive(Debug, Clone)]
pub struct Content {
    pub materials: MaterialTable,
    pub reactions: Vec<Reaction>,
    pub tags: TagTable,
    /// Parts, buildings, recipes, technologies and milestones.
    pub factory: FactoryContent,
}

impl Content {
    /// The id of a material, by its string id.
    pub fn material(&self, id: &str) -> Option<MaterialId> {
        self.materials.find(id)
    }

    /// An item (material or part) by its string id.
    pub fn item(&self, id: &str) -> Option<ItemRef> {
        self.material(id).map(ItemRef::Material).or_else(|| self.factory.part(id).map(ItemRef::Part))
    }

    /// The name shown to the player for an item.
    pub fn item_name(&self, item: ItemRef) -> &str {
        match item {
            ItemRef::Material(m) => &self.materials.names[m.index()],
            ItemRef::Part(p) => &self.factory.part_def(p).name,
        }
    }

    /// The id of a material. Panics if it does not exist. Use it in tests and for materials the code needs.
    pub fn expect_material(&self, id: &str) -> MaterialId {
        self.material(id).unwrap_or_else(|| panic!("material `{id}` is not in the data files"))
    }
}
