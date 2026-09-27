//! Colors, fonts and the egui style. The look follows the Factorio GUI:
//! dark gray frames with a bevel, darker "deep" frames for slots, light gray buttons with
//! dark text, orange for hover and selection, green and red for good and bad.

use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId, TextStyle};
use std::sync::Arc;

/// All sizes are in points at UI scale 1. The UI scale setting changes the egui zoom factor.
pub mod size {
    /// An item slot.
    pub const SLOT: f32 = 40.0;
    /// An icon inside a slot.
    pub const ICON: f32 = 32.0;
    /// Height of a window title bar.
    pub const TITLE_H: f32 = 36.0;
    /// Space between the window edge and its content.
    pub const PAD: f32 = 12.0;
    /// Space between content parts.
    pub const GAP: f32 = 8.0;
    /// A standard button.
    pub const BUTTON_H: f32 = 32.0;
}

pub mod color {
    use egui::Color32;

    /// Outer window background.
    pub const WINDOW: Color32 = Color32::from_rgb(49, 48, 49);
    pub const WINDOW_LIGHT: Color32 = Color32::from_rgb(78, 77, 78);
    pub const WINDOW_DARK: Color32 = Color32::from_rgb(20, 20, 20);
    /// A raised panel inside a window ("inside shallow frame").
    pub const SHALLOW: Color32 = Color32::from_rgb(64, 63, 64);
    pub const SHALLOW_LIGHT: Color32 = Color32::from_rgb(84, 83, 84);
    /// A sunken panel for slot grids and lists ("inside deep frame").
    pub const DEEP: Color32 = Color32::from_rgb(36, 35, 36);
    pub const DEEP_EDGE_DARK: Color32 = Color32::from_rgb(14, 14, 14);
    pub const DEEP_EDGE_LIGHT: Color32 = Color32::from_rgb(70, 69, 70);

    /// Item slot.
    pub const SLOT: Color32 = Color32::from_rgb(122, 121, 122);
    pub const SLOT_LIGHT: Color32 = Color32::from_rgb(158, 157, 158);
    pub const SLOT_DARK: Color32 = Color32::from_rgb(78, 77, 78);
    /// An empty slot in a quickbar or a building (darker than a normal slot).
    pub const SLOT_EMPTY: Color32 = Color32::from_rgb(58, 57, 58);
    /// Recipe that the player cannot make now.
    pub const SLOT_RED: Color32 = Color32::from_rgb(150, 62, 52);
    pub const SLOT_RED_LIGHT: Color32 = Color32::from_rgb(186, 88, 76);
    pub const SLOT_RED_DARK: Color32 = Color32::from_rgb(96, 36, 30);

    /// Standard button.
    pub const BUTTON: Color32 = Color32::from_rgb(142, 141, 142);
    pub const BUTTON_LIGHT: Color32 = Color32::from_rgb(186, 185, 186);
    pub const BUTTON_DARK: Color32 = Color32::from_rgb(86, 85, 86);
    pub const BUTTON_HOVER: Color32 = Color32::from_rgb(250, 190, 100);
    pub const BUTTON_PRESSED: Color32 = Color32::from_rgb(240, 150, 50);
    pub const BUTTON_TEXT: Color32 = Color32::from_rgb(10, 10, 10);
    pub const BUTTON_DISABLED: Color32 = Color32::from_rgb(92, 91, 92);
    pub const BUTTON_DISABLED_TEXT: Color32 = Color32::from_rgb(58, 57, 58);
    /// Confirm button (Play, Save, Load).
    pub const GREEN_BUTTON: Color32 = Color32::from_rgb(94, 182, 99);
    pub const GREEN_BUTTON_HOVER: Color32 = Color32::from_rgb(130, 214, 128);
    /// Back and delete buttons.
    pub const RED_BUTTON: Color32 = Color32::from_rgb(210, 70, 58);
    pub const RED_BUTTON_HOVER: Color32 = Color32::from_rgb(240, 110, 90);

    /// Hover and selection.
    pub const ORANGE: Color32 = Color32::from_rgb(255, 166, 48);
    pub const ORANGE_DIM: Color32 = Color32::from_rgb(196, 122, 30);
    /// Window titles and headings.
    pub const HEADING: Color32 = Color32::from_rgb(255, 230, 192);
    pub const TEXT: Color32 = Color32::from_rgb(255, 255, 255);
    pub const TEXT_DIM: Color32 = Color32::from_rgb(190, 188, 186);
    pub const TEXT_FAINT: Color32 = Color32::from_rgb(130, 128, 126);

