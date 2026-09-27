//! `UiModel`: everything the UI shows, as plain data.
//!
//! The game owns one `UiModel`. Each frame it updates the model from the newest snapshot
//! and gives it to [`crate::FoundryUi::show`]. The UI never changes the model. It returns
//! [`crate::UiAction`] values instead.
//!
//! Update the model in place (clear and refill the `Vec`s) so that no new memory is needed
//! each frame.

use crate::graph::TimeSeries;
use crate::item::{Catalog, ItemId, ItemStack};
use foundry_core::{BuildingId, CellPos, MaterialId, RecipeId};

/// Where the game is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GameState {
    /// No world is loaded. The UI shows the main menu.
    #[default]
    MainMenu,
    /// A world runs. The UI shows the HUD and the game windows.
    Playing,
    /// The pause menu is open. The simulation does not run.
    Paused,
}

/// The whole model.
#[derive(Debug, Clone, Default)]
pub struct UiModel {
    pub state: GameState,
    /// All items and recipes.
    pub catalog: Catalog,
    pub player: PlayerView,
    /// The cell or building under the mouse, for the entity info panel.
    pub hover: Option<HoverView>,
    /// The research that runs now.
    pub research: Option<ResearchView>,
    pub alerts: Vec<AlertView>,
    /// The building whose window is open. The game sets it when the player clicks a building.
    /// The UI sends `UiAction::CloseWindow(WindowKind::Building)` when the player closes it.
    pub building: Option<BuildingView>,
    /// The power network whose window is open. The game sets it.
    pub power: Option<PowerNetworkView>,
    /// Production statistics. Only needed while the statistics window is open.
    pub stats: ProductionStatsView,
    /// Saved games, newest first.
    pub saves: Vec<SaveInfo>,
    pub settings: Settings,
    /// Frames per second of the renderer. Shown if `settings.show_fps` is on.
    pub fps: f32,
    /// A short line of text at the top center, for example "Game saved". Empty for none.
    pub message: String,
}

/// The player robot and its inventory.
#[derive(Debug, Clone, Default)]
pub struct PlayerView {
    pub hull: f32,
    pub hull_max: f32,
    /// Temperature of the robot in °C.
    pub temperature: f32,
    /// The robot takes damage above this temperature (°C).
    pub heat_limit: f32,
    /// Part slots. `None` is an empty slot. The character screen shows 10 slots per row.
    pub inventory: Vec<Option<ItemStack>>,
    /// Material tank slots.
    pub tank: Vec<TankSlot>,
    /// The item that the mouse holds (the "hand" or cursor stack), as in Factorio.
    pub hand: Option<ItemStack>,
    /// Quickbar slots: 20 slots (2 rows of 10). A slot holds an item type, not items.
    /// The count shown is the number of that item in the inventory.
    pub hotbar: Vec<Option<ItemId>>,
    /// The selected quickbar slot (0 to 19), if any.
    pub selected_hotbar: Option<usize>,
    /// The hand crafting queue. The first job is the one in progress.
    pub crafting: Vec<CraftJobView>,
    /// Hand crafting speed. 1 normally, 2 near a workbench.
    pub craft_speed: f32,
}

/// One slot of the material tank.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TankSlot {
    /// `None` for an empty slot.
    pub material: Option<MaterialId>,
    pub units: u32,
    pub capacity: u32,
}

/// One job in the hand crafting queue.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CraftJobView {
    pub recipe: RecipeId,
    /// How many times the recipe still runs (including the one in progress).
    pub count: u32,
    /// Progress of the current run, 0 to 1. Only the first job in the queue has progress.
    pub progress: f32,
}

/// What is under the mouse in the world.
#[derive(Debug, Clone, PartialEq)]
pub enum HoverView {
    Cell {
        pos: CellPos,
        material: MaterialId,
        /// °C
        temperature: f32,
    },
    Building {
        id: BuildingId,
        /// The building type as an item (for the name and the icon).
        item: ItemId,
        status: MachineStatus,
        recipe: Option<RecipeId>,
        progress: f32,
        /// °C, and the maximum temperature of the building.
        temperature: Option<(f32, f32)>,
        /// Power use in watts, if the building uses power.
        power_w: Option<f64>,
    },
}

/// The research that runs now.
#[derive(Debug, Clone, PartialEq)]
pub struct ResearchView {
    pub name: String,
    /// An item that shows the research (usually the main unlock).
    pub icon: ItemId,
    /// 0 to 1.
    pub progress: f32,
    /// Kits for each research unit.
    pub kits: Vec<ItemStack>,
}

/// The kind of an alert. It sets the icon and the color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertKind {
    Fire,
    Leak,
    MachineStopped,
    LowPower,
    Gas,
    Flood,
    TooHot,
    Damage,
}

