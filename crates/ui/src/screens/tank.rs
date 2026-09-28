//! The robot's material tanks: the trash button that empties a tank, and the yes/no question
//! before a large amount is deleted.

use super::Cx;
use crate::action::UiAction;
use crate::format;
use crate::item;
use crate::theme::{self, color, font, size, text};
use crate::widgets::{self, ButtonKind};
use crate::UiState;
use egui::{Color32, CornerRadius, Id, LayerId, Order, Rect, Response, Sense, Stroke, Ui, Vec2, pos2, vec2};
use foundry_content::ItemRef;

/// Emptying a tank with this many units or more asks the player first.
pub const EMPTY_CONFIRM_UNITS: u32 = 1000;

/// A small button with a trash can icon. `enabled`: the tank has material.
pub(crate) fn trash_button(ui: &Ui, id: Id, r: Rect, enabled: bool) -> Response {
    let resp = ui.interact(r, id, if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let fill = if !enabled {
        color::SLOT_EMPTY
    } else if resp.is_pointer_button_down_on() {
        color::BUTTON_PRESSED
    } else if resp.hovered() {
        theme::shade(color::RED_BUTTON, 0.9)
    } else {
        theme::shade(color::WINDOW, 1.25)
    };
    widgets::raised(p, r, fill, theme::shade(fill, 1.4), theme::shade(fill, 0.6));
    // The can: a lid and a body with two lines.
    let c = r.center();
    let ink = if enabled { color::TEXT } else { color::TEXT_FAINT };
    let stroke = Stroke::new(1.5, ink);
    let h = (r.height() - 6.0).max(6.0);
    let body = Rect::from_center_size(c + vec2(0.0, 1.5), vec2(h * 0.6, h * 0.7));
    p.rect_stroke(body, CornerRadius::same(1), stroke, egui::StrokeKind::Middle);
    p.hline(body.left() - 2.0..=body.right() + 2.0, body.top() - 2.0, stroke);
    p.hline(c.x - 1.5..=c.x + 1.5, body.top() - 4.0, stroke);
    for dx in [-0.18, 0.18] {
        let x = c.x + dx * body.width();
        p.vline(x, body.top() + 2.0..=body.bottom() - 2.0, Stroke::new(1.0, ink));
    }
    resp
}

/// The trash button of tank `i` was clicked: empty a small amount at once, or ask first.
pub(crate) fn ask_empty(cx: &mut Cx, st: &mut UiState, i: usize) {
    let Some(t) = cx.model.player.tank.get(i) else { return };
    if t.material.is_none() {
        return;
    }
    if t.units >= EMPTY_CONFIRM_UNITS {
        st.confirm_empty = Some(i);
    } else {
        cx.act(UiAction::EmptyTank(i));
    }
}

/// The tooltip text of a trash button.
pub(crate) fn trash_tip(cx: &Cx, i: usize) -> Option<(String, String)> {
    let t = cx.model.player.tank.get(i)?;
    let m = t.material?;
    let name = item::name(&cx.model.content, ItemRef::Material(m));
    Some(("Empty this tank".into(), format!("Deletes {} units of {name}. The material is gone.", format::count_full(t.units as u64))))
}

/// The yes/no question before a tank is emptied. One full-screen layer: a dim that takes the
/// clicks, then the window.
pub(crate) fn confirm_empty(cx: &mut Cx, st: &mut UiState) {
    let Some(i) = st.confirm_empty else { return };
    let Some(t) = cx.model.player.tank.get(i).copied() else {
        st.confirm_empty = None;
        return;
    };
    let Some(m) = t.material else {
        st.confirm_empty = None;
        return;
    };
    let name = item::name(&cx.model.content, ItemRef::Material(m)).to_string();
    let screen = cx.ctx.content_rect();
    let id = Id::new("confirm-empty-tank");
    let ctx = cx.ctx;
    let (mut cancel, mut ok) = (false, false);
    widgets::area(ctx, id, Order::Foreground, screen, |ui| {
        ui.painter().rect_filled(screen, CornerRadius::ZERO, Color32::from_black_alpha(110));
        let _ = ui.interact(screen, id.with("dim"), Sense::click());
        let content = vec2(440.0, 70.0 + 12.0 + size::BUTTON_H);
        let rect = widgets::place(screen, widgets::window_outer(content), Vec2::ZERO);
        let f = widgets::window(ui, id.with("window"), rect, "Empty the tank?", false);
        let body = format!("This deletes {} units of {name}. You cannot undo this.", format::count_full(t.units as u64));
        let p = ui.painter();
        let galley = p.layout(body, font(text::BODY), color::TEXT, f.content.width());
        p.galley(f.content.min + vec2(0.0, 8.0), galley, color::TEXT);
        let by = f.content.bottom() - size::BUTTON_H;
        cancel = widgets::button(ui, id.with("cancel"), Rect::from_min_size(pos2(f.content.left(), by), vec2(140.0, size::BUTTON_H)), "Cancel", ButtonKind::Normal, true)
            .clicked();
        ok = widgets::button(ui, id.with("ok"), Rect::from_min_size(pos2(f.content.right() - 140.0, by), vec2(140.0, size::BUTTON_H)), "Empty", ButtonKind::Back, true)
            .clicked();
    });
    ctx.move_to_top(LayerId::new(Order::Foreground, id));
    if ok {
        cx.act(UiAction::EmptyTank(i));
    }
    if ok || cancel {
        st.confirm_empty = None;
    }
}
