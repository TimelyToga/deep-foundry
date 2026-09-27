//! The character screen (key E): the inventory on the left, hand crafting on the right.

use super::inventory::{self, GRID_W, HEAD};
use super::{Cx, window_id};
use crate::action::{UiAction, WindowKind};
use crate::crafting::{click_count, craftable_count};
use crate::item::{CraftGroup, ItemId, RecipeView};
use crate::theme::{color, font_regular, size, text};
use crate::tooltip::Tip;
use crate::widgets::{self, SlotContent, SlotLook};
use crate::UiState;
use egui::{Align2, Id, Order, Rect, Ui, Vec2, pos2, vec2};

/// Rows of the recipe grid that show without scrolling.
const RECIPE_ROWS: usize = 7;
const TAB_H: f32 = 64.0;
const CRAFT_W: f32 = GRID_W + 12.0;

pub(crate) fn show(cx: &mut Cx, st: &mut UiState) {
    let inv = inventory::panel_size(cx);
    let craft_h = HEAD + TAB_H + 12.0 + RECIPE_ROWS as f32 * size::SLOT + 4.0;
    let content = vec2(inv.x + 16.0 + CRAFT_W, inv.y.max(craft_h));
    let outer = widgets::window_outer(content);
    let screen = cx.ctx.content_rect();
    let rect = widgets::place(screen, outer, st.offset(WindowKind::Character) - vec2(0.0, 40.0));
    let id = window_id(WindowKind::Character);
    widgets::area(cx.ctx, id, Order::Middle, rect, |ui| {
        let f = widgets::window(ui, id, rect, "Character", true);
        st.move_window(WindowKind::Character, f.drag);
        if f.close_clicked {
            st.close(WindowKind::Character, cx.actions);
            return;
        }
        let inv_rect = Rect::from_min_size(f.content.min, inv);
        inventory::panel(ui, cx, inv_rect, "");
        let craft_rect = Rect::from_min_size(pos2(inv_rect.right() + 16.0, f.content.top()), vec2(CRAFT_W, craft_h));
        crafting_panel(ui, cx, st, craft_rect);
    });
}

fn group_icon(cx: &Cx, group: CraftGroup) -> Option<ItemId> {
    // The icon of a tab is the first recipe result of that group.
    cx.model.catalog.recipes().iter().filter(|r| r.group == group).find_map(|r| r.main_item())
}

fn short_label(group: CraftGroup) -> &'static str {
    match group {
        CraftGroup::Logistics => "Logistics",
        CraftGroup::Production => "Production",
        CraftGroup::Intermediate => "Intermediate",
        CraftGroup::Power => "Power",
        CraftGroup::Research => "Research",
    }
}

fn matches(r: &RecipeView, search: &str) -> bool {
    search.is_empty() || r.name.to_lowercase().contains(search)
}

fn visible(r: &RecipeView) -> bool {
    r.unlocked && r.hand_craftable()
}

fn crafting_panel(ui: &mut Ui, cx: &mut Cx, st: &mut UiState, r: Rect) {
    let painter = ui.painter().clone();
    widgets::heading(&painter, r.min + vec2(0.0, 4.0), "Crafting");
    // Search field.
    let field = Rect::from_min_size(pos2(r.right() - 200.0, r.top() + 1.0), vec2(200.0, 24.0));
    widgets::text_field(ui, Id::new("craft-search"), field, &mut st.search, "Search recipes");
    let search = st.search.trim().to_lowercase();

    // Count matching recipes per group. Switch tabs if the current one has none.
    let count_in = |g: CraftGroup| cx.model.catalog.recipes().iter().filter(|r| r.group == g && visible(r) && matches(r, &search)).count();
    if count_in(st.craft_tab) == 0
        && let Some(g) = CraftGroup::ALL.into_iter().find(|g| count_in(*g) > 0)
    {
        st.craft_tab = g;
    }

    // Tabs.
    let tabs_top = r.top() + HEAD;
    let tab_w = (CRAFT_W - 4.0 * 4.0) / 5.0;
    for (i, g) in CraftGroup::ALL.into_iter().enumerate() {
        let tr = Rect::from_min_size(pos2(r.left() + i as f32 * (tab_w + 4.0), tabs_top), vec2(tab_w, TAB_H));
        let icon = group_icon(cx, g);
        let n = count_in(g);
        let resp = match icon {
            Some(item) => widgets::icon_tab(ui, Id::new(("craft-tab", i)), tr, item, short_label(g), st.craft_tab == g, n == 0, cx.atlas),
            None => continue,
        };
        if resp.hovered() {
            cx.tip(Tip::Text { title: g.label().into(), body: format!("{n} recipes") });
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

    // Recipes of the tab, in rows. A new data row starts a new grid row.
    let mut list: Vec<&RecipeView> =
        cx.model.catalog.recipes().iter().filter(|r| r.group == st.craft_tab && visible(r) && matches(r, &search)).collect();
    list.sort_by_key(|r| r.row);
    let mut cells: Vec<(usize, &RecipeView)> = vec![];
    let (mut col, mut row) = (0usize, 0usize);
    let mut last_row = list.first().map(|r| r.row);
    for rec in list {
        if Some(rec.row) != last_row || col == inventory::COLS {
            if col > 0 {
                row += 1;
            }
            col = 0;
            last_row = Some(rec.row);
        }
        cells.push((row * inventory::COLS + col, rec));
        col += 1;
    }
    let total_rows = cells.last().map(|(i, _)| i / inventory::COLS + 1).unwrap_or(1);
    let inner = grid.shrink(2.0);
    let content_h = total_rows.max(RECIPE_ROWS) as f32 * size::SLOT;
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
        egui::ScrollArea::vertical().id_salt("recipe-scroll").max_height(inner.height()).auto_shrink([false, false]).show(ui, |ui| {
            let (area, _) = ui.allocate_exact_size(vec2(inner.width(), content_h), egui::Sense::hover());
            let (shift, _) = cx.modifiers();
            for (i, rec) in &cells {
                let sr = Rect::from_min_size(
                    area.min + vec2((i % inventory::COLS) as f32 * size::SLOT, (i / inventory::COLS) as f32 * size::SLOT),
                    Vec2::splat(size::SLOT),
                );
                let can = craftable_count(rec, cx.stock);
                let look = if can > 0 { SlotLook::Normal } else { SlotLook::Red };
                let amount = rec.results.first().map(|x| x.amount).unwrap_or(1);
                let count = (amount > 1).then(|| amount.to_string());
                let content = SlotContent { item: rec.main_item(), count: count.as_deref(), ..Default::default() };
                let resp = widgets::slot(ui, Id::new(("recipe", rec.id.0)), sr, look, &content, cx.atlas);
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &rec.name));
                if resp.hovered() {
                    cx.tip(Tip::Recipe { recipe: rec.id, hand: true });
                }
                let n = if resp.clicked() {
                    click_count(false, shift, can)
                } else if resp.secondary_clicked() {
                    click_count(true, shift, can)
                } else {
                    0
                };
                if n > 0 {
                    cx.act(UiAction::Craft { recipe: rec.id, count: n });
                }
            }
        });
    });
    if cells.is_empty() {
        painter.text(grid.center(), Align2::CENTER_CENTER, "No recipes match the search.", font_regular(text::BODY), color::TEXT_DIM);
    }
}
