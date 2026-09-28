//! The character screen (key E): the inventory on the left, hand crafting on the right.

use super::inventory::{self, GRID_W, HEAD};
use super::{Cx, window_id};
use crate::action::{UiAction, WindowKind};
use crate::crafting::{click_count, craftable_count};
use crate::item::{self, CraftGroup};
use crate::theme::{color, font_regular, size, text};
use crate::tooltip::Tip;
use crate::widgets::{self, SlotContent, SlotLook};
use crate::UiState;
use crate::model::UiModel;
use egui::{Align2, Id, Order, Rect, Ui, Vec2, pos2, vec2};
use foundry_content::{Content, ItemRef, Recipe};
use foundry_core::RecipeId;

/// Rows of the recipe grid that show without scrolling.
const RECIPE_ROWS: usize = 7;
const TAB_H: f32 = 64.0;
const CRAFT_W: f32 = GRID_W + 12.0;

pub(crate) fn show(cx: &mut Cx, st: &mut UiState) {
    if cx.model.sandbox.is_some() {
        return show_sandbox(cx, st);
    }
    let ctx = cx.ctx;
    let inv = inventory::panel_size(cx);
    // The keep or drop list of dug materials is under the inventory.
    let keep_h = super::keep::list_height(cx);
    let craft_h = HEAD + TAB_H + 12.0 + RECIPE_ROWS as f32 * size::SLOT + 4.0;
    let content = vec2(inv.x + 16.0 + CRAFT_W, (inv.y + keep_h).max(craft_h));
    let outer = widgets::window_outer(content);
    let rect = widgets::place(ctx.content_rect(), outer, st.offset(WindowKind::Character) - vec2(0.0, 40.0));
    let id = window_id(WindowKind::Character);
    widgets::area(ctx, id, Order::Middle, rect, |ui| {
        let f = widgets::window(ui, id, rect, "Character", true);
        st.move_window(WindowKind::Character, f.drag);
        if f.close_clicked {
            st.close(WindowKind::Character, cx.actions);
            return;
        }
        let inv_rect = Rect::from_min_size(f.content.min, inv);
        inventory::panel(ui, cx, st, inv_rect, "");
        super::keep::list(ui, cx, Rect::from_min_size(inv_rect.left_bottom(), vec2(inv.x, keep_h)));
        let craft_rect = Rect::from_min_size(pos2(inv_rect.right() + 16.0, f.content.top()), vec2(CRAFT_W, craft_h));
        crafting_panel(ui, cx, st, craft_rect);
    });
}

const BRUSH_W: f32 = 300.0;

/// The sandbox character screen: all materials on the left, the brush on the right.
fn show_sandbox(cx: &mut Cx, st: &mut UiState) {
    let ctx = cx.ctx;
    let inv = inventory::panel_size(cx);
    let content = vec2(inv.x + 16.0 + BRUSH_W, inv.y.max(376.0));
    let outer = widgets::window_outer(content);
    let rect = widgets::place(ctx.content_rect(), outer, st.offset(WindowKind::Character) - vec2(0.0, 40.0));
    let id = window_id(WindowKind::Character);
    widgets::area(ctx, id, Order::Middle, rect, |ui| {
        let f = widgets::window(ui, id, rect, "Sandbox", true);
        st.move_window(WindowKind::Character, f.drag);
        if f.close_clicked {
            st.close(WindowKind::Character, cx.actions);
            return;
        }
        let inv_rect = Rect::from_min_size(f.content.min, inv);
        inventory::panel(ui, cx, st, inv_rect, "");
        let brush = Rect::from_min_size(pos2(inv_rect.right() + 16.0, f.content.top()), vec2(BRUSH_W, content.y));
        brush_panel(ui, cx, brush);
    });
}

