//! The HUD: quickbar with hull and heat bars, tank summary, crafting queue, research box,
//! guide tracker, the hover box (top center) and alerts.

use super::Cx;
use crate::action::{UiAction, WindowKind};
use crate::crafting::cancel_count;
use crate::format;
use crate::item;
use foundry_content::ItemRef;
use crate::model::{AlertKind, DigState, HoverView, NextGoal, UiModel, next_goal};
use crate::theme::{self, color, font, font_bold, font_regular, size, text};
use crate::tooltip::Tip;
use crate::widgets::{self, SlotContent, SlotLook};
use crate::UiState;
use egui::{Align2, Color32, CornerRadius, Id, Order, Painter, Rect, Stroke, Ui, Vec2, pos2, vec2};

const MARGIN: f32 = 8.0;
const PANEL_PAD: f32 = 6.0;
const BARS_H: f32 = 18.0;

pub(crate) fn show(cx: &mut Cx, st: &mut UiState) {
    let screen = cx.ctx.content_rect();
    let qb = quickbar(cx, screen);
    tank_summary(cx, qb);
    super::drag::first_hint(cx, st, qb);
    crafting_queue(cx, screen);
    let left_top = research(cx, st, screen);
    guide_tracker(cx, st, screen, left_top);
    perf_box(cx, screen);
    hover_box(cx, screen);
    alerts(cx, screen);
}

/// The performance box at the top right. Returns the y below it.
fn perf_box(cx: &mut Cx, screen: Rect) -> f32 {
    let Some(pf) = cx.model.perf else {
        return if cx.model.settings.show_fps { screen.top() + 30.0 } else { screen.top() + MARGIN };
    };
    let width = 300.0;
    let rect = Rect::from_min_size(pos2(screen.right() - MARGIN - width, screen.top() + MARGIN), vec2(width, 62.0));
    panel_area(cx.ctx, "perf", rect, |ui| {
        let p = ui.painter().clone();
        let inner = rect.shrink(10.0);
        let cols = [
            ("FPS", if pf.fps > 0.0 { format!("{:.0}", pf.fps) } else { "-".into() }),
            ("Tick", format!("{:.2} ms", pf.tick_ms)),
            ("Ticks/s", if pf.ticks_per_second > 0.0 { format!("{:.0}", pf.ticks_per_second) } else { "-".into() }),
            ("Awake", format!("{} / {}", pf.awake_chunks, pf.loaded_chunks)),
        ];
        let w = inner.width() / cols.len() as f32;
        for (i, (label, value)) in cols.iter().enumerate() {
            let x = inner.left() + i as f32 * w;
            p.text(pos2(x, inner.top()), Align2::LEFT_TOP, *label, font_regular(text::SMALL), color::TEXT_DIM);
            p.text(pos2(x, inner.top() + 18.0), Align2::LEFT_TOP, value, font_bold(text::BODY), color::TEXT);
        }
        if ui.interact(rect, Id::new("hud-perf"), egui::Sense::hover()).hovered() {
            cx.tip(Tip::Text {
                title: "Performance".into(),
                body: "FPS: frames per second of the window. Tick: time of one simulation step. \
                       Ticks/s: simulation steps per second (60 is full speed). \
                       Awake: chunks that the simulation updates, of all loaded chunks."
                    .into(),
            });
        }
    });
    rect.bottom() + 8.0
}

/// A HUD panel: the window frame without a title.
fn panel_area(ctx: &egui::Context, name: &str, rect: Rect, add: impl FnOnce(&mut Ui)) {
    widgets::area(ctx, Id::new(("hud", name)), Order::Middle, rect, |ui| {
        widgets::window_frame(ui.painter(), rect);
        let _ = ui.interact(rect, Id::new(("hud-bg", name)), egui::Sense::click());
        add(ui);
    });
}

fn count_in_inventory(model: &UiModel, it: ItemRef) -> u64 {
    if item::is_bulk(it) {
        model.player.tank.iter().filter(|t| t.material.map(ItemRef::Material) == Some(it)).map(|t| t.units as u64).sum()
    } else {
        model.player.inventory.iter().flatten().filter(|s| s.item == it).map(|s| s.count as u64).sum()
    }
}

fn quickbar(cx: &mut Cx, screen: Rect) -> Rect {
    let ctx = cx.ctx;
    let grid_w = 10.0 * size::SLOT;
    let outer = vec2(grid_w + 2.0 * PANEL_PAD + 4.0, PANEL_PAD + BARS_H + 6.0 + 2.0 * size::SLOT + 4.0 + PANEL_PAD);
    let rect = Rect::from_min_size(pos2((screen.center().x - outer.x * 0.5).round(), screen.bottom() - MARGIN - outer.y), outer);
    panel_area(ctx, "quickbar", rect, |ui| {
        let p = ui.painter().clone();
        // A building slot dropped on the bar goes back to the robot.
        let zone = ui.interact(rect, Id::new("hud-quickbar-drop"), egui::Sense::hover());
        super::drag::robot_zone(cx, &zone);
        let bars = Rect::from_min_size(rect.min + vec2(PANEL_PAD, PANEL_PAD), vec2(outer.x - 2.0 * PANEL_PAD, BARS_H));
        if let Some(sb) = cx.model.sandbox {
            brush_line(&p, cx.model, bars, sb);
        } else {
            hull_and_heat(ui, cx, &p, bars);
        }
        quickbar_slots(ui, cx, &p, rect, bars, grid_w);
    });
    rect
}

