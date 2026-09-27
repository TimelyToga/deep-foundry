//! The screens: HUD, character screen, building window, power window, statistics, menus.

pub(crate) mod building;
pub(crate) mod character;
pub(crate) mod hand;
pub(crate) mod hud;
pub(crate) mod inventory;
pub(crate) mod menus;
pub(crate) mod power;
pub(crate) mod production;

use crate::action::{UiAction, WindowKind};
use crate::crafting::Stock;
use crate::icons::IconAtlas;
use crate::model::{GameState, UiModel};
use crate::tooltip::Tip;
use crate::UiState;
use egui::{Id, LayerId, Order};

/// What every screen needs.
pub(crate) struct Cx<'a> {
    pub ctx: &'a egui::Context,
    pub model: &'a UiModel,
    pub atlas: &'a IconAtlas,
    pub stock: &'a Stock,
    pub actions: &'a mut Vec<UiAction>,
    pub tip: &'a mut Option<Tip>,
}

impl Cx<'_> {
    pub fn act(&mut self, a: UiAction) {
        self.actions.push(a);
    }

    pub fn tip(&mut self, t: Tip) {
        *self.tip = Some(t);
    }

    /// Shift and Ctrl (Cmd on macOS counts as Ctrl).
    pub fn modifiers(&self) -> (bool, bool) {
        self.ctx.input(|i| (i.modifiers.shift, i.modifiers.ctrl || i.modifiers.command))
    }
}

/// The egui layer id of a window.
pub(crate) fn window_id(kind: WindowKind) -> Id {
    Id::new(("foundry-window", kind))
}

pub(crate) fn show_all(cx: &mut Cx, st: &mut UiState) {
    match cx.model.state {
        GameState::MainMenu => menus::main_menu(cx, st),
        GameState::Playing | GameState::Paused => {
            hud::show(cx, st);
            for kind in st.stack.clone() {
                if !st.is_open(kind) {
                    continue; // Closed by a window drawn before it.
                }
                match kind {
                    WindowKind::Character => character::show(cx, st),
                    WindowKind::Building => building::show(cx, st),
                    WindowKind::PowerNetwork => power::show(cx, st),
                    WindowKind::Production => production::show(cx, st),
                    // Drawn by the research and guide screens (not written yet).
                    WindowKind::Research | WindowKind::Guide => {}
                }
            }
            raise_on_click(cx.ctx, st);
            // Keep the egui layer order the same as the window stack.
            for kind in &st.stack {
                cx.ctx.move_to_top(LayerId::new(Order::Middle, window_id(*kind)));
            }
            if cx.model.state == GameState::Paused {
                menus::pause_menu(cx, st);
            }
        }
    }
    hud::overlay_text(cx);
}

/// A press on a window puts it on top of the stack.
fn raise_on_click(ctx: &egui::Context, st: &mut UiState) {
    let pressed = ctx.input(|i| i.pointer.any_pressed());
    if !pressed {
        return;
    }
    let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) else { return };
    let Some(layer) = ctx.layer_id_at(pos) else { return };
    if let Some(kind) = st.stack.iter().copied().find(|k| layer.id == window_id(*k)) {
        st.raise(kind);
    }
}
