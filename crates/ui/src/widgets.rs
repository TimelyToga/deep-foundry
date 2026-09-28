//! Drawing helpers and widgets in the Factorio style.
//!
//! Most screens place their parts with exact rectangles, so the widgets here take a `Rect`
//! and return an `egui::Response`.

use crate::icons::IconAtlas;
use foundry_content::ItemRef;
use crate::theme::{self, color, font, font_bold, size, text};
use egui::{Align2, Color32, CornerRadius, FontId, Id, Painter, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};

// ---------------------------------------------------------------- frames

/// A raised box: fill, a light line on the top and left, a dark line on the bottom and right.
pub fn raised(p: &Painter, r: Rect, fill: Color32, light: Color32, dark: Color32) {
    p.rect_filled(r, CornerRadius::ZERO, fill);
    p.hline(r.x_range(), r.top() + 0.5, Stroke::new(1.0, light));
    p.vline(r.left() + 0.5, r.y_range(), Stroke::new(1.0, light));
    p.hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, dark));
    p.vline(r.right() - 0.5, r.y_range(), Stroke::new(1.0, dark));
}

/// A sunken box: dark lines on the top and left, a light line on the bottom and right.
pub fn sunken(p: &Painter, r: Rect, fill: Color32) {
    p.rect_filled(r, CornerRadius::ZERO, fill);
    p.hline(r.x_range(), r.top() + 0.5, Stroke::new(1.0, color::DEEP_EDGE_DARK));
    p.vline(r.left() + 0.5, r.y_range(), Stroke::new(1.0, color::DEEP_EDGE_DARK));
    p.hline(r.x_range(), r.top() + 1.5, Stroke::new(1.0, theme::shade(fill, 0.6)));
    p.hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, color::DEEP_EDGE_LIGHT));
    p.vline(r.right() - 0.5, r.y_range(), Stroke::new(1.0, color::DEEP_EDGE_LIGHT));
}

/// The outer frame of a window, with a soft shadow.
pub fn window_frame(p: &Painter, r: Rect) {
    for i in 1..=6 {
        let a = (26 - i * 4) as u8;
        p.rect_filled(r.expand(i as f32).translate(vec2(0.0, 3.0)), CornerRadius::same(4), Color32::from_black_alpha(a));
    }
    p.rect_filled(r.expand(1.0), CornerRadius::same(2), color::WINDOW_DARK);
    raised(p, r, color::WINDOW, color::WINDOW_LIGHT, color::WINDOW_DARK);
}

/// A lighter panel inside a window (Factorio "inside shallow frame").
pub fn shallow(p: &Painter, r: Rect) {
    p.rect_filled(r.expand(1.0), CornerRadius::ZERO, color::WINDOW_DARK);
    raised(p, r, color::SHALLOW, color::SHALLOW_LIGHT, theme::shade(color::SHALLOW, 0.7));
}

/// A dark sunken panel for slot grids and lists (Factorio "inside deep frame").
pub fn deep(p: &Painter, r: Rect) {
    sunken(p, r, color::DEEP);
}

/// Text with a dark shadow, for text on busy backgrounds.
pub fn text_shadow(p: &Painter, pos: Pos2, anchor: Align2, text: &str, font: FontId, color: Color32) -> Rect {
    p.text(pos + vec2(1.0, 1.0), anchor, text, font.clone(), Color32::from_black_alpha(200));
    p.text(pos, anchor, text, font, color)
}

/// A count in the lower right corner of a slot, with a dark outline so it is readable on any icon.
pub fn corner_count(p: &Painter, slot: Rect, text: &str) {
    text_outlined(p, slot.right_bottom() + vec2(-3.0, 0.0), Align2::RIGHT_BOTTOM, text, font_bold(text::COUNT), color::TEXT);
}

/// A small label in the upper left corner of a slot (quickbar key numbers).
pub fn corner_label(p: &Painter, slot: Rect, text: &str) {
    let pos = slot.left_top() + vec2(3.0, 0.0);
    let f = font_bold(12.0);
    p.text(pos + vec2(1.0, 1.0), Align2::LEFT_TOP, text, f.clone(), Color32::from_black_alpha(220));
    p.text(pos, Align2::LEFT_TOP, text, f, color::TEXT_DIM);
}

