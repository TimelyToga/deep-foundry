//! Rich tooltips in the Factorio layout: a title bar with the name, then the description,
//! facts, recipe (ingredients in red when the player does not have enough), crafting time,
//! "Made in", and "Used in".
//!
//! Screens do not draw tooltips themselves. They set a [`Tip`] while the mouse is over
//! something. The UI draws the tooltip last, so it is on top of everything.

use crate::crafting::{Stock, craftable_count};
use crate::format;
use crate::icons::IconAtlas;
use crate::item::{self, Maker};
use crate::model::UiModel;
use crate::theme::{color, font, font_bold, font_regular, text};
use crate::widgets;
use egui::{Align2, Color32, CornerRadius, Id, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use foundry_content::{Content, ItemRef, Recipe};
use foundry_core::RecipeId;

/// What the tooltip shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Tip {
    /// An item. `amount` is an extra line, for example "1240 / 2000 units".
    Item { item: ItemRef, amount: Option<String> },
    /// A recipe in the crafting menu (`hand` = true) or the recipe selector.
    Recipe { recipe: RecipeId, hand: bool },
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
            let content = &model.content;
            match tip {
                Tip::Item { item, amount } => item_tip(ui, *item, amount.as_deref(), content, stock, atlas),
                Tip::Recipe { recipe, hand } => {
                    if let Some(r) = item::recipe(content, *recipe) {
                        recipe_tip(ui, r, *hand, model, stock, atlas);
                    }
                }
                Tip::Text { title, body } => {
                    header(ui, title, None);
                    if !body.is_empty() {
                        wrapped(ui, body, color::TEXT);
                    }
                }
            }
            ui.add_space(8.0);
            let full = Rect::from_min_max(top, pos2(top.x + WIDTH, ui.cursor().min.y));
            let mut shapes = vec![];
            for i in 1..=4 {
                shapes.push(egui::Shape::rect_filled(
                    full.expand(i as f32).translate(vec2(0.0, 2.0)),
                    CornerRadius::same(3),
                    Color32::from_black_alpha(30),
                ));
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
fn wrapped(ui: &mut Ui, s: &str, c: Color32) {
    let galley = ui.painter().layout(s.to_string(), font_regular(text::BODY), c, INNER);
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

/// "Label: value" on one line. Long values wrap below the label.
fn fact(ui: &mut Ui, label: &str, value: &str, value_color: Color32) {
    let p = ui.painter().clone();
    let label_galley = p.layout_no_wrap(format!("{label}:"), font_regular(text::BODY), color::TEXT_DIM);
    let lw = label_galley.size().x + 6.0;
    let value_galley = p.layout(value.to_string(), font(text::BODY), value_color, INNER - lw);
    let h = value_galley.size().y.max(20.0);
    let (r, _) = ui.allocate_exact_size(vec2(WIDTH, h), Sense::hover());
    p.galley(pos2(r.left() + 10.0, r.top()), label_galley, color::TEXT_DIM);
    p.galley(pos2(r.left() + 10.0 + lw, r.top()), value_galley, value_color);
}

/// One ingredient or result: icon, "4 × Name", and on the right the amount the player has.
fn item_line(ui: &mut Ui, atlas: &IconAtlas, item: ItemRef, text_left: &str, text_right: Option<(&str, Color32)>, c: Color32) {
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

fn amount_text(content: &Content, it: ItemRef, amount: u32) -> String {
    if item::is_bulk(it) {
        format!("{} units {}", amount, item::name(content, it))
    } else {
        format!("{} × {}", amount, item::name(content, it))
    }
}

fn makers_text(content: &Content, makers: &[Maker]) -> String {
    let names: Vec<&str> = makers
        .iter()
        .map(|m| match m {
            Maker::Hand => "Hand",
            Maker::Building(kind) => item::building_name(content, *kind),
        })
        .collect();
    if names.is_empty() { "Nowhere yet".into() } else { names.join(", ") }
}

fn names_list(content: &Content, recipes: impl Iterator<Item = RecipeId>, max: usize) -> String {
    let mut names: Vec<&str> = vec![];
    for id in recipes {
        if let Some(r) = item::recipe(content, id)
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

fn item_tip(ui: &mut Ui, it: ItemRef, amount: Option<&str>, content: &Content, stock: &Stock, atlas: &IconAtlas) {
    header(ui, item::name(content, it), Some(item::kind(content, it).label()));
    let desc = item::description(content, it);
    if !desc.is_empty() {
        wrapped(ui, &desc, color::TEXT);
    }
    match amount {
        Some(a) => fact(ui, "Amount", a, color::TEXT),
        None => {
            let have = stock.get(it);
            if have > 0 {
                let unit = if item::is_bulk(it) { " units" } else { "" };
                fact(ui, "In inventory", &format!("{have}{unit}"), color::TEXT);
            }
        }
    }
    for f in item::facts(content, it) {
        fact(ui, &f.label, &f.value, color::TEXT);
    }
    if let Some(r) = content.factory.recipes_making(it).filter_map(|id| item::recipe(content, id)).next() {
        section(ui, "Recipe");
        for input in &r.inputs {
            item_line(ui, atlas, input.item, &amount_text(content, input.item, input.count), None, color::TEXT);
        }
        clock_line(ui, r.time, "Crafting time");
        fact(ui, "Made in", &makers_text(content, &item::makers(content, r)), color::TEXT);
    }
    let mut used = content.factory.recipes_using(it).peekable();
    if used.peek().is_some() {
        section(ui, "Used in");
        wrapped(ui, &names_list(content, used, 6), color::TEXT);
    }
}

fn recipe_tip(ui: &mut Ui, r: &Recipe, hand: bool, model: &UiModel, stock: &Stock, atlas: &IconAtlas) {
    let content = &*model.content;
    header(ui, &r.name, Some("Recipe"));
    if let Some(main) = item::recipe_item(r) {
        let desc = item::description(content, main);
        if !desc.is_empty() {
            wrapped(ui, &desc, color::TEXT);
        }
    }
    section(ui, "Ingredients");
    for input in &r.inputs {
        let have = stock.get(input.item);
        let enough = have >= input.count as u64;
        let c = if enough { color::TEXT } else { color::RED_TEXT };
        let right = format!("have {}", format::count(have));
        let rc = if enough { color::TEXT_DIM } else { color::RED_TEXT };
        item_line(ui, atlas, input.item, &amount_text(content, input.item, input.count), Some((&right, rc)), c);
    }
    let speed = if hand { model.player.craft_speed.max(0.01) } else { 1.0 };
    clock_line(ui, r.time / speed, if hand && speed != 1.0 { "Crafting time (workbench)" } else { "Crafting time" });
    if let Some(t) = r.min_temp {
        fact(ui, "Needs heat", &format!("{} or more", format::celsius(t as f32)), color::YELLOW);
    }
    let many = r.outputs.len() > 1 || r.outputs.first().is_some_and(|x| x.count > 1) || !r.byproducts.is_empty();
    if many {
        section(ui, "Results");
        for out in &r.outputs {
            item_line(ui, atlas, out.item, &amount_text(content, out.item, out.count), None, color::TEXT);
        }
        for (b, chance) in &r.byproducts {
            let ch = format!("{}% chance", (chance * 100.0).round());
            item_line(ui, atlas, b.item, &amount_text(content, b.item, b.count), Some((&ch, color::TEXT_DIM)), color::TEXT);
        }
    }
    ui.add_space(2.0);
    fact(ui, "Made in", &makers_text(content, &item::makers(content, r)), color::TEXT);
    if let Some(main) = item::recipe_item(r) {
        let mut used = content.factory.recipes_using(main).peekable();
        if used.peek().is_some() {
            fact(ui, "Used in", &names_list(content, used, 3), color::TEXT);
        }
    }
    if hand {
        let can = craftable_count(r, stock);
        ui.add_space(4.0);
        if can > 0 {
            fact(ui, "You can make", &can.to_string(), color::GREEN);
        } else {
            fact(ui, "You can make", "0 (not enough ingredients)", color::RED_TEXT);
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

/// Paint a small label with a dark background at a position (used by the graph hover).
pub fn small_label(p: &egui::Painter, at: Pos2, s: &str) {
    let galley = p.layout_no_wrap(s.to_string(), font(text::SMALL), color::TEXT);
    let r = Rect::from_min_size(at, galley.size() + vec2(12.0, 6.0));
    p.rect_filled(r, CornerRadius::same(2), color::TOOLTIP);
    widgets::text_shadow(p, r.center(), Align2::CENTER_CENTER, s, font(text::SMALL), color::TEXT);
}
