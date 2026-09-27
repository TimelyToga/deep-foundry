//! The building window (Factorio entity GUI), with the player inventory next to it.
//!
//! Layout from top to bottom: status line, picture, recipe selector, input slots, progress,
//! output slots, fuel slots, material buffers, power bar, temperature bar.

use super::inventory::{self, slot_click};
use super::{Cx, window_id};
use crate::action::{SlotRef, UiAction, WindowKind};
use crate::format;
use crate::item::ItemId;
use crate::model::{BuildingSlots, BuildingView};
use crate::theme::{self, color, font, font_bold, font_regular, size, text};
use crate::tooltip::Tip;
use crate::widgets::{self, ButtonKind, SlotContent, SlotLook};
use crate::UiState;
use egui::{Align2, Color32, CornerRadius, Id, LayerId, Order, Rect, Stroke, Ui, Vec2, pos2, vec2};

const WIDTH: f32 = 452.0;
const STATUS_H: f32 = 30.0;
const PICTURE_H: f32 = 128.0;
const RECIPE_H: f32 = 52.0;
const ROW_H: f32 = 32.0;
const GAP: f32 = 10.0;

fn slots_panel_h(b: &BuildingView) -> f32 {
    let mut h = 12.0 + 20.0 + size::SLOT + 4.0;
    if !b.fuel.is_empty() {
        h += 26.0 + 20.0 + size::SLOT + 4.0;
    }
    h
}

fn has_slots(b: &BuildingView) -> bool {
    !(b.inputs.is_empty() && b.outputs.is_empty() && b.fuel.is_empty())
}

fn content_height(b: &BuildingView) -> f32 {
    let mut h = STATUS_H + PICTURE_H + GAP;
    if !b.recipes.is_empty() || b.recipe.is_some() {
        h += RECIPE_H + GAP;
    }
    if has_slots(b) {
        h += slots_panel_h(b) + GAP;
    }
    if !b.buffers.is_empty() {
        h += 24.0 + b.buffers.len() as f32 * ROW_H + GAP;
    }
    if b.power.is_some() {
        h += ROW_H;
    }
    if b.temperature.is_some() {
        h += ROW_H;
    }
    h
}

pub(crate) fn show(cx: &mut Cx, st: &mut UiState) {
    let Some(b) = cx.model.building.as_ref() else { return };
    let ctx = cx.ctx;
    let inv = inventory::panel_size(cx);
    let inv_outer = widgets::window_outer(inv);
    let b_outer = widgets::window_outer(vec2(WIDTH, content_height(b)));
    let pair = vec2(inv_outer.x + 12.0 + b_outer.x, inv_outer.y.max(b_outer.y));
    let screen = ctx.content_rect();
    let rect = widgets::place(screen, pair, st.offset(WindowKind::Building) - vec2(0.0, 40.0));
    let inv_rect = Rect::from_min_size(rect.min, inv_outer);
    let b_rect = Rect::from_min_size(pos2(inv_rect.right() + 12.0, rect.top()), b_outer);
    let id = window_id(WindowKind::Building);
    let name = cx.model.catalog.name(b.item).to_string();
    let mut recipe_slot = Rect::NOTHING;
    widgets::area(ctx, id, Order::Middle, rect, |ui| {
        // Player inventory.
        let fi = widgets::window(ui, id.with("inv"), inv_rect, "Character", false);
        st.move_window(WindowKind::Building, fi.drag);
        inventory::panel(ui, cx, Rect::from_min_size(fi.content.min, inv), "Shift + click: move to building");
        // Building.
        let fb = widgets::window(ui, id.with("b"), b_rect, &name, true);
        st.move_window(WindowKind::Building, fb.drag);
        if fb.close_clicked {
            st.close(WindowKind::Building, cx.actions);
            return;
        }
        recipe_slot = building_panel(ui, cx, st, b, fb.content);
    });
    if st.picker_open && st.is_open(WindowKind::Building) {
        recipe_picker(cx, st, b, recipe_slot);
    }
}

