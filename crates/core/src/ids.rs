//! Number ids. Data files use string ids; `foundry_content` turns them into these numbers.

use serde::{Deserialize, Serialize};

/// A material. Index into the flat material tables of `foundry_content`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub struct MaterialId(pub u16);

impl MaterialId {
    /// Empty space. The loader always gives air the id 0.
    pub const AIR: MaterialId = MaterialId(0);

    #[inline(always)]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    #[inline(always)]
    pub const fn is_air(self) -> bool {
        self.0 == 0
    }
}

/// A building type (from data). Used from Milestone 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BuildingKindId(pub u16);

/// A recipe (from data). Used from Milestone 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RecipeId(pub u16);

/// One placed building. The generation changes when a slot is reused. Used from Milestone 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BuildingId {
    pub index: u32,
    pub generation: u32,
}
