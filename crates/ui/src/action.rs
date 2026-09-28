//! `UiAction`: everything the player can ask for through the UI.
//!
//! The game turns these into simulation commands (or handles them itself, for example
//! menus and saves). The UI never changes game state directly.

use foundry_content::ItemRef;
use crate::model::BuildingSlots;
use foundry_core::{BuildingId, RecipeId, TechId};

/// The windows the UI can show. Used in `OpenWindow` and `CloseWindow`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowKind {
    /// Inventory and hand crafting (key E).
    Character,
    /// The open building (`UiModel::building`).
    Building,
    /// The open power network (`UiModel::power`).
    PowerNetwork,
    /// Production statistics (key P).
    Production,
    /// Technologies: choose the research (key T). The game fills `UiModel::techs` while it is open.
    Research,
    /// The guide: goals for each tier (key G). The game fills `UiModel::guide`.
    Guide,
}

/// The kind of game that "New game" starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameMode {
    /// The robot, the factory, research and the Hub.
    #[default]
    Normal,
    /// No robot and no factory: paint any material with no limit (like the Factorio cheat mode).
    Sandbox,
}

impl GameMode {
    pub const ALL: [GameMode; 2] = [GameMode::Normal, GameMode::Sandbox];

    pub fn label(self) -> &'static str {
        match self {
            GameMode::Normal => "Normal game",
            GameMode::Sandbox => "Sandbox",
        }
    }
}

/// A slot that the player can click.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlotRef {
    /// A part slot of the player inventory.
    Inventory(usize),
    /// A material tank slot of the player.
    Tank(usize),
    /// A slot of the open building.
    Building { building: BuildingId, group: BuildingSlots, index: usize },
}

impl SlotRef {
    /// True for slots of the player (inventory and tank).
    pub fn is_player(self) -> bool {
        matches!(self, SlotRef::Inventory(_) | SlotRef::Tank(_))
    }
}

/// The mouse button of a slot click.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClickButton {
    Left,
    Right,
}

/// A click on a slot, with the modifier keys. `crate::slots` has the rules for what it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlotClick {
    pub button: ClickButton,
    pub shift: bool,
    pub ctrl: bool,
}

impl SlotClick {
    pub const LEFT: SlotClick = SlotClick { button: ClickButton::Left, shift: false, ctrl: false };
    pub const RIGHT: SlotClick = SlotClick { button: ClickButton::Right, shift: false, ctrl: false };
    pub const SHIFT_LEFT: SlotClick = SlotClick { button: ClickButton::Left, shift: true, ctrl: false };
    pub const SHIFT_RIGHT: SlotClick = SlotClick { button: ClickButton::Right, shift: true, ctrl: false };
    pub const CTRL_LEFT: SlotClick = SlotClick { button: ClickButton::Left, shift: false, ctrl: true };
    pub const CTRL_RIGHT: SlotClick = SlotClick { button: ClickButton::Right, shift: false, ctrl: true };
}

/// World size presets for a new game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WorldSize {
    Small,
    #[default]
    Normal,
    Large,
}

impl WorldSize {
    pub const ALL: [WorldSize; 3] = [WorldSize::Small, WorldSize::Normal, WorldSize::Large];

    /// Width and height in chunks of 64 × 64 cells. These are the sizes of the sandbox world.
    /// The full game (game design section 5.1) will use larger worlds.
    pub fn chunks(self) -> (i32, i32) {
        match self {
            WorldSize::Small => (32, 16),
            WorldSize::Normal => (64, 32),
            WorldSize::Large => (128, 64),
        }
    }

    /// Width and height in cells.
    pub fn cells(self) -> (i32, i32) {
        let (w, h) = self.chunks();
        (w * foundry_core::CHUNK_SIZE, h * foundry_core::CHUNK_SIZE)
    }

    pub fn label(self) -> &'static str {
        match self {
            WorldSize::Small => "Small",
            WorldSize::Normal => "Normal",
            WorldSize::Large => "Large",
        }
    }
}

/// A change to one setting.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingChange {
    UiScale(f32),
    Vsync(bool),
    ShowFps(bool),
    /// Show or hide the debug panel (F3).
    ShowDebug(bool),
    /// A number setting of the simulation (`SimSetting::key`).
    Simulation { key: String, value: f32 },
    /// Match keys by the character that they type (true) or by their position (false).
    KeysByLetter(bool),
    /// The player clicked a row of the Controls list (`KeyRow::id`): the next key press becomes
    /// its key. The same id again stops waiting.
    RebindKey(String),
    /// All keys back to the defaults.
    ResetKeys,
}

/// A key of the UI that the game read with its key bindings (`FoundryUi::press_key`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiKey {
    Character,
    Production,
    Research,
    Guide,
    /// Quickbar slot 0 to 19.
    Quickbar(usize),
}

/// Everything the player can ask for.
#[derive(Debug, Clone, PartialEq)]
pub enum UiAction {
    // ----- Windows -----
    /// A window opened. For `Character` and `Production` this is only information
    /// (for example, the game can start to collect statistics).
    OpenWindow(WindowKind),
    /// The player closed a window. For `Building` and `PowerNetwork` the game must clear
    /// `UiModel::building` or `UiModel::power`.
    CloseWindow(WindowKind),
    /// Show the power network of this building (a click on the power bar of a building window).
    OpenPowerNetwork(BuildingId),

    // ----- Slots and the quickbar -----
    /// A click on a slot. Apply it with the rules in `crate::slots`.
    ClickSlot { slot: SlotRef, click: SlotClick },
    /// Select a quickbar slot (0 to 19). The game puts that item in the hand or the build tool.
    SelectHotbar(usize),
    /// Put an item type in a quickbar slot, or clear it with `None`.
    SetHotbar { index: usize, item: Option<ItemRef> },
    /// Put the item in the hand back into the inventory (a click on empty UI space, or Q).
    ClearHand,
    /// Delete the material in this tank of the robot (the trash button). For a large amount
    /// the UI asks the player first.
    EmptyTank(usize),

    // ----- Crafting -----
    /// Add hand crafting jobs to the queue.
    Craft { recipe: RecipeId, count: u32 },
    /// Remove runs from a job in the crafting queue. `count` is at most the job count.
    CancelCraft { index: usize, count: u32 },

    // ----- Buildings -----
    /// Change the recipe of a building. `None` clears it.
    SetRecipe { building: BuildingId, recipe: Option<RecipeId> },

    // ----- Research -----
    /// Research this technology now. The technologies it needs are queued first.
    StartResearch(TechId),

    // ----- Alerts -----
    /// Show the place of an alert (by `AlertView::id`).
    ShowAlert(u32),

    // ----- Game flow -----
    NewGame { seed: u64, size: WorldSize, mode: GameMode },
    /// Load the newest save.
    Continue,
    /// Open the pause menu and stop the simulation.
    Pause,
    /// Close the pause menu and start the simulation again.
    Resume,
    /// Save the game. `overwrite` is true when the player said yes to replacing a save.
    Save { name: String, overwrite: bool },
    /// Load a save (by `SaveInfo::id`).
    Load(String),
    /// Delete a save (by `SaveInfo::id`). The player already said yes.
    DeleteSave(String),
    QuitToMenu,
    QuitGame,
    ChangeSetting(SettingChange),
}