/// A section heading inside a window: bold warm text.
pub fn heading(p: &Painter, pos: Pos2, text: &str) -> Rect {
    p.text(pos, Align2::LEFT_TOP, text, font_bold(text::BODY + 1.0), color::HEADING)
}

// ---------------------------------------------------------------- slots

/// How a slot looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotLook {
    /// Light gray, as inventory slots.
    Normal,
    /// Dark: an empty quickbar slot or a building slot.
    Dark,
    /// Red: a recipe the player cannot make now.
    Red,
}

/// What to draw in a slot.
#[derive(Debug, Clone, Default)]
pub struct SlotContent<'a> {
    pub item: Option<ItemRef>,
    /// Text in the lower right corner (the count).
    pub count: Option<&'a str>,
    /// A faint icon for an empty slot that expects an item.
    pub ghost: Option<ItemRef>,
    /// A fill level from the bottom (0 to 1) in a color, for bulk material slots.
    pub fill: Option<(f32, Color32)>,
    /// A progress bar along the bottom (0 to 1).
    pub progress: Option<f32>,
    /// Text in the upper left corner.
    pub label: Option<&'a str>,
    /// Orange frame: the selected slot.
    pub selected: bool,
    /// Draw the icon darker (for example, 0 items of a quickbar item).
    pub dim: bool,
}

/// Draw a slot and make it clickable. Left and right clicks both count.
pub fn slot(ui: &Ui, id: Id, r: Rect, look: SlotLook, content: &SlotContent, atlas: &IconAtlas) -> Response {
    let resp = ui.interact(r, id, Sense::click());
    let hovered = resp.hovered();
    let p = ui.painter();
    let (fill, light, dark) = match look {
        SlotLook::Normal => (color::SLOT, color::SLOT_LIGHT, color::SLOT_DARK),
        SlotLook::Dark => (color::SLOT_EMPTY, theme::shade(color::SLOT_EMPTY, 1.35), theme::shade(color::SLOT_EMPTY, 0.6)),
        SlotLook::Red => (color::SLOT_RED, color::SLOT_RED_LIGHT, color::SLOT_RED_DARK),
    };
    let fill = if hovered { theme::mix(fill, color::BUTTON_HOVER, 0.55) } else { fill };
    raised(p, r, fill, light, dark);
    if let Some((frac, c)) = content.fill {
        let inner = r.shrink(2.0);
        let h = inner.height() * frac.clamp(0.0, 1.0);
        if h > 0.0 {
            let fr = Rect::from_min_max(pos2(inner.left(), inner.bottom() - h), inner.right_bottom());
            p.rect_filled(fr, CornerRadius::ZERO, c.gamma_multiply(0.85));
            p.hline(fr.x_range(), fr.top() + 0.5, Stroke::new(1.0, theme::shade(c, 1.35)));
        }
    }
    let icon_rect = Rect::from_center_size(r.center(), Vec2::splat(r.width() * (size::ICON / size::SLOT)));
    if let Some(item) = content.item {
        let tint = if content.dim { Color32::from_gray(110) } else { Color32::WHITE };
        atlas.paint(p, item, icon_rect, tint);
    } else if let Some(g) = content.ghost {
        atlas.paint(p, g, icon_rect, Color32::from_white_alpha(70));
    }
    if let Some(prog) = content.progress {
        let bar = Rect::from_min_max(pos2(r.left() + 2.0, r.bottom() - 6.0), pos2(r.right() - 2.0, r.bottom() - 2.0));
        p.rect_filled(bar, CornerRadius::ZERO, color::BAR_TRACK);
        let w = bar.width() * prog.clamp(0.0, 1.0);
        p.rect_filled(Rect::from_min_size(bar.min, vec2(w, bar.height())), CornerRadius::ZERO, color::PROGRESS);
    }
    if let Some(label) = content.label {
        corner_label(p, r, label);
    }
    if let Some(count) = content.count {
        corner_count(p, r, count);
    }
    if content.selected {
        p.rect_stroke(r.shrink(1.0), CornerRadius::ZERO, Stroke::new(2.0, color::ORANGE), StrokeKind::Inside);
    } else if hovered {
        p.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, color::ORANGE), StrokeKind::Inside);
    }
    resp
}

