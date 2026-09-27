//! Production statistics (key P): graphs and lists of what was made and used per minute.

use super::power::{draw_graph, series_color};
use super::{Cx, window_id};
use crate::action::WindowKind;
use crate::format;
use crate::graph::TimeRange;
use crate::model::ProductionRow;
use crate::theme::{color, font, font_bold, font_regular, text};
use crate::tooltip::Tip;
use crate::widgets;
use crate::UiState;
use egui::{Align2, Color32, CornerRadius, Id, Order, Painter, Rect, Ui, Vec2, pos2, vec2};

const WIDTH: f32 = 900.0;
const GRAPH_H: f32 = 210.0;
const ROW_H: f32 = 34.0;
const LIST_ROWS: usize = 9;
/// Items with a line in the graph.
const GRAPH_ITEMS: usize = 6;

fn per_minute(v: f32) -> String {
    format::per_minute(v)
}

fn per_minute_f64(v: f64) -> String {
    format::per_minute(v as f32)
}

pub(crate) fn show(cx: &mut Cx, st: &mut UiState) {
    let ctx = cx.ctx;
    let list_h = 26.0 + LIST_ROWS as f32 * ROW_H + 4.0;
    let content = vec2(WIDTH, 34.0 + 24.0 + GRAPH_H + 16.0 + list_h);
    let outer = widgets::window_outer(content);
    let rect = widgets::place(ctx.content_rect(), outer, st.offset(WindowKind::Production) - vec2(0.0, 40.0));
    let id = window_id(WindowKind::Production);
    widgets::area(ctx, id, Order::Middle, rect, |ui| {
        let f = widgets::window(ui, id, rect, "Production statistics", true);
        st.move_window(WindowKind::Production, f.drag);
        if f.close_clicked {
            st.close(WindowKind::Production, cx.actions);
            return;
        }
        let p = ui.painter().clone();
        let r = f.content;
        let mut y = r.top();
        p.text(pos2(r.left(), y + 13.0), Align2::LEFT_CENTER, "Time range", font(text::BODY), color::TEXT_DIM);
        for (i, tr) in TimeRange::ALL.into_iter().enumerate() {
            let br = Rect::from_min_size(pos2(r.left() + 90.0 + i as f32 * 52.0, y), vec2(48.0, 26.0));
            if widgets::toggle(ui, Id::new(("st-range", i)), br, tr.label(), st.stats_range == tr).clicked() {
                st.stats_range = tr;
            }
        }
        p.text(pos2(r.right(), y + 13.0), Align2::RIGHT_CENTER, "Amounts are averages per minute.", font_regular(text::SMALL), color::TEXT_FAINT);
        y += 34.0;
        let range = st.stats_range;
        let rows = &cx.model.stats.rows;
        let made = sorted(rows, range, true);
        let used = sorted(rows, range, false);
        let col_w = (r.width() - 12.0) * 0.5;
        let g1 = Rect::from_min_size(pos2(r.left(), y + 24.0), vec2(col_w, GRAPH_H));
        let g2 = Rect::from_min_size(pos2(r.left() + col_w + 12.0, y + 24.0), vec2(col_w, GRAPH_H));
        widgets::heading(&p, pos2(g1.left(), y), "Made");
        widgets::heading(&p, pos2(g2.left(), y), "Used");
        let made_series: Vec<(&[f32], Color32)> =
            made.iter().take(GRAPH_ITEMS).enumerate().map(|(i, row)| (row.made.samples(range), series_color(i))).collect();
        let used_series: Vec<(&[f32], Color32)> =
            used.iter().take(GRAPH_ITEMS).enumerate().map(|(i, row)| (row.used.samples(range), series_color(i))).collect();
        draw_graph(ui, &p, g1, None, &made_series, per_minute_f64, range, "st-g1");
        draw_graph(ui, &p, g2, None, &used_series, per_minute_f64, range, "st-g2");
        y += 24.0 + GRAPH_H + 16.0;
        let l1 = Rect::from_min_size(pos2(r.left(), y), vec2(col_w, list_h));
        let l2 = Rect::from_min_size(pos2(r.left() + col_w + 12.0, y), vec2(col_w, list_h));
        item_list(ui, cx, &p, l1, &made, range, true, "made");
        item_list(ui, cx, &p, l2, &used, range, false, "used");
    });
}

