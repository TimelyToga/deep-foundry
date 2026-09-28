//! Keep or drop for dug materials (normal mode).
//!
//! - Each tank slot (HUD and inventory) has a small button in its top-left corner: a green check
//!   (keep) or a red arrow down (drop). One click changes it (`UiAction::SetKeep`).
//! - The character screen has the list of known materials with the same buttons.
//!
//! "Keep": dug units of the material go into the tanks. "Drop": the robot throws them out behind
//! itself as loose material.

use super::Cx;
use crate::action::UiAction;
use crate::item;
use crate::theme::{self, color, font_regular, size, text};
use crate::tooltip::Tip;
use crate::widgets::{self, SlotContent, SlotLook};
use egui::{Align2, Color32, CornerRadius, Id, Painter, Rect, Sense, Shape, Stroke, Ui, Vec2, pos2, vec2};
use foundry_content::ItemRef;
use foundry_core::MaterialId;

/// Size of the keep or drop button in a slot corner.
pub const MARK: f32 = 16.0;
/// Slots per row of the list.
const COLS: usize = 10;
/// Height of the heading above the list.
const HEAD: f32 = 28.0;
/// Height of the key line under the list.
const LEGEND_H: f32 = 24.0;

/// Draw the keep (green check) or drop (red arrow down) mark in `r`.
pub fn paint_mark(p: &Painter, r: Rect, keep: bool, hovered: bool) {
    let (fill, edge) = if keep { (Color32::from_rgb(34, 70, 32), color::GREEN) } else { (Color32::from_rgb(150, 36, 28), color::RED) };
    let fill = if hovered { theme::shade(fill, 1.5) } else { fill };
    p.rect_filled(r, CornerRadius::same(2), fill);
    p.rect_stroke(r, CornerRadius::same(2), Stroke::new(1.0, if hovered { color::ORANGE } else { edge }), egui::StrokeKind::Inside);
    if keep {
        widgets::check_mark(p, r.shrink(3.0), color::GREEN);
    } else {
        // An arrow down onto a ground line: "thrown out".
        let c = r.center();
        let w = r.width();
        let ink = Stroke::new(1.6, Color32::WHITE);
        p.line_segment([pos2(c.x, r.top() + 2.5), pos2(c.x, r.bottom() - 5.5)], ink);
        p.add(Shape::convex_polygon(
            vec![pos2(c.x - w * 0.26, r.bottom() - 7.0), pos2(c.x + w * 0.26, r.bottom() - 7.0), pos2(c.x, r.bottom() - 3.5)],
            Color32::WHITE,
            Stroke::NONE,
        ));
        p.hline(r.left() + 2.5..=r.right() - 2.5, r.bottom() - 2.5, ink);
    }
}

/// The tooltip text for a material's setting.
fn tip(cx: &Cx, m: MaterialId, keep: bool) -> Tip {
    let name = item::name(&cx.model.content, ItemRef::Material(m)).to_string();
    let body = if keep {
        format!("Keep: dug {} goes into your tanks. Click to drop it instead: the robot then throws it out behind itself.", name.to_lowercase())
    } else {
        format!("Drop: the robot throws dug {} out behind itself. Click to keep it in your tanks instead.", name.to_lowercase())
    };
    Tip::Text { title: format!("{name}: {}", if keep { "keep" } else { "drop" }), body }
}

/// The keep or drop button in the top-left corner of a tank slot `slot`. Draw it after the
/// slot, so it gets the click. Nothing for an empty tank or a material with no setting.
pub(crate) fn corner_button(ui: &Ui, cx: &mut Cx, id: Id, slot: Rect, m: Option<MaterialId>) {
    let Some(m) = m else { return };
    let Some(keep) = cx.model.player.keeps(m) else { return };
    let r = Rect::from_min_size(slot.min + vec2(1.0, 1.0), Vec2::splat(MARK));
    let resp = ui.interact(r, id, Sense::click());
    let label = if keep { "Keep" } else { "Drop" };
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("{label} {}", item::name(&cx.model.content, ItemRef::Material(m)))));
    paint_mark(ui.painter(), r, keep, resp.hovered());
    if resp.hovered() {
        cx.tip(tip(cx, m, keep));
    }
    if resp.clicked() {
        cx.act(UiAction::SetKeep { material: m, keep: !keep });
    }
}

/// The height of the list in the character screen (0 when there is no list).
pub fn list_height(cx: &Cx) -> f32 {
    let n = cx.model.player.dig.len();
    if n == 0 {
        return 0.0;
    }
    10.0 + HEAD + n.div_ceil(COLS) as f32 * size::SLOT + 4.0 + LEGEND_H
}

/// The list of known materials with their keep or drop button, in `r` (see `list_height`).
pub(crate) fn list(ui: &Ui, cx: &mut Cx, r: Rect) {
    let rules = cx.model.player.dig.clone();
    if rules.is_empty() {
        return;
    }
    let p = ui.painter().clone();
    let top = r.top() + 10.0;
    widgets::heading(&p, pos2(r.left(), top + 4.0), "Digging: keep or drop");
    p.text(pos2(r.right(), top + 14.0), Align2::RIGHT_CENTER, "Click a material to change it", font_regular(text::SMALL), color::TEXT_FAINT);
    let rows = rules.len().div_ceil(COLS);
    let grid = Rect::from_min_size(pos2(r.left(), top + HEAD), vec2(COLS as f32 * size::SLOT + 4.0, rows as f32 * size::SLOT + 4.0));
    widgets::deep(&p, grid);
    for (i, rule) in rules.iter().enumerate() {
        let sr = Rect::from_min_size(grid.min + vec2(2.0 + (i % COLS) as f32 * size::SLOT, 2.0 + (i / COLS) as f32 * size::SLOT), Vec2::splat(size::SLOT));
        let it = ItemRef::Material(rule.material);
        let content = SlotContent { item: Some(it), dim: !rule.keep, ..Default::default() };
        let look = if rule.keep { SlotLook::Normal } else { SlotLook::Dark };
        let resp = widgets::slot(ui, Id::new(("dig-rule", i)), sr, look, &content, cx.atlas);
        let label = if rule.keep { "Keep" } else { "Drop" };
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Dig list {label} {}", item::name(&cx.model.content, it))));
        paint_mark(&p, Rect::from_min_size(sr.min + vec2(1.0, 1.0), Vec2::splat(MARK)), rule.keep, resp.hovered());
        if resp.hovered() {
            cx.tip(tip(cx, rule.material, rule.keep));
        }
        if resp.clicked() || resp.secondary_clicked() {
            cx.act(UiAction::SetKeep { material: rule.material, keep: !rule.keep });
        }
    }
    // The key: what the two marks mean.
    let y = grid.bottom() + 4.0 + LEGEND_H * 0.5;
    let mut x = r.left();
    for (keep, words) in [(true, "Keep: into the tanks"), (false, "Drop: thrown out behind the robot")] {
        paint_mark(&p, Rect::from_min_size(pos2(x, y - MARK * 0.5), Vec2::splat(MARK)), keep, false);
        let t = p.text(pos2(x + MARK + 6.0, y), Align2::LEFT_CENTER, words, font_regular(text::SMALL), color::TEXT_DIM);
        x = t.right() + 18.0;
    }
}
