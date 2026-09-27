//! Rich tooltips in the Factorio layout: a title bar with the name, then the description,
//! facts, recipe (ingredients in red when the player does not have enough), crafting time,
//! "Made in", and "Used in".
//!
//! Screens do not draw tooltips themselves. They set a [`Tip`] while the mouse is over
//! something. The UI draws the tooltip last, so it is on top of everything.

use crate::crafting::Stock;
use crate::format;
use crate::icons::IconAtlas;
use crate::item::{ItemId, Maker, RecipeView};
use crate::model::UiModel;
use crate::theme::{color, font, font_bold, font_regular, text};
use crate::widgets;
use egui::{Align2, Color32, CornerRadius, Id, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};

/// What the tooltip shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Tip {
    /// An item. `amount` is an extra line, for example "1,240 / 2,000 units".
    Item { item: ItemId, amount: Option<String> },
    /// A recipe in the crafting menu or the recipe selector.
    Recipe { recipe: foundry_core::RecipeId, hand: bool },
    /// Plain text with a title.
    Text { title: String, body: String },
}

const WIDTH: f32 = 340.0;
const INNER: f32 = WIDTH - 20.0;

/// Draw the tooltip near the mouse.
pub fn show(ctx: &egui::Context, tip: &Tip, model: &UiModel, stock: &Stock, atlas: &IconAtlas) {
    let Some(pointer) = ctx.pointer_hover_pos() else { return };
    let id = Id::new("foundry-tooltip");
    let screen = ctx.content_rect();
    // Use the size from the last frame to keep the tooltip on the screen.
    let last = egui::AreaState::load(ctx, id).and_then(|s| s.size).unwrap_or(vec2(WIDTH, 120.0));
    let mut pos = pointer + vec2(20.0, 20.0);
    if pos.x + last.x > screen.right() - 4.0 {
        pos.x = (pointer.x - 16.0 - last.x).max(screen.left() + 4.0);
    }
    if pos.y + last.y > screen.bottom() - 4.0 {
        pos.y = (screen.bottom() - 4.0 - last.y).max(screen.top() + 4.0);
    }
    egui::Area::new(id)
        .order(egui::Order::Tooltip)
        .fixed_pos(pos.round())
        .interactable(false)
        .fade_in(false)
        .constrain(false)
        .show(ctx, |ui| {
            ui.set_width(WIDTH);
            let bg = ui.painter().add(egui::Shape::Noop);
            let top = ui.cursor().min;
            match tip {
                Tip::Item { item, amount } => item_tip(ui, *item, amount.as_deref(), model, stock, atlas),
                Tip::Recipe { recipe, hand } => {
                    if let Some(r) = model.catalog.recipe(*recipe) {
                        recipe_tip(ui, r, *hand, model, stock, atlas);
                    }
                }
                Tip::Text { title, body } => {
                    header(ui, title, None);
                    if !body.is_empty() {
                        wrapped(ui, body, color::TEXT, false);
                    }
                }
            }
            ui.add_space(8.0);
            let full = Rect::from_min_max(top, pos2(top.x + WIDTH, ui.cursor().min.y));
            let mut shapes = vec![];
            for i in 1..=4 {
                shapes.push(egui::Shape::rect_filled(full.expand(i as f32).translate(vec2(0.0, 2.0)), CornerRadius::same(3), Color32::from_black_alpha(30)));
            }
            shapes.push(egui::Shape::rect_filled(full.expand(1.0), CornerRadius::same(2), Color32::from_black_alpha(230)));
            shapes.push(egui::Shape::rect_filled(full, CornerRadius::same(2), color::TOOLTIP));
            ui.painter().set(bg, egui::Shape::Vec(shapes));
        });
}

/// The title bar of a tooltip.
fn header(ui: &mut Ui, title: &str, kind: Option<&str>) {
    let (r, _) = ui.allocate_exact_size(vec2(WIDTH, 34.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, CornerRadius { nw: 2, ne: 2, sw: 0, se: 0 }, color::TOOLTIP_HEADER);
    p.hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, Color32::from_black_alpha(160)));
    let title_rect = p.text(pos2(r.left() + 10.0, r.center().y), Align2::LEFT_CENTER, title, font_bold(text::TITLE), color::HEADING);
    if let Some(k) = kind {
        let x = (r.right() - 10.0).max(title_rect.right() + 8.0);
        p.text(pos2(x, r.center().y + 1.0), Align2::RIGHT_CENTER, k, font_regular(text::SMALL), color::TEXT_DIM);
    }
    ui.add_space(4.0);
}