// ---------------------------------------------------------------- buttons

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    /// Light gray.
    Normal,
    /// Green: confirm (Play, Save, Load).
    Confirm,
    /// Red: back, cancel or delete.
    Back,
}

/// A text button in a rectangle.
pub fn button(ui: &Ui, id: Id, r: Rect, label: &str, kind: ButtonKind, enabled: bool) -> Response {
    let resp = ui.interact(r, id, if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    // Remember where the button is in this frame: the smoke test clicks buttons by label.
    let pass = ui.ctx().cumulative_pass_nr();
    ui.ctx().data_mut(|d| d.insert_temp(button_rect_id(label), (r, pass)));
    let p = ui.painter();
    let (base, hover) = match kind {
        ButtonKind::Normal => (color::BUTTON, color::BUTTON_HOVER),
        ButtonKind::Confirm => (color::GREEN_BUTTON, color::GREEN_BUTTON_HOVER),
        ButtonKind::Back => (color::RED_BUTTON, color::RED_BUTTON_HOVER),
    };
    let pressed = enabled && resp.is_pointer_button_down_on();
    let fill = if !enabled {
        color::BUTTON_DISABLED
    } else if pressed {
        color::BUTTON_PRESSED
    } else if resp.hovered() {
        hover
    } else {
        base
    };
    p.rect_filled(r.expand(1.0), CornerRadius::same(2), color::WINDOW_DARK);
    if pressed {
        sunken(p, r, fill);
    } else {
        raised(p, r, fill, theme::shade(fill, 1.3), theme::shade(fill, 0.6));
    }
    let tc = if enabled { color::BUTTON_TEXT } else { color::BUTTON_DISABLED_TEXT };
    let offset = if pressed { vec2(0.0, 1.0) } else { Vec2::ZERO };
    p.text(r.center() + offset, Align2::CENTER_CENTER, label, font_bold(text::BUTTON), tc);
    resp
}

/// The egui memory key of the place of the button with this label.
fn button_rect_id(label: &str) -> Id {
    Id::new(("foundry-button-rect", label))
}

/// Where the button with this label was drawn in the last frame, if it was.
pub fn button_rect(ctx: &egui::Context, label: &str) -> Option<Rect> {
    let pass = ctx.cumulative_pass_nr();
    let (r, at) = ctx.data(|d| d.get_temp::<(Rect, u64)>(button_rect_id(label)))?;
    (at + 1 >= pass).then_some(r)
}

/// A toggle button that stays orange while `selected` (time range tabs, presets).
pub fn toggle(ui: &Ui, id: Id, r: Rect, label: &str, selected: bool) -> Response {
    let resp = ui.interact(r, id, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label));
    let p = ui.painter();
    let fill = if selected {
        color::BUTTON_PRESSED
    } else if resp.hovered() {
        color::BUTTON_HOVER
    } else {
        color::BUTTON
    };
    p.rect_filled(r.expand(1.0), CornerRadius::same(2), color::WINDOW_DARK);
    if selected {
        sunken(p, r, fill);
    } else {
        raised(p, r, fill, theme::shade(fill, 1.3), theme::shade(fill, 0.6));
    }
    p.text(r.center(), Align2::CENTER_CENTER, label, font_bold(text::BODY), color::BUTTON_TEXT);
    resp
}

/// The small [X] button in a title bar.
pub fn close_button(ui: &Ui, id: Id, r: Rect) -> Response {
    let resp = ui.interact(r, id, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Close"));
    let p = ui.painter();
    let fill = if resp.is_pointer_button_down_on() {
        color::BUTTON_PRESSED
    } else if resp.hovered() {
        theme::shade(color::RED_BUTTON, 0.9)
    } else {
        theme::shade(color::WINDOW, 1.1)
    };
    p.rect_filled(r.expand(1.0), CornerRadius::ZERO, color::WINDOW_DARK);
    raised(p, r, fill, theme::shade(fill, 1.4), theme::shade(fill, 0.6));
    let c = r.center();
    let d = r.width() * 0.2;
    let stroke = Stroke::new(2.0, color::TEXT);
    p.line_segment([c + vec2(-d, -d), c + vec2(d, d)], stroke);
    p.line_segment([c + vec2(-d, d), c + vec2(d, -d)], stroke);
    resp
}

/// A checkbox with a label. Returns the response of the whole row.
pub fn checkbox(ui: &Ui, id: Id, r: Rect, label: &str, checked: bool) -> Response {
    let resp = ui.interact(r, id, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, checked, label));
    let p = ui.painter();
    let bx = Rect::from_min_size(pos2(r.left(), r.center().y - 10.0), vec2(20.0, 20.0));
    let fill = if resp.hovered() { color::BUTTON_HOVER } else { color::FIELD };
    p.rect_filled(bx.expand(1.0), CornerRadius::ZERO, color::WINDOW_DARK);
    sunken(p, bx, fill);
    if checked {
        let s = Stroke::new(3.0, color::ORANGE_DIM);
        p.line_segment([bx.left_top() + vec2(4.5, 10.0), bx.left_top() + vec2(8.5, 14.5)], s);
        p.line_segment([bx.left_top() + vec2(8.5, 14.5), bx.left_top() + vec2(15.5, 5.0)], s);
    }
    p.text(pos2(bx.right() + 10.0, r.center().y), Align2::LEFT_CENTER, label, font(text::BODY), color::TEXT);
    resp
}

