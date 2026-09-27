//! The research window (key T): all technologies by tier. Each technology shows its state, its
//! cost, the recipes it unlocks, the lock reasons, and a Research button.

use super::{Cx, window_id};
use crate::action::{UiAction, WindowKind};
use crate::format;
use crate::item;
use crate::model::{TechEntry, TechState};
use crate::theme::{self, color, font, font_bold, font_regular, text};
use crate::tooltip::Tip;
use crate::widgets::{self, ButtonKind, SlotContent, SlotLook};
use crate::UiState;
use egui::{Align2, Color32, Id, Order, Painter, Rect, Ui, Vec2, pos2, vec2};
use foundry_content::{Content, Tech};
use std::collections::BTreeMap;

const WIDTH: f32 = 820.0;
/// The line with the discovery points above the list.
const TOP_H: f32 = 34.0;
/// A tier heading in the list.
pub(crate) const TIER_H: f32 = 34.0;
const ICON: f32 = 48.0;
/// A slot of an unlocked recipe.
const UNLOCK: f32 = 36.0;
const UNLOCK_COLS: usize = 6;
const BUTTON_W: f32 = 110.0;
const PAD: f32 = 8.0;
/// Space between two technologies.
const GAP: f32 = 4.0;
/// Height of the name line and of the cost line.
const LINE_H: f32 = 24.0;

pub(crate) fn show(cx: &mut Cx, st: &mut UiState) {
    let ctx = cx.ctx;
    let screen = ctx.content_rect();
    let height = (screen.height() - 160.0).clamp(320.0, 680.0);
    let outer = widgets::window_outer(vec2(WIDTH, height));
    let rect = widgets::place(screen, outer, st.offset(WindowKind::Research) - vec2(0.0, 40.0));
    let id = window_id(WindowKind::Research);
    widgets::area(ctx, id, Order::Middle, rect, |ui| {
        let f = widgets::window(ui, id, rect, "Research", true);
        st.move_window(WindowKind::Research, f.drag);
        if f.close_clicked {
            st.close(WindowKind::Research, cx.actions);
            return;
        }
        top_line(ui, cx, f.content);
        let list = Rect::from_min_max(pos2(f.content.left(), f.content.top() + TOP_H), f.content.right_bottom());
        tech_list(ui, cx, list);
    });
}

/// The discovery points on the left, a short hint on the right.
fn top_line(ui: &Ui, cx: &mut Cx, r: Rect) {
    let p = ui.painter();
    let cy = r.top() + 13.0;
    let label = p.text(pos2(r.left(), cy), Align2::LEFT_CENTER, "Discovery points", font(text::BODY), color::TEXT_DIM);
    let value = p.text(pos2(label.right() + 8.0, cy), Align2::LEFT_CENTER, cx.model.discovery_points.to_string(), font_bold(text::BODY + 2.0), color::HEADING);
    p.text(
        pos2(r.right(), cy),
        Align2::RIGHT_CENTER,
        "Research a technology: the technologies it needs are done first.",
        font_regular(text::SMALL),
        color::TEXT_FAINT,
    );
    let hover = Rect::from_min_max(pos2(label.left(), r.top()), pos2(value.right(), r.top() + 26.0));
    if ui.interact(hover, Id::new("research-points"), egui::Sense::hover()).hovered() {
        cx.tip(Tip::Text {
            title: "Discovery points".into(),
            body: "You get discovery points when you scan a new material, see a new reaction or finish a guide goal. \
                   Some technologies need them."
                .into(),
        });
    }
}

/// One line of the list.
enum Row<'a> {
    Tier { tier: u8, done: usize, total: usize },
    Tech { entry: &'a TechEntry, tech: &'a Tech, height: f32 },
}

impl Row<'_> {
    fn height(&self) -> f32 {
        match self {
            Row::Tier { .. } => TIER_H,
            Row::Tech { height, .. } => height + GAP,
        }
    }
}

/// The places of the parts of a technology card.
struct Columns {
    text_left: f32,
    text_right: f32,
    unlock_left: f32,
    button_left: f32,
}

