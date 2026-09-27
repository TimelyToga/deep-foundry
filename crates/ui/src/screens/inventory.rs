//! The player inventory panel: the part slot grid and the material tank slots.
//! The character screen and the building window both show it.

use super::Cx;
use crate::action::{ClickButton, SlotClick, SlotRef, UiAction};
use crate::format;
use crate::item;
use foundry_content::ItemRef;
use crate::theme::{color, rgba, size, text, font_regular};
use crate::tooltip::Tip;
use crate::widgets::{self, SlotContent, SlotLook};
use egui::{Align2, Id, Rect, Response, Ui, Vec2, pos2, vec2};

/// Slots per row.
pub const COLS: usize = 10;
/// Height of a small heading above a slot grid.
pub const HEAD: f32 = 28.0;
/// Width of a slot grid with its frame.
pub const GRID_W: f32 = COLS as f32 * size::SLOT + 4.0;

fn rows(n: usize) -> usize {
    n.div_ceil(COLS).max(1)
}

/// The size of the inventory panel.
pub fn panel_size(cx: &Cx) -> Vec2 {
    let p = &cx.model.player;
    let inv_h = rows(p.inventory.len()) as f32 * size::SLOT + 4.0;
    let tank_h = rows(p.tank.len()) as f32 * size::SLOT + 4.0;
    vec2(GRID_W, HEAD + inv_h + 10.0 + HEAD + tank_h)
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
pub fn panel(ui: &mut Ui, cx: &mut Cx, r: Rect, hint: &str) {
    let p = &cx.model.player;
    let painter = ui.painter().clone();
    widgets::heading(&painter, r.min + vec2(0.0, 4.0), "Inventory");
    if !hint.is_empty() {
        painter.text(pos2(r.right(), r.top() + 14.0), Align2::RIGHT_CENTER, hint, font_regular(text::SMALL), color::TEXT_FAINT);
    }
    let inv_h = rows(p.inventory.len()) as f32 * size::SLOT + 4.0;
    let grid = Rect::from_min_size(r.min + vec2(0.0, HEAD), vec2(GRID_W, inv_h));
    widgets::deep(&painter, grid);
    for (i, stack) in p.inventory.iter().enumerate() {
        let sr = cell(grid, i);
        let count = stack.filter(|s| s.count > 1).map(|s| format::count(s.count as u64));
        let content = SlotContent { item: stack.map(|s| s.item), count: count.as_deref(), ..Default::default() };
        let resp = widgets::slot(ui, Id::new(("inv", i)), sr, SlotLook::Normal, &content, cx.atlas);
        if let Some(s) = stack {
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, item::name(&cx.model.content, s.item)));
        }
        if let Some(s) = stack
            && resp.hovered()
        {
            cx.tip(Tip::Item { item: s.item, amount: Some(format!("{}", s.count)) });
        }
        if let Some(click) = slot_click(cx, &resp) {
            cx.act(UiAction::ClickSlot { slot: SlotRef::Inventory(i), click });
        }
    }

    let tank_top = grid.bottom() + 10.0;
    widgets::heading(&painter, pos2(r.left(), tank_top + 4.0), "Material tank");
    let used: u64 = p.tank.iter().map(|t| t.units as u64).sum();
    let cap: u64 = p.tank.iter().map(|t| t.capacity as u64).sum();
    painter.text(
        pos2(r.right(), tank_top + 14.0),
        Align2::RIGHT_CENTER,
        format!("{} / {} units", format::count(used), format::count(cap)),
        font_regular(text::SMALL),
        color::TEXT_DIM,
    );
    let tank_h = rows(p.tank.len()) as f32 * size::SLOT + 4.0;
    let tgrid = Rect::from_min_size(pos2(r.left(), tank_top + HEAD), vec2(GRID_W, tank_h));
    widgets::deep(&painter, tgrid);
    for (i, t) in p.tank.iter().enumerate() {
        let sr = cell(tgrid, i);
        let it = t.material.map(ItemRef::Material);
        let fill_color = it.map(|x| rgba(item::color(&cx.model.content, x))).unwrap_or(color::GRAY);
        let frac = if t.capacity > 0 { t.units as f32 / t.capacity as f32 } else { 0.0 };
        let count = it.map(|_| format::count(t.units as u64));
        let content = SlotContent {
            item: it,
            count: count.as_deref(),
            fill: it.map(|_| (frac, fill_color)),
            ..Default::default()
        };
        let resp = widgets::slot(ui, Id::new(("tank", i)), sr, SlotLook::Dark, &content, cx.atlas);
        if resp.hovered() {
            match it {
                Some(x) => cx.tip(Tip::Item { item: x, amount: Some(format!("{} / {} units", t.units, t.capacity)) }),
                None => cx.tip(Tip::Text {
                    title: "Empty tank slot".into(),
                    body: format!("Holds up to {} units of one material. Dig to fill it.", t.capacity),
                }),
            }
        }
        if let Some(click) = slot_click(cx, &resp) {
            cx.act(UiAction::ClickSlot { slot: SlotRef::Tank(i), click });
        }
    }
}

/// The rectangle of slot `i` in a grid frame.
pub fn cell(grid: Rect, i: usize) -> Rect {
    let (col, row) = (i % COLS, i / COLS);
    Rect::from_min_size(grid.min + vec2(2.0 + col as f32 * size::SLOT, 2.0 + row as f32 * size::SLOT), Vec2::splat(size::SLOT))
}