/// The sandbox line above the quickbar: the brush material and size, and the pause state.
fn brush_line(p: &Painter, model: &UiModel, r: Rect, sb: crate::model::SandboxView) {
    let name = model.player.hand.map(|h| item::name(&model.content, h.item)).unwrap_or("Nothing in the hand");
    let left = format!("Brush: {name}, {} cells across", sb.brush_radius * 2 + 1);
    widgets::text_outlined(p, pos2(r.left() + 2.0, r.center().y), Align2::LEFT_CENTER, &left, font_bold(text::SMALL), color::HEADING);
    let (right, c) = if sb.sim_paused { ("Simulation paused (Space)", color::YELLOW) } else { ("[ ] size    Q empty hand", color::TEXT_FAINT) };
    widgets::text_outlined(p, pos2(r.right() - 2.0, r.center().y), Align2::RIGHT_CENTER, right, font_regular(text::SMALL), c);
}

fn hull_and_heat(ui: &mut Ui, cx: &mut Cx, p: &Painter, bars: Rect) {
    let pl = &cx.model.player;
    let half = (bars.width() - 8.0) * 0.5;
    let hull_r = Rect::from_min_size(bars.min, vec2(half, BARS_H));
    let heat_r = Rect::from_min_size(bars.min + vec2(half + 8.0, 0.0), vec2(half, BARS_H));
    let hull_frac = if pl.hull_max > 0.0 { pl.hull / pl.hull_max } else { 0.0 };
    let hull_color = if hull_frac < 0.3 { color::RED } else { color::HULL };
    widgets::bar(p, hull_r, hull_frac, hull_color, Some(&format!("Hull {} / {}", pl.hull.round(), pl.hull_max.round())));
    let heat_frac = if pl.heat_limit > 0.0 { (pl.temperature / pl.heat_limit).max(0.0) } else { 0.0 };
    let heat_text = format!("Heat {} of {}", format::celsius(pl.temperature), format::celsius(pl.heat_limit));
    widgets::bar(p, heat_r, heat_frac, widgets::danger_color(heat_frac), Some(&heat_text));
    let hull_hover = ui.interact(hull_r, Id::new("hud-hull"), egui::Sense::hover()).hovered();
    let heat_hover = ui.interact(heat_r, Id::new("hud-heat"), egui::Sense::hover()).hovered();
    if hull_hover {
        cx.tip(Tip::Text { title: "Hull".into(), body: "The health of the robot. At 0 the robot is rebuilt at the Hub.".into() });
    }
    if heat_hover {
        cx.tip(Tip::Text {
            title: "Heat".into(),
            body: format!("The temperature of the robot. Above {} the robot takes damage.", format::celsius(pl.heat_limit)),
        });
    }
}

/// The 20 quickbar slots. The bottom row is slots 1-10 (keys 1-0), the top row is 11-20 (Shift + 1-0).
fn quickbar_slots(ui: &mut Ui, cx: &mut Cx, p: &Painter, rect: Rect, bars: Rect, grid_w: f32) {
    let model = cx.model;
    let pl = &model.player;
    let sandbox = model.sandbox.is_some();
    let grid = Rect::from_min_size(pos2(rect.left() + PANEL_PAD, bars.bottom() + 6.0), vec2(grid_w + 4.0, 2.0 * size::SLOT + 4.0));
    widgets::deep(p, grid);
    const KEYS: [&str; 10] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];
    for i in 0..20usize {
        let (row, col) = if i < 10 { (1, i) } else { (0, i - 10) };
        let sr = Rect::from_min_size(grid.min + vec2(2.0 + col as f32 * size::SLOT, 2.0 + row as f32 * size::SLOT), Vec2::splat(size::SLOT));
        let item = pl.hotbar.get(i).copied().flatten();
        // In the sandbox every material has no limit, so there is no count.
        let have = if sandbox { 1 } else { item.map(|it| count_in_inventory(model, it)).unwrap_or(0) };
        let count = item.filter(|_| have > 0 && !sandbox).map(|_| format::count(have));
        let content = SlotContent {
            item,
            count: count.as_deref(),
            label: KEYS.get(i).copied(),
            selected: pl.selected_hotbar == Some(i),
            dim: item.is_some() && have == 0,
            ..Default::default()
        };
        let look = if item.is_some() { SlotLook::Normal } else { SlotLook::Dark };
        let resp = widgets::slot_drag(ui, Id::new(("hotbar", i)), sr, look, &content, cx.atlas);
        if let Some(it) = item {
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Quickbar {}", item::name(&model.content, it))));
            super::drag::source(&resp, super::drag::Drag::Quickbar(i));
        }
        super::drag::quickbar_zone(cx, &resp, i);
        if resp.hovered() {
            match item {
                Some(it) if sandbox => cx.tip(Tip::Item {
                    item: it,
                    amount: Some("Click: paint with it. Right click: clear the slot.".into()),
                }),
                Some(it) => {
                    let amount = match &model.building {
                        Some(b) if item::is_bulk(it) => {
                            Some(format!("Click: move it into the {}. You can also drag it there.", item::building_name(&model.content, b.kind)))
                        }
                        Some(b) => Some(format!(
                            "Click: take it in the hand. Shift + click: move all of it into the {}.",
                            item::building_name(&model.content, b.kind)
                        )),
                        None => None,
                    };
                    cx.tip(Tip::Item { item: it, amount })
                }
                None => cx.tip(Tip::Text {
                    title: "Empty quickbar slot".into(),
                    body: "Hold an item and click here to put it in the quickbar. You can also drag a tank or an item here.".into(),
                }),
            }
        }
        // With a building window open: a click on a material, or Shift + click on a part, moves
        // it into the building.
        let (shift, _) = cx.modifiers();
        let to_building = model.building.is_some() && pl.hand.is_none() && item.is_some_and(|it| item::is_bulk(it) || shift);
        if resp.clicked() && to_building {
            let click = super::inventory::slot_click(cx, &resp).unwrap_or(crate::action::SlotClick::LEFT);
            super::drag::quickbar_to_building(cx, i, click);
        } else if resp.clicked() {
            // In the sandbox the hand always holds the brush, so a click on a full slot selects it.
            match (pl.hand, item) {
                (Some(h), None) => cx.act(UiAction::SetHotbar { index: i, item: Some(h.item) }),
                (Some(h), Some(_)) if !sandbox => cx.act(UiAction::SetHotbar { index: i, item: Some(h.item) }),
                (_, Some(_)) => cx.act(UiAction::SelectHotbar(i)),
                (None, None) => {}
            }
        }
        if resp.secondary_clicked() && item.is_some() {
            cx.act(UiAction::SetHotbar { index: i, item: None });
        }
    }
}

