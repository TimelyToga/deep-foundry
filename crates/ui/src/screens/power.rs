//! The power network window (Factorio "electric network info"): satisfaction, production,
//! storage and current bars, warnings, lists of consumers and producers by building type,
//! and graphs over time.

use super::{Cx, window_id};
use crate::action::WindowKind;
use crate::format;
use crate::item;
use crate::graph::{self, TimeRange};
use crate::model::{PowerEntry, PowerWarning};
use crate::theme::{self, color, font, font_bold, font_regular, text};
use crate::tooltip::Tip;
use crate::widgets;
use crate::UiState;
use egui::{Align2, Color32, CornerRadius, Id, Order, Painter, Rect, Shape, Stroke, Ui, Vec2, pos2, vec2};

const WIDTH: f32 = 780.0;
const SUMMARY_H: f32 = 108.0;
const LIST_ROWS: usize = 5;
const ROW_H: f32 = 38.0;
const GRAPH_H: f32 = 190.0;

pub(crate) fn show(cx: &mut Cx, st: &mut UiState) {
    let Some(net) = cx.model.power.as_ref() else { return };
    let ctx = cx.ctx;
    let warn_h = net.warnings.len() as f32 * 30.0 + if net.warnings.is_empty() { 0.0 } else { 8.0 };
    let list_h = 26.0 + LIST_ROWS as f32 * ROW_H + 4.0;
    let content = vec2(WIDTH, SUMMARY_H + 10.0 + warn_h + list_h + 12.0 + 32.0 + GRAPH_H + 24.0);
    let outer = widgets::window_outer(content);
    let rect = widgets::place(ctx.content_rect(), outer, st.offset(WindowKind::PowerNetwork) + vec2(60.0, -20.0));
    let id = window_id(WindowKind::PowerNetwork);
    let title = format!("Power network #{}", net.id);
    widgets::area(ctx, id, Order::Middle, rect, |ui| {
        let f = widgets::window(ui, id, rect, &title, true);
        st.move_window(WindowKind::PowerNetwork, f.drag);
        if f.close_clicked {
            st.close(WindowKind::PowerNetwork, cx.actions);
            return;
        }
        let p = ui.painter().clone();
        let r = f.content;
        let mut y = r.top();
        summary(ui, cx, &p, Rect::from_min_size(pos2(r.left(), y), vec2(r.width(), SUMMARY_H)));
        y += SUMMARY_H + 10.0;
        for w in &net.warnings {
            let wr = Rect::from_min_size(pos2(r.left(), y), vec2(r.width(), 26.0));
            warning_row(&p, cx, wr, w);
            y += 30.0;
        }
        if !net.warnings.is_empty() {
            y += 8.0;
        }
        let col_w = (r.width() - 12.0) * 0.5;
        let cons = Rect::from_min_size(pos2(r.left(), y), vec2(col_w, list_h));
        let prod = Rect::from_min_size(pos2(r.left() + col_w + 12.0, y), vec2(col_w, list_h));
        entry_list(ui, cx, &p, cons, "Consumption", &net.consumers, "cons");
        entry_list(ui, cx, &p, prod, "Production", &net.producers, "prod");
        y += list_h + 12.0;

        // Time range tabs.
        p.text(pos2(r.left(), y + 13.0), Align2::LEFT_CENTER, "Time range", font(text::BODY), color::TEXT_DIM);
        for (i, tr) in TimeRange::ALL.into_iter().enumerate() {
            let br = Rect::from_min_size(pos2(r.left() + 90.0 + i as f32 * 52.0, y), vec2(48.0, 26.0));
            if widgets::toggle(ui, Id::new(("pw-range", i)), br, tr.label(), st.power_range == tr).clicked() {
                st.power_range = tr;
            }
        }
        y += 32.0;
        let g1 = Rect::from_min_size(pos2(r.left(), y + 22.0), vec2(col_w, GRAPH_H));
        let g2 = Rect::from_min_size(pos2(r.left() + col_w + 12.0, y + 22.0), vec2(col_w, GRAPH_H));
        widgets::heading(&p, pos2(g1.left(), y), "Consumption over time");
        widgets::heading(&p, pos2(g2.left(), y), "Production over time");
        let range = st.power_range;
        let cons_series: Vec<(&[f32], Color32)> =
            net.consumers.iter().enumerate().map(|(i, e)| (e.history.samples(range), series_color(i))).collect();
        let prod_series: Vec<(&[f32], Color32)> =
            net.producers.iter().enumerate().map(|(i, e)| (e.history.samples(range), series_color(i))).collect();
        draw_graph(ui, &p, g1, Some(net.consumption_history.samples(range)), &cons_series, format::watts, range, "pw-g1");
        draw_graph(ui, &p, g2, Some(net.production_history.samples(range)), &prod_series, format::watts, range, "pw-g2");
    });
}

