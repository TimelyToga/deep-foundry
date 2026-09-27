//! The guide window (key G): the goals of each tier, like a quest book. Done goals are short
//! and dim. Open goals show their text, their count and the reward.

use super::research::{TIER_H, tier_heading};
use super::{Cx, window_id};
use crate::action::WindowKind;
use crate::model::GuideGoal;
use crate::theme::{self, color, font, font_bold, font_regular, text};
use crate::widgets;
use crate::UiState;
use egui::{Align2, Order, Painter, Rect, Ui, pos2, vec2};
use std::collections::BTreeMap;

const WIDTH: f32 = 620.0;
const TOP_H: f32 = 34.0;
const PAD: f32 = 8.0;
const GAP: f32 = 4.0;
/// A done goal: one line.
const DONE_H: f32 = 28.0;
const TITLE_H: f32 = 24.0;
/// The count bar line of an open goal.
const COUNT_H: f32 = 24.0;

pub(crate) fn show(cx: &mut Cx, st: &mut UiState) {
    let ctx = cx.ctx;
    let screen = ctx.content_rect();
    let height = (screen.height() - 160.0).clamp(320.0, 640.0);
    let outer = widgets::window_outer(vec2(WIDTH, height));
    let rect = widgets::place(screen, outer, st.offset(WindowKind::Guide) - vec2(0.0, 40.0));
    let id = window_id(WindowKind::Guide);
    widgets::area(ctx, id, Order::Middle, rect, |ui| {
        let f = widgets::window(ui, id, rect, "Guide", true);
        st.move_window(WindowKind::Guide, f.drag);
        if f.close_clicked {
            st.close(WindowKind::Guide, cx.actions);
            return;
        }
        let model = cx.model;
        let p = ui.painter();
        let r = f.content;
        let cy = r.top() + 13.0;
        let done = model.guide.iter().filter(|g| g.done).count();
        p.text(pos2(r.left(), cy), Align2::LEFT_CENTER, format!("{done} of {} goals done", model.guide.len()), font(text::BODY), color::TEXT_DIM);
        let value = p.text(pos2(r.right(), cy), Align2::RIGHT_CENTER, model.discovery_points.to_string(), font_bold(text::BODY + 2.0), color::HEADING);
        p.text(pos2(value.left() - 8.0, cy), Align2::RIGHT_CENTER, "Discovery points", font(text::BODY), color::TEXT_DIM);
        let list = Rect::from_min_max(pos2(r.left(), r.top() + TOP_H), r.right_bottom());
        goal_list(ui, cx, list);
    });
}

/// The height of an open goal.
fn open_height(ctx: &egui::Context, g: &GuideGoal, text_w: f32) -> f32 {
    let text_h = if g.text.is_empty() { 0.0 } else { widgets::text_height(ctx, &g.text, font_regular(text::BODY), text_w) };
    PAD + TITLE_H + text_h + if g.count.is_some() { COUNT_H } else { 0.0 } + PAD
}

fn points_text(n: u32) -> String {
    if n == 1 { "+1 discovery point".into() } else { format!("+{n} discovery points") }
}

fn goal_list(ui: &mut Ui, cx: &mut Cx, list: Rect) {
    let model = cx.model;
    widgets::deep(ui.painter(), list);
    if model.guide.is_empty() {
        ui.painter().text(list.center(), Align2::CENTER_CENTER, "No goals yet.", font_regular(text::BODY), color::TEXT_FAINT);
        return;
    }
    let mut tiers: BTreeMap<u8, Vec<&GuideGoal>> = BTreeMap::new();
    for g in &model.guide {
        tiers.entry(g.tier).or_default().push(g);
    }
    let inner = list.shrink(2.0);
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
        egui::ScrollArea::vertical().id_salt("guide-list").auto_shrink([false, false]).show(ui, |ui| {
            let width = ui.available_width();
            let card_w = width - 2.0 * PAD;
            let text_w = card_w - 2.0 * PAD;
            let height = |g: &GuideGoal| if g.done { DONE_H } else { open_height(cx.ctx, g, text_w) + GAP };
            let total: f32 = tiers.values().map(|gs| TIER_H + gs.iter().map(|g| height(g)).sum::<f32>()).sum::<f32>() + PAD;
            let (area, _) = ui.allocate_exact_size(vec2(width, total), egui::Sense::hover());
            let p = ui.painter().clone();
            let mut y = area.top();
            for (tier, goals) in &tiers {
                let done = goals.iter().filter(|g| g.done).count();
                tier_heading(&p, Rect::from_min_size(pos2(area.left() + PAD, y), vec2(card_w, TIER_H)), *tier, &format!("{done} of {} done", goals.len()));
                y += TIER_H;
                for (i, g) in goals.iter().enumerate() {
                    let h = height(g);
                    let r = Rect::from_min_size(pos2(area.left() + PAD, y), vec2(card_w, if g.done { DONE_H } else { h - GAP }));
                    if g.done {
                        done_goal(&p, r, g, i);
                    } else {
                        open_goal(&p, r, g, text_w);
                    }
                    y += h;
                }
            }
        });
    });
}

fn done_goal(p: &Painter, r: Rect, g: &GuideGoal, i: usize) {
    if i % 2 == 1 {
        p.rect_filled(r, 0.0, egui::Color32::from_white_alpha(4));
    }
    let check = Rect::from_center_size(pos2(r.left() + 14.0, r.center().y), vec2(18.0, 18.0));
    widgets::check_mark(p, check, theme::shade(color::GREEN, 0.8));
    p.text(pos2(r.left() + 32.0, r.center().y), Align2::LEFT_CENTER, &g.title, font(text::BODY), color::TEXT_FAINT);
    if g.reward_points > 0 {
        p.text(pos2(r.right() - PAD, r.center().y), Align2::RIGHT_CENTER, points_text(g.reward_points), font_regular(text::SMALL), color::TEXT_FAINT);
    }
}

fn open_goal(p: &Painter, r: Rect, g: &GuideGoal, text_w: f32) {
    widgets::raised(p, r, color::SHALLOW, color::SHALLOW_LIGHT, theme::shade(color::SHALLOW, 0.7));
    let x = r.left() + PAD;
    let title_y = r.top() + PAD + TITLE_H * 0.5 - 2.0;
    p.text(pos2(x, title_y), Align2::LEFT_CENTER, &g.title, font_bold(text::BODY + 1.0), color::HEADING);
    if g.reward_points > 0 {
        p.text(pos2(r.right() - PAD, title_y), Align2::RIGHT_CENTER, points_text(g.reward_points), font_regular(text::SMALL), color::TEXT_DIM);
    }
    let mut y = r.top() + PAD + TITLE_H;
    if !g.text.is_empty() {
        y = widgets::wrapped(p, pos2(x, y), &g.text, font_regular(text::BODY), color::TEXT, text_w).bottom();
    }
    if let Some((have, need)) = g.count {
        let bar = Rect::from_min_size(pos2(x, y + 6.0), vec2(180.0, 14.0));
        let frac = if need > 0 { have as f32 / need as f32 } else { 1.0 };
        widgets::bar(p, bar, frac, color::PROGRESS, None);
        p.text(pos2(bar.right() + 10.0, bar.center().y), Align2::LEFT_CENTER, format!("{have} / {need}"), font_bold(text::BODY), color::TEXT);
    }
}

