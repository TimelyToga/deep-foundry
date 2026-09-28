//! The Deep Foundry user interface (egui), in the style of Factorio.
//!
//! The UI is a function of a read-only [`UiModel`] and its own small state (which windows are
//! open, where they are, the search text). Each frame the game calls:
//!
//! ```ignore
//! let actions: Vec<UiAction> = ui.show(ctx, &model);
//! ```
//!
//! and then turns the [`UiAction`] values into simulation commands. See `docs/design/ui.md`.
//!
//! This crate does not depend on the renderer, the game or the simulation.

pub mod action;
pub mod crafting;
pub mod format;
pub mod graph;
pub mod icons;
pub mod item;
pub mod mock;
pub mod model;
pub mod slots;
pub mod theme;
pub mod tooltip;
pub mod widgets;

mod screens;

pub use action::{ClickButton, GameMode, SettingChange, SlotClick, SlotRef, UiAction, WindowKind, WorldSize};
pub use foundry_content::{ItemRef, Stack};
pub use item::{CraftGroup, ItemKind, Maker};
pub use model::*;

use crafting::Stock;
use foundry_core::BuildingId;
use graph::TimeRange;
use icons::IconAtlas;
use std::collections::HashMap;
use tooltip::Tip;

/// The pages of the main menu and the pause menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MenuPage {
    /// The list of buttons.
    #[default]
    Root,
    NewGame,
    Save,
    Load,
    Settings,
}

/// A yes/no question on top of a menu.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Confirm {
    /// Replace the save with this name.
    Overwrite(String),
    /// Delete the save with this id.
    Delete(String),
}

/// State of the menus.
#[derive(Debug, Clone, Default)]
pub(crate) struct MenuState {
    pub page: MenuPage,
    pub confirm: Option<Confirm>,
    pub seed_text: String,
    pub world_size: WorldSize,
    pub mode: GameMode,
    pub save_name: String,
    pub selected_save: Option<String>,
}

/// The state that belongs to the UI (not to the game).
#[derive(Debug, Clone)]
pub(crate) struct UiState {
    /// Open windows, the top one last. Esc closes the last one.
    pub stack: Vec<WindowKind>,
    /// How far the player moved each window from its default place.
    pub offsets: HashMap<WindowKind, egui::Vec2>,
    pub craft_tab: CraftGroup,
    pub search: String,
    /// The recipe selector of the building window is open.
    pub picker_open: bool,
    pub power_range: TimeRange,
    pub stats_range: TimeRange,
    pub menu: MenuState,
    /// `None` before the first frame.
    pub last_state: Option<GameState>,
    pub last_building: Option<BuildingId>,
    pub last_power: Option<u32>,
    /// The main menu background, built once for each screen size.
    pub menu_background: Option<(egui::Rect, std::sync::Arc<egui::Mesh>)>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            stack: vec![],
            offsets: HashMap::new(),
            craft_tab: CraftGroup::Intermediate,
            search: String::new(),
            picker_open: false,
            power_range: TimeRange::Minutes1,
            stats_range: TimeRange::Minutes10,
            menu: MenuState::default(),
            last_state: None,
            last_building: None,
            last_power: None,
            menu_background: None,
        }
    }
}

/// The windows that replace each other, as in Factorio: only one of them is open at a time.
/// A building window closes them.
pub(crate) const MAIN_WINDOWS: [WindowKind; 4] = [WindowKind::Character, WindowKind::Production, WindowKind::Research, WindowKind::Guide];

impl UiState {
    /// Open a window that the UI owns. The main windows (`MAIN_WINDOWS`) replace each other.
    /// Sends `CloseWindow` for each window that it closes.
    pub fn open(&mut self, kind: WindowKind, actions: &mut Vec<UiAction>) {
        if matches!(kind, WindowKind::Building | WindowKind::PowerNetwork) {
            return;
        }
        for other in MAIN_WINDOWS {
            if other != kind && self.is_open(other) {
                self.close(other, actions);
            }
        }
        self.raise(kind);
    }

    /// Close a window and tell the game.
    pub fn close(&mut self, kind: WindowKind, actions: &mut Vec<UiAction>) {
        if kind == WindowKind::Building {
            self.picker_open = false;
        }
        self.stack.retain(|k| *k != kind);
        actions.push(UiAction::CloseWindow(kind));
    }

    /// Open a window (and send `OpenWindow`), or close it if it is open.
    pub fn toggle(&mut self, kind: WindowKind, actions: &mut Vec<UiAction>) {
        if self.is_open(kind) {
            self.close(kind, actions);
        } else {
            self.open(kind, actions);
            actions.push(UiAction::OpenWindow(kind));
        }
    }