/// Rows with a value in this range, largest first.
fn sorted(rows: &[ProductionRow], range: TimeRange, made: bool) -> Vec<&ProductionRow> {
    let value = |r: &ProductionRow| if made { r.made.average(range) } else { r.used.average(range) };
    let mut v: Vec<&ProductionRow> = rows.iter().filter(|r| value(r) > 0.0).collect();
    v.sort_by(|a, b| value(b).total_cmp(&value(a)));
    v
}

#[allow(clippy::too_many_arguments)]
fn item_list(ui: &mut Ui, cx: &mut Cx, p: &Painter, r: Rect, rows: &[&ProductionRow], range: TimeRange, made: bool, key: &str) {
    let title = if made { "Made per minute" } else { "Used per minute" };
    widgets::heading(p, r.min, title);
    let list = Rect::from_min_max(pos2(r.left(), r.top() + 26.0), r.right_bottom());
    widgets::deep(p, list);
    let value = |row: &ProductionRow| if made { row.made.average(range) } else { row.used.average(range) };
    let max = rows.iter().map(|x| value(x)).fold(0.0f32, f32::max).max(0.0001);
    let inner = list.shrink(2.0);
    let atlas = cx.atlas;
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
        egui::ScrollArea::vertical().id_salt(("st-list", key)).auto_shrink([false, false]).show(ui, |ui| {
            let (area, _) = ui.allocate_exact_size(vec2(inner.width(), rows.len() as f32 * ROW_H), egui::Sense::hover());
            let p = ui.painter().clone();
            for (i, row) in rows.iter().enumerate() {
                let rr = Rect::from_min_size(pos2(area.left(), area.top() + i as f32 * ROW_H), vec2(area.width(), ROW_H));
                if i % 2 == 1 {
                    p.rect_filled(rr, CornerRadius::ZERO, Color32::from_white_alpha(4));
                }
                let slot = Rect::from_min_size(rr.min + vec2(4.0, 3.0), Vec2::splat(28.0));
                widgets::raised(&p, slot, color::SLOT, color::SLOT_LIGHT, color::SLOT_DARK);
                atlas.paint(&p, row.item, slot.shrink(2.0), Color32::WHITE);
                if i < GRAPH_ITEMS {
                    p.rect_filled(Rect::from_min_size(pos2(slot.right() + 6.0, rr.center().y - 5.0), Vec2::splat(10.0)), CornerRadius::same(1), series_color(i));
                }
                let name = cx.model.catalog.name(row.item);
                p.text(pos2(slot.right() + 22.0, rr.top() + 9.0), Align2::LEFT_CENTER, name, font(text::BODY), color::TEXT);
                let v = value(row);
                let bar = Rect::from_min_max(pos2(slot.right() + 22.0, rr.bottom() - 12.0), pos2(rr.right() - 100.0, rr.bottom() - 6.0));
                widgets::bar(&p, bar, v / max, if made { color::GREEN } else { color::PROGRESS }, None);
                p.text(pos2(rr.right() - 8.0, rr.center().y), Align2::RIGHT_CENTER, per_minute(v), font_bold(text::BODY), color::TEXT);
                if ui.interact(rr, Id::new(("st-row", key, i)), egui::Sense::hover()).hovered() {
                    let other = if made { row.used.average(range) } else { row.made.average(range) };
                    cx.tip(Tip::Text {
                        title: name.to_string(),
                        body: format!(
                            "Made: {}\nUsed: {}\nAverage over the last {}.",
                            per_minute(if made { v } else { other }),
                            per_minute(if made { other } else { v }),
                            range.label()
                        ),
                    });
                }
            }
        });
    });
    if rows.is_empty() {
        p.text(list.center(), Align2::CENTER_CENTER, "Nothing in this time range.", font_regular(text::BODY), color::TEXT_FAINT);
    }
}