/// Width of the text next to the tank slots in the HUD.
const TANK_TEXT_W: f32 = 196.0;
/// The message when the dig tool finds no room in the tanks.
pub const TANKS_FULL: &str = "Tanks full: put material in a crate, spray it out, or empty a tank";

/// The key or button of the spray tool, from the key list of the settings.
fn spray_key(model: &UiModel) -> String {
    model
        .settings
        .key_bindings
        .iter()
        .find(|r| r.action.starts_with("Spray"))
        .map(|r| r.key.clone())
        .unwrap_or_else(|| "Right mouse".into())
}

/// The tanks next to the quickbar: a slot for each tank (the spray material has an orange
/// frame), which material the spray tool puts out, and a warning when the tanks are full.
fn tank_summary(cx: &mut Cx, qb: Rect) {
    let model = cx.model;
    let tank = &model.player.tank;
    if tank.is_empty() {
        return;
    }
    let cols = tank.len().div_ceil(2).max(1);
    let grid_w = cols as f32 * size::SLOT + 4.0;
    let outer = vec2(grid_w + 2.0 * PANEL_PAD + 10.0 + TANK_TEXT_W, qb.height());
    let rect = Rect::from_min_size(pos2(qb.right() + 8.0, qb.top()), outer);
    panel_area(cx.ctx, "tank", rect, |ui| {
        let p = ui.painter().clone();
        // A building slot dropped on the tanks goes back to the robot.
        let zone = ui.interact(rect, Id::new("hud-tank-drop"), egui::Sense::hover());
        super::drag::robot_zone(cx, &zone);
        if super::drag::dragging_from_building(cx.ctx) {
            super::drag::outline(&p, rect.shrink(3.0));
        }
        let head_y = rect.top() + PANEL_PAD + BARS_H * 0.5;
        p.text(pos2(rect.left() + PANEL_PAD + 2.0, head_y), Align2::LEFT_CENTER, "Tanks", font_bold(text::SMALL), color::HEADING);
        let used: u64 = tank.iter().map(|t| t.units as u64).sum();
        let cap: u64 = tank.iter().map(|t| t.capacity as u64).sum();
        let frac = if cap > 0 { used as f32 / cap as f32 } else { 0.0 };
        let fill_c = if model.player.tanks_full { color::RED_TEXT } else { color::TEXT_DIM };
        p.text(
            pos2(rect.left() + PANEL_PAD + grid_w, head_y),
            Align2::RIGHT_CENTER,
            format!("{} full", format::percent(frac)),
            font_regular(text::SMALL),
            fill_c,
        );
        let grid = Rect::from_min_size(pos2(rect.left() + PANEL_PAD, rect.top() + PANEL_PAD + BARS_H + 6.0), vec2(grid_w, 2.0 * size::SLOT + 4.0));
        widgets::deep(&p, grid);
        for (i, t) in tank.iter().enumerate() {
            let (col, row) = (i / 2, i % 2);
            let sr = Rect::from_min_size(grid.min + vec2(2.0 + col as f32 * size::SLOT, 2.0 + row as f32 * size::SLOT), Vec2::splat(size::SLOT));
            let it = t.material.map(ItemRef::Material);
            let count = it.map(|_| format::count(t.units as u64));
            let content = SlotContent { count: count.as_deref(), ..super::inventory::tank_slot_content(model, t) };
            let resp = widgets::slot_drag(ui, Id::new(("hud-tank", i)), sr, SlotLook::Dark, &content, cx.atlas);
            if let Some(x) = it {
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("HUD tank {}", item::name(&model.content, x))));
                super::drag::source(&resp, super::drag::Drag::Tank(i));
            }
            if resp.hovered() {
                match it {
                    Some(x) => {
                        let what = match &model.building {
                            Some(b) => format!(
                                "Click: move it into the {}. Right click: half. You can also drag it there.",
                                item::building_name(&model.content, b.kind)
                            ),
                            None => "Click to spray this material.".to_string(),
                        };
                        cx.tip(Tip::Item {
                            item: x,
                            amount: Some(format!("{} / {} units. {what}", format::count_full(t.units as u64), format::count_full(t.capacity as u64))),
                        })
                    }
                    None => cx.tip(Tip::Text {
                        title: "Empty tank".into(),
                        body: format!("Holds up to {} units of one material. Dig to fill it.", format::count_full(t.capacity as u64)),
                    }),
                }
            }
            if let Some(click) = super::inventory::slot_click(cx, &resp) {
                cx.act(UiAction::ClickSlot { slot: crate::action::SlotRef::Tank(i), click });
            }
            super::keep::corner_button(ui, cx, Id::new(("hud-keep", i)), sr, t.material);
        }
        // Which material the spray tool puts out, and how.
        let tx = grid.right() + 10.0;
        let mut y = grid.top() + 2.0;
        let spray = model.player.spray.filter(|m| tank.iter().any(|t| t.material == Some(*m) && t.units > 0));
        match spray {
            Some(m) => {
                let it = ItemRef::Material(m);
                p.text(pos2(tx, y), Align2::LEFT_TOP, "Spray tool:", font_regular(text::SMALL), color::TEXT_DIM);
                y += 17.0;
                let icon = Rect::from_min_size(pos2(tx, y), Vec2::splat(18.0));
                cx.atlas.paint(&p, it, icon, Color32::WHITE);
                p.text(pos2(icon.right() + 5.0, icon.center().y), Align2::LEFT_CENTER, item::name(&model.content, it), font_bold(text::BODY), color::ORANGE);
                y += 22.0;
                let how = format!("Hold {} to spray it out.", spray_key(model).to_lowercase());
                y = widgets::wrapped(&p, pos2(tx, y), &how, font_regular(text::SMALL), color::TEXT, TANK_TEXT_W).bottom() + 2.0;
                widgets::wrapped(&p, pos2(tx, y), "Click a tank to choose another.", font_regular(text::SMALL), color::TEXT_FAINT, TANK_TEXT_W);
            }
            None => {
                let text = if used == 0 { "The tanks are empty. Dig to fill them." } else { "Click a tank to choose what the spray tool puts out." };
                widgets::wrapped(&p, pos2(tx, y), text, font_regular(text::SMALL), color::TEXT_DIM, TANK_TEXT_W);
            }
        }
    });
    if model.player.tanks_full {
        tanks_full_warning(cx, rect);
    }
}

