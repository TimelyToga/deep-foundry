//! The HUD: quickbar with hull and heat bars, tank summary, crafting queue, research box,
//! entity info panel and alerts.

use super::Cx;
use crate::action::UiAction;
use crate::crafting::cancel_count;
use crate::format;
use crate::item::ItemId;
use crate::model::{AlertKind, HoverView, UiModel};
use crate::theme::{self, color, font, font_bold, font_regular, rgba, size, text};
use crate::tooltip::Tip;
use crate::widgets::{self, SlotContent, SlotLook};
use crate::UiState;
use egui::{Align2, Color32, CornerRadius, Id, Order, Painter, Rect, Stroke, Ui, Vec2, pos2, vec2};

const MARGIN: f32 = 8.0;
const PANEL_PAD: f32 = 6.0;
const BARS_H: f32 = 18.0;

pub(crate) fn show(cx: &mut Cx, _st: &mut UiState) {
    let screen = cx.ctx.content_rect();
    let qb = quickbar(cx, screen);
    tank_summary(cx, qb);
    crafting_queue(cx, screen);
    research(cx, screen);
    entity_info(cx, screen);
    alerts(cx, screen);
}

/// A HUD panel: the window frame without a title.
fn panel_area(ctx: &egui::Context, name: &str, rect: Rect, add: impl FnOnce(&mut Ui)) {
    widgets::area(ctx, Id::new(("hud", name)), Order::Middle, rect, |ui| {
        widgets::window_frame(ui.painter(), rect);
        let _ = ui.interact(rect, Id::new(("hud-bg", name)), egui::Sense::click());
        add(ui);
    });
}

fn count_in_inventory(model: &UiModel, item: ItemId) -> u64 {
    if item.is_bulk() {
        model.player.tank.iter().filter(|t| t.material.map(ItemId::Material) == Some(item)).map(|t| t.units as u64).sum()
    } else {
        model.player.inventory.iter().flatten().filter(|s| s.item == item).map(|s| s.count as u64).sum()
    }
}

fn quickbar(cx: &mut Cx, screen: Rect) -> Rect {
    let ctx = cx.ctx;
    let grid_w = 10.0 * size::SLOT;
    let outer = vec2(grid_w + 2.0 * PANEL_PAD + 4.0, PANEL_PAD + BARS_H + 6.0 + 2.0 * size::SLOT + 4.0 + PANEL_PAD);
    let rect = Rect::from_min_size(pos2((screen.center().x - outer.x * 0.5).round(), screen.bottom() - MARGIN - outer.y), outer);
    panel_area(ctx, "quickbar", rect, |ui| {
        let p = ui.painter().clone();
        let pl = &cx.model.player;
        // Hull and heat bars.
        let bars = Rect::from_min_size(rect.min + vec2(PANEL_PAD, PANEL_PAD), vec2(outer.x - 2.0 * PANEL_PAD, BARS_H));
        let half = (bars.width() - 8.0) * 0.5;
        let hull_r = Rect::from_min_size(bars.min, vec2(half, BARS_H));
        let heat_r = Rect::from_min_size(bars.min + vec2(half + 8.0, 0.0), vec2(half, BARS_H));
        let hull_frac = if pl.hull_max > 0.0 { pl.hull / pl.hull_max } else { 0.0 };
        let hull_color = if hull_frac < 0.3 { color::RED } else { color::HULL };
        widgets::bar(&p, hull_r, hull_frac, hull_color, Some(&format!("Hull {} / {}", pl.hull.round(), pl.hull_max.round())));
        let heat_frac = if pl.heat_limit > 0.0 { (pl.temperature / pl.heat_limit).max(0.0) } else { 0.0 };
        widgets::bar(&p, heat_r, heat_frac, widgets::danger_color(heat_frac), Some(&format!("Heat {} of {}", format::celsius(pl.temperature), format::celsius(pl.heat_limit))));
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

        // Slots: the bottom row is slots 1-10 (keys 1-0), the top row is 11-20 (Shift + 1-0).
        let grid = Rect::from_min_size(pos2(rect.left() + PANEL_PAD, bars.bottom() + 6.0), vec2(grid_w + 4.0, 2.0 * size::SLOT + 4.0));
        widgets::deep(&p, grid);
        const KEYS: [&str; 10] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];
        for i in 0..20usize {
            let (row, col) = if i < 10 { (1, i) } else { (0, i - 10) };
            let sr = Rect::from_min_size(grid.min + vec2(2.0 + col as f32 * size::SLOT, 2.0 + row as f32 * size::SLOT), Vec2::splat(size::SLOT));
            let item = pl.hotbar.get(i).copied().flatten();
            let have = item.map(|it| count_in_inventory(cx.model, it)).unwrap_or(0);
            let count = item.filter(|_| have > 0).map(|_| format::count(have));
            let content = SlotContent {
                item,
                count: count.as_deref(),
                label: if i < 10 { Some(KEYS[i]) } else { None },
                selected: pl.selected_hotbar == Some(i),
                dim: item.is_some() && have == 0,
                ..Default::default()
            };
            let look = if item.is_some() { SlotLook::Normal } else { SlotLook::Dark };
            let resp = widgets::slot(ui, Id::new(("hotbar", i)), sr, look, &content, cx.atlas);
            if resp.hovered() {
                match item {
                    Some(it) => cx.tip(Tip::Item { item: it, amount: None }),
                    None => cx.tip(Tip::Text {
                        title: "Empty quickbar slot".into(),
                        body: "Hold an item and click here to put it in the quickbar.".into(),
                    }),
                }
            }
            if resp.clicked() {
                match pl.hand {
                    Some(h) => cx.act(UiAction::SetHotbar { index: i, item: Some(h.item) }),
                    None if item.is_some() => cx.act(UiAction::SelectHotbar(i)),
                    None => {}
                }
            }
            if resp.secondary_clicked() && item.is_some() {
                cx.act(UiAction::SetHotbar { index: i, item: None });
            }
        }
    });
    rect
}