pub(crate) fn series_color(i: usize) -> Color32 {
    color::SERIES[i % color::SERIES.len()]
}

fn summary(ui: &mut Ui, cx: &mut Cx, p: &Painter, r: Rect) {
    let model = cx.model;
    let Some(net) = model.power.as_ref() else { return };
    widgets::shallow(p, r);
    // Voltage badge.
    let badge = Rect::from_min_size(r.min + vec2(10.0, 10.0), vec2(88.0, r.height() - 20.0));
    let tc = theme::tier_color(net.voltage.game_tier());
    widgets::deep(p, badge);
    p.rect_stroke(badge.shrink(3.0), CornerRadius::same(2), Stroke::new(2.0, tc), egui::StrokeKind::Inside);
    p.text(badge.center() - vec2(0.0, 10.0), Align2::CENTER_CENTER, net.voltage.label(), font_bold(28.0), tc);
    p.text(badge.center() + vec2(0.0, 20.0), Align2::CENTER_CENTER, format!("{} V", net.voltage.volts()), font_regular(text::BODY), color::TEXT_DIM);
    if ui.interact(badge, Id::new("pw-voltage"), egui::Sense::hover()).hovered() {
        cx.tip(Tip::Text {
            title: format!("Voltage: {} ({} V)", net.voltage.label(), net.voltage.volts()),
            body: "Buildings of a lower voltage tier explode on this network. Use a transformer.".into(),
        });
    }

    let x0 = badge.right() + 16.0;
    let label_w = 100.0;
    let text_w = 150.0;
    let bar_x = x0 + label_w;
    let bar_right = r.right() - 12.0 - text_w;
    let rows: [(&str, f32, Color32, String, &str); 4] = [
        (
            "Satisfaction",
            net.satisfaction,
            if net.satisfaction < 0.5 { color::RED } else if net.satisfaction < 0.99 { color::YELLOW } else { color::GREEN },
            format::percent(net.satisfaction),
            "How much of the demand the generators give. Below 100%, machines run slower.",
        ),
        (
            "Production",
            if net.capacity_w > 0.0 { (net.production_w / net.capacity_w) as f32 } else { 0.0 },
            color::POWER,
            format!("{} of {}", format::watts(net.production_w), format::watts(net.capacity_w)),
            "Power the generators make now, and the most they can make.",
        ),
        (
            "Storage",
            if net.storage_capacity_j > 0.0 { (net.stored_j / net.storage_capacity_j) as f32 } else { 0.0 },
            color::STORAGE,
            format!("{} of {}", format::joules(net.stored_j), format::joules(net.storage_capacity_j)),
            "Energy in batteries. Batteries fill when production is higher than use.",
        ),
        (
            "Current",
            if net.limit_amps > 0.0 { net.amps / net.limit_amps } else { 0.0 },
            widgets::danger_color(if net.limit_amps > 0.0 { net.amps / net.limit_amps } else { 0.0 }),
            format!("{:.0} A of {:.0} A", net.amps, net.limit_amps),
            "Current in the network and the limit of the weakest cable. Above the limit, cables get hot and melt.",
        ),
    ];
    for (i, (label, frac, c, value, help)) in rows.iter().enumerate() {
        let cy = r.top() + 16.0 + i as f32 * 25.0;
        p.text(pos2(x0, cy), Align2::LEFT_CENTER, *label, font(text::BODY), color::TEXT_DIM);
        let bar = Rect::from_min_max(pos2(bar_x, cy - 8.0), pos2(bar_right, cy + 8.0));
        widgets::bar(p, bar, *frac, *c, None);
        p.text(pos2(r.right() - 12.0, cy), Align2::RIGHT_CENTER, value, font(text::BODY), color::TEXT);
        let row = Rect::from_min_max(pos2(x0, cy - 11.0), pos2(r.right() - 12.0, cy + 11.0));
        if ui.interact(row, Id::new(("pw-sum", i)), egui::Sense::hover()).hovered() {
            cx.tip(Tip::Text { title: label.to_string(), body: help.to_string() });
        }
    }
}