    pub const GREEN: Color32 = Color32::from_rgb(104, 212, 84);
    pub const YELLOW: Color32 = Color32::from_rgb(240, 202, 56);
    pub const RED: Color32 = Color32::from_rgb(242, 74, 58);
    pub const RED_TEXT: Color32 = Color32::from_rgb(255, 104, 90);
    pub const GRAY: Color32 = Color32::from_rgb(150, 150, 150);

    /// Progress bars.
    pub const PROGRESS: Color32 = Color32::from_rgb(255, 170, 40);
    pub const BAR_TRACK: Color32 = Color32::from_rgb(24, 24, 24);
    pub const HULL: Color32 = Color32::from_rgb(92, 196, 70);
    pub const HEAT: Color32 = Color32::from_rgb(255, 128, 40);
    pub const POWER: Color32 = Color32::from_rgb(92, 196, 70);
    pub const STORAGE: Color32 = Color32::from_rgb(90, 160, 240);

    /// Tooltip.
    pub const TOOLTIP: Color32 = Color32::from_rgba_premultiplied(34, 33, 34, 246);
    pub const TOOLTIP_HEADER: Color32 = Color32::from_rgb(92, 70, 38);
    pub const TOOLTIP_LINE: Color32 = Color32::from_rgb(70, 69, 70);

    /// Text field (light, as in Factorio).
    pub const FIELD: Color32 = Color32::from_rgb(226, 226, 226);
    pub const FIELD_TEXT: Color32 = Color32::from_rgb(12, 12, 12);

    /// Dark overlay behind menus.
    pub const DIM: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 150);

    /// The colored band of machine icons and tier labels, by tier.
    pub const TIER: [Color32; 6] = [
        Color32::from_rgb(150, 104, 60),  // 0 Hand and Fire: wood brown
        Color32::from_rgb(214, 144, 62),  // 1 Steam: bronze
        Color32::from_rgb(150, 170, 190), // 2 LV: steel blue gray
        Color32::from_rgb(230, 200, 70),  // 3 MV: yellow
        Color32::from_rgb(90, 190, 220),  // 4 HV: cyan
        Color32::from_rgb(190, 110, 230), // 5 EV: violet
    ];

    /// Colors for graph lines, in order.
    pub const SERIES: [Color32; 8] = [
        Color32::from_rgb(255, 170, 40),
        Color32::from_rgb(90, 190, 255),
        Color32::from_rgb(120, 220, 90),
        Color32::from_rgb(240, 90, 90),
        Color32::from_rgb(200, 130, 255),
        Color32::from_rgb(255, 230, 90),
        Color32::from_rgb(90, 230, 200),
        Color32::from_rgb(240, 150, 200),
    ];
}

/// Font families. `Proportional` is Titillium Web SemiBold (the main UI font).
pub fn regular() -> FontFamily {
    FontFamily::Name("regular".into())
}

pub fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}

pub fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn font_regular(size: f32) -> FontId {
    FontId::new(size, regular())
}

pub fn font_bold(size: f32) -> FontId {
    FontId::new(size, bold())
}

/// Text sizes.
pub mod text {
    pub const BODY: f32 = 15.0;
    pub const SMALL: f32 = 13.0;
    pub const TITLE: f32 = 18.0;
    pub const COUNT: f32 = 14.0;
    pub const BUTTON: f32 = 16.0;
}

static TITILLIUM_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/TitilliumWeb-Regular.ttf");
static TITILLIUM_SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/TitilliumWeb-SemiBold.ttf");
static TITILLIUM_BOLD: &[u8] = include_bytes!("../../../assets/fonts/TitilliumWeb-Bold.ttf");

/// The font definitions: Titillium Web first, the egui fonts as fallback for symbols.
pub fn fonts() -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let fallback: Vec<String> = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    defs.font_data.insert("titillium-regular".into(), Arc::new(FontData::from_static(TITILLIUM_REGULAR)));
    defs.font_data.insert("titillium-semibold".into(), Arc::new(FontData::from_static(TITILLIUM_SEMIBOLD)));
    defs.font_data.insert("titillium-bold".into(), Arc::new(FontData::from_static(TITILLIUM_BOLD)));
    let family = |first: &str| {
        let mut list = vec![first.to_string()];
        list.extend(fallback.iter().cloned());
        list
    };
    defs.families.insert(FontFamily::Proportional, family("titillium-semibold"));
    defs.families.insert(regular(), family("titillium-regular"));
    defs.families.insert(bold(), family("titillium-bold"));
    defs
}

