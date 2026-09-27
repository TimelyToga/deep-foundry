//! `UiAction`: everything the player can ask for through the UI.
//!
//! The game turns these into simulation commands (or handles them itself, for example
//! menus and saves). The UI never changes game state directly.

use foundry_content::ItemRef;
use crate::model::BuildingSlots;
use foundry_core::{BuildingId, RecipeId};

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

    // ----- Crafting -----
    /// Add hand crafting jobs to the queue.
    Craft { recipe: RecipeId, count: u32 },
    /// Remove runs from a job in the crafting queue. `count` is at most the job count.
    CancelCraft { index: usize, count: u32 },

    // ----- Buildings -----
    /// Change the recipe of a building. `None` clears it.
    SetRecipe { building: BuildingId, recipe: Option<RecipeId> },

    // ----- Alerts -----
    /// Show the place of an alert (by `AlertView::id`).
    ShowAlert(u32),

    // ----- Game flow -----
    NewGame { seed: u64, size: WorldSize },
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
