//! Flat lookup tables. Index every `Vec` with `MaterialId::index()`.

use crate::defs::{Grain, Phase};
use foundry_core::MaterialId;
use std::collections::HashMap;

/// A phase change after loading.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Change {
    pub at: i16,
    pub into: MaterialId,
}

/// Most gases one burning material can make (`Burn::gases`).
pub const MAX_BURN_GASES: usize = 4;

/// Burn data after loading.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Burn {
    pub ignite_at: i16,
    pub needs_air: bool,
    pub fire_temp: i16,
    pub chance: f32,
    pub into: MaterialId,
    pub fire: MaterialId,
    pub smoke: Option<MaterialId>,
    pub smoke_chance: f32,
    /// All gases it makes while it burns, with a chance per tick: `smoke` first, then the
    /// `gases` list of the data file. Unused entries are `None`.
    pub gases: [Option<(MaterialId, f32)>; MAX_BURN_GASES],
    /// Charring: what it becomes when it is hot with no air next to it for `char_ticks` ticks.
    pub char_into: Option<MaterialId>,
    pub char_ticks: u32,
}

/// A slow change after a time, after loading.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timer {
    pub ticks: u32,
    pub into: MaterialId,
    /// The time counts only while an air cell is next to it.
    pub needs_air: bool,
}

/// Material properties as flat arrays (structure of arrays).
#[derive(Debug, Clone, Default)]
pub struct MaterialTable {
    pub ids: Vec<String>,
    pub names: Vec<String>,
    pub phase: Vec<Phase>,
    pub density: Vec<f32>,
    pub flow: Vec<u8>,
    /// Chance (out of 65536) that a moving liquid cell keeps its momentum for one more tick.
    pub momentum: Vec<u16>,
    pub splash: Vec<f32>,
    /// Chance (0 to 1) that a liquid cell that can flow sideways waits one tick instead.
    pub viscosity: Vec<f32>,
    pub friction: Vec<f32>,
    pub grain: Vec<Grain>,
    pub hardness: Vec<u8>,
    pub heat_capacity: Vec<f32>,
    pub conductivity: Vec<f32>,
    /// Temperature of new cells (°C).
    pub temperature: Vec<i16>,
    pub melt: Vec<Option<Change>>,
    pub freeze: Vec<Option<Change>>,
    pub boil: Vec<Option<Change>>,
    pub condense: Vec<Option<Change>>,
    pub burn: Vec<Option<Burn>>,
    /// The material itself if it has no broken form.
    pub broken_into: Vec<MaterialId>,
    pub life: Vec<Option<(u8, u8)>>,
    pub decay_into: Vec<MaterialId>,
    pub timer: Vec<Option<Timer>>,
    pub drag_limit: Vec<f32>,
    pub glow: Vec<f32>,
    /// One bit for each tag. See `TagTable`.
    pub tags: Vec<u64>,
    /// 0 = no special behavior. Otherwise an index into `behavior_names`.
    pub behavior: Vec<u16>,
    /// `behavior_names[0]` is "" (none).
    pub behavior_names: Vec<String>,
    /// Colors as RGBA bytes. At least one for each material.
    pub colors: Vec<Vec<[u8; 4]>>,
    pub(crate) by_id: HashMap<String, MaterialId>,
}

impl MaterialTable {
    /// Number of materials.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn find(&self, id: &str) -> Option<MaterialId> {
        self.by_id.get(id).copied()
    }

    /// All material ids in order.
    pub fn all(&self) -> impl Iterator<Item = MaterialId> + '_ {
        (0..self.len() as u16).map(MaterialId)
    }

    #[inline(always)]
    pub fn has_tag(&self, m: MaterialId, tag_bit: u8) -> bool {
        self.tags[m.index()] & (1u64 << tag_bit) != 0
    }

    /// The id of a behavior name, if any material uses it.
    pub fn behavior_id(&self, name: &str) -> Option<u16> {
        self.behavior_names.iter().position(|n| n == name).map(|i| i as u16)
    }
}

/// Tag names and their bit numbers.
#[derive(Debug, Clone, Default)]
pub struct TagTable {
    pub names: Vec<String>,
}

impl TagTable {
    pub fn bit(&self, name: &str) -> Option<u8> {
        self.names.iter().position(|n| n == name).map(|i| i as u8)
    }
}

/// What a reaction input matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Matcher {
    Material(MaterialId),
    /// Any material with this tag bit.
    Tag(u8),
    Any,
}

impl Matcher {
    #[inline]
    pub fn matches(&self, m: MaterialId, table: &MaterialTable) -> bool {
        match *self {
            Matcher::Material(x) => x == m,
            Matcher::Tag(bit) => table.has_tag(m, bit),
            Matcher::Any => true,
        }
    }
}

/// A result that depends on the matched material (the `"$freeze"` words in the data).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnChange {
    Freeze,
    Melt,
    Boil,
    Condense,
    Broken,
}

impl OwnChange {
    /// The word in the data files for this result, for example `"$freeze"`.
    pub fn from_word(word: &str) -> Option<OwnChange> {
        Some(match word {
            "$freeze" => OwnChange::Freeze,
            "$melt" => OwnChange::Melt,
            "$boil" => OwnChange::Boil,
            "$condense" => OwnChange::Condense,
            "$broken" => OwnChange::Broken,
            _ => return None,
        })
    }

    /// The material that `m` becomes, if `m` has this data.
    pub fn of(self, m: MaterialId, table: &MaterialTable) -> Option<MaterialId> {
        let i = m.index();
        match self {
            OwnChange::Freeze => table.freeze[i].map(|c| c.into),
            OwnChange::Melt => table.melt[i].map(|c| c.into),
            OwnChange::Boil => table.boil[i].map(|c| c.into),
            OwnChange::Condense => table.condense[i].map(|c| c.into),
            OwnChange::Broken => (table.broken_into[i] != m).then_some(table.broken_into[i]),
        }
    }
}

/// The second outcome of a reaction, after loading.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Alt {
    pub chance: f32,
    pub into_a: Option<MaterialId>,
    pub into_b: Option<MaterialId>,
}

/// A reaction after loading.
#[derive(Debug, Clone, PartialEq)]
pub struct Reaction {
    pub a: Matcher,
    pub b: Matcher,
    pub chance: f32,
    /// `i16::MIN` if there is no lower limit.
    pub min_temp: i16,
    /// `i16::MAX` if there is no upper limit.
    pub max_temp: i16,
    /// `None` also when `into_a_own` is set.
    pub into_a: Option<MaterialId>,
    pub into_b: Option<MaterialId>,
    /// A result from the matched material's own data (`"$freeze"`, ...), in place of `into_a`.
    pub into_a_own: Option<OwnChange>,
    pub into_b_own: Option<OwnChange>,
    pub heat: i16,
    pub needs_air: bool,
    /// One of `defs::REACTION_EVENTS`.
    pub event: Option<String>,
    /// Extra results (material, chance) that go into free neighbor cells of A.
    pub extra: Vec<(MaterialId, f32)>,
    pub alt: Option<Alt>,
}