/// The material in the hand, the brush size, and the paint controls.
fn brush_panel(ui: &mut Ui, cx: &mut Cx, r: Rect) {
    let model = cx.model;
    let content = &*model.content;
    let p = ui.painter().clone();
    widgets::heading(&p, r.min + vec2(0.0, 4.0), "Brush");
    let panel = Rect::from_min_max(pos2(r.left(), r.top() + HEAD), r.right_bottom());
    widgets::shallow(&p, panel);
    let x = panel.left() + 10.0;
    let mut y = panel.top() + 10.0;
    let icon = Rect::from_min_size(pos2(x, y), Vec2::splat(56.0));
    widgets::deep(&p, icon);
    match model.player.hand {
        Some(hand) => {
            cx.atlas.paint(&p, hand.item, icon.shrink(4.0), egui::Color32::WHITE);
            p.text(pos2(icon.right() + 10.0, y + 6.0), Align2::LEFT_TOP, item::name(content, hand.item), crate::theme::font_bold(text::TITLE), color::HEADING);
            p.text(pos2(icon.right() + 10.0, y + 30.0), Align2::LEFT_TOP, item::kind(content, hand.item).label(), font_regular(text::SMALL), color::TEXT_DIM);
        }
        None => {
            p.text(pos2(icon.right() + 10.0, icon.center().y), Align2::LEFT_CENTER, "Nothing in the hand", crate::theme::font(text::BODY), color::TEXT_DIM);
        }
    }
    y = icon.bottom() + 12.0;
    let radius = model.sandbox.map(|s| s.brush_radius).unwrap_or(0);
    p.text(pos2(x, y), Align2::LEFT_TOP, format!("Size: {} cells across", radius * 2 + 1), crate::theme::font(text::BODY), color::TEXT);
    y += 30.0;
    let k = |id: &str| model.settings.first_key(id).to_string();
    let lines = [
        ("Left mouse".to_string(), "paint"),
        ("Right mouse".to_string(), "erase"),
        (format!("{}  and  {}", k("brush_smaller"), k("brush_larger")), "brush size"),
        (format!("{} - {}", k("quickbar1"), k("quickbar10")), "quickbar materials"),
        (k("pipette"), "empty the hand"),
        ("Mouse wheel".to_string(), "zoom"),
        (format!("{} {} {} {}", k("camera_up"), k("camera_left"), k("camera_down"), k("camera_right")), "move the view"),
        (k("pause"), "pause the simulation"),
    ];
    for (key, what) in lines {
        p.text(pos2(x, y), Align2::LEFT_TOP, key, crate::theme::font_bold(text::BODY), color::HEADING);
        p.text(pos2(x + 110.0, y), Align2::LEFT_TOP, what, font_regular(text::BODY), color::TEXT);
        y += 21.0;
    }
    let tip = "Hold a material and click a quickbar slot to put it there.";
    let galley = p.layout(tip.to_string(), font_regular(text::SMALL), color::TEXT_DIM, panel.width() - 20.0);
    p.galley(pos2(x, y + 8.0), galley, color::TEXT_DIM);
}

/// Recipes that show in the hand crafting menu: known and hand-craftable.
fn hand_recipes(model: &UiModel) -> impl Iterator<Item = (RecipeId, &Recipe)> + '_ {
    model
        .content
        .factory
        .recipes
        .iter()
        .enumerate()
        .filter(|(_, r)| r.hand && item::recipe_known(r, &model.finished_techs))
        .map(|(i, r)| (RecipeId(i as u16), r))
}

/// Recipes with the same key stay together in the grid (plates with plates, gears with gears).
fn row_key<'a>(content: &'a Content, r: &Recipe) -> &'a str {
    match item::recipe_item(r) {
        Some(ItemRef::Part(p)) => match content.factory.parts.get(p.0 as usize) {
            Some(part) => match part.building.and_then(|b| content.factory.buildings.get(b.0 as usize)) {
                Some(b) => b.kind.as_str(),
                None => part.icon.as_deref().unwrap_or("part"),
            },
            None => "part",
        },
        Some(ItemRef::Material(_)) => "material",
        None => "none",
    }
}

fn matches(r: &Recipe, search: &str) -> bool {
    search.is_empty() || r.name.to_lowercase().contains(search)
}