// ---------------------------------------------------------------- bars

/// A horizontal bar with a dark track, a fill, and optional centered text.
pub fn bar(p: &Painter, r: Rect, frac: f32, fill: Color32, label: Option<&str>) {
    p.rect_filled(r.expand(1.0), CornerRadius::ZERO, color::WINDOW_DARK);
    sunken(p, r, color::BAR_TRACK);
    let inner = r.shrink(1.0);
    let w = inner.width() * frac.clamp(0.0, 1.0);
    if w > 0.5 {
        let fr = Rect::from_min_size(inner.min, vec2(w, inner.height()));
        p.rect_filled(fr, CornerRadius::ZERO, fill);
        // A light top half gives the bar some depth.
        let top = Rect::from_min_size(fr.min, vec2(fr.width(), (fr.height() * 0.4).max(1.0)));
        p.rect_filled(top, CornerRadius::ZERO, Color32::from_white_alpha(38));
    }
    if let Some(t) = label {
        text_outlined(p, r.center(), Align2::CENTER_CENTER, t, font_bold(text::SMALL), color::TEXT);
    }
}

/// A small label in a colored box (tier, voltage, research state). `left_center` is the middle of
/// its left edge. Returns the box.
pub fn badge(p: &Painter, left_center: Pos2, label: &str, c: Color32) -> Rect {
    let galley = p.layout_no_wrap(label.to_string(), font_bold(text::SMALL), color::TEXT);
    let r = Rect::from_min_size(pos2(left_center.x, left_center.y - 10.0), vec2(galley.size().x + 14.0, 20.0));
    p.rect_filled(r, CornerRadius::same(2), theme::shade(c, 0.45));
    p.rect_stroke(r, CornerRadius::same(2), Stroke::new(1.0, c), StrokeKind::Inside);
    p.galley(r.center() - galley.size() * 0.5, galley, color::TEXT);
    r
}

/// A check mark in a square of size `r` (done goals, checkboxes).
pub fn check_mark(p: &Painter, r: Rect, c: Color32) {
    let s = Stroke::new(3.0, c);
    let at = |x: f32, y: f32| pos2(r.left() + x * r.width(), r.top() + y * r.height());
    p.line_segment([at(0.22, 0.5), at(0.42, 0.72)], s);
    p.line_segment([at(0.42, 0.72), at(0.78, 0.25)], s);
}

/// The height of a text that wraps at `width`.
pub fn text_height(ctx: &egui::Context, text: &str, font: FontId, width: f32) -> f32 {
    ctx.fonts_mut(|f| f.layout(text.to_string(), font, Color32::WHITE, width).size().y)
}

/// Draw a text that wraps at `width`, with its top left at `pos`. Returns its rectangle.
pub fn wrapped(p: &Painter, pos: Pos2, text: &str, font: FontId, c: Color32, width: f32) -> Rect {
    let galley = p.layout(text.to_string(), font, c, width);
    let r = Rect::from_min_size(pos, galley.size());
    p.galley(pos, galley, c);
    r
}

/// A bar whose color goes from green to yellow to red as it fills (temperature, load).
pub fn danger_color(frac: f32) -> Color32 {
    if frac < 0.6 {
        color::GREEN
    } else if frac < 0.85 {
        theme::mix(color::GREEN, color::YELLOW, (frac - 0.6) / 0.25)
    } else {
        theme::mix(color::YELLOW, color::RED, ((frac - 0.85) / 0.15).min(1.0))
    }
}