impl AlertKind {
    pub fn label(self) -> &'static str {
        match self {
            AlertKind::Fire => "Fire",
            AlertKind::Leak => "Leak",
            AlertKind::MachineStopped => "Machine stopped",
            AlertKind::LowPower => "Low power",
            AlertKind::Gas => "Gas",
            AlertKind::Flood => "Flood",
            AlertKind::TooHot => "Too hot",
            AlertKind::Damage => "Damage",
        }
    }
}

/// One alert. Alerts of the same kind are grouped by the game (see `count`).
#[derive(Debug, Clone, PartialEq)]
pub struct AlertView {
    /// A number the game picks. The UI sends it back in `UiAction::ShowAlert`.
    pub id: u32,
    pub kind: AlertKind,
    /// One short sentence, for example "Steam crusher stopped: no input".
    pub text: String,
    /// How many alerts of this kind there are.
    pub count: u32,
    /// Where it is. A click on the alert shows this place.
    pub pos: Option<CellPos>,
}

/// The status of a machine, as in the design (game design section 12.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MachineStatus {
    Working,
    #[default]
    NoRecipe,
    NoInput,
    OutputFull,
    NoPower,
    LowPower,
    NoFuel,
    TooHot,
    WrongVoltage,
    RoomNotValid,
    Disabled,
}

/// The color of a status dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusColor {
    Green,
    Yellow,
    Red,
    Gray,
}

impl MachineStatus {
    pub fn label(self) -> &'static str {
        match self {
            MachineStatus::Working => "Working",
            MachineStatus::NoRecipe => "No recipe",
            MachineStatus::NoInput => "No input",
            MachineStatus::OutputFull => "Output full",
            MachineStatus::NoPower => "No power",
            MachineStatus::LowPower => "Low power",
            MachineStatus::NoFuel => "No fuel",
            MachineStatus::TooHot => "Too hot",
            MachineStatus::WrongVoltage => "Wrong voltage",
            MachineStatus::RoomNotValid => "Room not valid",
            MachineStatus::Disabled => "Disabled by signal",
        }
    }

    pub fn color(self) -> StatusColor {
        match self {
            MachineStatus::Working => StatusColor::Green,
            MachineStatus::NoInput | MachineStatus::OutputFull | MachineStatus::LowPower | MachineStatus::NoFuel => {
                StatusColor::Yellow
            }
            MachineStatus::NoPower
            | MachineStatus::TooHot
            | MachineStatus::WrongVoltage
            | MachineStatus::RoomNotValid => StatusColor::Red,
            MachineStatus::NoRecipe | MachineStatus::Disabled => StatusColor::Gray,
        }
    }
}

/// A group of part slots in a building.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuildingSlots {
    Input,
    Output,
    Fuel,
}

/// A slot in a building window. `filter` is the item the slot expects (drawn as a faint icon
/// when the slot is empty).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BuildingSlot {
    pub stack: Option<ItemStack>,
    pub filter: Option<ItemId>,
}

/// A material buffer (tank) inside a building, for example the water of a boiler.
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialBuffer {
    /// Short name, for example "Water in" or "Steam out".
    pub label: String,
    pub material: Option<MaterialId>,
    pub units: u32,
    pub capacity: u32,
    /// True if the machine fills it (an output). False if the machine uses it (an input).
    pub output: bool,
}

/// Voltage tiers (game design section 14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Voltage {
    Lv,
    Mv,
    Hv,
    Ev,
}

impl Voltage {
    pub fn label(self) -> &'static str {
        match self {
            Voltage::Lv => "LV",
            Voltage::Mv => "MV",
            Voltage::Hv => "HV",
            Voltage::Ev => "EV",
        }
    }

    pub fn volts(self) -> u32 {
        match self {
            Voltage::Lv => 32,
            Voltage::Mv => 128,
            Voltage::Hv => 512,
            Voltage::Ev => 2048,
        }
    }
}

/// The power use of one building.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PowerUse {
    /// Power use now, in watts.
    pub use_w: f64,
    /// Power use when it works at full speed, in watts.
    pub max_w: f64,
    /// The voltage tier of the building.
    pub voltage: Voltage,
    /// The voltage tier of its network, if it is connected.
    pub network_voltage: Option<Voltage>,
    /// Satisfaction of its network, 0 to 1.
    pub satisfaction: f32,
}

/// The open building window.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildingView {
    pub id: BuildingId,
    /// The building type as an item. The UI takes the name and the icon from the catalog.
    pub item: ItemId,
    pub status: MachineStatus,
    /// Extra words for the status line, for example "needs 1100 °C". Empty for none.
    pub status_detail: String,
    /// The recipe it makes now.
    pub recipe: Option<RecipeId>,
    /// Recipes that this building can make. The recipe selector shows these.
    /// Empty if the building has a fixed job (a boiler, a chest).
    pub recipes: Vec<RecipeId>,
    pub inputs: Vec<BuildingSlot>,
    pub outputs: Vec<BuildingSlot>,
    pub fuel: Vec<BuildingSlot>,
    pub buffers: Vec<MaterialBuffer>,
    /// Progress of the current recipe run, 0 to 1.
    pub progress: f32,
    /// Crafting speed (1 = normal).
    pub speed: f32,
    pub power: Option<PowerUse>,
    /// °C
    pub temperature: Option<f32>,
    /// Maximum temperature of the building in °C. Above it, the building stops.
    pub max_temperature: Option<f32>,
}