    /// Put a window on top.
    pub fn raise(&mut self, kind: WindowKind) {
        self.stack.retain(|k| *k != kind);
        self.stack.push(kind);
    }

    pub fn is_open(&self, kind: WindowKind) -> bool {
        self.stack.contains(&kind)
    }

    /// How far the player moved a window.
    pub fn offset(&self, kind: WindowKind) -> egui::Vec2 {
        self.offsets.get(&kind).copied().unwrap_or(egui::Vec2::ZERO)
    }

    pub fn move_window(&mut self, kind: WindowKind, delta: egui::Vec2) {
        if delta != egui::Vec2::ZERO {
            *self.offsets.entry(kind).or_default() += delta;
        }
    }
}

/// The game UI. Make one with [`FoundryUi::new`] and call [`FoundryUi::show`] each frame.
pub struct FoundryUi {
    atlas: Option<IconAtlas>,
    state: UiState,
    stock: Stock,
    actions: Vec<UiAction>,
}

/// The name the design documents use for the UI type.
pub type Ui = FoundryUi;

impl FoundryUi {
    /// Install the fonts and the style into `ctx`. Call once, before the first `show`.
    pub fn new(ctx: &egui::Context) -> Self {
        theme::install(ctx);
        Self { atlas: None, state: UiState::default(), stock: Stock::default(), actions: vec![] }
    }

    /// Draw the UI for one frame and return what the player asked for.
    pub fn show(&mut self, ctx: &egui::Context, model: &UiModel) -> Vec<UiAction> {
        // New fonts become active at the start of the next frame. Draw nothing until then.
        if !ctx.fonts(|f| f.definitions().families.contains_key(&theme::bold())) {
            theme::install(ctx);
            ctx.request_repaint();
            return vec![];
        }
        if self.atlas.as_ref().is_none_or(|a| !a.is_for(&model.content)) {
            self.atlas = Some(IconAtlas::build(ctx, &model.content));
        }
        let scale = model.settings.ui_scale.clamp(0.75, 2.0);
        if (ctx.zoom_factor() - scale).abs() > 0.001 {
            ctx.set_zoom_factor(scale);
        }
        self.stock.fill(&model.player);
        self.sync_with_model(model);
        self.handle_keys(ctx, model);

        let atlas = self.atlas.as_ref().expect("atlas is built above");
        let mut tip: Option<Tip> = None;
        {
            let mut cx = screens::Cx { ctx, model, atlas, stock: &self.stock, actions: &mut self.actions, tip: &mut tip };
            screens::show_all(&mut cx, &mut self.state);
        }
        if let Some(t) = &tip {
            tooltip::show(ctx, t, model, &self.stock, atlas);
        }
        screens::hand::show(ctx, model, atlas);
        std::mem::take(&mut self.actions)
    }

    /// The item icons (after the first `show`), so the game can draw icons over the world.
    pub fn atlas(&self) -> Option<&IconAtlas> {
        self.atlas.as_ref()
    }

    /// Open a window that the UI owns (`Character`, `Production`, `Research` or `Guide`).
    /// `Building` and `PowerNetwork` open when the game fills `UiModel::building` or `UiModel::power`.
    pub fn open_window(&mut self, kind: WindowKind) {
        self.state.open(kind, &mut self.actions);
    }

    /// True if the window is open.
    pub fn is_open(&self, kind: WindowKind) -> bool {
        self.state.is_open(kind)
    }

    /// Show a page of the main menu or the pause menu.
    pub fn open_menu(&mut self, page: MenuPage) {
        self.state.menu.page = page;
        self.state.menu.confirm = None;
    }

    /// Open the recipe selector of the building window (for tests and previews).
    pub fn open_recipe_picker(&mut self) {
        self.state.picker_open = true;
    }

    /// Select a crafting tab (for tests and previews).
    pub fn select_craft_tab(&mut self, group: CraftGroup) {
        self.state.craft_tab = group;
    }

    /// Set the crafting search text (for tests and previews).
    pub fn set_search(&mut self, text: &str) {
        self.state.search = text.to_string();
    }

    /// Ask for a yes/no confirmation to delete a save (for tests and previews).
    pub fn confirm_delete(&mut self, save_id: &str) {
        self.state.menu.page = MenuPage::Load;
        self.state.menu.selected_save = Some(save_id.to_string());
        self.state.menu.confirm = Some(Confirm::Delete(save_id.to_string()));
    }

    /// Ask for a yes/no confirmation to overwrite a save (for tests and previews).
    pub fn confirm_overwrite(&mut self, name: &str) {
        self.state.menu.page = MenuPage::Save;
        self.state.menu.save_name = name.to_string();
        self.state.menu.confirm = Some(Confirm::Overwrite(name.to_string()));
    }