/// A colored status dot.
pub fn status_dot(p: &Painter, center: Pos2, c: Color32) {
    p.circle_filled(center + vec2(0.0, 1.0), 6.0, Color32::from_black_alpha(160));
    p.circle_filled(center, 5.5, c);
    p.circle_filled(center + vec2(-1.5, -1.5), 2.0, Color32::from_white_alpha(90));
}

pub fn status_color(c: crate::model::StatusColor) -> Color32 {
    match c {
        crate::model::StatusColor::Green => color::GREEN,
        crate::model::StatusColor::Yellow => color::YELLOW,
        crate::model::StatusColor::Red => color::RED,
        crate::model::StatusColor::Gray => color::GRAY,
    }
}

// ---------------------------------------------------------------- tabs

/// A big tab with an icon (the crafting groups). `selected` joins it to the panel below.
#[allow(clippy::too_many_arguments)]
pub fn icon_tab(ui: &Ui, id: Id, r: Rect, item: ItemRef, label: &str, selected: bool, dim: bool, atlas: &IconAtlas) -> Response {
    let resp = ui.interact(r, id, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label));
    let p = ui.painter();
    let fill = if selected {
        color::SHALLOW
    } else if resp.hovered() {
        theme::mix(color::DEEP, color::BUTTON_HOVER, 0.35)
    } else {
        theme::shade(color::DEEP, 1.25)
    };
    p.rect_filled(r.expand(1.0), CornerRadius::ZERO, color::WINDOW_DARK);
    raised(p, r, fill, theme::shade(fill, 1.35), theme::shade(fill, 0.6));
    if selected {
        // Hide the line between the tab and the panel, and mark it with orange.
        p.hline(r.x_range().shrink(1.0), r.bottom() + 0.5, Stroke::new(2.0, color::SHALLOW));
        p.hline(r.x_range().shrink(2.0), r.top() + 1.5, Stroke::new(2.0, color::ORANGE));
    }
    let icon = Rect::from_center_size(r.center() - vec2(0.0, 6.0), Vec2::splat(40.0));
    let tint = if dim { Color32::from_gray(90) } else { Color32::WHITE };
    atlas.paint(p, item, icon, tint);
    let tc = if selected { color::HEADING } else if dim { color::TEXT_FAINT } else { color::TEXT_DIM };
    p.text(pos2(r.center().x, r.bottom() - 4.0), Align2::CENTER_BOTTOM, label, font_bold(12.0), tc);
    resp
}

// ---------------------------------------------------------------- windows

/// Result of drawing a window frame.
pub struct WindowFrame {
    /// The area inside the frame, below the title bar.
    pub content: Rect,
    pub close_clicked: bool,
    /// How far the player dragged the title bar this frame.
    pub drag: Vec2,
}

/// Draw a window frame with a title bar, a drag area and a close button, inside `outer`.
pub fn window(ui: &Ui, id: Id, outer: Rect, title: &str, closable: bool) -> WindowFrame {
    let p = ui.painter();
    window_frame(p, outer);
    // Block clicks from going through the window.
    let _ = ui.interact(outer, id.with("bg"), Sense::click());
    let title_rect = Rect::from_min_size(outer.min, vec2(outer.width(), size::TITLE_H));
    let title_pos = pos2(outer.left() + size::PAD, title_rect.center().y);
    let t = p.text(title_pos, Align2::LEFT_CENTER, title, font_bold(text::TITLE), color::HEADING);
    let close_size = 24.0;
    let close_rect = Rect::from_center_size(pos2(outer.right() - size::PAD - close_size * 0.5, title_rect.center().y), Vec2::splat(close_size));
    let drag_right = if closable { close_rect.left() - 8.0 } else { outer.right() - size::PAD };
    let drag_rect = Rect::from_min_max(pos2(t.right() + 10.0, title_rect.top() + 10.0), pos2(drag_right, title_rect.bottom() - 10.0));
    if drag_rect.width() > 8.0 {
        drag_pattern(p, drag_rect);
    }
    let drag_resp = ui.interact(title_rect, id.with("drag"), Sense::drag());
    let drag = if drag_resp.dragged() { drag_resp.drag_delta() } else { Vec2::ZERO };
    if drag_resp.hovered() || drag_resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    let close_clicked = closable && close_button(ui, id.with("close"), close_rect).clicked();
    let content = Rect::from_min_max(pos2(outer.left() + size::PAD, title_rect.bottom()), pos2(outer.right() - size::PAD, outer.bottom() - size::PAD));
    WindowFrame { content, close_clicked, drag }
}