fn tank_summary(cx: &mut Cx, qb: Rect) {
    let tank = &cx.model.player.tank;
    if tank.is_empty() {
        return;
    }
    let cols = tank.len().div_ceil(2).max(1);
    let grid_w = cols as f32 * size::SLOT + 4.0;
    let outer = vec2(grid_w + 2.0 * PANEL_PAD, qb.height());
    let rect = Rect::from_min_size(pos2(qb.right() + 8.0, qb.top()), outer);
    panel_area(cx.ctx, "tank", rect, |ui| {
        let p = ui.painter().clone();
        p.text(pos2(rect.left() + PANEL_PAD + 2.0, rect.top() + PANEL_PAD + BARS_H * 0.5), Align2::LEFT_CENTER, "Tank", font_bold(text::SMALL), color::HEADING);
        let grid = Rect::from_min_size(pos2(rect.left() + PANEL_PAD, rect.top() + PANEL_PAD + BARS_H + 6.0), vec2(grid_w, 2.0 * size::SLOT + 4.0));
        widgets::deep(&p, grid);
        for (i, t) in cx.model.player.tank.iter().enumerate() {
            let (col, row) = (i / 2, i % 2);
            let sr = Rect::from_min_size(grid.min + vec2(2.0 + col as f32 * size::SLOT, 2.0 + row as f32 * size::SLOT), Vec2::splat(size::SLOT));
            let item = t.material.map(ItemId::Material);
            let fill_color = item.and_then(|it| cx.model.catalog.item(it)).map(|info| rgba(info.color())).unwrap_or(color::GRAY);
            let frac = if t.capacity > 0 { t.units as f32 / t.capacity as f32 } else { 0.0 };
            let count = item.map(|_| format::count(t.units as u64));
            let content = SlotContent { item, count: count.as_deref(), fill: item.map(|_| (frac, fill_color)), ..Default::default() };
            let resp = widgets::slot(ui, Id::new(("hud-tank", i)), sr, SlotLook::Dark, &content, cx.atlas);
            if resp.hovered() {
                match item {
                    Some(it) => cx.tip(Tip::Item { item: it, amount: Some(format!("{} / {} units", t.units, t.capacity)) }),
                    None => cx.tip(Tip::Text { title: "Empty tank slot".into(), body: format!("Holds up to {} units of one material.", t.capacity) }),
                }
            }
        }
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
            let recipe = cx.model.catalog.recipe(job.recipe);
            let item = recipe.and_then(|r| r.main_item());
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

fn research(cx: &mut Cx, screen: Rect) {
    let Some(res) = &cx.model.research else { return };
    let outer = vec2(330.0, 84.0);
    let rect = Rect::from_min_size(screen.min + vec2(MARGIN, MARGIN), outer);
    panel_area(cx.ctx, "research", rect, |ui| {
        let p = ui.painter().clone();
        let icon = Rect::from_min_size(rect.min + vec2(PANEL_PAD + 2.0, PANEL_PAD + 2.0), Vec2::splat(64.0));
        widgets::deep(&p, icon.expand(2.0));
        cx.atlas.paint(&p, res.icon, icon.shrink(8.0), Color32::WHITE);
        let x = icon.right() + 12.0;
        p.text(pos2(x, rect.top() + 12.0), Align2::LEFT_TOP, "Research", font_regular(text::SMALL), color::TEXT_DIM);
        p.text(pos2(x, rect.top() + 26.0), Align2::LEFT_TOP, &res.name, font_bold(text::BODY + 1.0), color::HEADING);
        let bar = Rect::from_min_max(pos2(x, rect.bottom() - 26.0), pos2(rect.right() - PANEL_PAD - 4.0, rect.bottom() - 12.0));
        widgets::bar(&p, bar, res.progress, color::PROGRESS, Some(&format::percent(res.progress)));
        // Kits per unit, as small icons on the right of the title line.
        let mut kx = rect.right() - PANEL_PAD - 4.0;
        for kit in res.kits.iter().rev() {
            let r = Rect::from_min_size(pos2(kx - 22.0, rect.top() + 10.0), Vec2::splat(22.0));
            cx.atlas.paint(&p, kit.item, r, Color32::WHITE);
            kx -= 24.0;
        }
        let resp = ui.interact(rect, Id::new("hud-research"), egui::Sense::hover());
        if resp.hovered() {
            let kits: Vec<String> = res.kits.iter().map(|k| format!("{} × {}", k.count, cx.model.catalog.name(k.item))).collect();
            cx.tip(Tip::Text {
                title: res.name.clone(),
                body: format!("Research in progress: {}.\nEach unit needs: {}.", format::percent(res.progress), kits.join(", ")),
            });
        }
    });
}

fn info_line(p: &Painter, y: &mut f32, x: f32, w: f32, label: &str, value: &str, vc: Color32) {
    p.text(pos2(x, *y), Align2::LEFT_TOP, label, font_regular(text::BODY), color::TEXT_DIM);
    p.text(pos2(x + w, *y), Align2::RIGHT_TOP, value, font(text::BODY), vc);
    *y += 21.0;
}

fn entity_info(cx: &mut Cx, screen: Rect) {
    let Some(hover) = &cx.model.hover else { return };
    let width = 300.0;
    let (lines, has_bar) = match hover {
        HoverView::Cell { .. } => (4, false),
        HoverView::Building { recipe, temperature, power_w, .. } => {
            (1 + recipe.is_some() as usize + temperature.is_some() as usize + power_w.is_some() as usize, recipe.is_some())
        }
    };
    let top_offset = if cx.model.settings.show_fps { 30.0 } else { MARGIN };
    let h = 58.0 + lines as f32 * 21.0 + if has_bar { 22.0 } else { 0.0 } + 8.0;
    let rect = Rect::from_min_size(pos2(screen.right() - MARGIN - width, screen.top() + top_offset), vec2(width, h));
    let model = cx.model;
    let atlas = cx.atlas;
    widgets::area(cx.ctx, Id::new(("hud", "entity")), Order::Middle, rect, |ui| {
        let p = ui.painter().clone();
        widgets::window_frame(&p, rect);
        let inner = rect.shrink(10.0);
        let (item, title) = match hover {
            HoverView::Cell { material, .. } => (ItemId::Material(*material), model.catalog.name(ItemId::Material(*material)).to_string()),
            HoverView::Building { item, .. } => (*item, model.catalog.name(*item).to_string()),
        };
        let icon = Rect::from_min_size(inner.min, Vec2::splat(40.0));
        widgets::deep(&p, icon);
        atlas.paint(&p, item, icon.shrink(4.0), Color32::WHITE);
        p.text(pos2(icon.right() + 10.0, inner.top() + 1.0), Align2::LEFT_TOP, &title, font_bold(text::BODY + 1.0), color::HEADING);
        let kind = model.catalog.item(item).map(|i| i.kind.label()).unwrap_or("");
        p.text(pos2(icon.right() + 10.0, inner.top() + 21.0), Align2::LEFT_TOP, kind, font_regular(text::SMALL), color::TEXT_DIM);
        let mut y = icon.bottom() + 8.0;
        let x = inner.left();
        let w = inner.width();
        match hover {
            HoverView::Cell { pos, temperature, .. } => {
                let tc = temp_color(*temperature);
                info_line(&p, &mut y, x, w, "Temperature", &format::celsius(*temperature), tc);
                if let Some(info) = model.catalog.item(item) {
                    for f in info.facts.iter().take(2) {
                        info_line(&p, &mut y, x, w, &f.label, &f.value, color::TEXT);
                    }
                }
                info_line(&p, &mut y, x, w, "Position", &format!("{}, {}", pos.x, pos.y), color::TEXT_DIM);
            }
            HoverView::Building { status, recipe, progress, temperature, power_w, .. } => {
                let sc = widgets::status_color(status.color());
                widgets::status_dot(&p, pos2(x + 6.0, y + 10.0), sc);
                p.text(pos2(x + 18.0, y), Align2::LEFT_TOP, status.label(), font(text::BODY), color::TEXT);
                y += 21.0;
                if let Some(rid) = recipe {
                    let name = model.catalog.recipe(*rid).map(|r| r.name.as_str()).unwrap_or("?");
                    info_line(&p, &mut y, x, w, "Recipe", name, color::TEXT);
                    let bar = Rect::from_min_size(pos2(x, y + 1.0), vec2(w, 14.0));
                    widgets::bar(&p, bar, *progress, color::PROGRESS, None);
                    y += 22.0;
                }
                if let Some((t, max)) = temperature {
                    info_line(&p, &mut y, x, w, "Temperature", &format!("{} / {}", format::celsius(*t), format::celsius(*max)), widgets::danger_color(t / max.max(1.0)));
                }
                if let Some(pw) = power_w {
                    info_line(&p, &mut y, x, w, "Power", &format::watts(*pw), color::TEXT);
                }
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

/// A small symbol for an alert kind, drawn with lines.
fn alert_symbol(p: &Painter, r: Rect, kind: AlertKind) {
    let c = alert_color(kind);
    let center = r.center();
    // Warning triangle background.
    let tri = vec![pos2(center.x, r.top() + 2.0), pos2(r.right() - 1.0, r.bottom() - 2.0), pos2(r.left() + 1.0, r.bottom() - 2.0)];
    p.add(egui::Shape::convex_polygon(tri, theme::shade(c, 0.35), Stroke::new(2.0, c)));
    let sym = match kind {
        AlertKind::Fire => "F",
        AlertKind::Leak => "L",
        AlertKind::MachineStopped => "!",
        AlertKind::LowPower => "P",
        AlertKind::Gas => "G",
        AlertKind::Flood => "W",
        AlertKind::TooHot => "H",
        AlertKind::Damage => "D",
    };
    p.text(center + vec2(0.0, 3.0), Align2::CENTER_CENTER, sym, font_bold(12.0), color::TEXT);
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
    let show_fps = cx.model.settings.show_fps;
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
        let r = Rect::from_center_size(pos2(screen.center().x, screen.top() + 40.0), galley.size() + vec2(28.0, 12.0));
        painter.rect_filled(r, CornerRadius::same(3), Color32::from_black_alpha(170));
        painter.galley(r.center() - galley.size() * 0.5, galley, color::TEXT);
    }
}