fn warning_row(p: &Painter, cx: &Cx, r: Rect, w: &PowerWarning) {
    let (text, c) = match w {
        PowerWarning::CableOverloaded { amps, limit_amps, cable } => {
            let name = item::building_name(&cx.model.content, *cable).to_lowercase();
            (format!("Cable overloaded: {amps:.0} A on a {name} (limit {limit_amps:.0} A). The cable gets hot."), color::RED)
        }
        PowerWarning::WrongVoltage { building, building_voltage } => (
            format!(
                "Wrong voltage: {} is {}. It will explode on this network.",
                item::building_name(&cx.model.content, *building),
                building_voltage.label()
            ),
            color::RED,
        ),
        PowerWarning::NotEnoughPower => ("Not enough power: machines run slower.".to_string(), color::YELLOW),
        PowerWarning::NoGenerators => ("No generators on this network.".to_string(), color::YELLOW),
    };
    p.rect_filled(r, CornerRadius::same(2), theme::shade(c, 0.28));
    p.rect_stroke(r, CornerRadius::same(2), Stroke::new(1.0, theme::shade(c, 0.8)), egui::StrokeKind::Inside);
    let tri = vec![pos2(r.left() + 16.0, r.top() + 5.0), pos2(r.left() + 25.0, r.bottom() - 5.0), pos2(r.left() + 7.0, r.bottom() - 5.0)];
    p.add(Shape::convex_polygon(tri, c, Stroke::NONE));
    p.text(pos2(r.left() + 16.0, r.center().y + 2.0), Align2::CENTER_CENTER, "!", font_bold(11.0), Color32::BLACK);
    p.text(pos2(r.left() + 34.0, r.center().y), Align2::LEFT_CENTER, text, font(text::BODY), color::TEXT);
}

fn entry_list(ui: &mut Ui, cx: &mut Cx, p: &Painter, r: Rect, title: &str, entries: &[PowerEntry], key: &str) {
    let total: f64 = entries.iter().map(|e| e.watts).sum();
    widgets::heading(p, r.min, title);
    p.text(pos2(r.right(), r.top() + 9.0), Align2::RIGHT_CENTER, format::watts(total), font_bold(text::BODY), color::TEXT);
    let list = Rect::from_min_max(pos2(r.left(), r.top() + 26.0), r.right_bottom());
    widgets::deep(p, list);
    let max = entries.iter().map(|e| e.watts).fold(0.0, f64::max).max(1.0);
    let inner = list.shrink(2.0);
    let atlas = cx.atlas;
    let content = &*cx.model.content;
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
        egui::ScrollArea::vertical().id_salt(("pw-list", key)).auto_shrink([false, false]).show(ui, |ui| {
            let (area, _) = ui.allocate_exact_size(vec2(inner.width(), entries.len() as f32 * ROW_H), egui::Sense::hover());
            let p = ui.painter().clone();
            for (i, e) in entries.iter().enumerate() {
                let row = Rect::from_min_size(pos2(area.left(), area.top() + i as f32 * ROW_H), vec2(area.width(), ROW_H));
                if i % 2 == 1 {
                    p.rect_filled(row, CornerRadius::ZERO, Color32::from_white_alpha(4));
                }
                let sc = series_color(i);
                let slot = Rect::from_min_size(row.min + vec2(4.0, 3.0), Vec2::splat(32.0));
                widgets::raised(&p, slot, color::SLOT, color::SLOT_LIGHT, color::SLOT_DARK);
                if let Some(it) = item::building_item(content, e.kind) {
                    atlas.paint(&p, it, slot.shrink(3.0), Color32::WHITE);
                }
                widgets::corner_count(&p, slot.expand(2.0), &e.count.to_string());
                let name = item::building_name(content, e.kind);
                p.text(pos2(slot.right() + 10.0, row.top() + 10.0), Align2::LEFT_CENTER, name, font(text::BODY), color::TEXT);
                let bar = Rect::from_min_max(pos2(slot.right() + 10.0, row.bottom() - 13.0), pos2(row.right() - 90.0, row.bottom() - 6.0));
                widgets::bar(&p, bar, (e.watts / max) as f32, sc, None);
                p.text(pos2(row.right() - 8.0, row.center().y), Align2::RIGHT_CENTER, format::watts(e.watts), font_bold(text::BODY), color::TEXT);
                let resp = ui.interact(row, Id::new(("pw-row", key, i)), egui::Sense::hover());
                if resp.hovered() {
                    let each = if e.count > 0 { e.watts / e.count as f64 } else { 0.0 };
                    cx.tip(Tip::Text {
                        title: format!("{} × {}", e.count, name),
                        body: format!("{} in total, {} each on average.", format::watts(e.watts), format::watts(each)),
                    });
                }
            }
        });
    });
    if entries.is_empty() {
        p.text(list.center(), Align2::CENTER_CENTER, "None", font_regular(text::BODY), color::TEXT_FAINT);
    }
}

