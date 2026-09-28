//! The player inventory panel: the part slot grid and the material tank slots.
//! The character screen and the building window both show it.
//!
//! In the sandbox mode the grid holds every material with no limit, so it shows no counts.

use super::Cx;
use crate::action::{ClickButton, SlotClick, SlotRef, UiAction};
use crate::format;
use crate::item;
use crate::theme::{color, font_regular, rgba, size, text};
use crate::tooltip::Tip;
use crate::widgets::{self, SlotContent, SlotLook};
use crate::UiState;
use egui::{Align2, Id, Rect, Response, Ui, Vec2, pos2, vec2};
use foundry_content::ItemRef;

/// Slots per row.
pub const COLS: usize = 10;
/// Rows of the inventory grid that show without scrolling.
pub const MAX_ROWS: usize = 8;
/// Height of a small heading above a slot grid.
pub const HEAD: f32 = 28.0;
/// Width of a slot grid with its frame.
pub const GRID_W: f32 = COLS as f32 * size::SLOT + 4.0;
/// Extra width for the scroll bar when the inventory has more than `MAX_ROWS` rows.
const SCROLL_W: f32 = 14.0;
/// Height of the row of trash buttons under the tank slots.
const TRASH_H: f32 = 20.0;
/// Height of the help text under the tanks (two lines).
const HELP_H: f32 = 36.0;

fn rows(n: usize) -> usize {
    n.div_ceil(COLS).max(1)
}

fn inventory_height(n: usize) -> f32 {
    rows(n).min(MAX_ROWS) as f32 * size::SLOT + 4.0
}

fn inventory_width(n: usize) -> f32 {
    GRID_W + if rows(n) > MAX_ROWS { SCROLL_W } else { 0.0 }
}

/// The size of the inventory panel.
pub fn panel_size(cx: &Cx) -> Vec2 {
    let p = &cx.model.player;
    let mut h = HEAD + inventory_height(p.inventory.len());
    if !p.tank.is_empty() {
        h += 10.0 + HEAD + tank_grid_h(p.tank.len()) + HELP_H;
    }
    vec2(inventory_width(p.inventory.len()), h)
}

/// The height of the tank grid: each row has the slots and their trash buttons.
fn tank_grid_h(n: usize) -> f32 {
    rows(n) as f32 * (size::SLOT + TRASH_H) + 4.0
}

/// A click on a slot as a `SlotClick`, or `None`.
pub fn slot_click(cx: &Cx, resp: &Response) -> Option<SlotClick> {
    let (shift, ctrl) = cx.modifiers();
    let button = if resp.clicked() {
        ClickButton::Left
    } else if resp.secondary_clicked() {
        ClickButton::Right
    } else {
        return None;
    };
    Some(SlotClick { button, shift, ctrl })
}

