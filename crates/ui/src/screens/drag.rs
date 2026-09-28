//! Moving items without the inventory window: drag and drop between the HUD bar (the quickbar and
//! the tank panel), the inventory panel and the open building window.
//!
//! A drop becomes the same `UiAction` as a click, so the game applies the transfer rules of
//! `foundry_factory::transfer` (this module has no rules of its own):
//!
//! | Drag from | Drop on | Action |
//! |---|---|---|
//! | A tank | The building window | `ClickSlot(Tank, click)`: the tank moves into the building. |
//! | A quickbar material | The building window | Ctrl + click on a tank of it: from every tank. |
//! | A quickbar part | The building window | Ctrl + click on a slot of it: all of that part. |
//! | An inventory slot | The building window | Shift + click: the stack moves into the building. |
//! | A building slot | The HUD bar or the inventory | Shift + click: it moves back to the robot. |
//! | A tank or an inventory slot | A quickbar slot | `SetHotbar`: the quickbar shows that item. |
//! | A quickbar slot | Another quickbar slot | The two quickbar slots change places. |
//!
//! With a building window open, a click on a quickbar material (or Shift + click on a quickbar
//! part) moves it into the building too (`quickbar_to_building`).

use super::Cx;
use crate::action::{SlotClick, SlotRef, UiAction};
use crate::icons::IconAtlas;
use crate::item;
use crate::model::{BuildingSlots, UiModel};
use crate::theme::{color, font_bold, font_regular, size, text};
use crate::widgets;
use crate::UiState;
use egui::{Color32, CornerRadius, DragAndDrop, Id, LayerId, Order, Rect, Response, Stroke, Vec2, pos2, vec2};
use foundry_content::ItemRef;
use foundry_core::BuildingId;

/// What the player drags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Drag {
    /// A robot tank.
    Tank(usize),
    /// A quickbar slot.
    Quickbar(usize),
    /// A part slot of the robot inventory.
    Inventory(usize),
    /// A slot of the open building.
    Building { building: BuildingId, group: BuildingSlots, index: usize },
}

/// Start a drag of `what` when the slot response starts a drag.
pub(crate) fn source(resp: &Response, what: Drag) {
    resp.dnd_set_drag_payload(what);
}

/// The item of a drag, for the picture at the mouse.
fn item_of(model: &UiModel, d: Drag) -> Option<ItemRef> {
    let p = &model.player;
    match d {
        Drag::Tank(i) => p.tank.get(i)?.material.map(ItemRef::Material),
        Drag::Quickbar(i) => p.hotbar.get(i).copied().flatten(),
        Drag::Inventory(i) => p.inventory.get(i).copied().flatten().map(|s| s.item),
        Drag::Building { group, index, .. } => model.building.as_ref()?.slots(group).get(index)?.stack.map(|s| s.item),
    }
}

/// The drag that the mouse button released over `zone` in this frame, if `accept` takes it.
fn released_on(cx: &Cx, zone: &Response, accept: impl Fn(Drag) -> bool) -> Option<Drag> {
    if !zone.contains_pointer() || !cx.ctx.input(|i| i.pointer.any_released()) {
        return None;
    }
    let d = *DragAndDrop::payload::<Drag>(cx.ctx)?;
    if !accept(d) {
        return None;
    }
    DragAndDrop::clear_payload(cx.ctx);
    Some(d)
}

/// True while the player drags something of the robot that the building window takes.
pub(crate) fn dragging_to_building(ctx: &egui::Context) -> bool {
    DragAndDrop::payload::<Drag>(ctx).is_some_and(|d| !matches!(*d, Drag::Building { .. }))
}

/// True while the player drags a slot of the building.
pub(crate) fn dragging_from_building(ctx: &egui::Context) -> bool {
    DragAndDrop::payload::<Drag>(ctx).is_some_and(|d| matches!(*d, Drag::Building { .. }))
}

/// The building window as a drop zone: things of the robot go into the building.
pub(crate) fn building_zone(cx: &mut Cx, zone: &Response) {
    let Some(d) = released_on(cx, zone, |d| !matches!(d, Drag::Building { .. })) else { return };
    match d {
        Drag::Tank(i) => cx.act(UiAction::ClickSlot { slot: SlotRef::Tank(i), click: SlotClick::LEFT }),
        Drag::Quickbar(i) => {
            quickbar_to_building(cx, i, SlotClick::LEFT);
        }
        Drag::Inventory(i) => cx.act(UiAction::ClickSlot { slot: SlotRef::Inventory(i), click: SlotClick::SHIFT_LEFT }),
        Drag::Building { .. } => {}
    }
}

/// The robot (the HUD bar or the inventory panel) as a drop zone: a building slot moves back to
/// the robot.
pub(crate) fn robot_zone(cx: &mut Cx, zone: &Response) {
    if let Some(Drag::Building { building, group, index }) = released_on(cx, zone, |d| matches!(d, Drag::Building { .. })) {
        cx.act(UiAction::ClickSlot { slot: SlotRef::Building { building, group, index }, click: SlotClick::SHIFT_LEFT });
    }
}