/// A paragraph of wrapped text.
fn wrapped(ui: &mut Ui, s: &str, c: Color32, bold: bool) {
    let f = if bold { font_bold(text::BODY) } else { font_regular(text::BODY) };
    let galley = ui.painter().layout(s.to_string(), f, c, INNER);
    let (r, _) = ui.allocate_exact_size(vec2(WIDTH, galley.size().y + 4.0), Sense::hover());
    ui.painter().galley(pos2(r.left() + 10.0, r.top() + 2.0), galley, c);
}

/// A section title with a line.
fn section(ui: &mut Ui, title: &str) {
    ui.add_space(4.0);
    let (r, _) = ui.allocate_exact_size(vec2(WIDTH, 24.0), Sense::hover());
    let p = ui.painter();
    p.hline((r.left() + 10.0)..=(r.right() - 10.0), r.top() + 1.5, Stroke::new(1.0, color::TOOLTIP_LINE));
    p.text(pos2(r.left() + 10.0, r.center().y + 2.0), Align2::LEFT_CENTER, title, font_bold(text::BODY), color::HEADING);
}

/// "Label: value" on one line.
fn fact(ui: &mut Ui, label: &str, value: &str, value_color: Color32) {
    let (r, _) = ui.allocate_exact_size(vec2(WIDTH, 20.0), Sense::hover());
    let p = ui.painter();
    let l = p.text(pos2(r.left() + 10.0, r.center().y), Align2::LEFT_CENTER, format!("{label}:"), font_regular(text::BODY), color::TEXT_DIM);
    p.text(pos2(l.right() + 6.0, r.center().y), Align2::LEFT_CENTER, value, font(text::BODY), value_color);
}

/// One ingredient or result: icon, "4 x Name", and on the right the amount the player has.
fn item_line(ui: &mut Ui, atlas: &IconAtlas, item: ItemId, text_left: &str, text_right: Option<(&str, Color32)>, c: Color32) {
    let (r, _) = ui.allocate_exact_size(vec2(WIDTH, 28.0), Sense::hover());
    let p = ui.painter();
    let icon = Rect::from_min_size(pos2(r.left() + 10.0, r.top() + 2.0), Vec2::splat(24.0));
    p.rect_filled(icon.expand(1.0), CornerRadius::ZERO, color::DEEP);
    atlas.paint(p, item, icon, Color32::WHITE);
    p.text(pos2(icon.right() + 8.0, r.center().y), Align2::LEFT_CENTER, text_left, font(text::BODY), c);
    if let Some((t, tc)) = text_right {
        p.text(pos2(r.right() - 10.0, r.center().y), Align2::RIGHT_CENTER, t, font_regular(text::SMALL), tc);
    }
}

fn clock_line(ui: &mut Ui, seconds: f32, label: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(WIDTH, 26.0), Sense::hover());
    let p = ui.painter();
    let c = pos2(r.left() + 22.0, r.center().y);
    p.circle_filled(c, 10.0, color::DEEP);
    p.circle_stroke(c, 9.0, Stroke::new(1.5, color::TEXT_DIM));
    p.line_segment([c, c + vec2(0.0, -6.0)], Stroke::new(1.5, color::TEXT));
    p.line_segment([c, c + vec2(4.0, 1.0)], Stroke::new(1.5, color::TEXT));
    let t = p.text(pos2(r.left() + 42.0, r.center().y), Align2::LEFT_CENTER, format::seconds(seconds), font_bold(text::BODY), color::TEXT);
    p.text(pos2(t.right() + 6.0, r.center().y), Align2::LEFT_CENTER, label, font_regular(text::BODY), color::TEXT_DIM);
}

fn amount_text(model: &UiModel, item: ItemId, amount: u32) -> String {
    if item.is_bulk() {
        format!("{} units {}", amount, model.catalog.name(item))
    } else {
        format!("{} × {}", amount, model.catalog.name(item))
    }
}

fn makers_text(model: &UiModel, makers: &[Maker]) -> String {
    let names: Vec<&str> = makers
        .iter()
        .map(|m| match m {
            Maker::Hand => "Hand",
            Maker::Building(kind) => model.catalog.name(ItemId::Building(*kind)),
        })
        .collect();
    names.join(", ")
}

fn names_list(model: &UiModel, recipes: &[foundry_core::RecipeId], max: usize) -> String {
    let mut names: Vec<&str> = vec![];
    for id in recipes {
        if let Some(r) = model.catalog.recipe(*id)
            && !names.contains(&r.name.as_str())
        {
            names.push(&r.name);
        }
    }
    let shown = names.len().min(max);
    let mut s = names[..shown].join(", ");
    if names.len() > shown {
        s.push_str(&format!(" and {} more", names.len() - shown));
    }
    s
}