/// Draw a line graph. `total` is drawn as a filled area; each series as a line.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_graph(
    ui: &mut Ui,
    p: &Painter,
    r: Rect,
    total: Option<&[f32]>,
    series: &[(&[f32], Color32)],
    fmt: fn(f64) -> String,
    range: TimeRange,
    key: &str,
) {
    widgets::deep(p, r);
    let label_w = 62.0;
    let plot = Rect::from_min_max(pos2(r.left() + label_w, r.top() + 10.0), pos2(r.right() - 10.0, r.bottom() - 22.0));
    let mut max = 0.0f32;
    if let Some(t) = total {
        max = t.iter().copied().fold(max, f32::max);
    }
    for (s, _) in series {
        max = s.iter().copied().fold(max, f32::max);
    }
    let top = graph::nice_ceiling(max as f64 * 1.05);
    // Grid lines and labels.
    for i in 0..=4 {
        let frac = i as f32 / 4.0;
        let y = plot.bottom() - plot.height() * frac;
        p.hline(plot.x_range(), y.round() + 0.5, Stroke::new(1.0, Color32::from_white_alpha(if i == 0 { 40 } else { 14 })));
        p.text(pos2(plot.left() - 6.0, y), Align2::RIGHT_CENTER, fmt(top * frac as f64), font_regular(12.0), color::TEXT_DIM);
    }
    let secs = range.seconds();
    for i in 0..=4 {
        let frac = i as f32 / 4.0;
        let x = plot.left() + plot.width() * frac;
        p.vline(x.round() + 0.5, plot.y_range(), Stroke::new(1.0, Color32::from_white_alpha(10)));
        let ago = secs * (1.0 - frac as f64);
        let label = if i == 4 { "now".to_string() } else { time_label(ago) };
        let anchor = if i == 0 { Align2::LEFT_TOP } else if i == 4 { Align2::RIGHT_TOP } else { Align2::CENTER_TOP };
        p.text(pos2(x, plot.bottom() + 4.0), anchor, label, font_regular(12.0), color::TEXT_FAINT);
    }
    let clip = p.with_clip_rect(plot.expand(1.0));
    let rect_tuple = (plot.left(), plot.top(), plot.width(), plot.height());
    let mut pts = vec![];
    if let Some(t) = total {
        graph::graph_points(t, rect_tuple, top, &mut pts);
        if pts.len() >= 2 {
            let mut mesh = egui::Mesh::default();
            let fill = Color32::from_rgba_unmultiplied(255, 255, 255, 22);
            for (i, &(x, y)) in pts.iter().enumerate() {
                mesh.colored_vertex(pos2(x, y), fill);
                mesh.colored_vertex(pos2(x, plot.bottom()), fill);
                if i > 0 {
                    let b = (i * 2) as u32;
                    mesh.add_triangle(b - 2, b - 1, b);
                    mesh.add_triangle(b - 1, b + 1, b);
                }
            }
            clip.add(Shape::mesh(mesh));
            clip.add(Shape::line(pts.iter().map(|&(x, y)| pos2(x, y)).collect(), Stroke::new(1.5, Color32::from_gray(220))));
        }
    }
    for (s, c) in series {
        graph::graph_points(s, rect_tuple, top, &mut pts);
        if pts.len() >= 2 {
            clip.add(Shape::line(pts.iter().map(|&(x, y)| pos2(x, y)).collect(), Stroke::new(1.5, *c)));
        }
    }
    // Hover: a line and the value of the total at that time.
    let resp = ui.interact(plot, Id::new(("graph", key)), egui::Sense::hover());
    if let (Some(pos), Some(t)) = (resp.hover_pos(), total)
        && !t.is_empty()
    {
        let step = plot.width() / (graph::SAMPLES - 1) as f32;
        let from_right = ((plot.right() - pos.x) / step).round() as usize;
        if from_right < t.len() {
            let v = t[t.len() - 1 - from_right];
            clip.vline(pos.x, plot.y_range(), Stroke::new(1.0, color::ORANGE));
            let ago = secs * (from_right as f64 / (graph::SAMPLES - 1) as f64);
            crate::tooltip::small_label(&clip, pos + vec2(8.0, -28.0), &format!("{} ({} ago)", fmt(v as f64), time_label(ago)));
        }
    }
}

fn time_label(secs: f64) -> String {
    if secs >= 3600.0 {
        let h = secs / 3600.0;
        if (h - h.round()).abs() < 0.01 { format!("{}h", h.round()) } else { format!("{h:.1}h") }
    } else if secs >= 60.0 {
        let m = secs / 60.0;
        if (m - m.round()).abs() < 0.01 { format!("{}m", m.round()) } else { format!("{m:.1}m") }
    } else {
        format!("{}s", secs.round())
    }
}