/// Quickbar slot `index` as a drop zone: it shows the dropped item, or two quickbar slots change
/// places.
pub(crate) fn quickbar_zone(cx: &mut Cx, zone: &Response, index: usize) {
    let accept = |d: Drag| matches!(d, Drag::Tank(_) | Drag::Inventory(_)) || matches!(d, Drag::Quickbar(j) if j != index);
    let Some(d) = released_on(cx, zone, accept) else { return };
    let hotbar = &cx.model.player.hotbar;
    let here = hotbar.get(index).copied().flatten();
    match d {
        Drag::Quickbar(j) => {
            let there = hotbar.get(j).copied().flatten();
            cx.act(UiAction::SetHotbar { index, item: there });
            cx.act(UiAction::SetHotbar { index: j, item: here });
        }
        d => {
            if let Some(it) = item_of(cx.model, d) {
                cx.act(UiAction::SetHotbar { index, item: Some(it) });
            }
        }
    }
}

/// Move the item of quickbar slot `i` into the open building: a material from every tank (a
/// right click: half), a part all of it. Uses the click rules of the tank or inventory slot that
/// holds it. Returns false if the robot has none of it.
pub(crate) fn quickbar_to_building(cx: &mut Cx, i: usize, click: SlotClick) -> bool {
    let p = &cx.model.player;
    let Some(it) = p.hotbar.get(i).copied().flatten() else { return false };
    let ctrl = SlotClick { ctrl: true, shift: false, ..click };
    let slot = match it {
        ItemRef::Material(m) => p.tank.iter().position(|t| t.material == Some(m) && t.units > 0).map(SlotRef::Tank),
        ItemRef::Part(_) => p.inventory.iter().position(|s| s.is_some_and(|s| s.item == it)).map(SlotRef::Inventory),
    };
    match slot {
        Some(slot) => {
            cx.act(UiAction::ClickSlot { slot, click: ctrl });
            true
        }
        None => false,
    }
}

/// Draw the dragged item at the mouse, and outline the place where it can go.
pub(crate) fn paint(ctx: &egui::Context, model: &UiModel, atlas: &IconAtlas) {
    let Some(d) = DragAndDrop::payload::<Drag>(ctx) else { return };
    let Some(it) = item_of(model, *d) else { return };
    let Some(pos) = ctx.pointer_hover_pos() else { return };
    let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::new("foundry-drag")));
    let r = Rect::from_center_size(pos, Vec2::splat(size::SLOT));
    painter.rect_filled(r, CornerRadius::same(2), Color32::from_black_alpha(90));
    painter.rect_stroke(r, CornerRadius::same(2), Stroke::new(1.5, color::ORANGE), egui::StrokeKind::Inside);
    atlas.paint(&painter, it, Rect::from_center_size(r.center(), Vec2::splat(size::ICON)), Color32::WHITE);
}

/// An orange frame around a drop zone while the player drags something that it takes.
pub(crate) fn outline(p: &egui::Painter, r: Rect) {
    p.rect_stroke(r.expand(2.0), CornerRadius::same(3), Stroke::new(2.0, color::ORANGE), egui::StrokeKind::Outside);
}

/// The hint the first time a building window opens: how to move items with the HUD bar. It
/// shows above the quickbar `qb` while that first window is open.
pub(crate) fn first_hint(cx: &mut Cx, st: &UiState, qb: Rect) {
    let Some(b) = cx.model.building.as_ref() else { return };
    if st.transfer_hint != Some(b.id) {
        return;
    }
    let name = item::building_name(&cx.model.content, b.kind);
    let title = "Move items with the bar below";
    let body = format!(
        "Click a tank (or a material on the quickbar) to move it into the {name}. Shift + click a slot of the {name} to take it back. You can also drag items between the bar and the window."
    );
    let w = qb.width();
    let body_h = widgets::text_height(cx.ctx, &body, font_regular(text::BODY), w - 20.0);
    let h = 10.0 + 20.0 + body_h + 10.0;
    let rect = Rect::from_min_size(pos2(qb.left(), qb.top() - 8.0 - h), vec2(w, h));
    widgets::area(cx.ctx, Id::new(("hud", "transfer-hint")), Order::Middle, rect, |ui| {
        let resp = ui.interact(rect, Id::new("transfer-hint"), egui::Sense::hover());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, title));
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::same(3), Color32::from_rgba_unmultiplied(40, 34, 20, 240));
        p.rect_stroke(rect, CornerRadius::same(3), Stroke::new(2.0, color::ORANGE), egui::StrokeKind::Inside);
        p.text(rect.min + vec2(10.0, 10.0), egui::Align2::LEFT_TOP, title, font_bold(text::BODY), color::HEADING);
        widgets::wrapped(p, rect.min + vec2(10.0, 30.0), &body, font_regular(text::BODY), color::TEXT, w - 20.0);
    });
}