/// A red box above the tank panel: the tanks are full, and what to do.
fn tanks_full_warning(cx: &mut Cx, tank_panel: Rect) {
    let w = tank_panel.width();
    let body = widgets::text_height(cx.ctx, TANKS_FULL, font_bold(text::BODY), w - 20.0);
    let rect = Rect::from_min_size(pos2(tank_panel.left(), tank_panel.top() - 8.0 - body - 16.0), vec2(w, body + 16.0));
    widgets::area(cx.ctx, Id::new(("hud", "tanks-full")), Order::Middle, rect, |ui| {
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::same(3), Color32::from_rgba_unmultiplied(70, 16, 12, 235));
        p.rect_stroke(rect, CornerRadius::same(3), Stroke::new(2.0, color::RED), egui::StrokeKind::Inside);
        widgets::wrapped(p, rect.min + vec2(10.0, 8.0), TANKS_FULL, font_bold(text::BODY), color::TEXT, w - 20.0);
    });
}

fn crafting_queue(cx: &mut Cx, screen: Rect) {
    let queue = &cx.model.player.crafting;
    if queue.is_empty() {
        return;
    }
    let shown = queue.len().min(10);
    let grid_w = shown as f32 * size::SLOT + 4.0;
    let outer = vec2(grid_w.max(150.0) + 2.0 * PANEL_PAD, PANEL_PAD + 20.0 + size::SLOT + 4.0 + PANEL_PAD);
    let rect = Rect::from_min_size(pos2(screen.left() + MARGIN, screen.bottom() - MARGIN - outer.y), outer);
    panel_area(cx.ctx, "queue", rect, |ui| {
        let p = ui.painter().clone();
        let total: u32 = cx.model.player.crafting.iter().map(|j| j.count).sum();
        p.text(pos2(rect.left() + PANEL_PAD + 2.0, rect.top() + PANEL_PAD + 9.0), Align2::LEFT_CENTER, "Crafting", font_bold(text::SMALL), color::HEADING);
        p.text(pos2(rect.right() - PANEL_PAD - 2.0, rect.top() + PANEL_PAD + 9.0), Align2::RIGHT_CENTER, format!("{total} left"), font_regular(text::SMALL), color::TEXT_DIM);
        let grid = Rect::from_min_size(pos2(rect.left() + PANEL_PAD, rect.top() + PANEL_PAD + 20.0), vec2(grid_w, size::SLOT + 4.0));
        widgets::deep(&p, grid);
        let (shift, _) = cx.modifiers();
        for (i, job) in cx.model.player.crafting.iter().take(shown).enumerate() {
            let sr = Rect::from_min_size(grid.min + vec2(2.0 + i as f32 * size::SLOT, 2.0), Vec2::splat(size::SLOT));
            let recipe = item::recipe(&cx.model.content, job.recipe);
            let item = recipe.and_then(item::recipe_item);
            let count = format::count(job.count as u64);
            let content = SlotContent { item, count: Some(&count), progress: (i == 0).then_some(job.progress), ..Default::default() };
            let resp = widgets::slot(ui, Id::new(("queue", i)), sr, SlotLook::Normal, &content, cx.atlas);
            if resp.hovered() {
                let name = recipe.map(|r| r.name.as_str()).unwrap_or("Unknown recipe");
                cx.tip(Tip::Text {
                    title: name.to_string(),
                    body: format!("{} in the queue.\nClick: cancel 1. Right click: cancel 5. Shift + click: cancel all.", job.count),
                });
            }
            let n = if resp.clicked() {
                cancel_count(false, shift, job.count)
            } else if resp.secondary_clicked() {
                cancel_count(true, shift, job.count)
            } else {
                0
            };
            if n > 0 {
                cx.act(UiAction::CancelCraft { index: i, count: n });
            }
        }
    });
}