    /// True if the mouse is over the UI, so the game should not use the click for the world.
    pub fn wants_pointer(ctx: &egui::Context) -> bool {
        ctx.is_pointer_over_egui() || ctx.egui_is_using_pointer()
    }

    /// True if a text field has the keyboard, so the game should ignore key presses.
    pub fn wants_keyboard(ctx: &egui::Context) -> bool {
        ctx.egui_wants_keyboard_input()
    }

    /// Open and close windows when the game changes the model.
    fn sync_with_model(&mut self, model: &UiModel) {
        let st = &mut self.state;
        match st.last_state {
            None => st.last_state = Some(model.state),
            Some(last) if last != model.state => {
                if model.state == GameState::MainMenu {
                    st.stack.clear();
                    st.picker_open = false;
                }
                // A new menu (or the game) starts at its first page.
                st.menu.page = MenuPage::Root;
                st.menu.confirm = None;
                st.last_state = Some(model.state);
            }
            Some(_) => {}
        }
        let building = model.building.as_ref().map(|b| b.id);
        if building != st.last_building {
            st.picker_open = false;
            if building.is_some() {
                // As in Factorio: a building window replaces the character screen and the other
                // main windows.
                for kind in MAIN_WINDOWS {
                    if st.stack.contains(&kind) {
                        self.actions.push(UiAction::CloseWindow(kind));
                    }
                }
                st.stack.retain(|k| !MAIN_WINDOWS.contains(k) && *k != WindowKind::Building);
                st.stack.push(WindowKind::Building);
            } else {
                st.stack.retain(|k| *k != WindowKind::Building);
            }
            st.last_building = building;
        }
        let power = model.power.as_ref().map(|p| p.id);
        if power != st.last_power {
            st.stack.retain(|k| *k != WindowKind::PowerNetwork);
            if power.is_some() {
                st.stack.push(WindowKind::PowerNetwork);
            }
            st.last_power = power;
        }
    }

    fn close(&mut self, kind: WindowKind) {
        self.state.close(kind, &mut self.actions);
    }

    fn toggle(&mut self, kind: WindowKind) {
        self.state.toggle(kind, &mut self.actions);
    }

    fn handle_keys(&mut self, ctx: &egui::Context, model: &UiModel) {
        // A text field has the keyboard: it uses the keys (Esc leaves the field).
        if ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        use egui::Key;
        const DIGITS: [Key; 10] =
            [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9, Key::Num0];
        let (esc, e, shift, digit) = ctx.input(|i| {
            let digit = DIGITS.iter().position(|k| i.key_pressed(*k));
            (i.key_pressed(Key::Escape), i.key_pressed(Key::E), i.modifiers.shift, digit)
        });
        // Windows of the normal game. The sandbox has no statistics, research or guide.
        let normal_keys = [(Key::P, WindowKind::Production), (Key::T, WindowKind::Research), (Key::G, WindowKind::Guide)];
        let normal: Vec<WindowKind> = ctx.input(|i| normal_keys.iter().filter(|(k, _)| i.key_pressed(*k)).map(|(_, w)| *w).collect());
        match model.state {
            GameState::MainMenu | GameState::Paused => {
                if esc {
                    let menu = &mut self.state.menu;
                    if menu.confirm.is_some() {
                        menu.confirm = None;
                    } else if menu.page != MenuPage::Root {
                        menu.page = MenuPage::Root;
                    } else if model.state == GameState::Paused {
                        self.actions.push(UiAction::Resume);
                    }
                }
            }
            GameState::Playing => {
                if esc {
                    if self.state.picker_open {
                        self.state.picker_open = false;
                    } else if let Some(top) = self.state.stack.last().copied() {
                        self.close(top);
                    } else {
                        self.actions.push(UiAction::Pause);
                    }
                }
                if e {
                    // As in Factorio: E closes an open building or network window, else it toggles
                    // the character screen.
                    let game_windows: Vec<WindowKind> =
                        self.state.stack.iter().copied().filter(|k| matches!(k, WindowKind::Building | WindowKind::PowerNetwork)).collect();
                    if game_windows.is_empty() {
                        self.toggle(WindowKind::Character);
                    } else {
                        for k in game_windows {
                            self.close(k);
                        }
                    }
                }
                if model.sandbox.is_none() {
                    for kind in normal {
                        self.toggle(kind);
                    }
                }
                if let Some(d) = digit {
                    self.actions.push(UiAction::SelectHotbar(d + if shift { 10 } else { 0 }));
                }
            }
        }
    }
}