impl Columns {
    fn new(card: Rect) -> Self {
        let button_left = card.right() - PAD - BUTTON_W;
        let unlock_left = button_left - 12.0 - (UNLOCK_COLS as f32 * UNLOCK + 4.0);
        Self { text_left: card.left() + PAD + ICON + 12.0, text_right: unlock_left - 16.0, unlock_left, button_left }
    }

    fn text_width(&self) -> f32 {
        self.text_right - self.text_left
    }
}

fn reasons_text(e: &TechEntry) -> String {
    e.reasons.join("\n")
}

fn shows_progress(e: &TechEntry) -> bool {
    e.progress > 0.0 && e.state != TechState::Done
}

/// The height of a technology card.
fn card_height(ctx: &egui::Context, e: &TechEntry, tech: &Tech, text_w: f32) -> f32 {
    let mut text_h = PAD + 2.0 * LINE_H;
    if !e.reasons.is_empty() {
        text_h += widgets::text_height(ctx, &reasons_text(e), font_regular(text::SMALL), text_w) + 2.0;
    } else if shows_progress(e) {
        text_h += 20.0;
    }
    text_h += PAD;
    let unlock_rows = tech.unlocks.len().div_ceil(UNLOCK_COLS);
    let unlock_h = if unlock_rows > 0 { PAD + 18.0 + unlock_rows as f32 * UNLOCK + PAD } else { 0.0 };
    text_h.max(unlock_h).max(ICON + 2.0 * PAD)
}

fn tech_list(ui: &mut Ui, cx: &mut Cx, list: Rect) {
    let model = cx.model;
    let content = &*model.content;
    widgets::deep(ui.painter(), list);
    if model.techs.is_empty() {
        ui.painter().text(list.center(), Align2::CENTER_CENTER, "No technologies yet.", font_regular(text::BODY), color::TEXT_FAINT);
        return;
    }
    // Group by tier, in the order of the model.
    let mut tiers: BTreeMap<u8, Vec<(&TechEntry, &Tech)>> = BTreeMap::new();
    for e in &model.techs {
        if let Some(t) = item::tech(content, e.id) {
            tiers.entry(t.tier).or_default().push((e, t));
        }
    }
    let inner = list.shrink(2.0);
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
        egui::ScrollArea::vertical().id_salt("research-list").auto_shrink([false, false]).show(ui, |ui| {
            let width = ui.available_width();
            let text_w = Columns::new(Rect::from_min_size(egui::Pos2::ZERO, vec2(width - 2.0 * PAD, 10.0))).text_width();
            let mut rows = vec![];
            for (tier, list) in &tiers {
                let done = list.iter().filter(|(e, _)| e.state == TechState::Done).count();
                rows.push(Row::Tier { tier: *tier, done, total: list.len() });
                for (e, t) in list {
                    rows.push(Row::Tech { entry: e, tech: t, height: card_height(cx.ctx, e, t, text_w) });
                }
            }
            let total: f32 = rows.iter().map(Row::height).sum::<f32>() + PAD;
            let (area, _) = ui.allocate_exact_size(vec2(width, total), egui::Sense::hover());
            let p = ui.painter().clone();
            let mut y = area.top();
            for row in &rows {
                match row {
                    Row::Tier { tier, done, total } => {
                        let r = Rect::from_min_size(pos2(area.left() + PAD, y), vec2(width - 2.0 * PAD, TIER_H));
                        tier_heading(&p, r, *tier, &format!("{done} of {total} done"));
                    }
                    Row::Tech { entry, tech, height } => {
                        let card = Rect::from_min_size(pos2(area.left() + PAD, y), vec2(width - 2.0 * PAD, *height));
                        tech_card(ui, cx, &p, card, entry, tech);
                    }
                }
                y += row.height();
            }
        });
    });
}

/// A tier heading: a band in the tier color, "Tier 1: Steam", and a count on the right.
pub(crate) fn tier_heading(p: &Painter, r: Rect, tier: u8, right: &str) {
    let band = Rect::from_min_size(pos2(r.left(), r.top() + 8.0), vec2(4.0, r.height() - 14.0));
    p.rect_filled(band, 0.0, theme::tier_color(tier));
    let cy = band.center().y;
    p.text(pos2(r.left() + 12.0, cy), Align2::LEFT_CENTER, item::tier_title(tier), font_bold(text::BODY + 1.0), color::HEADING);
    p.text(pos2(r.right() - 4.0, cy), Align2::RIGHT_CENTER, right, font_regular(text::SMALL), color::TEXT_DIM);
}