/// Draw the inventory panel in `r`.
pub fn panel(ui: &mut Ui, cx: &mut Cx, st: &mut UiState, r: Rect, hint: &str) {
    let model = cx.model;
    let p = &model.player;
    let sandbox = model.sandbox.is_some();
    let painter = ui.painter().clone();
    widgets::heading(&painter, r.min + vec2(0.0, 4.0), if sandbox { "Materials" } else { "Inventory" });
    let hint = if sandbox && hint.is_empty() { "Click a material to paint with it." } else { hint };
    if !hint.is_empty() {
        painter.text(pos2(r.right(), r.top() + 14.0), Align2::RIGHT_CENTER, hint, font_regular(text::SMALL), color::TEXT_FAINT);
    }
    let n = p.inventory.len();
    let grid = Rect::from_min_size(r.min + vec2(0.0, HEAD), vec2(inventory_width(n), inventory_height(n)));
    widgets::deep(&painter, grid);
    let inner = grid.shrink(2.0);
    let content_h = rows(p.inventory.len()) as f32 * size::SLOT;
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
        egui::ScrollArea::vertical().id_salt("inventory-scroll").auto_shrink([false, false]).show(ui, |ui| {
            let (area, _) = ui.allocate_exact_size(vec2(COLS as f32 * size::SLOT, content_h), egui::Sense::hover());
            for (i, stack) in p.inventory.iter().enumerate() {
                let sr = Rect::from_min_size(
                    area.min + vec2((i % COLS) as f32 * size::SLOT, (i / COLS) as f32 * size::SLOT),
                    Vec2::splat(size::SLOT),
                );
                let count = stack.filter(|s| s.count > 1 && !sandbox).map(|s| format::count(s.count as u64));
                let selected = sandbox && stack.is_some() && stack.map(|s| s.item) == p.hand.map(|h| h.item);
                let content = SlotContent { item: stack.map(|s| s.item), count: count.as_deref(), selected, ..Default::default() };
                let resp = widgets::slot(ui, Id::new(("inv", i)), sr, SlotLook::Normal, &content, cx.atlas);
                if let Some(s) = stack {
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, item::name(&model.content, s.item)));
                    if resp.hovered() {
                        let amount = if sandbox { "No limit (sandbox)".to_string() } else { s.count.to_string() };
                        cx.tip(Tip::Item { item: s.item, amount: Some(amount) });
                    }
                }
                if let Some(click) = slot_click(cx, &resp) {
                    cx.act(UiAction::ClickSlot { slot: SlotRef::Inventory(i), click });
                }
            }
        });
    });

    if p.tank.is_empty() {
        return;
    }
    let tank_top = grid.bottom() + 10.0;
    widgets::heading(&painter, pos2(r.left(), tank_top + 4.0), "Material tanks");
    let used: u64 = p.tank.iter().map(|t| t.units as u64).sum();
    let cap: u64 = p.tank.iter().map(|t| t.capacity as u64).sum();
    painter.text(
        pos2(r.right(), tank_top + 14.0),
        Align2::RIGHT_CENTER,
        format!("{} / {} units", format::count_full(used), format::count_full(cap)),
        font_regular(text::SMALL),
        color::TEXT_DIM,
    );
    let tgrid = Rect::from_min_size(pos2(r.left(), tank_top + HEAD), vec2(GRID_W, tank_grid_h(p.tank.len())));
    widgets::deep(&painter, tgrid);
    let building_open = model.building.is_some();
    for (i, t) in p.tank.iter().enumerate() {
        let (col, row) = (i % COLS, i / COLS);
        let sr = Rect::from_min_size(
            tgrid.min + vec2(2.0 + col as f32 * size::SLOT, 2.0 + row as f32 * (size::SLOT + TRASH_H)),
            Vec2::splat(size::SLOT),
        );
        let it = t.material.map(ItemRef::Material);
        let content = tank_slot_content(model, t);
        let count = it.map(|_| format::count(t.units as u64));
        let content = SlotContent { count: count.as_deref(), ..content };
        let resp = widgets::slot(ui, Id::new(("tank", i)), sr, SlotLook::Dark, &content, cx.atlas);
        if let Some(x) = it {
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Tank {}", item::name(&model.content, x))));
        }
        if resp.hovered() {
            match it {
                Some(x) => {
                    let what = if building_open {
                        "Click: move into the building. Right click: half. Ctrl + click: from every tank."
                    } else {
                        "Click: spray this material (hold the right mouse button)."
                    };
                    cx.tip(Tip::Item { item: x, amount: Some(format!("{} / {} units. {what}", format::count_full(t.units as u64), format::count_full(t.capacity as u64))) })
                }
                None => cx.tip(Tip::Text {
                    title: "Empty tank".into(),
                    body: format!("Holds up to {} units of one material. Dig to fill it.", format::count_full(t.capacity as u64)),
                }),
            }
        }
        if let Some(click) = slot_click(cx, &resp) {
            cx.act(UiAction::ClickSlot { slot: SlotRef::Tank(i), click });
        }
        // The trash button under the slot.
        let tr = Rect::from_min_size(pos2(sr.left() + 2.0, sr.bottom() + 1.0), vec2(size::SLOT - 4.0, TRASH_H - 4.0));
        let trash = super::tank::trash_button(ui, Id::new(("tank-trash", i)), tr, it.is_some());
        trash.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, it.is_some(), format!("Empty tank {}", i + 1)));
        if trash.hovered()
            && let Some((title, body)) = super::tank::trash_tip(cx, i)
        {
            cx.tip(Tip::Text { title, body });
        }
        if trash.clicked() {
            super::tank::ask_empty(cx, st, i);
        }
    }
    let help = if building_open {
        "Click a tank to move it into the building (right click: half). Shift + click a building slot to take it back."
    } else {
        "Click a tank to choose it for the spray tool. The trash button under a tank empties it."
    };
    widgets::wrapped(&painter, pos2(r.left(), tgrid.bottom() + 4.0), help, font_regular(text::SMALL), color::TEXT_FAINT, r.width());
}

/// The look of a tank slot: the material icon and a fill bar in the material color. The spray
/// material has an orange frame.
pub(crate) fn tank_slot_content(model: &crate::model::UiModel, t: &crate::model::TankSlot) -> SlotContent<'static> {
    let it = t.material.map(ItemRef::Material);
    let fill_color = it.map(|x| rgba(item::color(&model.content, x))).unwrap_or(color::GRAY);
    let frac = if t.capacity > 0 { t.units as f32 / t.capacity as f32 } else { 0.0 };
    let selected = t.material.is_some() && t.material == model.player.spray;
    SlotContent { item: it, fill: it.map(|_| (frac, fill_color)), selected, ..Default::default() }
}
