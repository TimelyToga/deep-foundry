//! The building window (Factorio entity GUI), with the player inventory next to it.
//!
//! Layout from top to bottom: status line, picture, Hub repair stage, recipe selector, input
//! slots, progress, output slots, fuel slots, material buffers, power bar, temperature bar.

use super::inventory::{self, slot_click};
use super::{Cx, window_id};
use crate::action::{SlotRef, UiAction, WindowKind};
use crate::format;
use crate::item;
use crate::model::{BuildingSlots, BuildingView, MilestoneView};
use foundry_content::ItemRef;
use foundry_core::RecipeId;
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

/// Slots in one row of a slot grid. More slots wrap to more rows.
const MAX_COLS: usize = 10;
/// The smallest space between the input and the output grids (room for the progress arrow).
const ARROW_MIN: f32 = 64.0;
const HUB_HINT: &str = "Put these items into the Hub: shift + click a stack in your inventory. The Hub takes only what the repair stages need.";
/// Height of the title line of a later Hub stage, and of one row of its items.
const LATER_TITLE_H: f32 = 22.0;
const LATER_ROW_H: f32 = 26.0;
/// Items in one row of a later Hub stage.
const LATER_COLS: usize = 4;

/// The size of a slot grid with `n` slots (at least one row, so an empty grid keeps its place).
fn grid_size(n: usize) -> Vec2 {
    let cols = n.clamp(1, MAX_COLS);
    let rows = n.div_ceil(MAX_COLS).max(1);
    vec2(cols as f32 * size::SLOT + 4.0, rows as f32 * size::SLOT + 4.0)
}

/// True if the input and the output grids fit next to each other.
fn side_by_side(b: &BuildingView) -> bool {
    b.inputs.is_empty() || b.outputs.is_empty() || grid_size(b.inputs.len()).x + grid_size(b.outputs.len()).x + ARROW_MIN + 16.0 <= WIDTH
}

/// A machine shows its progress. Storage (a crate, the Hub) does not.
fn shows_progress(b: &BuildingView) -> bool {
    b.recipe.is_some() || !b.outputs.is_empty() || !b.fuel.is_empty()
}

/// The height of the input and output grids with their labels.
fn main_grids_h(b: &BuildingView) -> f32 {
    let (ins, outs) = (grid_size(b.inputs.len()).y, grid_size(b.outputs.len()).y);
    // An empty grid has the height of one row, so `max` also works when one of them is empty.
    if side_by_side(b) {
        ins.max(outs)
    } else {
        ins + 6.0 + 20.0 + outs
    }
}

fn slots_panel_h(b: &BuildingView) -> f32 {
    let mut h = 12.0 + 20.0 + main_grids_h(b);
    if !b.fuel.is_empty() {
        h += 26.0 + 20.0 + grid_size(b.fuel.len()).y;
    }
    h
}

fn has_slots(b: &BuildingView) -> bool {
    !(b.inputs.is_empty() && b.outputs.is_empty() && b.fuel.is_empty())
}

/// The height of the Hub repair stage panel.
fn milestone_h(ctx: &egui::Context, m: &MilestoneView) -> f32 {
    let w = WIDTH - 20.0;
    let desc = if m.description.is_empty() { 0.0 } else { widgets::text_height(ctx, &m.description, font_regular(text::BODY), w) + 6.0 };
    let hint = widgets::text_height(ctx, HUB_HINT, font_regular(text::SMALL), w);
    10.0 + 26.0 + desc + m.items.len() as f32 * ROW_H + 4.0 + hint + 10.0
}

/// The height of the list of later Hub stages.
fn later_stages_h(b: &BuildingView) -> f32 {
    if b.later_stages.is_empty() {
        return 0.0;
    }
    let rows: usize = b.later_stages.iter().map(|m| m.items.len().div_ceil(LATER_COLS).max(1)).sum();
    10.0 + 24.0 + b.later_stages.len() as f32 * LATER_TITLE_H + rows as f32 * LATER_ROW_H + 8.0
}

