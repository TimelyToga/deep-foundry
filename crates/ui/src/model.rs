//! `UiModel`: everything the UI shows, as plain data.
//!
//! Items, recipes, buildings and technologies are the ids of `foundry_content`
//! ([`ItemRef`], [`Stack`], `RecipeId`, `TechId`, `BuildingKindId`), so the game fills the
//! model from its state without a translation layer. Names, icons and recipes come from
//! `UiModel::content`.
//!
//! The game owns one `UiModel`. Each frame it updates the model from the newest snapshot
//! and gives it to [`crate::FoundryUi::show`]. The UI never changes the model. It returns
//! [`crate::UiAction`] values instead. Update the model in place (clear and refill the `Vec`s)
//! so that no new memory is needed each frame.

use crate::graph::TimeSeries;
use foundry_content::{Content, ItemRef, Stack};
use foundry_core::{BuildingId, BuildingKindId, CellPos, MaterialId, RecipeId, TechId};
use std::sync::Arc;

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
#[derive(Debug, Clone)]
pub struct UiModel {
    /// All materials, parts, buildings, recipes and technologies. When the game reloads the data,
    /// it puts a new `Arc` here; the UI then builds its icons again.
    pub content: Arc<Content>,
    pub state: GameState,
    pub player: PlayerView,
    /// Finished technologies. A recipe that a technology unlocks shows only when it is here.
    pub finished_techs: Vec<TechId>,
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
    /// `Some` in the sandbox mode: every material is in the inventory with no limit, and the
    /// stack in the hand is the paint brush. There is no crafting.
    pub sandbox: Option<SandboxView>,
    /// Performance numbers. When `Some`, the HUD shows them in a small box at the top right.
    pub perf: Option<PerfView>,
    /// All technologies with their state. The game fills it while the research window is open.
    /// Names, descriptions, kits and unlocks come from the content.
    pub techs: Vec<TechEntry>,
    /// Discovery points the player can spend (some technologies cost them).
    pub discovery_points: u32,
    /// The guide goals of the open tiers, in order. The HUD shows the first goals that are not done.
    pub guide: Vec<GuideGoal>,
}

/// The state of a technology in the research window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TechState {
    Done,
    /// The current research.
    Researching,
    /// It can start now.
    Available,
    /// It cannot start now (see `TechEntry::reasons`).
    #[default]
    Locked,
}

/// One technology in the research window.
#[derive(Debug, Clone, PartialEq)]
pub struct TechEntry {
    pub id: TechId,
    pub state: TechState,
    /// 0 to 1. Above 0 also for a technology that started and then stopped.
    pub progress: f32,
    /// Why it cannot start, as sentences for the player. Empty unless `Locked`.
    pub reasons: Vec<String>,
    /// Place in the research queue (0 = next). `None` if it is not queued.
    pub queue_position: Option<usize>,
}

/// One goal of the guide (like a quest in the GTNH quest book).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GuideGoal {
    pub id: String,
    pub tier: u8,
    pub title: String,
    /// A short hint for the player.
    pub text: String,
    pub done: bool,
    /// (have, need) for goals that count something, for example 40 of 64 clay.
    pub count: Option<(u32, u32)>,
    /// Discovery points the goal gives when it is done.
    pub reward_points: u32,
    /// The game cannot do this goal yet: what it still needs (for example "heat"). The guide
    /// shows the goal gray, and the HUD tracker skips it.
    pub waits_for: Option<String>,
}

/// The next repair stage of the Hub, for the Hub window.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MilestoneView {
    pub stage: u8,
    pub name: String,
    pub description: String,
    /// What the stage needs and what the Hub has received so far.
    pub items: Vec<Delivery>,
}

/// One item of a Hub repair stage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Delivery {
    pub item: ItemRef,
    pub delivered: u32,
    pub need: u32,
}

/// The sandbox mode (like the Factorio cheat mode).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SandboxView {
    /// Radius of the paint brush in cells.
    pub brush_radius: u16,
    /// The simulation is paused (with the pause key, not the pause menu).
    pub sim_paused: bool,
}

/// Performance numbers for the HUD.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PerfView {
    pub fps: f32,
    /// Time of one simulation tick in milliseconds.
    pub tick_ms: f32,
    pub ticks_per_second: f32,
    pub awake_chunks: u32,
    pub loaded_chunks: u32,
}