/// The ribbed area in a title bar that shows "drag here" (as in Factorio).
pub fn drag_pattern(p: &Painter, r: Rect) {
    sunken(p, r, theme::shade(color::WINDOW, 0.85));
    let mut x = r.left() + 3.0;
    while x < r.right() - 2.0 {
        p.vline(x + 0.5, (r.top() + 3.0)..=(r.bottom() - 3.0), Stroke::new(1.0, theme::shade(color::WINDOW, 1.45)));
        p.vline(x + 1.5, (r.top() + 3.0)..=(r.bottom() - 3.0), Stroke::new(1.0, theme::shade(color::WINDOW, 0.55)));
        x += 4.0;
    }
}

/// The outer size of a window whose content has size `content`.
pub fn window_outer(content: Vec2) -> Vec2 {
    vec2(content.x + 2.0 * size::PAD, content.y + size::TITLE_H + size::PAD)
}

/// Place a window of size `outer` at the screen center plus `offset`, kept inside the screen.
pub fn place(screen: Rect, outer: Vec2, offset: Vec2) -> Rect {
    let mut r = Rect::from_center_size(screen.center() + offset, outer);
    // Keep the title bar on the screen.
    let dx = if r.left() < screen.left() {
        screen.left() - r.left()
    } else if r.right() > screen.right() {
        screen.right() - r.right()
    } else {
        0.0
    };
    let dy = if r.top() < screen.top() {
        screen.top() - r.top()
    } else if r.top() > screen.bottom() - size::TITLE_H {
        screen.bottom() - size::TITLE_H - r.top()
    } else {
        0.0
    };
    r = r.translate(vec2(dx, dy));
    Rect::from_min_size(r.min.round(), r.size())
}

/// Show an egui area that holds a window at a fixed place.
pub fn area(ctx: &egui::Context, id: Id, order: egui::Order, rect: Rect, add: impl FnOnce(&mut Ui)) -> egui::InnerResponse<()> {
    egui::Area::new(id)
        .order(order)
        .fixed_pos(rect.min)
        .constrain(false)
        .fade_in(false)
        .movable(false)
        .show(ctx, |ui| {
            ui.set_min_size(rect.size());
            ui.set_max_size(rect.size());
            let _ = ui.allocate_exact_size(rect.size(), Sense::hover());
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), add);
        })
}

/// A light text field (Factorio style). Returns the egui response.
pub fn text_field(ui: &mut Ui, id: Id, r: Rect, text: &mut String, hint: &str) -> Response {
    ui.painter().rect_filled(r.expand(1.0), CornerRadius::ZERO, color::WINDOW_DARK);
    sunken(ui.painter(), r, color::FIELD);
    ui.scope(|ui| {
        // The hint text uses the "weak" text color: make it dark gray on the light field.
        ui.visuals_mut().weak_text_color = Some(Color32::from_gray(128));
        let edit = egui::TextEdit::singleline(text)
            .id(id)
            .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(7, 3)))
            .text_color(color::FIELD_TEXT)
            .font(font(text::BODY))
            .hint_text(hint)
            .desired_width(r.width() - 14.0)
            .vertical_align(egui::Align::Center);
        ui.put(r, edit)
    })
    .inner
}

/// Text with a dark outline on all sides, readable on any background (light or dark bars).
pub fn text_outlined(p: &Painter, pos: Pos2, anchor: Align2, text: &str, font: FontId, color: Color32) -> Rect {
    let shadow = Color32::from_black_alpha(220);
    for d in [vec2(-1.0, 0.0), vec2(1.0, 0.0), vec2(0.0, -1.0), vec2(0.0, 1.0), vec2(1.0, 1.0)] {
        p.text(pos + d, anchor, text, font.clone(), shadow);
    }
    p.text(pos, anchor, text, font, color)
}