/// True if the window is the Hub.
fn is_hub(cx: &Cx, b: &BuildingView) -> bool {
    cx.model.content.factory.buildings.get(b.kind.0 as usize).is_some_and(|d| d.kind == "hub")
}

fn content_height(ctx: &egui::Context, b: &BuildingView, recipes: &[RecipeId]) -> f32 {
    let mut h = STATUS_H + PICTURE_H + GAP;
    if let Some(m) = &b.milestone {
        h += milestone_h(ctx, m) + GAP;
    }
    if !b.later_stages.is_empty() {
        h += later_stages_h(b) + GAP;
    }
    if !recipes.is_empty() || b.recipe.is_some() {
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
    let model = cx.model;
    let Some(b) = model.building.as_ref() else { return };
    let ctx = cx.ctx;
    let recipes = item::building_recipes(&model.content, b.kind, &model.finished_techs);
    let inv = inventory::panel_size(cx);
    let inv_outer = widgets::window_outer(inv);
    let b_outer = widgets::window_outer(vec2(WIDTH, content_height(ctx, b, &recipes)));
    let pair = vec2(inv_outer.x + 12.0 + b_outer.x, inv_outer.y.max(b_outer.y));
    let screen = ctx.content_rect();
    let rect = widgets::place(screen, pair, st.offset(WindowKind::Building) - vec2(0.0, 40.0));
    let inv_rect = Rect::from_min_size(rect.min, inv_outer);
    let b_rect = Rect::from_min_size(pos2(inv_rect.right() + 12.0, rect.top()), b_outer);
    let id = window_id(WindowKind::Building);
    let name = item::building_name(&model.content, b.kind);
    let mut recipe_slot = Rect::NOTHING;
    widgets::area(ctx, id, Order::Middle, rect, |ui| {
        // Player inventory.
        let fi = widgets::window(ui, id.with("inv"), inv_rect, "Character", false);
        st.move_window(WindowKind::Building, fi.drag);
        inventory::panel(ui, cx, st, Rect::from_min_size(fi.content.min, inv), "Shift + click: move to building");
        // Building.
        let fb = widgets::window(ui, id.with("b"), b_rect, name, true);
        st.move_window(WindowKind::Building, fb.drag);
        if fb.close_clicked {
            st.close(WindowKind::Building, cx.actions);
            return;
        }
        recipe_slot = building_panel(ui, cx, st, b, &recipes, fb.content);
    });
    // The picker is a Foreground layer: hide it under the pause menu.
    if st.picker_open && st.is_open(WindowKind::Building) && cx.model.state == crate::model::GameState::Playing {
        recipe_picker(cx, st, b, &recipes, recipe_slot);
    }
}

fn building_panel(ui: &mut Ui, cx: &mut Cx, st: &mut UiState, b: &BuildingView, recipes: &[RecipeId], r: Rect) -> Rect {
    let model = cx.model;
    let content = &*model.content;
    let p = ui.painter().clone();
    let mut y = r.top();
    let def = content.factory.buildings.get(b.kind.0 as usize);
    let tier = def.map(|d| d.tier).unwrap_or(0);
    let building_item = item::building_item(content, b.kind);

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
    if let Some(it) = building_item {
        cx.atlas.paint(&p, it, Rect::from_center_size(pic.center(), Vec2::splat(112.0)), Color32::WHITE);
    }
    if b.speed != 1.0 {
        widgets::text_shadow(&p, pic.left_bottom() + vec2(8.0, -6.0), Align2::LEFT_BOTTOM, &format!("Speed ×{}", b.speed), font_bold(text::SMALL), color::TEXT_DIM);
    }
    y += PICTURE_H + GAP;

    // Hub: the next repair stage.
    if let Some(m) = &b.milestone {
        let panel = Rect::from_min_size(pos2(r.left(), y), vec2(r.width(), milestone_h(cx.ctx, m)));
        milestone_panel(ui, cx, &p, m, panel);
        y += panel.height() + GAP;
    }
    if !b.later_stages.is_empty() {
        let panel = Rect::from_min_size(pos2(r.left(), y), vec2(r.width(), later_stages_h(b)));
        later_stages_panel(ui, cx, &p, b, panel);
        y += panel.height() + GAP;
    }

    // Recipe selector.
    let mut recipe_slot = Rect::NOTHING;
    if !recipes.is_empty() || b.recipe.is_some() {
        let row = Rect::from_min_size(pos2(r.left(), y), vec2(r.width(), RECIPE_H));
        widgets::shallow(&p, row);
        let slot_r = Rect::from_min_size(pos2(row.left() + 6.0, row.center().y - size::SLOT * 0.5), Vec2::splat(size::SLOT));
        recipe_slot = slot_r;
        let recipe = b.recipe.and_then(|id| item::recipe(content, id).map(|r| (id, r)));
        let slot_content = SlotContent { item: recipe.and_then(|(_, r)| item::recipe_item(r)), selected: st.picker_open, ..Default::default() };
        let look = if recipe.is_some() { SlotLook::Normal } else { SlotLook::Dark };
        let resp = widgets::slot(ui, Id::new("b-recipe"), slot_r, look, &slot_content, cx.atlas);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Select recipe"));
        let tx = slot_r.right() + 10.0;
        match recipe {
            Some((_, rec)) => {
                p.text(pos2(tx, row.top() + 8.0), Align2::LEFT_TOP, &rec.name, font_bold(text::BODY + 1.0), color::HEADING);
                let secs = rec.time / b.speed.max(0.01);
                let hint = if recipes.len() > 1 { "  Click the icon to change." } else { "" };
                p.text(pos2(tx, row.top() + 28.0), Align2::LEFT_TOP, format!("{} per run.{hint}", format::seconds(secs)), font_regular(text::SMALL), color::TEXT_DIM);
            }
            None => {
                p.text(pos2(tx, row.center().y), Align2::LEFT_CENTER, "No recipe. Click the slot to choose one.", font(text::BODY), color::YELLOW);
            }
        }
        if resp.hovered() {
            match recipe {
                Some((id, _)) => cx.tip(Tip::Recipe { recipe: id, hand: false }),
                None => cx.tip(Tip::Text { title: "Select recipe".into(), body: "Choose what this building makes.".into() }),
            }
        }
        if resp.clicked() && !recipes.is_empty() {
            st.picker_open = !st.picker_open;
        }
        y += RECIPE_H + GAP;
    }

    // Slots.
    if has_slots(b) {
        let panel = Rect::from_min_size(pos2(r.left(), y), vec2(r.width(), slots_panel_h(b)));
        widgets::shallow(&p, panel);
        let sy = panel.top() + 8.0;
        let in_grid = Rect::from_min_size(pos2(panel.left() + 8.0, sy + 20.0), grid_size(b.inputs.len()));
        let out_size = grid_size(b.outputs.len());
        let beside = side_by_side(b);
        let out_grid = if beside {
            Rect::from_min_size(pos2(panel.right() - 8.0 - out_size.x, sy + 20.0), out_size)
        } else {
            // Below the inputs, on the right.
            Rect::from_min_size(pos2(panel.right() - 8.0 - out_size.x, in_grid.bottom() + 6.0 + 20.0), out_size)
        };
        if !b.inputs.is_empty() {
            // Storage and the Hub (no recipe, no outputs, no fuel) hold items: "Contents".
            let storage = b.recipe.is_none() && b.outputs.is_empty() && b.fuel.is_empty();
            let (label, help) = if is_hub(cx, b) {
                ("Held items", "Click an item to take it back.")
            } else if storage {
                ("Contents", "Shift + click: take it back.")
            } else {
                ("Input", "")
            };
            p.text(pos2(in_grid.left(), sy + 9.0), Align2::LEFT_CENTER, label, font_regular(text::SMALL), color::TEXT_DIM);
            p.text(pos2(panel.right() - 8.0, sy + 9.0), Align2::RIGHT_CENTER, help, font_regular(text::SMALL), color::TEXT_FAINT);
            slot_grid(ui, cx, b, BuildingSlots::Input, in_grid);
        }
        if !b.outputs.is_empty() {
            p.text(pos2(out_grid.right(), out_grid.top() - 11.0), Align2::RIGHT_CENTER, "Output", font_regular(text::SMALL), color::TEXT_DIM);
            slot_grid(ui, cx, b, BuildingSlots::Output, out_grid);
        }
        // Progress arrow before the outputs: between the grids, or left of the outputs when
        // they are below the inputs.
        let left = if b.inputs.is_empty() || !beside { panel.left() + 8.0 } else { in_grid.right() + 12.0 };
        let right = if b.outputs.is_empty() { panel.right() - 8.0 } else { out_grid.left() - 12.0 };
        if shows_progress(b) && right - left > 40.0 {
            let cy = if beside { in_grid.top() + (size::SLOT + 4.0) * 0.5 } else { out_grid.top() + (size::SLOT + 4.0) * 0.5 };
            let bar = Rect::from_min_max(pos2(left, cy - 8.0), pos2(right - 14.0, cy + 8.0));
            widgets::bar(&p, bar, b.progress, color::PROGRESS, Some(&format::percent(b.progress)));
            let tip = pos2(right, cy);
            p.add(egui::Shape::convex_polygon(
                vec![pos2(right - 12.0, cy - 11.0), tip, pos2(right - 12.0, cy + 11.0)],
                if b.progress > 0.0 { color::PROGRESS } else { color::BAR_TRACK },
                Stroke::new(1.0, color::WINDOW_DARK),
            ));
        }
        let fy = sy + 20.0 + main_grids_h(b) + 6.0;
        if !b.fuel.is_empty() {
            p.text(pos2(panel.left() + 8.0, fy + 9.0), Align2::LEFT_CENTER, "Fuel", font_regular(text::SMALL), color::TEXT_DIM);
            let fgrid = Rect::from_min_size(pos2(panel.left() + 8.0, fy + 20.0), grid_size(b.fuel.len()));
            slot_grid(ui, cx, b, BuildingSlots::Fuel, fgrid);
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
            let mat = buf.material.map(ItemRef::Material);
            if let Some(it) = mat {
                cx.atlas.paint(&p, it, icon.shrink(2.0), Color32::WHITE);
            }
            let name = mat.map(|it| item::name(content, it)).unwrap_or("Empty");
            p.text(pos2(icon.right() + 8.0, row.center().y), Align2::LEFT_CENTER, &buf.label, font(text::BODY), color::TEXT_DIM);
            let bar = Rect::from_min_max(pos2(r.left() + 130.0, row.top() + 3.0), pos2(r.right(), row.bottom() - 3.0));
            let frac = if buf.capacity > 0 { buf.units as f32 / buf.capacity as f32 } else { 0.0 };
            let fill = mat.map(|it| theme::opaque(item::color(content, it))).unwrap_or(color::GRAY);
            widgets::bar(&p, bar, frac, fill, None);
            widgets::text_outlined(&p, pos2(bar.left() + 8.0, bar.center().y), Align2::LEFT_CENTER, name, font(text::SMALL), color::TEXT);
            let amount = format!("{} / {}", format::count(buf.units as u64), format::count(buf.capacity as u64));
            widgets::text_outlined(&p, pos2(bar.right() - 8.0, bar.center().y), Align2::RIGHT_CENTER, &amount, font_bold(text::SMALL), color::TEXT);
            let resp = ui.interact(row, Id::new(("b-buffer", i)), egui::Sense::hover());
            if resp.hovered() {
                let dir = if buf.output { "The building puts its product here." } else { "The building takes material from here." };
                match mat {
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
        let badge_color = if wrong { color::RED } else { theme::tier_color(pw.voltage.game_tier()) };
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
        let max = def.map(|d| d.max_temp as f32).unwrap_or(1000.0).max(1.0);
        let frac = (t / max).clamp(0.0, 1.0);
        let label = format!("{} of {} max", format::celsius(t), format::celsius(max));
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

/// Draw a slot grid. The slots fill rows of `MAX_COLS` from the top left.
fn slot_grid(ui: &mut Ui, cx: &mut Cx, b: &BuildingView, group: BuildingSlots, grid: Rect) {
    let model = cx.model;
    widgets::deep(ui.painter(), grid);
    for (i, s) in b.slots(group).iter().enumerate() {
        let (col, row) = (i % MAX_COLS, i / MAX_COLS);
        let sr = Rect::from_min_size(grid.min + vec2(2.0 + col as f32 * size::SLOT, 2.0 + row as f32 * size::SLOT), Vec2::splat(size::SLOT));
        let count = s.stack.filter(|x| x.count > 1).map(|x| format::count(x.count as u64));
        // A material in a storage slot: a fill bar in its color.
        let fill = s.stack.filter(|_| s.capacity > 0).map(|x| {
            (x.count as f32 / s.capacity as f32, theme::rgba(item::color(&model.content, x.item)))
        });
        let content = SlotContent { item: s.stack.map(|x| x.item), count: count.as_deref(), ghost: s.filter, fill, ..Default::default() };
        // A material slot is dark, like a tank, so its fill bar shows.
        let look = if s.stack.is_some() && s.capacity == 0 { SlotLook::Normal } else { SlotLook::Dark };
        let resp = widgets::slot(ui, Id::new(("b-slot", group, i)), sr, look, &content, cx.atlas);
        if let Some(x) = s.stack {
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, item::name(&model.content, x.item)));
        }
        if resp.hovered() {
            if let Some(st) = s.stack {
                let amount = if s.capacity > 0 {
                    format!("{} / {} units", format::count_full(st.count as u64), format::count_full(s.capacity as u64))
                } else {
                    st.count.to_string()
                };
                cx.tip(Tip::Item { item: st.item, amount: Some(amount) });
            } else if let Some(f) = s.filter {
                let what = match group {
                    BuildingSlots::Input => "Needs",
                    BuildingSlots::Output => "Makes",
                    BuildingSlots::Fuel => "Burns",
                };
                cx.tip(Tip::Text { title: format!("{what}: {}", item::name(&model.content, f)), body: "Empty slot.".into() });
            }
        }
        if let Some(click) = slot_click(cx, &resp) {
            cx.act(UiAction::ClickSlot { slot: SlotRef::Building { building: b.id, group, index: i }, click });
        }
    }
}

/// The Hub repair stage: the name, the description, one row per item with a bar, and a hint.
fn milestone_panel(ui: &mut Ui, cx: &mut Cx, p: &egui::Painter, m: &MilestoneView, panel: Rect) {
    let content = &*cx.model.content;
    widgets::shallow(p, panel);
    let inner = panel.shrink(10.0);
    let mut y = inner.top();
    widgets::heading(p, pos2(inner.left(), y), &format!("Repair stage {}: {}", m.stage, m.name));
    y += 26.0;
    if !m.description.is_empty() {
        y = widgets::wrapped(p, pos2(inner.left(), y), &m.description, font_regular(text::BODY), color::TEXT_DIM, inner.width()).bottom() + 6.0;
    }
    for (i, d) in m.items.iter().enumerate() {
        let row = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), ROW_H - 4.0));
        let icon = Rect::from_min_size(row.min, Vec2::splat(row.height()));
        widgets::deep(p, icon);
        cx.atlas.paint(p, d.item, icon.shrink(2.0), Color32::WHITE);
        let name = item::name(content, d.item);
        p.text(pos2(icon.right() + 8.0, row.center().y), Align2::LEFT_CENTER, name, font(text::BODY), color::TEXT);
        let bar = Rect::from_min_max(pos2(inner.left() + 190.0, row.top() + 4.0), pos2(inner.right(), row.bottom() - 4.0));
        let done = d.delivered >= d.need;
        let frac = if d.need > 0 { d.delivered as f32 / d.need as f32 } else { 1.0 };
        let label = format!("{} / {}", d.delivered, d.need);
        widgets::bar(p, bar, frac, if done { color::GREEN } else { color::PROGRESS }, Some(&label));
        let resp = ui.interact(row, Id::new(("b-delivery", i)), egui::Sense::hover());
        if resp.hovered() {
            let amount = if done { format!("Delivered: {} of {}. Done.", d.delivered, d.need) } else { format!("Delivered: {} of {}. {} more.", d.delivered, d.need, d.need - d.delivered) };
            cx.tip(Tip::Item { item: d.item, amount: Some(amount) });
        }
        y += ROW_H;
    }
    widgets::wrapped(p, pos2(inner.left(), y + 4.0), HUB_HINT, font_regular(text::SMALL), color::TEXT_FAINT, inner.width());
}

fn recipe_picker(cx: &mut Cx, st: &mut UiState, b: &BuildingView, recipes: &[RecipeId], anchor: Rect) {
    let model = cx.model;
    let cols = 8usize;
    let n = recipes.len();
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
        for (i, rid) in recipes.iter().enumerate() {
            let Some(rec) = item::recipe(&model.content, *rid) else { continue };
            let sr = Rect::from_min_size(grid.min + vec2(2.0 + (i % cols) as f32 * size::SLOT, 2.0 + (i / cols) as f32 * size::SLOT), Vec2::splat(size::SLOT));
            let content = SlotContent { item: item::recipe_item(rec), selected: b.recipe == Some(*rid), ..Default::default() };
            let resp = widgets::slot(ui, Id::new(("pick", rid.0)), sr, SlotLook::Normal, &content, cx.atlas);
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Use recipe {}", rec.name)));
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

/// The Hub repair stages after the next one: a title and the items each needs.
fn later_stages_panel(ui: &mut Ui, cx: &mut Cx, p: &egui::Painter, b: &BuildingView, panel: Rect) {
    widgets::shallow(p, panel);
    let inner = panel.shrink(10.0);
    let mut y = inner.top();
    widgets::heading(p, pos2(inner.left(), y), "Later repair stages");
    y += 24.0;
    let cell_w = inner.width() / LATER_COLS as f32;
    for m in &b.later_stages {
        p.text(pos2(inner.left(), y + LATER_TITLE_H * 0.5), Align2::LEFT_CENTER, format!("Stage {}: {}", m.stage, m.name), font_bold(text::SMALL), color::TEXT_DIM);
        y += LATER_TITLE_H;
        for (i, d) in m.items.iter().enumerate() {
            let (col, row) = (i % LATER_COLS, i / LATER_COLS);
            let cell = Rect::from_min_size(pos2(inner.left() + col as f32 * cell_w, y + row as f32 * LATER_ROW_H), vec2(cell_w, LATER_ROW_H));
            let icon = Rect::from_min_size(pos2(cell.left(), cell.top() + 2.0), Vec2::splat(22.0));
            widgets::deep(p, icon);
            cx.atlas.paint(p, d.item, icon.shrink(1.0), Color32::WHITE);
            p.text(pos2(icon.right() + 6.0, icon.center().y), Align2::LEFT_CENTER, format!("× {}", d.need), font(text::SMALL), color::TEXT);
            let resp = ui.interact(cell, Id::new(("b-later", m.stage, i)), egui::Sense::hover());
            if resp.hovered() {
                let amount = format!("Stage {} needs {}. The Hub keeps them until then.", m.stage, d.need);
                cx.tip(Tip::Item { item: d.item, amount: Some(amount) });
            }
        }
        y += m.items.len().div_ceil(LATER_COLS).max(1) as f32 * LATER_ROW_H;
    }
}