impl UiModel {
    /// An empty model in the main menu.
    pub fn new(content: Arc<Content>) -> Self {
        Self {
            content,
            state: GameState::MainMenu,
            player: PlayerView::default(),
            finished_techs: vec![],
            hover: None,
            research: None,
            alerts: vec![],
            building: None,
            power: None,
            stats: ProductionStatsView::default(),
            saves: vec![],
            settings: Settings::default(),
            fps: 0.0,
            message: String::new(),
            sandbox: None,
            perf: None,
            techs: vec![],
            discovery_points: 0,
            guide: vec![],
        }
    }

    /// True if the player knows the recipe.
    pub fn recipe_known(&self, id: RecipeId) -> bool {
        crate::item::recipe(&self.content, id).is_some_and(|r| crate::item::recipe_known(r, &self.finished_techs))
    }
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
    pub inventory: Vec<Option<Stack>>,
    /// Material tank slots.
    pub tank: Vec<TankSlot>,
    /// The material the spray tool puts out now (right mouse button). The HUD marks its tanks.
    pub spray: Option<MaterialId>,
    /// The dig tool found no room in the tanks a moment ago. The HUD says what to do.
    pub tanks_full: bool,
    /// The stack that the mouse holds (the "hand" or cursor stack), as in Factorio.
    pub hand: Option<Stack>,
    /// Quickbar slots: 20 slots (2 rows of 10). A slot holds an item type, not items.
    /// The count shown is the number of that item in the inventory.
    pub hotbar: Vec<Option<ItemRef>>,
    /// The selected quickbar slot (0 to 19), if any.
    pub selected_hotbar: Option<usize>,
    /// The hand crafting queue. The first job is the one in progress.
    pub crafting: Vec<CraftJobView>,
    /// Hand crafting speed. 1 normally, 2 near a workbench.
    pub craft_speed: f32,
}

impl PlayerView {
    /// The capacity of the first tank slot (the stack size of bulk materials in the hand).
    pub fn tank_capacity(&self) -> u32 {
        self.tank.first().map(|t| t.capacity).unwrap_or(6000)
    }
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
        kind: BuildingKindId,
        status: MachineStatus,
        recipe: Option<RecipeId>,
        progress: f32,
        /// °C now. The maximum comes from the building type.
        temperature: Option<f32>,
        /// Power use in watts, if the building uses electric power.
        power_w: Option<f64>,
    },
}

/// The research that runs now. The name, the kits and the unlocks come from the content.
#[derive(Debug, Clone, PartialEq)]
pub struct ResearchView {
    pub tech: TechId,
    /// 0 to 1.
    pub progress: f32,
}

/// The kind of an alert. It sets the symbol and the color.
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

/// One alert. The game groups alerts of the same kind (see `count`).
#[derive(Debug, Clone, PartialEq)]
pub struct AlertView {
    /// A number the game picks. The UI sends it back in `UiAction::ShowAlert`.
    pub id: u32,
    pub kind: AlertKind,
    /// One short sentence, for example "Steam crusher stopped: output full".
    pub text: String,
    /// How many alerts of this kind there are.
    pub count: u32,
    /// Where it is. A click on the alert shows this place.
    pub pos: Option<CellPos>,
}

/// The status of a machine (game design section 12.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MachineStatus {
    Working,
    /// Nothing to do and nothing is wrong (storage, walls, an empty belt).
    Idle,
    #[default]
    NoRecipe,
    NoInput,
    OutputFull,
    /// An output port has no free cells in front of it.
    OutputBlocked,
    /// The recipe needs a higher temperature.
    TooCold,
    /// Hit points are at 0.
    Broken,
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
            MachineStatus::Idle => "Idle",
            MachineStatus::NoRecipe => "No recipe",
            MachineStatus::OutputBlocked => "Output blocked",
            MachineStatus::TooCold => "Too cold",
            MachineStatus::Broken => "Broken",
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
            MachineStatus::NoInput
            | MachineStatus::OutputFull
            | MachineStatus::OutputBlocked
            | MachineStatus::TooCold
            | MachineStatus::LowPower
            | MachineStatus::NoFuel => StatusColor::Yellow,
            MachineStatus::NoPower
            | MachineStatus::TooHot
            | MachineStatus::WrongVoltage
            | MachineStatus::RoomNotValid
            | MachineStatus::Broken => StatusColor::Red,
            MachineStatus::NoRecipe | MachineStatus::Idle | MachineStatus::Disabled => StatusColor::Gray,
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
    pub stack: Option<Stack>,
    pub filter: Option<ItemRef>,
    /// For a material in a storage slot: the units the slot holds (for the fill bar). 0 for parts.
    pub capacity: u32,
}

