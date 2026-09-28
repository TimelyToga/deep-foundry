//! The room panel of the building window, for a room machine (kiln, coke oven, blast furnace).
//!
//! From top to bottom: the title "Room" with the size and the hatches, the state of the room (or
//! what is wrong with it), the wall blocks it takes, the room temperature with a mark at the
//! temperature the recipe needs, and the fire (fuel cells that burn, the blast, the ash).

use super::Cx;
use crate::format;
use crate::model::RoomPanel;
use crate::theme::{color, font, font_bold, font_regular, text};
use crate::tooltip::Tip;
use crate::widgets;
use egui::{Align2, Id, Rect, Stroke, Ui, pos2, vec2};

const PAD: f32 = 10.0;
const HEAD_H: f32 = 24.0;
const ROW_H: f32 = 28.0;
const LINE_H: f32 = 20.0;

/// The sentence under the title: the state of the room.
fn state_text(r: &RoomPanel) -> String {
    match &r.problem {
        Some(p) => p.clone(),
        None if r.valid => "Closed room. The fire burns the fuel from the fuel slot on the floor.".into(),
        None => "Checking the room.".into(),
    }
}

fn walls_text(r: &RoomPanel) -> String {
    format!("Walls: {}. The controller and at least one hatch go in the wall.", r.walls)
}

/// The height of the panel for a width.
pub(crate) fn height(ctx: &egui::Context, r: &RoomPanel, width: f32) -> f32 {
    let w = width - 2.0 * PAD - 18.0;
    let state = widgets::text_height(ctx, &state_text(r), font_regular(text::BODY), w).max(LINE_H);
    let walls = widgets::text_height(ctx, &walls_text(r), font_regular(text::SMALL), w).max(LINE_H);
    PAD + HEAD_H + state + 4.0 + walls + 6.0 + ROW_H + ROW_H + PAD
}

/// Draw the panel in `panel`. `max_temp` is the highest temperature of the controller (the end
/// of the temperature bar).
pub(crate) fn panel(ui: &mut Ui, cx: &mut Cx, r: &RoomPanel, panel: Rect, max_temp: f32) {
    let p = ui.painter().clone();
    widgets::shallow(&p, panel);
    let inner = panel.shrink(PAD);
    let mut y = inner.top();

    // Title and size.
    widgets::heading(&p, pos2(inner.left(), y), "Room");
    if r.valid {
        let hatches = if r.hatches == 1 { "1 hatch".to_string() } else { format!("{} hatches", r.hatches) };
        let size = format!("{} of {} tiles, {hatches}", r.tiles, r.max_tiles);
        p.text(pos2(inner.right(), y + 9.0), Align2::RIGHT_CENTER, size, font_regular(text::SMALL), color::TEXT_DIM);
    }
    y += HEAD_H;

    // The state of the room.
    let dot = if r.valid { color::GREEN } else { color::RED };
    widgets::status_dot(&p, pos2(inner.left() + 6.0, y + 10.0), dot);
    let text_color = if r.valid { color::TEXT } else { color::RED_TEXT };
    let state = widgets::wrapped(&p, pos2(inner.left() + 18.0, y), &state_text(r), font_regular(text::BODY), text_color, inner.width() - 18.0);
    y += state.height().max(LINE_H) + 4.0;
    let walls = widgets::wrapped(&p, pos2(inner.left() + 18.0, y + 2.0), &walls_text(r), font_regular(text::SMALL), color::TEXT_FAINT, inner.width() - 18.0);
    y += walls.height().max(LINE_H) + 6.0;

    // Room temperature.
    let row = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), ROW_H - 4.0));
    p.text(pos2(row.left(), row.center().y), Align2::LEFT_CENTER, "Room heat", font(text::BODY), color::TEXT_DIM);
    let bar = Rect::from_min_max(pos2(row.left() + 110.0, row.top() + 3.0), pos2(row.right(), row.bottom() - 3.0));
    let max = max_temp.max(1.0);
    let t = r.temperature.unwrap_or(0.0);
    let hot_enough = r.needs.is_none_or(|n| t >= n);
    let fill = if hot_enough { color::GREEN } else { color::HEAT };
    let label = match (r.temperature, r.needs) {
        (Some(t), Some(n)) => format!("{}, needs {}", format::celsius(t), format::celsius(n)),
        (Some(t), None) => format::celsius(t),
        (None, _) => "No room".to_string(),
    };
    widgets::bar(&p, bar, t / max, fill, None);
    if let Some(n) = r.needs {
        // A mark at the temperature the recipe needs.
        let x = bar.left() + bar.width() * (n / max).clamp(0.0, 1.0);
        p.line_segment([pos2(x, bar.top() - 3.0), pos2(x, bar.bottom() + 3.0)], Stroke::new(2.0, color::TEXT));
    }
    widgets::text_outlined(&p, bar.center(), Align2::CENTER_CENTER, &label, font_bold(text::SMALL), color::TEXT);
    if ui.interact(row, Id::new("room-heat"), egui::Sense::hover()).hovered() {
        cx.tip(Tip::Text {
            title: "Room heat".into(),
            body: "The average temperature of the cells inside the room. The recipe runs when it reaches the white mark. \
                   The controller adds fuel until the room is a little hotter than that."
                .into(),
        });
    }
    y += ROW_H;

    // The fire.
    let row = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), ROW_H - 4.0));
    p.text(pos2(row.left(), row.center().y), Align2::LEFT_CENTER, "Fire", font(text::BODY), color::TEXT_DIM);
    let fire = if r.fuel_cells == 0 {
        "No fuel in the room".to_string()
    } else {
        format!("{} of {} fuel cells burn", r.burning, r.fuel_cells)
    };
    let fire_color = if r.burning > 0 { color::ORANGE } else { color::TEXT_DIM };
    let mut x = row.left() + 110.0;
    let t = p.text(pos2(x, row.center().y), Align2::LEFT_CENTER, fire, font(text::BODY), fire_color);
    x = t.right() + 10.0;
    if r.blast > 0.0 {
        widgets::badge(&p, pos2(x, row.center().y), &format!("Blast +{}", format::celsius(r.blast)), color::ORANGE);
    }
    p.text(pos2(row.right(), row.center().y), Align2::RIGHT_CENTER, format!("Ash {}", format::count(r.ash as u64)), font(text::SMALL), color::TEXT_DIM);
    if ui.interact(row, Id::new("room-fire"), egui::Sense::hover()).hovered() {
        cx.tip(Tip::Text {
            title: "Fire".into(),
            body: "Fuel from the fuel slot burns on the floor of the room. Bellows or a blower in the wall make it hotter. \
                   The ash goes out through a hatch in the floor or a side wall."
                .into(),
        });
    }
}