fn crafting_panel(ui: &mut Ui, cx: &mut Cx, st: &mut UiState, r: Rect) {
    let painter = ui.painter().clone();
    let model = cx.model;
    let content = model.content.clone();
    widgets::heading(&painter, r.min + vec2(0.0, 4.0), "Crafting");
    if cx.model.player.craft_speed > 1.0 {
        painter.text(
            pos2(r.left() + 78.0, r.top() + 14.0),
            Align2::LEFT_CENTER,
            format!("Speed × {} (workbench)", cx.model.player.craft_speed),
            font_regular(text::SMALL),
            color::GREEN,
        );
    }
    // Search field.
    let field = Rect::from_min_size(pos2(r.right() - 200.0, r.top() + 1.0), vec2(200.0, 24.0));
    widgets::text_field(ui, Id::new("craft-search"), field, &mut st.search, "Search recipes");
    let search = st.search.trim().to_lowercase();

    // Count matching recipes per group. Switch tabs if the current one has none.
    let mut counts = [0usize; 5];
    for (_, rec) in hand_recipes(model).filter(|(_, rec)| matches(rec, &search)) {
        counts[CraftGroup::parse(&rec.group) as usize] += 1;
    }
    if counts[st.craft_tab as usize] == 0
        && let Some(g) = CraftGroup::ALL.into_iter().find(|g| counts[*g as usize] > 0)
    {
        st.craft_tab = g;
    }

    // Tabs.
    let tabs_top = r.top() + HEAD;
    let tab_w = (CRAFT_W - 4.0 * 4.0) / 5.0;
    for (i, g) in CraftGroup::ALL.into_iter().enumerate() {
        let tr = Rect::from_min_size(pos2(r.left() + i as f32 * (tab_w + 4.0), tabs_top), vec2(tab_w, TAB_H));
        // The icon of a tab is the first recipe result of that group.
        let icon = content.factory.recipes.iter().filter(|x| CraftGroup::parse(&x.group) == g).find_map(item::recipe_item);
        let n = counts[g as usize];
        let Some(icon) = icon else { continue };
        let resp = widgets::icon_tab(ui, Id::new(("craft-tab", i)), tr, icon, g.short_label(), st.craft_tab == g, n == 0, cx.atlas);
        if resp.hovered() {
            cx.tip(Tip::Text { title: g.label().into(), body: format!("{n} recipes you can make by hand.") });
        }
        if resp.clicked() {
            st.craft_tab = g;
        }
    }

    // Panel with the recipe grid.
    let panel = Rect::from_min_max(pos2(r.left(), tabs_top + TAB_H), r.right_bottom());
    widgets::shallow(&painter, panel);
    let grid = Rect::from_min_size(panel.min + vec2(6.0, 6.0), vec2(GRID_W, RECIPE_ROWS as f32 * size::SLOT + 4.0));
    widgets::deep(&painter, grid);

    // Recipes of the tab, grouped by kind, in data order.
    let mut keys: Vec<&str> = vec![];
    let mut list: Vec<(usize, RecipeId, &Recipe)> = vec![];
    for (id, rec) in hand_recipes(model).filter(|(_, rec)| CraftGroup::parse(&rec.group) == st.craft_tab && matches(rec, &search)) {
        let k = row_key(&content, rec);
        let ki = keys.iter().position(|x| *x == k).unwrap_or_else(|| {
            keys.push(k);
            keys.len() - 1
        });
        list.push((ki, id, rec));
    }
    // Recipes of the same kind stay next to each other; the rows are full, so the grid stays small.
    list.sort_by_key(|x| x.0);
    let cells: Vec<(usize, RecipeId, &Recipe)> = list.into_iter().enumerate().map(|(i, (_, id, rec))| (i, id, rec)).collect();
    let total_rows = cells.last().map(|(i, _, _)| i / inventory::COLS + 1).unwrap_or(1);
    let inner = grid.shrink(2.0);
    let content_h = total_rows.max(RECIPE_ROWS) as f32 * size::SLOT;
    let (shift, _) = cx.modifiers();
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
        egui::ScrollArea::vertical().id_salt("recipe-scroll").max_height(inner.height()).auto_shrink([false, false]).show(ui, |ui| {
            let (area, _) = ui.allocate_exact_size(vec2(inner.width(), content_h), egui::Sense::hover());
            for (i, id, rec) in &cells {
                let sr = Rect::from_min_size(
                    area.min + vec2((i % inventory::COLS) as f32 * size::SLOT, (i / inventory::COLS) as f32 * size::SLOT),
                    Vec2::splat(size::SLOT),
                );
                let can = craftable_count(rec, cx.stock);
                let look = if can > 0 { SlotLook::Normal } else { SlotLook::Red };
                let amount = rec.outputs.first().map(|x| x.count).unwrap_or(1);
                let count = (amount > 1).then(|| amount.to_string());
                let slot_content = SlotContent { item: item::recipe_item(rec), count: count.as_deref(), ..Default::default() };
                let resp = widgets::slot(ui, Id::new(("recipe", id.0)), sr, look, &slot_content, cx.atlas);
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Craft {}", rec.name)));
                if resp.hovered() {
                    cx.tip(Tip::Recipe { recipe: *id, hand: true });
                }
                let n = if resp.clicked() {
                    click_count(false, shift, can)
                } else if resp.secondary_clicked() {
                    click_count(true, shift, can)
                } else {
                    0
                };
                if n > 0 {
                    cx.act(UiAction::Craft { recipe: *id, count: n });
                }
            }
        });
    });
    if cells.is_empty() {
        painter.text(grid.center(), Align2::CENTER_CENTER, "No recipes match the search.", font_regular(text::BODY), color::TEXT_DIM);
    }
}