fn building_panel(ui: &mut Ui, cx: &mut Cx, st: &mut UiState, b: &BuildingView, r: Rect) -> Rect {
    let p = ui.painter().clone();
    let mut y = r.top();
    let tier = cx.model.catalog.item(b.item).map(|i| i.tier).unwrap_or(0);

    // Status line.
    let sc = widgets::status_color(b.status.color());
    widgets::status_dot(&p, pos2(r.left() + 7.0, y + STATUS_H * 0.5), sc);
    let t = p.text(pos2(r.left() + 20.0, y + STATUS_H * 0.5), Align2::LEFT_CENTER, b.status.label(), font_bold(text::BODY + 1.0), color::TEXT);
    if !b.status_detail.is_empty() {
        p.text(pos2(t.right() + 8.0, y + STATUS_H * 0.5), Align2::LEFT_CENTER, &b.status_detail, font_regular(text::BODY), color::TEXT_DIM);
    }
    let tier_text = format!("Tier {tier}");
    let tier_rect = Rect::from_min_size(pos2(r.right() - 58.0, y + 5.0), vec2(58.0, 20.0));
    p.rect_filled(tier_rect, CornerRadius::same(2), theme::shade(theme::tier_color(tier), 0.45));
    p.rect_stroke(tier_rect, CornerRadius::same(2), Stroke::new(1.0, theme::tier_color(tier)), egui::StrokeKind::Inside);
    p.text(tier_rect.center(), Align2::CENTER_CENTER, tier_text, font_bold(text::SMALL), color::TEXT);
    y += STATUS_H;

    // Picture.
    let pic = Rect::from_min_size(pos2(r.left(), y), vec2(r.width(), PICTURE_H));
    widgets::deep(&p, pic);
    let glow = if b.status == crate::model::MachineStatus::Working { color::ORANGE } else { Color32::TRANSPARENT };
    if glow != Color32::TRANSPARENT {
        for i in 0..6 {
            let rr = Rect::from_center_size(pic.center(), Vec2::splat(96.0 + i as f32 * 8.0));
            p.rect_filled(rr, CornerRadius::same(20), glow.gamma_multiply(0.018));
        }
    }
    cx.atlas.paint(&p, b.item, Rect::from_center_size(pic.center(), Vec2::splat(112.0)), Color32::WHITE);
    if b.speed != 1.0 {
        widgets::text_shadow(&p, pic.left_bottom() + vec2(8.0, -6.0), Align2::LEFT_BOTTOM, &format!("Speed ×{}", b.speed), font_bold(text::SMALL), color::TEXT_DIM);
    }
    y += PICTURE_H + GAP;

    // Recipe selector.
    let mut recipe_slot = Rect::NOTHING;
    if !b.recipes.is_empty() || b.recipe.is_some() {
        let row = Rect::from_min_size(pos2(r.left(), y), vec2(r.width(), RECIPE_H));
        widgets::shallow(&p, row);
        let slot_r = Rect::from_min_size(pos2(row.left() + 6.0, row.center().y - size::SLOT * 0.5), Vec2::splat(size::SLOT));
        recipe_slot = slot_r;
        let recipe = b.recipe.and_then(|id| cx.model.catalog.recipe(id));
        let content = SlotContent { item: recipe.and_then(|r| r.main_item()), selected: st.picker_open, ..Default::default() };
        let look = if recipe.is_some() { SlotLook::Normal } else { SlotLook::Dark };
        let resp = widgets::slot(ui, Id::new("b-recipe"), slot_r, look, &content, cx.atlas);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Select recipe"));
        let tx = slot_r.right() + 10.0;
        match recipe {
            Some(rec) => {
                p.text(pos2(tx, row.top() + 8.0), Align2::LEFT_TOP, &rec.name, font_bold(text::BODY + 1.0), color::HEADING);
                let secs = rec.time / b.speed.max(0.01);
                p.text(pos2(tx, row.top() + 28.0), Align2::LEFT_TOP, format!("{} per run.  Click the icon to change.", format::seconds(secs)), font_regular(text::SMALL), color::TEXT_DIM);
            }
            None => {
                p.text(pos2(tx, row.center().y), Align2::LEFT_CENTER, "No recipe. Click the slot to choose one.", font(text::BODY), color::YELLOW);
            }
        }
        if resp.hovered() {
            match recipe {
                Some(rec) => cx.tip(Tip::Recipe { recipe: rec.id, hand: false }),
                None => cx.tip(Tip::Text { title: "Select recipe".into(), body: "Choose what this building makes.".into() }),
            }
        }
        if resp.clicked() && !b.recipes.is_empty() {
            st.picker_open = !st.picker_open;
        }
        y += RECIPE_H + GAP;
    }

    // Slots.
    if has_slots(b) {
        let panel = Rect::from_min_size(pos2(r.left(), y), vec2(r.width(), slots_panel_h(b)));
        widgets::shallow(&p, panel);
        let mut sy = panel.top() + 8.0;
        let ins_w = b.inputs.len() as f32 * size::SLOT + 4.0;
        let outs_w = b.outputs.len() as f32 * size::SLOT + 4.0;
        let in_grid = Rect::from_min_size(pos2(panel.left() + 8.0, sy + 20.0), vec2(ins_w, size::SLOT + 4.0));
        let out_grid = Rect::from_min_size(pos2(panel.right() - 8.0 - outs_w, sy + 20.0), vec2(outs_w, size::SLOT + 4.0));
        if !b.inputs.is_empty() {
            p.text(pos2(in_grid.left(), sy + 9.0), Align2::LEFT_CENTER, "Input", font_regular(text::SMALL), color::TEXT_DIM);
            slot_row(ui, cx, b, BuildingSlots::Input, in_grid);
        }
        if !b.outputs.is_empty() {
            p.text(pos2(out_grid.right(), sy + 9.0), Align2::RIGHT_CENTER, "Output", font_regular(text::SMALL), color::TEXT_DIM);
            slot_row(ui, cx, b, BuildingSlots::Output, out_grid);
        }
        // Progress arrow between inputs and outputs.
        let left = if b.inputs.is_empty() { panel.left() + 8.0 } else { in_grid.right() + 12.0 };
        let right = if b.outputs.is_empty() { panel.right() - 8.0 } else { out_grid.left() - 12.0 };
        if right - left > 40.0 {
            let cy = in_grid.center().y;
            let bar = Rect::from_min_max(pos2(left, cy - 8.0), pos2(right - 14.0, cy + 8.0));
            widgets::bar(&p, bar, b.progress, color::PROGRESS, Some(&format::percent(b.progress)));
            let tip = pos2(right, cy);
            p.add(egui::Shape::convex_polygon(
                vec![pos2(right - 12.0, cy - 11.0), tip, pos2(right - 12.0, cy + 11.0)],
                if b.progress > 0.0 { color::PROGRESS } else { color::BAR_TRACK },
                Stroke::new(1.0, color::WINDOW_DARK),
            ));
        }
        sy = in_grid.bottom() + 6.0;
        if !b.fuel.is_empty() {
            p.text(pos2(panel.left() + 8.0, sy + 9.0), Align2::LEFT_CENTER, "Fuel", font_regular(text::SMALL), color::TEXT_DIM);
            let fw = b.fuel.len() as f32 * size::SLOT + 4.0;
            let fgrid = Rect::from_min_size(pos2(panel.left() + 8.0, sy + 20.0), vec2(fw, size::SLOT + 4.0));
            slot_row(ui, cx, b, BuildingSlots::Fuel, fgrid);
        }
        y += panel.height() + GAP;
    }

    // Material buffers.
    if !b.buffers.is_empty() {
        widgets::heading(&p, pos2(r.left(), y + 2.0), "Materials");
        y += 24.0;
        for (i, buf) in b.buffers.iter().enumerate() {
            let row = Rect::from_min_size(pos2(r.left(), y), vec2(r.width(), ROW_H - 4.0));
            let icon = Rect::from_min_size(row.min, Vec2::splat(row.height()));
            widgets::deep(&p, icon);
            let item = buf.material.map(ItemId::Material);
            if let Some(it) = item {
                cx.atlas.paint(&p, it, icon.shrink(2.0), Color32::WHITE);
            }
            let name = item.map(|it| cx.model.catalog.name(it)).unwrap_or("Empty");
            p.text(pos2(icon.right() + 8.0, row.top() + 1.0), Align2::LEFT_TOP, &buf.label, font_regular(text::SMALL), color::TEXT_DIM);
            p.text(pos2(icon.right() + 8.0, row.bottom() + 1.0), Align2::LEFT_BOTTOM, name, font(text::SMALL), color::TEXT);
            let bar = Rect::from_min_max(pos2(r.left() + 170.0, row.top() + 5.0), pos2(r.right(), row.bottom() - 5.0));
            let frac = if buf.capacity > 0 { buf.units as f32 / buf.capacity as f32 } else { 0.0 };
            let fill = item.and_then(|it| cx.model.catalog.item(it)).map(|info| theme::opaque(info.color())).unwrap_or(color::GRAY);
            widgets::bar(&p, bar, frac, fill, Some(&format!("{} / {}", format::count(buf.units as u64), format::count(buf.capacity as u64))));
            let resp = ui.interact(row, Id::new(("b-buffer", i)), egui::Sense::hover());
            if resp.hovered() {
                let dir = if buf.output { "The building puts its product here." } else { "The building takes material from here." };
                match item {
                    Some(it) => cx.tip(Tip::Item { item: it, amount: Some(format!("{} / {} units. {dir}", buf.units, buf.capacity)) }),
                    None => cx.tip(Tip::Text { title: buf.label.clone(), body: format!("Empty. Holds {} units. {dir}", buf.capacity) }),
                }
            }
            y += ROW_H;
        }
        y += GAP;
    }

    // Power.
    if let Some(pw) = &b.power {
        let row = Rect::from_min_size(pos2(r.left(), y), vec2(r.width(), ROW_H - 4.0));
        let resp = ui.interact(row, Id::new("b-power"), egui::Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Power network"));
        if resp.hovered() {
            p.rect_filled(row.expand(2.0), CornerRadius::same(2), Color32::from_white_alpha(10));
        }
        p.text(pos2(r.left(), row.center().y), Align2::LEFT_CENTER, "Power", font(text::BODY), color::TEXT_DIM);
        let badge = Rect::from_min_size(pos2(r.right() - 44.0, row.top() + 2.0), vec2(44.0, row.height() - 4.0));
        let wrong = pw.network_voltage.is_some_and(|v| v != pw.voltage);
        let badge_color = if wrong { color::RED } else { theme::tier_color(voltage_tier(pw.voltage)) };
        p.rect_filled(badge, CornerRadius::same(2), theme::shade(badge_color, 0.4));
        p.rect_stroke(badge, CornerRadius::same(2), Stroke::new(1.0, badge_color), egui::StrokeKind::Inside);
        p.text(badge.center(), Align2::CENTER_CENTER, pw.voltage.label(), font_bold(text::SMALL), color::TEXT);
        let bar = Rect::from_min_max(pos2(r.left() + 110.0, row.top() + 5.0), pos2(badge.left() - 8.0, row.bottom() - 5.0));
        let frac = if pw.max_w > 0.0 { (pw.use_w / pw.max_w) as f32 } else { 0.0 };
        let bar_color = if pw.satisfaction < 0.5 { color::RED } else if pw.satisfaction < 0.99 { color::YELLOW } else { color::POWER };
        widgets::bar(&p, bar, frac, bar_color, Some(&format!("{} of {}", format::watts(pw.use_w), format::watts(pw.max_w))));
        if resp.hovered() {
            let mut body = format!(
                "Uses {} now, {} at full speed. Network satisfaction: {}.",
                format::watts(pw.use_w),
                format::watts(pw.max_w),
                format::percent(pw.satisfaction)
            );
            if let Some(nv) = pw.network_voltage
                && wrong
            {
                body.push_str(&format!("\nWrong voltage: the building is {}, the network is {}.", pw.voltage.label(), nv.label()));
            }
            body.push_str("\nClick to open the power network.");
            cx.tip(Tip::Text { title: "Power".into(), body });
        }
        if resp.clicked() {
            cx.act(UiAction::OpenPowerNetwork(b.id));
        }
        y += ROW_H;
    }

    // Temperature.
    if let Some(t) = b.temperature {
        let row = Rect::from_min_size(pos2(r.left(), y), vec2(r.width(), ROW_H - 4.0));
        p.text(pos2(r.left(), row.center().y), Align2::LEFT_CENTER, "Temperature", font(text::BODY), color::TEXT_DIM);
        let bar = Rect::from_min_max(pos2(r.left() + 110.0, row.top() + 5.0), pos2(r.right(), row.bottom() - 5.0));
        let max = b.max_temperature.unwrap_or(1000.0).max(1.0);
        let frac = (t / max).clamp(0.0, 1.0);
        let label = match b.max_temperature {
            Some(m) => format!("{} of {} max", format::celsius(t), format::celsius(m)),
            None => format::celsius(t),
        };
        widgets::bar(&p, bar, frac, widgets::danger_color(frac), Some(&label));
        if ui.interact(row, Id::new("b-temp"), egui::Sense::hover()).hovered() {
            cx.tip(Tip::Text {
                title: "Temperature".into(),
                body: format!("The building stops above {}. Far above it, the building takes damage.", format::celsius(max)),
            });
        }
    }
    recipe_slot
}

fn voltage_tier(v: crate::model::Voltage) -> u8 {
    match v {
        crate::model::Voltage::Lv => 2,
        crate::model::Voltage::Mv => 3,
        crate::model::Voltage::Hv => 4,
        crate::model::Voltage::Ev => 5,
    }
}

fn slot_row(ui: &mut Ui, cx: &mut Cx, b: &BuildingView, group: BuildingSlots, grid: Rect) {
    widgets::deep(ui.painter(), grid);
    for (i, s) in b.slots(group).iter().enumerate() {
        let sr = Rect::from_min_size(grid.min + vec2(2.0 + i as f32 * size::SLOT, 2.0), Vec2::splat(size::SLOT));
        let count = s.stack.filter(|x| x.count > 1).map(|x| format::count(x.count as u64));
        let content = SlotContent { item: s.stack.map(|x| x.item), count: count.as_deref(), ghost: s.filter, ..Default::default() };
        let look = if s.stack.is_some() { SlotLook::Normal } else { SlotLook::Dark };
        let resp = widgets::slot(ui, Id::new(("b-slot", group, i)), sr, look, &content, cx.atlas);
        if resp.hovered() {
            if let Some(st) = s.stack {
                cx.tip(Tip::Item { item: st.item, amount: Some(st.count.to_string()) });
            } else if let Some(f) = s.filter {
                let what = match group {
                    BuildingSlots::Input => "Needs",
                    BuildingSlots::Output => "Makes",
                    BuildingSlots::Fuel => "Burns",
                };
                cx.tip(Tip::Text { title: format!("{what}: {}", cx.model.catalog.name(f)), body: "Empty slot.".into() });
            }
        }
        if let Some(click) = slot_click(cx, &resp) {
            cx.act(UiAction::ClickSlot { slot: SlotRef::Building { building: b.id, group, index: i }, click });
        }
    }
}

fn recipe_picker(cx: &mut Cx, st: &mut UiState, b: &BuildingView, anchor: Rect) {
    let cols = 8usize;
    let n = b.recipes.len();
    let rows = n.div_ceil(cols).max(1);
    let grid_w = cols as f32 * size::SLOT + 4.0;
    let content = vec2(grid_w, rows as f32 * size::SLOT + 4.0 + 8.0 + size::BUTTON_H);
    let outer = widgets::window_outer(content);
    let screen = cx.ctx.content_rect();
    let mut pos = pos2(anchor.left() - size::PAD, anchor.bottom() + 6.0);
    if pos.y + outer.y > screen.bottom() {
        pos.y = (anchor.top() - 6.0 - outer.y).max(screen.top());
    }
    pos.x = pos.x.clamp(screen.left(), (screen.right() - outer.x).max(screen.left()));
    let rect = Rect::from_min_size(pos, outer);
    let id = Id::new("foundry-recipe-picker");
    widgets::area(cx.ctx, id, Order::Foreground, rect, |ui| {
        let f = widgets::window(ui, id, rect, "Select recipe", true);
        if f.close_clicked {
            st.picker_open = false;
            return;
        }
        let grid = Rect::from_min_size(f.content.min, vec2(grid_w, rows as f32 * size::SLOT + 4.0));
        widgets::deep(ui.painter(), grid);
        for (i, rid) in b.recipes.iter().enumerate() {
            let Some(rec) = cx.model.catalog.recipe(*rid) else { continue };
            let sr = Rect::from_min_size(grid.min + vec2(2.0 + (i % cols) as f32 * size::SLOT, 2.0 + (i / cols) as f32 * size::SLOT), Vec2::splat(size::SLOT));
            let content = SlotContent { item: rec.main_item(), selected: b.recipe == Some(*rid), ..Default::default() };
            let resp = widgets::slot(ui, Id::new(("pick", rid.0)), sr, SlotLook::Normal, &content, cx.atlas);
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &rec.name));
            if resp.hovered() {
                cx.tip(Tip::Recipe { recipe: *rid, hand: false });
            }
            if resp.clicked() {
                cx.act(UiAction::SetRecipe { building: b.id, recipe: Some(*rid) });
                st.picker_open = false;
            }
        }
        let br = Rect::from_min_size(pos2(f.content.left(), grid.bottom() + 8.0), vec2(150.0, size::BUTTON_H));
        if widgets::button(ui, Id::new("pick-clear"), br, "Clear recipe", ButtonKind::Back, b.recipe.is_some()).clicked() {
            cx.act(UiAction::SetRecipe { building: b.id, recipe: None });
            st.picker_open = false;
        }
    });
    cx.ctx.move_to_top(LayerId::new(Order::Foreground, id));
}
