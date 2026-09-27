//! Game content from data files in `assets/data/`.
//!
//! - `defs`: the types in the RON files (string ids, optional fields).
//! - `table`: flat lookup arrays indexed by `MaterialId`. Hot code reads only these.
//! - `load`: reads the files, checks them, and builds the tables.
//!
//! Rules for changes: you may add new optional fields (with `#[serde(default)]`).
//! Do not rename or remove fields without an entry in `docs/design/interface-requests.md`.

pub mod defs;
pub mod load;
pub mod table;

pub use defs::{BurnDef, Grain, MaterialDef, Phase, PhaseChange, ReactionDef};
pub use load::{ContentError, default_assets_dir};
pub use table::{Burn, Change, Matcher, MaterialTable, Reaction, TagTable};

use foundry_core::MaterialId;

/// All loaded content. Share it between threads with `Arc<Content>`.
#[derive(Debug, Clone)]
pub struct Content {
    pub materials: MaterialTable,
    pub reactions: Vec<Reaction>,
    pub tags: TagTable,
}

impl Content {
    /// The id of a material, by its string id.
    pub fn material(&self, id: &str) -> Option<MaterialId> {
        self.materials.find(id)
    }

    /// The id of a material. Panics if it does not exist. Use it in tests and for materials the code needs.
    pub fn expect_material(&self, id: &str) -> MaterialId {
        self.material(id).unwrap_or_else(|| panic!("material `{id}` is not in the data files"))
    }
}