fn item_tip(ui: &mut Ui, item: ItemId, amount: Option<&str>, model: &UiModel, stock: &Stock, atlas: &IconAtlas) {
    let Some(info) = model.catalog.item(item) else {
        header(ui, "Unknown item", None);
        return;
    };
    header(ui, &info.name, Some(info.kind.label()));
    if !info.description.is_empty() {
        wrapped(ui, &info.description, color::TEXT, false);
    }
    if let Some(a) = amount {
        fact(ui, "Amount", a, color::TEXT);
    } else {
        let have = stock.get(item);
        if have > 0 {
            let unit = if item.is_bulk() { " units" } else { "" };
            fact(ui, "In inventory", &format!("{have}{unit}"), color::TEXT);
        }
    }
    for f in &info.facts {
        fact(ui, &f.label, &f.value, color::TEXT);
    }
    let made_by = model.catalog.made_by(item);
    if let Some(r) = made_by.iter().filter_map(|id| model.catalog.recipe(*id)).find(|r| r.unlocked) {
        section(ui, "Recipe");
        for ing in &r.ingredients {
            item_line(ui, atlas, ing.item, &amount_text(model, ing.item, ing.amount), None, color::TEXT);
        }
        clock_line(ui, r.time, "Crafting time");
        fact(ui, "Made in", &makers_text(model, &r.made_in), color::TEXT);
    }
    let used = model.catalog.used_in(item);
    if !used.is_empty() {
        section(ui, "Used in");
        wrapped(ui, &names_list(model, used, 6), color::TEXT, false);
    }
}

fn recipe_tip(ui: &mut Ui, r: &RecipeView, hand: bool, model: &UiModel, stock: &Stock, atlas: &IconAtlas) {
    header(ui, &r.name, Some("Recipe"));
    if let Some(main) = r.main_item()
        && let Some(info) = model.catalog.item(main)
        && !info.description.is_empty()
    {
        wrapped(ui, &info.description, color::TEXT, false);
    }
    section(ui, "Ingredients");
    for ing in &r.ingredients {
        let have = stock.get(ing.item);
        let enough = have >= ing.amount as u64;
        let c = if enough { color::TEXT } else { color::RED_TEXT };
        let right = format!("have {}", format::count(have));
        item_line(ui, atlas, ing.item, &amount_text(model, ing.item, ing.amount), Some((&right, if enough { color::TEXT_DIM } else { color::RED_TEXT })), c);
    }
    clock_line(ui, r.time, "Crafting time");
    if let Some(t) = r.min_temperature {
        fact(ui, "Needs heat", &format!("{} or more", format::celsius(t)), color::YELLOW);
    }
    if r.results.len() > 1 || r.results.first().is_some_and(|x| x.amount > 1) {
        section(ui, "Results");
        for res in &r.results {
            let chance = if res.chance < 1.0 { Some((format!("{}% chance", (res.chance * 100.0).round()), color::TEXT_DIM)) } else { None };
            item_line(ui, atlas, res.item, &amount_text(model, res.item, res.amount), chance.as_ref().map(|(s, c)| (s.as_str(), *c)), color::TEXT);
        }
    }
    ui.add_space(2.0);
    fact(ui, "Made in", &makers_text(model, &r.made_in), color::TEXT);
    if let Some(main) = r.main_item() {
        let used = model.catalog.used_in(main);
        if !used.is_empty() {
            fact(ui, "Used in", &names_list(model, used, 3), color::TEXT);
        }
    }
    if hand {
        let can = crate::crafting::craftable_count(r, stock);
        ui.add_space(4.0);
        if can > 0 {
            fact(ui, "You can make", &can.to_string(), color::GREEN);
        } else {
            fact(ui, "You can make", "0 (missing ingredients)", color::RED_TEXT);
        }
        let (row, _) = ui.allocate_exact_size(vec2(WIDTH, 20.0), Sense::hover());
        ui.painter().text(
            pos2(row.left() + 10.0, row.center().y),
            Align2::LEFT_CENTER,
            "Click: make 1    Right click: 5    Shift + click: all",
            font_regular(text::SMALL),
            color::TEXT_FAINT,
        );
    }
}

/// Paint a small tooltip-like label at a position (used by the graph hover).
pub fn small_label(p: &egui::Painter, at: Pos2, s: &str) {
    let galley = p.layout_no_wrap(s.to_string(), font(text::SMALL), color::TEXT);
    let r = Rect::from_min_size(at, galley.size() + vec2(12.0, 6.0));
    p.rect_filled(r, CornerRadius::same(2), color::TOOLTIP);
    widgets::text_shadow(p, r.center(), Align2::CENTER_CENTER, s, font(text::SMALL), color::TEXT);
}