/// The research box at the top left. A click opens the research window. Returns the y below it.
fn research(cx: &mut Cx, st: &mut UiState, screen: Rect) -> f32 {
    let model = cx.model;
    let top = screen.top() + MARGIN;
    let Some(res) = &model.research else { return top };
    let Some(tech) = item::tech(&model.content, res.tech) else { return top };
    let icon_item = item::tech_icon(&model.content, tech);
    let outer = vec2(330.0, 84.0);
    let rect = Rect::from_min_size(screen.min + vec2(MARGIN, MARGIN), outer);
    panel_area(cx.ctx, "research", rect, |ui| {
        let p = ui.painter().clone();
        let resp = ui.interact(rect, Id::new("hud-research"), egui::Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Open research"));
        if resp.hovered() {
            p.rect_filled(rect.shrink(1.0), CornerRadius::same(1), Color32::from_white_alpha(8));
        }
        let icon = Rect::from_min_size(rect.min + vec2(PANEL_PAD + 2.0, PANEL_PAD + 2.0), Vec2::splat(64.0));
        widgets::deep(&p, icon.expand(2.0));
        if let Some(it) = icon_item {
            cx.atlas.paint(&p, it, icon.shrink(8.0), Color32::WHITE);
        }
        let x = icon.right() + 12.0;
        p.text(pos2(x, rect.top() + 12.0), Align2::LEFT_TOP, "Research", font_regular(text::SMALL), color::TEXT_DIM);
        p.text(pos2(x, rect.top() + 26.0), Align2::LEFT_TOP, &tech.name, font_bold(text::BODY + 1.0), color::HEADING);
        let bar = Rect::from_min_max(pos2(x, rect.bottom() - 26.0), pos2(rect.right() - PANEL_PAD - 4.0, rect.bottom() - 12.0));
        widgets::bar(&p, bar, res.progress, color::PROGRESS, Some(&format::percent(res.progress)));
        // Kits per unit, as small icons on the right of the title line.
        let mut kx = rect.right() - PANEL_PAD - 4.0;
        for kit in tech.kits.iter().rev() {
            let r = Rect::from_min_size(pos2(kx - 22.0, rect.top() + 10.0), Vec2::splat(22.0));
            cx.atlas.paint(&p, kit.item, r, Color32::WHITE);
            kx -= 24.0;
        }
        if resp.hovered() {
            let kits: Vec<String> = tech.kits.iter().map(|k| format!("{} × {}", k.count, item::name(&model.content, k.item))).collect();
            let mut body = format!("{}\nProgress: {}.", tech.description, format::percent(res.progress));
            if !kits.is_empty() {
                body.push_str(&format!("\nEach of the {} units needs: {}.", tech.units, kits.join(", ")));
            }
            body.push_str("\nClick to open the research window (T).");
            cx.tip(Tip::Text { title: tech.name.clone(), body });
        }
        if resp.clicked() {
            st.toggle(WindowKind::Research, cx.actions);
        }
    });
    rect.bottom() + 8.0
}

const GUIDE_W: f32 = 330.0;
const GUIDE_PAD: f32 = 10.0;
/// Goals that the guide tracker shows.
const GUIDE_GOALS: usize = 2;

/// One entry of the guide tracker.
struct TrackerRow {
    title: String,
    count: Option<(u32, u32)>,
    /// The hint, with the player's keys.
    text: String,
    /// The title color: white for a goal, yellow for "what comes next".
    color: Color32,
}

/// The rows of the guide tracker: the first open goals that the game can do. When the game can
/// do no more goals, one row says what comes next ("Next: the kiln. It comes in a later
/// update."), so the tracker is never empty while goals are open.
fn tracker_rows(model: &UiModel) -> Vec<TrackerRow> {
    let open = model.guide.iter().filter(|g| !g.done && g.waits_for.is_none()).take(GUIDE_GOALS);
    let mut rows: Vec<TrackerRow> = open
        .map(|g| TrackerRow { title: g.title.clone(), count: g.count, text: model.settings.with_keys(&g.text), color: color::TEXT })
        .collect();
    if let (true, NextGoal::Waiting(g)) = (rows.is_empty(), next_goal(&model.guide)) {
        rows.push(TrackerRow {
            title: "You did every goal for now".into(),
            count: None,
            text: g.next_text().unwrap_or_default(),
            color: color::YELLOW,
        });
    }
    rows
}

/// The height of the guide tracker: the heading, and a title line and the text for each row.
fn tracker_height(ctx: &egui::Context, rows: &[TrackerRow]) -> f32 {
    let text_w = GUIDE_W - 2.0 * GUIDE_PAD;
    let mut h = GUIDE_PAD + 20.0;
    for r in rows {
        h += 22.0;
        if !r.text.is_empty() {
            h += widgets::text_height(ctx, &r.text, font_regular(text::SMALL), text_w);
        }
        h += 6.0;
    }
    h + GUIDE_PAD - 4.0
}

/// The guide tracker on the left side: the next goals, or what comes next. A click opens the
/// guide.
fn guide_tracker(cx: &mut Cx, st: &mut UiState, screen: Rect, top: f32) {
    let model = cx.model;
    if model.sandbox.is_some() {
        return;
    }
    let rows = tracker_rows(model);
    if rows.is_empty() {
        return;
    }
    let h = tracker_height(cx.ctx, &rows);
    let rect = Rect::from_min_size(pos2(screen.left() + MARGIN, top), vec2(GUIDE_W, h));
    panel_area(cx.ctx, "guide", rect, |ui| {
        let p = ui.painter().clone();
        let resp = ui.interact(rect, Id::new("hud-guide"), egui::Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Open guide"));
        if resp.hovered() {
            p.rect_filled(rect.shrink(1.0), CornerRadius::same(1), Color32::from_white_alpha(8));
        }
        let x = rect.left() + GUIDE_PAD;
        let right = rect.right() - GUIDE_PAD;
        let text_w = right - x;
        let mut y = rect.top() + GUIDE_PAD;
        p.text(pos2(x, y + 8.0), Align2::LEFT_CENTER, "Guide", font_bold(text::SMALL), color::HEADING);
        let all = format!("{}: all goals", model.settings.key("guide"));
        p.text(pos2(right, y + 8.0), Align2::RIGHT_CENTER, &all, font_regular(text::SMALL), color::TEXT_FAINT);
        y += 20.0;
        for r in &rows {
            let cy = y + 10.0;
            let count = r.count.map(|(have, need)| format!("{have} / {need}"));
            let count_rect = count.as_ref().map(|c| p.text(pos2(right, cy), Align2::RIGHT_CENTER, c, font_bold(text::BODY), color::TEXT));
            let title_right = count_rect.map(|r| r.left() - 8.0).unwrap_or(right);
            let title = p.layout_no_wrap(r.title.clone(), font_bold(text::BODY), r.color);
            let clip = Rect::from_min_max(pos2(x, y), pos2(title_right, y + 22.0));
            p.with_clip_rect(clip).galley(pos2(x, cy - title.size().y * 0.5), title, r.color);
            y += 22.0;
            if !r.text.is_empty() {
                y = widgets::wrapped(&p, pos2(x, y), &r.text, font_regular(text::SMALL), color::TEXT_DIM, text_w).bottom();
            }
            y += 6.0;
        }
        if resp.hovered() {
            let body = format!("Goals for each tier, with hints. Click to see all goals ({}).", model.settings.key("guide"));
            cx.tip(Tip::Text { title: "Guide".into(), body });
        }
        if resp.clicked() {
            st.toggle(WindowKind::Guide, cx.actions);
        }
    });
}

/// Width of the hover box at the top center.
const HOVER_W: f32 = 380.0;
const HOVER_LINE: f32 = 19.0;
const HOVER_TITLE: f32 = 34.0;
const HOVER_PAD: f32 = 8.0;

/// One line of the hover box.
struct HoverLine {
    text: String,
    color: Color32,
    /// A status dot before the text.
    dot: Option<Color32>,
    /// A progress bar under the text (0 to 1).
    bar: Option<f32>,
}

impl HoverLine {
    fn new(text: impl Into<String>, color: Color32) -> Self {
        Self { text: text.into(), color, dot: None, bar: None }
    }
}

/// The name of a phase for the hover box.
fn phase_name(p: foundry_content::Phase) -> &'static str {
    use foundry_content::Phase;
    match p {
        Phase::Empty => "Empty",
        Phase::Solid => "Solid",
        Phase::Powder => "Powder",
        Phase::Liquid => "Liquid",
        Phase::Gas => "Gas",
        _ => "Fire",
    }
}

/// The lines under the title of the hover box (as in the Minecraft mod WAILA).
fn hover_lines(model: &UiModel, hover: &HoverView) -> Vec<HoverLine> {
    let content = &*model.content;
    let d = &model.hover_detail;
    let mut out = vec![];
    match hover {
        HoverView::Cell { material, temperature, .. } => {
            let m = material.index();
            let phase = content.materials.phase.get(m).copied().unwrap_or_default();
            let tc = if *temperature > 60.0 { temp_color(*temperature) } else { color::TEXT };
            out.push(HoverLine::new(format!("{}  ·  {}", phase_name(phase), format::celsius(*temperature)), tc));
            match &d.dig {
                Some(DigState::CanDig) => out.push(HoverLine::new("Can dig", color::GREEN)),
                Some(DigState::TooHard { needs }) => out.push(HoverLine::new(format!("Too hard: needs {needs}"), color::RED_TEXT)),
                Some(DigState::Never) => out.push(HoverLine::new("Cannot be dug", color::TEXT_DIM)),
                None => {}
            }
            if let Some(b) = content.materials.broken_into.get(m).filter(|b| b.index() != m && !b.is_air()) {
                out.push(HoverLine::new(format!("Breaks into {}", content.materials.names[b.index()]), color::TEXT_DIM));
            }
            if d.undiscovered {
                out.push(HoverLine::new(format!("Not discovered: scan with {} (hold)", model.settings.key("scan")), color::ORANGE));
            }
        }
        HoverView::Building { status, recipe, progress, temperature, .. } => {
            let label = status.label();
            // The reason often starts with the status ("Output full: take out ..."): then it is the whole line.
            let text = if d.reason.is_empty() {
                label.to_string()
            } else if d.reason.to_lowercase().starts_with(&label.to_lowercase()) {
                d.reason.clone()
            } else {
                format!("{label}: {}", d.reason)
            };
            out.push(HoverLine { dot: Some(widgets::status_color(status.color())), ..HoverLine::new(text, color::TEXT) });
            if let Some(rid) = recipe {
                let name = item::recipe(content, *rid).map(|r| r.name.as_str()).unwrap_or("?");
                out.push(HoverLine { bar: Some(*progress), ..HoverLine::new(format!("Recipe: {name}"), color::TEXT) });
            }
            let mut hp = d.hit_points.map(|(h, m)| format!("Hit points {h} / {m}")).unwrap_or_default();
            if let Some(t) = temperature {
                if !hp.is_empty() {
                    hp.push_str("  ·  ");
                }
                hp.push_str(&format::celsius(*t));
            }
            if !hp.is_empty() {
                out.push(HoverLine::new(hp, color::TEXT_DIM));
            }
        }
    }
    out
}

/// Where the hover box is: at the top center, right of the boxes on the left side.
fn hover_rect(model: &UiModel, screen: Rect) -> Option<Rect> {
    let hover = model.hover.as_ref()?;
    // Nothing to tell about air.
    if matches!(hover, HoverView::Cell { material, .. } if material.is_air()) {
        return None;
    }
    let lines = hover_lines(model, hover);
    let bars = lines.iter().filter(|l| l.bar.is_some()).count() as f32;
    let h = HOVER_PAD * 2.0 + HOVER_TITLE + lines.len() as f32 * HOVER_LINE + bars * 8.0;
    let left_column = screen.left() + MARGIN + GUIDE_W.max(330.0) + MARGIN;
    let x = (screen.center().x - HOVER_W * 0.5).max(left_column);
    Some(Rect::from_min_size(pos2(x, screen.top() + MARGIN), vec2(HOVER_W, h)))
}

/// The hover box for the cell or the building under the mouse.
fn hover_box(cx: &mut Cx, screen: Rect) {
    let model = cx.model;
    let Some(hover) = &model.hover else { return };
    let Some(rect) = hover_rect(model, screen) else { return };
    let content = &*model.content;
    let atlas = cx.atlas;
    let lines = hover_lines(model, hover);
    widgets::area(cx.ctx, Id::new(("hud", "hover")), Order::Middle, rect, |ui| {
        let p = ui.painter().clone();
        widgets::window_frame(&p, rect);
        let inner = rect.shrink(HOVER_PAD);
        let (it, title, kind) = match hover {
            HoverView::Cell { material, .. } => {
                let it = ItemRef::Material(*material);
                (Some(it), item::name(content, it), "Material")
            }
            HoverView::Building { kind, .. } => (item::building_item(content, *kind), item::building_name(content, *kind), "Building"),
        };
        let icon = Rect::from_min_size(inner.min, Vec2::splat(HOVER_TITLE - 4.0));
        widgets::deep(&p, icon);
        if let Some(it) = it {
            atlas.paint(&p, it, icon.shrink(3.0), Color32::WHITE);
        }
        let tx = icon.right() + 10.0;
        p.text(pos2(tx, icon.center().y), Align2::LEFT_CENTER, title, font_bold(text::BODY + 1.0), color::HEADING);
        p.text(pos2(inner.right(), icon.center().y), Align2::RIGHT_CENTER, kind, font_regular(text::SMALL), color::TEXT_FAINT);
        let mut y = inner.top() + HOVER_TITLE;
        let clip = p.with_clip_rect(inner);
        for l in &lines {
            let mut x = inner.left() + 2.0;
            if let Some(c) = l.dot {
                widgets::status_dot(&clip, pos2(x + 5.0, y + HOVER_LINE * 0.5), c);
                x += 16.0;
            }
            clip.text(pos2(x, y + HOVER_LINE * 0.5), Align2::LEFT_CENTER, &l.text, font_regular(text::BODY), l.color);
            y += HOVER_LINE;
            if let Some(v) = l.bar {
                widgets::bar(&clip, Rect::from_min_size(pos2(inner.left() + 2.0, y), vec2(inner.width() - 4.0, 5.0)), v, color::PROGRESS, None);
                y += 8.0;
            }
        }
    });
}

fn temp_color(t: f32) -> Color32 {
    if t < 50.0 {
        color::TEXT
    } else if t < 300.0 {
        color::YELLOW
    } else {
        color::RED_TEXT
    }
}

fn alert_color(kind: AlertKind) -> Color32 {
    match kind {
        AlertKind::Fire | AlertKind::TooHot | AlertKind::Damage => color::RED,
        AlertKind::Leak | AlertKind::Flood => Color32::from_rgb(90, 160, 255),
        AlertKind::Gas => Color32::from_rgb(170, 220, 80),
        AlertKind::MachineStopped | AlertKind::LowPower => color::YELLOW,
    }
}

/// A warning triangle in the color of the alert kind.
fn alert_symbol(p: &Painter, r: Rect, kind: AlertKind) {
    let c = alert_color(kind);
    let center = r.center();
    let tri = vec![pos2(center.x, r.top() + 2.0), pos2(r.right() - 1.0, r.bottom() - 2.0), pos2(r.left() + 1.0, r.bottom() - 2.0)];
    p.add(egui::Shape::convex_polygon(tri, c, Stroke::new(1.0, theme::shade(c, 0.5))));
    p.text(center + vec2(0.0, 3.0), Align2::CENTER_CENTER, "!", font_bold(15.0), Color32::from_rgb(20, 20, 20));
}

fn alerts(cx: &mut Cx, screen: Rect) {
    if cx.model.alerts.is_empty() {
        return;
    }
    let row_h = 34.0;
    let width = 340.0;
    let n = cx.model.alerts.len().min(6);
    let h = n as f32 * row_h + 12.0 + 22.0;
    let bottom = screen.bottom() - MARGIN - if screen.width() < 1400.0 { 130.0 } else { 0.0 };
    let rect = Rect::from_min_size(pos2(screen.right() - MARGIN - width, bottom - h), vec2(width, h));
    panel_area(cx.ctx, "alerts", rect, |ui| {
        let p = ui.painter().clone();
        p.text(pos2(rect.left() + 10.0, rect.top() + 14.0), Align2::LEFT_CENTER, "Alerts", font_bold(text::SMALL), color::HEADING);
        let list = Rect::from_min_max(pos2(rect.left() + 6.0, rect.top() + 26.0), pos2(rect.right() - 6.0, rect.bottom() - 6.0));
        widgets::deep(&p, list);
        for (i, a) in cx.model.alerts.iter().take(n).enumerate() {
            let r = Rect::from_min_size(pos2(list.left() + 2.0, list.top() + 2.0 + i as f32 * row_h), vec2(list.width() - 4.0, row_h - 2.0));
            let resp = ui.interact(r, Id::new(("alert", a.id)), egui::Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &a.text));
            if resp.hovered() {
                p.rect_filled(r, CornerRadius::ZERO, theme::mix(color::DEEP, color::BUTTON_HOVER, 0.25));
                cx.tip(Tip::Text { title: a.kind.label().into(), body: format!("{}\nClick to show the place.", a.text) });
            }
            alert_symbol(&p, Rect::from_min_size(r.min + vec2(4.0, 3.0), Vec2::splat(26.0)), a.kind);
            let text_rect = p.text(pos2(r.left() + 38.0, r.center().y), Align2::LEFT_CENTER, &a.text, font(text::BODY), color::TEXT);
            let _ = text_rect;
            if a.count > 1 {
                let badge = format!("×{}", a.count);
                p.text(pos2(r.right() - 8.0, r.center().y), Align2::RIGHT_CENTER, badge, font_bold(text::BODY), alert_color(a.kind));
            }
            if resp.clicked() {
                cx.act(UiAction::ShowAlert(a.id));
            }
        }
    });
}