/// Install the fonts and the style into an egui context. Call once at the start.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.options_mut(|o| {
        // The UI scale setting sets the zoom. Do not let Ctrl +/- change it.
        o.zoom_with_keyboard = false;
    });
    ctx.all_styles_mut(style);
}

/// Change an egui style to the Foundry look. Used for egui widgets (text fields, scroll bars)
/// and for debug panels that other crates draw with egui.
pub fn style(s: &mut egui::Style) {
    use color::*;
    s.text_styles = [
        (TextStyle::Heading, font(text::TITLE)),
        (TextStyle::Body, font(text::BODY)),
        (TextStyle::Button, font(text::BUTTON)),
        (TextStyle::Small, font(text::SMALL)),
        (TextStyle::Monospace, FontId::monospace(13.0)),
    ]
    .into();
    s.animation_time = 0.08;
    s.spacing.item_spacing = egui::vec2(8.0, 6.0);
    s.spacing.button_padding = egui::vec2(10.0, 4.0);
    s.spacing.interact_size = egui::vec2(32.0, 26.0);
    s.spacing.scroll.bar_width = 10.0;
    s.spacing.scroll.floating = false;
    s.spacing.tooltip_width = 360.0;

    let v = &mut s.visuals;
    v.dark_mode = true;
    v.override_text_color = None;
    v.panel_fill = WINDOW;
    v.window_fill = WINDOW;
    v.window_stroke = egui::Stroke::new(1.0, WINDOW_DARK);
    v.window_corner_radius = egui::CornerRadius::same(2);
    v.menu_corner_radius = egui::CornerRadius::same(2);
    v.window_shadow = egui::Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(120) };
    v.popup_shadow = egui::Shadow { offset: [0, 3], blur: 8, spread: 0, color: Color32::from_black_alpha(120) };
    v.extreme_bg_color = DEEP;
    v.faint_bg_color = SHALLOW;
    v.text_edit_bg_color = Some(FIELD);
    v.selection.bg_fill = ORANGE_DIM;
    v.selection.stroke = egui::Stroke::new(1.0, BUTTON_TEXT);
    v.hyperlink_color = ORANGE;
    v.warn_fg_color = YELLOW;
    v.error_fg_color = RED;
    v.text_cursor.stroke = egui::Stroke::new(2.0, FIELD_TEXT);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = WINDOW;
    w.noninteractive.weak_bg_fill = WINDOW;
    w.noninteractive.bg_stroke = egui::Stroke::new(1.0, DEEP_EDGE_DARK);
    w.noninteractive.fg_stroke = egui::Stroke::new(1.0, TEXT);
    w.inactive.bg_fill = BUTTON;
    w.inactive.weak_bg_fill = BUTTON;
    w.inactive.bg_stroke = egui::Stroke::new(1.0, BUTTON_DARK);
    w.inactive.fg_stroke = egui::Stroke::new(1.0, BUTTON_TEXT);
    w.hovered.bg_fill = BUTTON_HOVER;
    w.hovered.weak_bg_fill = BUTTON_HOVER;
    w.hovered.bg_stroke = egui::Stroke::new(1.0, ORANGE);
    w.hovered.fg_stroke = egui::Stroke::new(1.0, BUTTON_TEXT);
    w.active.bg_fill = BUTTON_PRESSED;
    w.active.weak_bg_fill = BUTTON_PRESSED;
    w.active.bg_stroke = egui::Stroke::new(1.0, ORANGE);
    w.active.fg_stroke = egui::Stroke::new(1.0, BUTTON_TEXT);
    w.open = w.active;
    for wv in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        wv.corner_radius = egui::CornerRadius::same(2);
        wv.expansion = 0.0;
    }
}

/// Convert RGBA bytes to an egui color.
pub fn rgba(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

/// The same color with full alpha.
pub fn opaque(c: [u8; 4]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// Multiply the brightness of a color (alpha stays).
pub fn shade(c: Color32, f: f32) -> Color32 {
    let m = |v: u8| ((v as f32 * f).round().clamp(0.0, 255.0)) as u8;
    Color32::from_rgba_premultiplied(m(c.r()), m(c.g()), m(c.b()), c.a())
}

/// Mix two opaque colors. `t` = 0 gives `a`, 1 gives `b`.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

/// The color of a tier band.
pub fn tier_color(tier: u8) -> Color32 {
    color::TIER[(tier as usize).min(color::TIER.len() - 1)]
}
