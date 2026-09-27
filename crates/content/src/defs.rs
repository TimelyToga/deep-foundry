//! The types in the RON data files.

use serde::{Deserialize, Serialize};

/// How a material moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Phase {
    /// Only air.
    #[default]
    Empty,
    /// Does not move.
    Solid,
    /// Falls and makes piles.
    Powder,
    /// Falls and spreads to the sides.
    Liquid,
    /// Rises or sinks by density and spreads out.
    Gas,
    /// Short life, rises, heats and ignites.
    Fire,
}

/// Grain size of a powder. Screens (sieves) sort by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Grain {
    Small,
    #[default]
    Medium,
    Large,
}

/// A change to another material at a temperature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhaseChange {
    /// Temperature in °C.
    pub at: i16,
    /// The material it becomes.
    pub into: String,
}

/// How a material burns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BurnDef {
    /// It starts to burn at or above this temperature (°C).
    pub ignite_at: i16,
    /// It burns only with air (or oxygen) next to it.
    #[serde(default = "yes")]
    pub needs_air: bool,
    /// Temperature of its fire (°C).
    pub fire_temp: i16,
    /// Chance per tick that a burning cell is used up.
    #[serde(default = "default_burn_chance")]
    pub chance: f32,
    /// What stays when it is used up. Default: air.
    #[serde(default)]
    pub into: Option<String>,
    /// The fire material it makes. Default: "fire".
    #[serde(default)]
    pub fire: Option<String>,
    /// The smoke material it makes.
    #[serde(default)]
    pub smoke: Option<String>,
    /// Chance per tick of smoke while it burns.
    #[serde(default)]
    pub smoke_chance: f32,
}

/// One material. The RON name is `Material(...)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename = "Material", deny_unknown_fields)]
pub struct MaterialDef {
    /// String id, for example "molten_copper". Lower case with underscores.
    pub id: String,
    /// Name shown to the player.
    pub name: String,
    pub phase: Phase,
    /// Colors as "#rrggbb" or "#rrggbbaa". Each cell picks one shade from these.
    pub colors: Vec<String>,
    /// kg/m³. Air is 1.2. Decides what sinks and what floats.
    #[serde(default)]
    pub density: f32,
    /// Liquids: the farthest a cell moves sideways in one tick.
    #[serde(default)]
    pub flow: u8,
    /// Liquids: how long sideways movement lasts (0 to 1). Each tick a moving cell keeps its
    /// momentum with this chance. Water about 0.9 (splashes out and levels fast); lava and mud 0.
    #[serde(default)]
    pub momentum: f32,
    /// Liquids: chance (0 to 1) that a cell that lands fast flies off as a droplet.
    #[serde(default)]
    pub splash: f32,
    /// Powders: chance (0 to 1) that a diagonal slide stops. Higher gives steeper piles.
    #[serde(default)]
    pub friction: f32,
    #[serde(default)]
    pub grain: Grain,
    /// 0 = soft. 255 = cannot break.
    #[serde(default)]
    pub hardness: u8,
    /// Relative to water = 1.0.
    #[serde(default = "one")]
    pub heat_capacity: f32,
    /// 0 (insulator) to 1 (best conductor).
    #[serde(default)]
    pub conductivity: f32,
    /// Temperature of new cells of this material (°C). Default: 20 °C.
    #[serde(default)]
    pub temperature: Option<i16>,
    /// Becomes another material at or above a temperature.
    #[serde(default)]
    pub melt: Option<PhaseChange>,
    /// Becomes another material below a temperature.
    #[serde(default)]
    pub freeze: Option<PhaseChange>,
    /// Becomes a gas at or above a temperature.
    #[serde(default)]
    pub boil: Option<PhaseChange>,
    /// Becomes a liquid below a temperature.
    #[serde(default)]
    pub condense: Option<PhaseChange>,
    #[serde(default)]
    pub burn: Option<BurnDef>,
    /// What a solid becomes when it is dug or blasted.
    #[serde(default)]
    pub broken_into: Option<String>,
    /// Life in ticks (min, max) for materials that fade (smoke, fire). A new cell picks a value in this range.
    #[serde(default)]
    pub life: Option<(u8, u8)>,
    /// What it becomes when its life ends. Default: air.
    #[serde(default)]
    pub decay_into: Option<String>,
    /// Liquids: powders with a lower density than this move with the liquid when it flows sideways.
    #[serde(default)]
    pub drag_limit: f32,
    /// Light it gives (0 to 1), apart from the glow of heat.
    #[serde(default)]
    pub glow: f32,
    /// Tags, for example "metal", "flammable". Reactions can match a tag. At most 64 different tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Name of a special behavior in code, for example "float_up".
    #[serde(default)]
    pub behavior: Option<String>,
    /// Free text for designers.
    #[serde(default)]
    pub note: Option<String>,
}

/// A reaction between two touching cells. The RON name is `Reaction(...)`.
///
/// `a` and `b` are a material id, `"tag:<name>"`, or `"any"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename = "Reaction", deny_unknown_fields)]
pub struct ReactionDef {
    pub a: String,
    pub b: String,
    /// Chance per tick when the two cells touch.
    #[serde(default = "one")]
    pub chance: f32,
    /// The reaction needs cell A at or above this temperature (°C).
    #[serde(default)]
    pub min_temp: Option<i16>,
    /// The reaction needs cell A at or below this temperature (°C).
    #[serde(default)]
    pub max_temp: Option<i16>,
    /// What A becomes. Default: no change.
    #[serde(default)]
    pub into_a: Option<String>,
    /// What B becomes. Default: no change.
    #[serde(default)]
    pub into_b: Option<String>,
    /// Temperature added to both result cells (°C). Can be negative.
    #[serde(default)]
    pub heat: i16,
    /// The reaction needs an air cell next to A.
    #[serde(default)]
    pub needs_air: bool,
    /// An event name, for example "explosion_small".
    #[serde(default)]
    pub event: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

fn one() -> f32 {
    1.0
}

fn yes() -> bool {
    true
}

fn default_burn_chance() -> f32 {
    0.02
}