/// A material buffer (tank) inside a building, for example the water of a boiler.
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialBuffer {
    /// Short name, for example "Water in" or the port name ("Tap").
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
    /// From `PowerDef::tier` of the content: 1 = LV, 2 = MV, 3 = HV, 4 = EV. 0 (no electric power) gives `None`.
    pub fn from_tier(tier: u8) -> Option<Voltage> {
        match tier {
            1 => Some(Voltage::Lv),
            2 => Some(Voltage::Mv),
            3 => Some(Voltage::Hv),
            4 => Some(Voltage::Ev),
            _ => None,
        }
    }

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

    /// The technology tier (game design section 14) that brings this voltage.
    pub fn game_tier(self) -> u8 {
        match self {
            Voltage::Lv => 2,
            Voltage::Mv => 3,
            Voltage::Hv => 4,
            Voltage::Ev => 5,
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
    /// The building type. The name, the icon, the maximum temperature and the recipes it can
    /// make come from the content.
    pub kind: BuildingKindId,
    pub status: MachineStatus,
    /// Extra words for the status line, for example "needs 1100 °C". Empty for none.
    pub status_detail: String,
    /// The recipe it makes now.
    pub recipe: Option<RecipeId>,
    pub inputs: Vec<BuildingSlot>,
    pub outputs: Vec<BuildingSlot>,
    pub fuel: Vec<BuildingSlot>,
    pub buffers: Vec<MaterialBuffer>,
    /// Progress of the current recipe run, 0 to 1.
    pub progress: f32,
    /// Crafting speed now (1 = the recipe time). It includes overclocking.
    pub speed: f32,
    pub power: Option<PowerUse>,
    /// °C
    pub temperature: Option<f32>,
    /// The Hub: the next repair stage and what it still needs. `None` for other buildings.
    pub milestone: Option<MilestoneView>,
    /// The Hub: the repair stages after the next one and what each needs.
    pub later_stages: Vec<MilestoneView>,
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
    pub kind: BuildingKindId,
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
    CableOverloaded { amps: f32, limit_amps: f32, cable: BuildingKindId },
    /// A building has a lower voltage tier than the network. It will explode.
    WrongVoltage { building: BuildingKindId, building_voltage: Voltage },
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
    /// What the generators make now, in watts.
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
    pub item: ItemRef,
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
    /// Short text about the world, for example "Seed 1234, 8192 × 8192".
    pub world: String,
}

/// Player settings. The UI shows them and sends `UiAction::ChangeSetting` for changes.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// Size of the UI, 0.75 to 2.0.
    pub ui_scale: f32,
    pub vsync: bool,
    pub show_fps: bool,
    /// Show the debug panel (key F3).
    pub show_debug: bool,
    /// Number settings of the simulation (for example the liquid rules). The settings screen
    /// shows one slider for each, in the "Simulation" section. Empty: the section says so.
    pub simulation: Vec<SimSetting>,
    /// (action, key) pairs. Read only for now.
    pub key_bindings: Vec<(String, String)>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ui_scale: 1.0,
            vsync: true,
            show_fps: false,
            show_debug: false,
            simulation: vec![],
            key_bindings: default_key_bindings(),
        }
    }
}

/// One number setting of the simulation. The UI sends
/// `UiAction::ChangeSetting(SettingChange::Simulation { key, value })` when the player moves the slider.
#[derive(Debug, Clone, PartialEq)]
pub struct SimSetting {
    /// The name the game uses, for example "liquid_spread".
    pub key: String,
    /// The name shown to the player, for example "Liquid spread".
    pub label: String,
    /// One short sentence about what it does.
    pub help: String,
    pub value: f32,
    pub min: f32,
    pub max: f32,
    /// Round the value to steps of this size (0: no rounding).
    pub step: f32,
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
        ("Research", "T"),
        ("Guide", "G"),
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

/// Make a stack.
pub fn stack(item: ItemRef, count: u32) -> Stack {
    Stack { item, count }
}