fn state_look(state: TechState) -> (&'static str, Color32) {
    match state {
        TechState::Done => ("Done", color::GREEN),
        TechState::Researching => ("Researching", color::ORANGE),
        TechState::Available => ("Available", color::STORAGE),
        TechState::Locked => ("Locked", color::GRAY),
    }
}

fn card_fill(state: TechState) -> Color32 {
    match state {
        TechState::Done => theme::mix(color::SHALLOW, color::GREEN, 0.10),
        TechState::Researching => theme::mix(color::SHALLOW, color::ORANGE, 0.16),
        TechState::Available => color::SHALLOW,
        TechState::Locked => theme::shade(color::SHALLOW, 0.8),
    }
}

/// The cost of a technology for a tooltip: kits, units, time, discovery points and discoveries.
fn cost_text(content: &Content, tech: &Tech) -> String {
    let mut lines = vec![];
    if !tech.kits.is_empty() {
        let kits: Vec<String> = tech.kits.iter().map(|k| format!("{} × {}", k.count, item::name(content, k.item))).collect();
        lines.push(format!("Each of the {} units needs: {}. One unit takes {} in a lab.", tech.units, kits.join(", "), format::seconds(tech.unit_time)));
    }
    if tech.discovery_points > 0 {
        lines.push(format!("Needs {} discovery points.", tech.discovery_points));
    }
    if !tech.discoveries.is_empty() {
        let names: Vec<String> = tech.discoveries.iter().map(|d| item::discovery_name(content, d)).collect();
        lines.push(format!("Needs these discoveries: {}.", names.join(", ")));
    }
    if lines.is_empty() {
        lines.push("No cost.".into());
    }
    lines.join("\n")
}