impl BuildingView {
    pub fn slots(&self, group: BuildingSlots) -> &[BuildingSlot] {
        match group {
            BuildingSlots::Input => &self.inputs,
            BuildingSlots::Output => &self.outputs,
            BuildingSlots::Fuel => &self.fuel,
        }
    }
}

/// One line in the power network window: all buildings of one type.
#[derive(Debug, Clone, PartialEq)]
pub struct PowerEntry {
    /// The building type as an item.
    pub item: ItemId,
    pub count: u32,
    /// Watts now (production or consumption).
    pub watts: f64,
    /// The history of `watts`, for the graph line of this building type.
    pub history: TimeSeries,
}

/// A warning about a power network.
#[derive(Debug, Clone, PartialEq)]
pub enum PowerWarning {
    /// A cable carries more current than its limit. The cable gets hot.
    CableOverloaded { amps: f32, limit_amps: f32, cable: String },
    /// A building has a lower voltage tier than the network. It will explode.
    WrongVoltage { building: ItemId, building_voltage: Voltage },
    /// Demand is higher than supply.
    NotEnoughPower,
    /// The network has no generators.
    NoGenerators,
}

/// The open power network window (Factorio "electric network info").
#[derive(Debug, Clone, PartialEq)]
pub struct PowerNetworkView {
    /// A number the game picks, shown in the title.
    pub id: u32,
    pub voltage: Voltage,
    /// Satisfaction, 0 to 1.
    pub satisfaction: f32,
    /// What the generators can make now, in watts.
    pub production_w: f64,
    /// Most the generators can make, in watts.
    pub capacity_w: f64,
    /// What the buildings use now, in watts.
    pub consumption_w: f64,
    /// Energy in batteries, in joules.
    pub stored_j: f64,
    pub storage_capacity_j: f64,
    /// Current in the network and the limit of its weakest cable, in amps.
    pub amps: f32,
    pub limit_amps: f32,
    pub producers: Vec<PowerEntry>,
    pub consumers: Vec<PowerEntry>,
    /// Total production and total consumption over time, in watts.
    pub production_history: TimeSeries,
    pub consumption_history: TimeSeries,
    pub warnings: Vec<PowerWarning>,
}

/// One item in the production statistics.
#[derive(Debug, Clone, PartialEq)]
pub struct ProductionRow {
    pub item: ItemId,
    /// Amount made per minute over time.
    pub made: TimeSeries,
    /// Amount used per minute over time.
    pub used: TimeSeries,
}

/// Production statistics (key P).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProductionStatsView {
    pub rows: Vec<ProductionRow>,
}

/// One saved game.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveInfo {
    /// The id the game uses to find the file (for example the file name). Sent back in actions.
    pub id: String,
    /// The name the player gave.
    pub name: String,
    /// Date and time of the save, already formatted, for example "2026-09-27 16:40".
    pub date: String,
    /// Play time in seconds.
    pub play_time_s: u64,
    /// Short text about the world, for example "Seed 1234, 8192 x 8192".
    pub world: String,
}

/// Player settings. The UI shows them and sends `UiAction::ChangeSetting` for changes.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// Size of the UI, 0.75 to 2.0.
    pub ui_scale: f32,
    pub vsync: bool,
    pub show_fps: bool,
    /// (action, key) pairs. Read only for now.
    pub key_bindings: Vec<(String, String)>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { ui_scale: 1.0, vsync: true, show_fps: false, key_bindings: default_key_bindings() }
    }
}

/// The keys of the game, for the settings screen.
pub fn default_key_bindings() -> Vec<(String, String)> {
    [
        ("Move left / right", "A / D"),
        ("Jump, jetpack", "Space"),
        ("Dig", "Left mouse"),
        ("Spray material", "Right mouse"),
        ("Character screen", "E"),
        ("Production statistics", "P"),
        ("Close window / menu", "Esc"),
        ("Quickbar slot 1-10", "1 - 0"),
        ("Quickbar slot 11-20", "Shift + 1 - 0"),
        ("Rotate", "R"),
        ("Flip", "F"),
        ("Pick building under cursor", "Q"),
        ("Remove", "X"),
        ("Back layer view", "Tab"),
        ("Info view", "Alt"),
        ("Undo / redo", "Ctrl + Z / Ctrl + Y"),
        ("Pick up / put down stack", "Left click"),
        ("Take half / put one", "Right click"),
        ("Move stack to other inventory", "Shift + left click"),
        ("Move all of this item", "Ctrl + left click"),
    ]
    .iter()
    .map(|(a, k)| (a.to_string(), k.to_string()))
    .collect()
}
