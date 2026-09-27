//! The item stack in the hand, drawn at the mouse (as in Factorio).

use crate::format;
use crate::icons::IconAtlas;
use crate::model::{GameState, UiModel};
use crate::theme::size;
use crate::widgets;
use egui::{Color32, Id, LayerId, Order, Rect, Vec2};

pub(crate) fn show(ctx: &egui::Context, model: &UiModel, atlas: &IconAtlas) {
    if model.state == GameState::MainMenu {
        return;
    }
    let Some(hand) = model.player.hand else { return };
    let Some(pos) = ctx.pointer_hover_pos() else { return };
    let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::new("foundry-hand")));
    let r = Rect::from_center_size(pos, Vec2::splat(size::SLOT));
    atlas.paint(&painter, hand.item, Rect::from_center_size(r.center(), Vec2::splat(size::ICON)), Color32::WHITE);
    if hand.count > 1 || crate::item::is_bulk(hand.item) {
        widgets::corner_count(&painter, r, &format::count(hand.count as u64));
    }
}