fn tech_card(ui: &mut Ui, cx: &mut Cx, p: &Painter, card: Rect, e: &TechEntry, tech: &Tech) {
    let model = cx.model;
    let content = &*model.content;
    let fill = card_fill(e.state);
    widgets::raised(p, card, fill, theme::shade(fill, 1.3), theme::shade(fill, 0.6));
    let cols = Columns::new(card);
    let dim = matches!(e.state, TechState::Done | TechState::Locked);

    // Icon.
    let icon = Rect::from_min_size(card.min + vec2(PAD, PAD), Vec2::splat(ICON));
    widgets::deep(p, icon);
    if let Some(it) = item::tech_icon(content, tech) {
        let tint = if e.state == TechState::Locked { Color32::from_gray(140) } else { Color32::WHITE };
        cx.atlas.paint(p, it, icon.shrink(6.0), tint);
    }

    // Name, state and queue place.
    let x = cols.text_left;
    let name_y = card.top() + PAD + LINE_H * 0.5;
    let name_color = if dim { color::TEXT_DIM } else { color::HEADING };
    let name = p.text(pos2(x, name_y), Align2::LEFT_CENTER, &tech.name, font_bold(text::BODY + 1.0), name_color);
    let (label, badge_color) = state_look(e.state);
    let badge = widgets::badge(p, pos2(name.right() + 10.0, name_y), label, badge_color);
    if let Some(q) = e.queue_position {
        p.text(pos2(badge.right() + 8.0, name_y), Align2::LEFT_CENTER, format!("Queue: {}", q + 1), font_regular(text::SMALL), color::TEXT_DIM);
    }

    // Cost: kits per unit × units, discovery points, discoveries.
    let cost_y = name_y + LINE_H;
    let cost_color = if dim { color::TEXT_DIM } else { color::TEXT };
    let mut cx_ = x;
    for kit in &tech.kits {
        let r = Rect::from_min_size(pos2(cx_, cost_y - 11.0), Vec2::splat(22.0));
        cx.atlas.paint(p, kit.item, r, Color32::WHITE);
        let t = p.text(pos2(r.right() + 2.0, cost_y), Align2::LEFT_CENTER, kit.count.to_string(), font_bold(text::SMALL), cost_color);
        cx_ = t.right() + 8.0;
    }
    let mut parts = vec![];
    if !tech.kits.is_empty() {
        parts.push(format!("× {} units", tech.units));
    }
    if tech.discovery_points > 0 {
        parts.push(format!("{} discovery points", tech.discovery_points));
    }
    if !tech.discoveries.is_empty() {
        let names: Vec<String> = tech.discoveries.iter().map(|d| item::discovery_name(content, d)).collect();
        parts.push(format!("Scan: {}", names.join(", ")));
    }
    if parts.is_empty() {
        parts.push("No cost".into());
    }
    let cost = p.text(pos2(cx_, cost_y), Align2::LEFT_CENTER, parts.join("    "), font(text::SMALL), cost_color);
    let cost_rect = Rect::from_min_max(pos2(x, cost_y - 11.0), pos2(cost.right().max(x + 40.0), cost_y + 11.0));

    // Lock reasons or progress.
    let line3 = cost_y + LINE_H * 0.5 + 2.0;
    if !e.reasons.is_empty() {
        widgets::wrapped(p, pos2(x, line3), &reasons_text(e), font_regular(text::SMALL), color::RED_TEXT, cols.text_width());
    } else if shows_progress(e) {
        let bar = Rect::from_min_size(pos2(x, line3 + 2.0), vec2(cols.text_width(), 14.0));
        widgets::bar(p, bar, e.progress, color::PROGRESS, Some(&format::percent(e.progress)));
    }

    // Recipes it unlocks.
    if !tech.unlocks.is_empty() {
        p.text(pos2(cols.unlock_left, card.top() + PAD), Align2::LEFT_TOP, "Unlocks", font_regular(text::SMALL), color::TEXT_FAINT);
        let top = card.top() + PAD + 18.0;
        for (i, rid) in tech.unlocks.iter().enumerate() {
            let Some(rec) = item::recipe(content, *rid) else { continue };
            let r = Rect::from_min_size(
                pos2(cols.unlock_left + (i % UNLOCK_COLS) as f32 * UNLOCK, top + (i / UNLOCK_COLS) as f32 * UNLOCK),
                Vec2::splat(UNLOCK),
            );
            let slot = SlotContent { item: item::recipe_item(rec), ..Default::default() };
            let resp = widgets::slot(ui, Id::new(("tech-unlock", e.id.0, i)), r, SlotLook::Normal, &slot, cx.atlas);
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Unlocks {}", rec.name)));
            if resp.hovered() {
                cx.tip(Tip::Recipe { recipe: *rid, hand: false });
            }
        }
    }

    // Research button.
    if e.state == TechState::Available {
        let br = Rect::from_min_size(pos2(cols.button_left, card.center().y - 16.0), vec2(BUTTON_W, 32.0));
        if widgets::button(ui, Id::new(("tech-research", e.id.0)), br, "Research", ButtonKind::Confirm, true).clicked() {
            cx.act(UiAction::StartResearch(e.id));
        }
    }

    // Tooltips: the description on the icon and the name, the cost on the cost line.
    let name_line = Rect::from_min_max(pos2(x, name_y - 11.0), pos2(badge.right(), name_y + 11.0));
    let icon_hover = ui.interact(icon, Id::new(("tech-icon", e.id.0)), egui::Sense::hover()).hovered();
    let name_hover = ui.interact(name_line, Id::new(("tech-name", e.id.0)), egui::Sense::hover()).hovered();
    if icon_hover || name_hover {
        let mut body = format!("{}\n{}.", tech.description, item::tier_title(tech.tier));
        if !tech.effects.is_empty() && tech.unlocks.is_empty() {
            body.push_str("\nIt changes how the robot or the factory works. It unlocks no recipes.");
        }
        cx.tip(Tip::Text { title: tech.name.clone(), body });
    } else if ui.interact(cost_rect, Id::new(("tech-cost", e.id.0)), egui::Sense::hover()).hovered() {
        cx.tip(Tip::Text { title: "Cost".into(), body: cost_text(content, tech) });
    }
}