/// FPS counter and the message line. Drawn in every game state.
pub(crate) fn overlay_text(cx: &mut Cx) {
    let screen = cx.ctx.content_rect();
    // The performance box shows the FPS when it is on.
    let show_fps = cx.model.settings.show_fps && cx.model.perf.is_none();
    let msg = &cx.model.message;
    if !show_fps && msg.is_empty() {
        return;
    }
    let painter = cx.ctx.layer_painter(egui::LayerId::new(Order::Foreground, Id::new("hud-overlay-text")));
    if show_fps {
        widgets::text_shadow(&painter, pos2(screen.right() - MARGIN, screen.top() + 6.0), Align2::RIGHT_TOP, &format!("{:.0} FPS", cx.model.fps), font_bold(text::BODY), color::TEXT);
    }
    if !msg.is_empty() {
        let galley = painter.layout_no_wrap(msg.clone(), font_bold(text::BODY + 1.0), color::TEXT);
        // Under the hover box, so the message does not cover it.
        let top = hover_rect(cx.model, screen).filter(|_| cx.model.state == crate::model::GameState::Playing).map_or(screen.top() + 40.0, |r| r.bottom() + 24.0);
        let r = Rect::from_center_size(pos2(screen.center().x, top), galley.size() + vec2(28.0, 12.0));
        painter.rect_filled(r, CornerRadius::same(3), Color32::from_black_alpha(170));
        painter.galley(r.center() - galley.size() * 0.5, galley, color::TEXT);
    }
}
