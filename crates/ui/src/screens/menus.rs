//! Main menu, pause menu, and their dialogs: new game, save, load, settings, and yes/no
//! confirmations.

use super::Cx;
use crate::action::{GameMode, SettingChange, UiAction, WorldSize};
use crate::format;
use crate::model::GameState;
use crate::theme::{self, color, font, font_bold, font_regular, size, text};
use crate::widgets::{self, ButtonKind};
use crate::{Confirm, MenuPage, UiState};
use egui::{Align2, Color32, CornerRadius, Id, LayerId, Mesh, Order, Painter, Rect, Shape, Ui, Vec2, pos2, vec2};
use std::sync::Arc;

const MENU_W: f32 = 320.0;
const BIG_BUTTON_H: f32 = 40.0;

pub(crate) fn main_menu(cx: &mut Cx, st: &mut UiState) {
    let screen = cx.ctx.content_rect();
    let bg = Id::new("menu-background");
    widgets::area(cx.ctx, bg, Order::Middle, screen, |ui| {
        let p = ui.painter();
        paint_strata(p, screen, &mut st.menu_background);
        let title_y = screen.top() + screen.height() * 0.17;
        let f = font_bold(84.0);
        p.text(pos2(screen.center().x + 4.0, title_y + 5.0), Align2::CENTER_CENTER, "DEEP FOUNDRY", f.clone(), Color32::from_black_alpha(200));
        p.text(pos2(screen.center().x, title_y), Align2::CENTER_CENTER, "DEEP FOUNDRY", f, color::HEADING);
        p.text(
            pos2(screen.center().x, title_y + 58.0),
            Align2::CENTER_CENTER,
            "Dig down. Build the factory. Reach the core.",
            font_regular(20.0),
            color::TEXT_DIM,
        );
        widgets::text_shadow(p, screen.left_bottom() + vec2(12.0, -10.0), Align2::LEFT_BOTTOM, concat!("Version ", env!("CARGO_PKG_VERSION")), font_regular(text::SMALL), color::TEXT_FAINT);
    });
    match st.menu.page {
        MenuPage::Root => main_root(cx, st),
        page => dialog_page(cx, st, page),
    }
    confirm_dialog(cx, st);
}

pub(crate) fn pause_menu(cx: &mut Cx, st: &mut UiState) {
    let screen = cx.ctx.content_rect();
    let dim = Id::new("pause-dim");
    widgets::area(cx.ctx, dim, Order::Foreground, screen, |ui| {
        ui.painter().rect_filled(screen, CornerRadius::ZERO, color::DIM);
        let _ = ui.interact(screen, Id::new("pause-dim-block"), egui::Sense::click());
        widgets::text_shadow(ui.painter(), pos2(screen.center().x, screen.top() + 60.0), Align2::CENTER_CENTER, "Paused", font_bold(40.0), color::HEADING);
    });
    cx.ctx.move_to_top(LayerId::new(Order::Foreground, dim));
    match st.menu.page {
        MenuPage::Root => pause_root(cx, st),
        page => dialog_page(cx, st, page),
    }
    confirm_dialog(cx, st);
}

/// A column of big buttons in a window. Returns the index of the clicked button.
fn button_column(cx: &mut Cx, id: Id, title: &str, buttons: &[(&str, ButtonKind, bool)], offset: Vec2) -> Option<usize> {
    let n = buttons.len() as f32;
    let content = vec2(MENU_W - 2.0 * size::PAD, n * BIG_BUTTON_H + (n - 1.0) * 10.0 + 4.0);
    let outer = widgets::window_outer(content);
    let rect = widgets::place(cx.ctx.content_rect(), outer, offset);
    let mut clicked = None;
    widgets::area(cx.ctx, id, Order::Foreground, rect, |ui| {
        let f = widgets::window(ui, id, rect, title, false);
        for (i, (label, kind, enabled)) in buttons.iter().enumerate() {
            let r = Rect::from_min_size(pos2(f.content.left(), f.content.top() + 2.0 + i as f32 * (BIG_BUTTON_H + 10.0)), vec2(f.content.width(), BIG_BUTTON_H));
            if widgets::button(ui, id.with(i), r, label, *kind, *enabled).clicked() {
                clicked = Some(i);
            }
        }
    });
    cx.ctx.move_to_top(LayerId::new(Order::Foreground, id));
    clicked
}

fn main_root(cx: &mut Cx, st: &mut UiState) {
    let has_save = !cx.model.saves.is_empty();
    let buttons = [
        ("Continue", ButtonKind::Normal, has_save),
        ("New game", ButtonKind::Normal, true),
        ("Load game", ButtonKind::Normal, has_save),
        ("Settings", ButtonKind::Normal, true),
        ("Quit game", ButtonKind::Normal, true),
    ];
    match button_column(cx, Id::new("main-menu"), "Main menu", &buttons, vec2(0.0, 90.0)) {
        Some(0) => cx.act(UiAction::Continue),
        Some(1) => open_new_game(cx, st),
        Some(2) => st.menu.page = MenuPage::Load,
        Some(3) => st.menu.page = MenuPage::Settings,
        Some(4) => cx.act(UiAction::QuitGame),
        _ => {}
    }
}

fn pause_root(cx: &mut Cx, st: &mut UiState) {
    let buttons = [
        ("Resume", ButtonKind::Confirm, true),
        ("Save game", ButtonKind::Normal, true),
        ("Load game", ButtonKind::Normal, !cx.model.saves.is_empty()),
        ("Settings", ButtonKind::Normal, true),
        ("Quit to main menu", ButtonKind::Normal, true),
        ("Quit game", ButtonKind::Back, true),
    ];
    match button_column(cx, Id::new("pause-menu"), "Menu", &buttons, Vec2::ZERO) {
        Some(0) => cx.act(UiAction::Resume),
        Some(1) => {
            st.menu.page = MenuPage::Save;
            if st.menu.save_name.is_empty() {
                st.menu.save_name = cx.model.saves.first().map(|s| s.name.clone()).unwrap_or_else(|| "My factory".into());
            }
        }
        Some(2) => st.menu.page = MenuPage::Load,
        Some(3) => st.menu.page = MenuPage::Settings,
        Some(4) => cx.act(UiAction::QuitToMenu),
        Some(5) => cx.act(UiAction::QuitGame),
        _ => {}
    }
}

fn open_new_game(cx: &Cx, st: &mut UiState) {
    st.menu.page = MenuPage::NewGame;
    if st.menu.seed_text.is_empty() {
        st.menu.seed_text = random_seed(cx.ctx.input(|i| i.time)).to_string();
    }
}

/// A seed from a number (the clock). Not for security; only to pick a world.
fn random_seed(x: f64) -> u32 {
    let mut h = (x * 1000.0) as u64 ^ 0x9e37_79b9_7f4a_7c15;
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    (h % 1_000_000_000) as u32
}

fn dialog_page(cx: &mut Cx, st: &mut UiState, page: MenuPage) {
    match page {
        MenuPage::Root => {}
        MenuPage::NewGame => new_game(cx, st),
        MenuPage::Save => save_dialog(cx, st),
        MenuPage::Load => load_dialog(cx, st),
        MenuPage::Settings => settings(cx, st),
    }
}

/// A dialog window with a Back button at the bottom left and an optional confirm button at the
/// bottom right, as in Factorio. Returns (back clicked, confirm clicked).
fn dialog(
    cx: &mut Cx,
    id: Id,
    title: &str,
    content: Vec2,
    confirm: Option<(&str, ButtonKind, bool)>,
    add: impl FnOnce(&mut Ui, &mut Cx, Rect),
) -> (bool, bool) {
    let outer = widgets::window_outer(content + vec2(0.0, 12.0 + size::BUTTON_H + 8.0));
    let screen = cx.ctx.content_rect();
    let mut rect = widgets::place(screen, outer, Vec2::ZERO);
    if cx.model.state == GameState::MainMenu {
        // Keep the dialog below the game title.
        let min_top = screen.top() + screen.height() * 0.17 + 90.0;
        let room = (screen.bottom() - rect.bottom()).max(0.0);
        rect = rect.translate(vec2(0.0, (min_top - rect.top()).clamp(0.0, room)));
    }
    let (mut back, mut ok) = (false, false);
    widgets::area(cx.ctx, id, Order::Foreground, rect, |ui| {
        let f = widgets::window(ui, id, rect, title, false);
        let body = Rect::from_min_size(f.content.min, content);
        add(ui, cx, body);
        let bar = Rect::from_min_max(pos2(f.content.left(), f.content.bottom() - size::BUTTON_H - 4.0), pos2(f.content.right(), f.content.bottom()));
        widgets::drag_pattern(ui.painter(), Rect::from_min_max(pos2(bar.left() + 150.0, bar.top() + 8.0), pos2(bar.right() - 150.0, bar.bottom() - 8.0)));
        back = widgets::button(ui, id.with("back"), Rect::from_min_size(bar.min, vec2(140.0, size::BUTTON_H)), "Back", ButtonKind::Back, true).clicked();
        if let Some((label, kind, enabled)) = confirm {
            let r = Rect::from_min_size(pos2(bar.right() - 140.0, bar.top()), vec2(140.0, size::BUTTON_H));
            ok = widgets::button(ui, id.with("ok"), r, label, kind, enabled).clicked();
        }
    });
    cx.ctx.move_to_top(LayerId::new(Order::Foreground, id));
    (back, ok)
}

fn new_game(cx: &mut Cx, st: &mut UiState) {
    let seed_ok = st.menu.seed_text.trim().parse::<u64>().is_ok();
    let id = Id::new("new-game");
    let menu = &mut st.menu;
    let (back, ok) = dialog(cx, id, "New game", vec2(560.0, 306.0), Some(("Play", ButtonKind::Confirm, seed_ok)), |ui, cx, r| {
        let p = ui.painter().clone();
        let inner = Rect::from_min_size(r.min, r.size());
        widgets::shallow(&p, inner);
        let x = inner.left() + 14.0;
        let mut y = inner.top() + 14.0;
        widgets::heading(&p, pos2(x, y), "Game mode");
        y += 26.0;
        let mode_w = (inner.width() - 28.0 - 10.0) * 0.5;
        for (i, mode) in GameMode::ALL.into_iter().enumerate() {
            let br = Rect::from_min_size(pos2(x + i as f32 * (mode_w + 10.0), y), vec2(mode_w, 52.0));
            let resp = widgets::toggle(ui, Id::new(("game-mode", i)), br, "", menu.mode == mode);
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, menu.mode == mode, mode.label()));
            let line = match mode {
                GameMode::Normal => "Dig, build the factory, repair the Hub.",
                GameMode::Sandbox => "Paint any material. No robot, no factory.",
            };
            p.text(br.center() - vec2(0.0, 9.0), Align2::CENTER_CENTER, mode.label(), font_bold(text::BODY + 1.0), color::BUTTON_TEXT);
            p.text(br.center() + vec2(0.0, 11.0), Align2::CENTER_CENTER, line, font_regular(text::SMALL), Color32::from_gray(40));
            if resp.clicked() {
                menu.mode = mode;
            }
        }
        y += 52.0 + 16.0;
        widgets::heading(&p, pos2(x, y), "World seed");
        y += 26.0;
        let field = Rect::from_min_size(pos2(x, y), vec2(300.0, 30.0));
        widgets::text_field(ui, Id::new("seed-field"), field, &mut menu.seed_text, "A number");
        menu.seed_text.retain(|c| c.is_ascii_digit());
        let rb = Rect::from_min_size(pos2(field.right() + 10.0, y), vec2(120.0, 30.0));
        if widgets::button(ui, Id::new("seed-random"), rb, "Random", ButtonKind::Normal, true).clicked() {
            let t = cx.ctx.input(|i| i.time);
            menu.seed_text = random_seed(t + menu.seed_text.len() as f64 * 7.3).to_string();
        }
        y += 36.0;
        p.text(pos2(x, y), Align2::LEFT_TOP, "The same seed always makes the same world.", font_regular(text::SMALL), color::TEXT_DIM);
        y += 28.0;
        widgets::heading(&p, pos2(x, y), "World size");
        y += 26.0;
        let size_w = (inner.width() - 28.0 - 20.0) / 3.0;
        for (i, ws) in WorldSize::ALL.into_iter().enumerate() {
            let br = Rect::from_min_size(pos2(x + i as f32 * (size_w + 10.0), y), vec2(size_w, 52.0));
            let selected = menu.world_size == ws;
            let resp = widgets::toggle(ui, Id::new(("world-size", i)), br, "", selected);
            let (w, h) = ws.cells();
            p.text(br.center() - vec2(0.0, 9.0), Align2::CENTER_CENTER, ws.label(), font_bold(text::BODY + 1.0), color::BUTTON_TEXT);
            p.text(br.center() + vec2(0.0, 11.0), Align2::CENTER_CENTER, format!("{w} × {h} cells"), font_regular(text::SMALL), Color32::from_gray(40));
            if resp.clicked() {
                menu.world_size = ws;
            }
        }
    });
    if back {
        st.menu.page = MenuPage::Root;
    }
    if ok && let Ok(seed) = st.menu.seed_text.trim().parse::<u64>() {
        cx.act(UiAction::NewGame { seed, size: st.menu.world_size, mode: st.menu.mode });
        st.menu.page = MenuPage::Root;
    }
}

/// The list of saves. Returns the id of a double-clicked save.
fn save_list(ui: &mut Ui, cx: &mut Cx, r: Rect, selected: &mut Option<String>, name_out: Option<&mut String>) -> Option<String> {
    let p = ui.painter().clone();
    // Column headers.
    let head = Rect::from_min_size(r.min, vec2(r.width(), 22.0));
    p.text(pos2(head.left() + 10.0, head.center().y), Align2::LEFT_CENTER, "Name", font_bold(text::SMALL), color::TEXT_DIM);
    p.text(pos2(head.right() - 150.0, head.center().y), Align2::RIGHT_CENTER, "Saved", font_bold(text::SMALL), color::TEXT_DIM);
    p.text(pos2(head.right() - 12.0, head.center().y), Align2::RIGHT_CENTER, "Play time", font_bold(text::SMALL), color::TEXT_DIM);
    let list = Rect::from_min_max(pos2(r.left(), head.bottom() + 2.0), r.right_bottom());
    widgets::deep(&p, list);
    let saves = &cx.model.saves;
    let inner = list.shrink(2.0);
    let row_h = 44.0;
    let mut double = None;
    let mut name_out = name_out;
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
        egui::ScrollArea::vertical().id_salt("save-list").auto_shrink([false, false]).show(ui, |ui| {
            let (area, _) = ui.allocate_exact_size(vec2(inner.width(), saves.len() as f32 * row_h), egui::Sense::hover());
            let p = ui.painter().clone();
            for (i, s) in saves.iter().enumerate() {
                let row = Rect::from_min_size(pos2(area.left(), area.top() + i as f32 * row_h), vec2(area.width(), row_h - 2.0));
                let resp = ui.interact(row, Id::new(("save-row", i)), egui::Sense::click());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &s.name));
                let is_sel = selected.as_deref() == Some(s.id.as_str());
                let fill = if is_sel {
                    theme::shade(color::ORANGE_DIM, 0.8)
                } else if resp.hovered() {
                    theme::mix(color::DEEP, color::BUTTON_HOVER, 0.2)
                } else if i % 2 == 1 {
                    theme::shade(color::DEEP, 1.25)
                } else {
                    color::DEEP
                };
                p.rect_filled(row, CornerRadius::ZERO, fill);
                p.text(pos2(row.left() + 10.0, row.top() + 12.0), Align2::LEFT_CENTER, &s.name, font_bold(text::BODY + 1.0), color::TEXT);
                p.text(pos2(row.left() + 10.0, row.bottom() - 11.0), Align2::LEFT_CENTER, &s.world, font_regular(text::SMALL), color::TEXT_DIM);
                p.text(pos2(row.right() - 150.0, row.center().y), Align2::RIGHT_CENTER, &s.date, font(text::BODY), color::TEXT);
                p.text(pos2(row.right() - 12.0, row.center().y), Align2::RIGHT_CENTER, format::play_time(s.play_time_s), font(text::BODY), color::TEXT);
                if resp.clicked() {
                    *selected = Some(s.id.clone());
                    if let Some(n) = name_out.as_deref_mut() {
                        *n = s.name.clone();
                    }
                }
                if resp.double_clicked() {
                    double = Some(s.id.clone());
                }
            }
        });
    });
    if saves.is_empty() {
        p.text(list.center(), Align2::CENTER_CENTER, "No saved games yet.", font_regular(text::BODY), color::TEXT_FAINT);
    }
    double
}

fn save_dialog(cx: &mut Cx, st: &mut UiState) {
    let name_ok = !st.menu.save_name.trim().is_empty();
    let id = Id::new("save-dialog");
    let menu = &mut st.menu;
    let mut double = None;
    let (back, ok) = dialog(cx, id, "Save game", vec2(640.0, 400.0), Some(("Save", ButtonKind::Confirm, name_ok)), |ui, cx, r| {
        let list = Rect::from_min_size(r.min, vec2(r.width(), r.height() - 70.0));
        let mut sel = menu.selected_save.clone();
        double = save_list(ui, cx, list, &mut sel, Some(&mut menu.save_name));
        menu.selected_save = sel;
        let p = ui.painter().clone();
        let y = list.bottom() + 16.0;
        p.text(pos2(r.left(), y + 15.0), Align2::LEFT_CENTER, "Name", font_bold(text::BODY), color::HEADING);
        let field = Rect::from_min_size(pos2(r.left() + 60.0, y), vec2(r.width() - 60.0, 30.0));
        widgets::text_field(ui, Id::new("save-name"), field, &mut menu.save_name, "Name of the save");
    });
    if back {
        st.menu.page = MenuPage::Root;
    }
    let target = if ok {
        Some(st.menu.save_name.trim().to_string())
    } else {
        double.and_then(|id| cx.model.saves.iter().find(|s| s.id == id).map(|s| s.name.clone()))
    };
    if let Some(name) = target
        && !name.is_empty()
    {
        if cx.model.saves.iter().any(|s| s.name == name) {
            st.menu.confirm = Some(Confirm::Overwrite(name));
        } else {
            cx.act(UiAction::Save { name, overwrite: false });
            st.menu.page = MenuPage::Root;
        }
    }
}

fn load_dialog(cx: &mut Cx, st: &mut UiState) {
    let id = Id::new("load-dialog");
    let has_sel = st.menu.selected_save.as_ref().is_some_and(|s| cx.model.saves.iter().any(|x| &x.id == s));
    let menu = &mut st.menu;
    let mut double = None;
    let mut delete = false;
    let (back, ok) = dialog(cx, id, "Load game", vec2(640.0, 400.0), Some(("Load", ButtonKind::Confirm, has_sel)), |ui, cx, r| {
        let list = Rect::from_min_size(r.min, vec2(r.width(), r.height() - 48.0));
        let mut sel = menu.selected_save.clone();
        double = save_list(ui, cx, list, &mut sel, None);
        menu.selected_save = sel;
        let dr = Rect::from_min_size(pos2(r.right() - 140.0, list.bottom() + 12.0), vec2(140.0, size::BUTTON_H));
        delete = widgets::button(ui, Id::new("save-delete"), dr, "Delete", ButtonKind::Back, has_sel).clicked();
    });
    if back {
        st.menu.page = MenuPage::Root;
    }
    if delete && let Some(id) = st.menu.selected_save.clone() {
        st.menu.confirm = Some(Confirm::Delete(id));
    }
    let load = if ok { st.menu.selected_save.clone() } else { double };
    if let Some(id) = load {
        cx.act(UiAction::Load(id));
        st.menu.page = MenuPage::Root;
    }
}

/// Rows of the "Simulation" section of the settings.
fn sim_rows(cx: &Cx) -> usize {
    cx.model.settings.simulation.len().max(1)
}

fn settings(cx: &mut Cx, st: &mut UiState) {
    let id = Id::new("settings-dialog");
    let sim_h = 34.0 + sim_rows(cx) as f32 * 34.0 + 6.0;
    let height = 150.0 + 12.0 + sim_h + 14.0 + CONTROLS_H;
    let (back, _) = dialog(cx, id, "Settings", vec2(640.0, height), None, |ui, cx, r| {
        let p = ui.painter().clone();
        let s = &cx.model.settings;
        let top = Rect::from_min_size(r.min, vec2(r.width(), 150.0));
        widgets::shallow(&p, top);
        let x = top.left() + 14.0;
        let mut y = top.top() + 12.0;
        widgets::heading(&p, pos2(x, y), "Interface");
        y += 28.0;
        p.text(pos2(x, y + 14.0), Align2::LEFT_CENTER, "UI scale", font(text::BODY), color::TEXT);
        let presets = [0.75f32, 1.0, 1.25, 1.5, 1.75, 2.0];
        for (i, v) in presets.iter().enumerate() {
            let br = Rect::from_min_size(pos2(x + 100.0 + i as f32 * 76.0, y), vec2(70.0, 28.0));
            let sel = (s.ui_scale - v).abs() < 0.01;
            if widgets::toggle(ui, Id::new(("scale", i)), br, &format::percent(*v), sel).clicked() && !sel {
                cx.act(UiAction::ChangeSetting(SettingChange::UiScale(*v)));
            }
        }
        y += 44.0;
        widgets::heading(&p, pos2(x, y), "Graphics");
        y += 26.0;
        let checks = [
            ("vsync", "Vertical sync", s.vsync, SettingChange::Vsync(!s.vsync)),
            ("show-fps", "Show FPS", s.show_fps, SettingChange::ShowFps(!s.show_fps)),
            ("show-debug", &*format!("Debug panel ({})", s.key("debug")), s.show_debug, SettingChange::ShowDebug(!s.show_debug)),
        ];
        for (i, (key, label, on, change)) in checks.into_iter().enumerate() {
            let cb = Rect::from_min_size(pos2(x + i as f32 * 200.0, y), vec2(190.0, 26.0));
            if widgets::checkbox(ui, Id::new(key), cb, label, on).clicked() {
                cx.act(UiAction::ChangeSetting(change));
            }
        }

        // Simulation settings (for example the liquid rules), one slider each.
        let sim = Rect::from_min_size(pos2(r.left(), top.bottom() + 12.0), vec2(r.width(), sim_h));
        widgets::shallow(&p, sim);
        widgets::heading(&p, pos2(x, sim.top() + 10.0), "Simulation");
        let list = &cx.model.settings.simulation;
        if list.is_empty() {
            p.text(
                pos2(x, sim.top() + 44.0),
                Align2::LEFT_TOP,
                "No simulation settings yet. The liquid settings will be here.",
                font_regular(text::BODY),
                color::TEXT_DIM,
            );
        }
        for (i, setting) in list.iter().enumerate() {
            let row_y = sim.top() + 40.0 + i as f32 * 34.0;
            let label = p.text(pos2(x, row_y + 12.0), Align2::LEFT_CENTER, &setting.label, font(text::BODY), color::TEXT);
            let slider_rect = Rect::from_min_size(pos2(x + 200.0, row_y), vec2(sim.width() - 240.0, 26.0));
            let mut value = setting.value;
            let slider = egui::Slider::new(&mut value, setting.min..=setting.max)
                .step_by(setting.step as f64)
                .show_value(true);
            let resp = ui
                .scope(|ui| {
                    // Leave room for the number box on the right.
                    ui.spacing_mut().slider_width = slider_rect.width() - 80.0;
                    ui.put(slider_rect, slider)
                })
                .inner;
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, &setting.label));
            if resp.changed() {
                cx.act(UiAction::ChangeSetting(SettingChange::Simulation { key: setting.key.clone(), value }));
            }
            let hover = Rect::from_min_max(pos2(x, row_y), pos2(label.right(), row_y + 26.0));
            if !setting.help.is_empty() && ui.interact(hover, Id::new(("sim-help", i)), egui::Sense::hover()).hovered() {
                cx.tip(crate::tooltip::Tip::Text { title: setting.label.clone(), body: setting.help.clone() });
            }
        }

        controls(ui, cx, &p, r, sim.bottom() + 14.0);
    });
    if back {
        st.menu.page = MenuPage::Root;
    }
}

/// Height of the Controls section of the settings: the title row and the list.
const CONTROLS_H: f32 = 34.0 + 330.0;

/// The Controls section of the settings: match keys by position or by letter, the list of keys
/// (click a row, then press a key), and a button for the default keys.
fn controls(ui: &mut Ui, cx: &mut Cx, p: &Painter, r: Rect, top: f32) {
    let s = &cx.model.settings;
    widgets::heading(p, pos2(r.left(), top), "Controls");
    // "Match keys by position / by letter".
    let mut x = r.left() + 110.0;
    p.text(pos2(x, top + 13.0), Align2::LEFT_CENTER, "Match keys by", font_regular(text::BODY), color::TEXT_DIM);
    x += 102.0;
    for (i, (label, letter)) in [("position", false), ("letter", true)].into_iter().enumerate() {
        let br = Rect::from_min_size(pos2(x + i as f32 * 90.0, top), vec2(84.0, 26.0));
        let sel = s.keys_by_letter == letter;
        let resp = widgets::toggle(ui, Id::new(("keys-by", i)), br, label, sel);
        if resp.hovered() {
            let body = if letter {
                "A key works by the letter it types on your keyboard layout."
            } else {
                "A key works by its place on the keyboard. On other layouts the keys stay where W A S D are on QWERTY."
            };
            cx.tip(crate::tooltip::Tip::Text { title: format!("Match keys by {label}"), body: body.into() });
        }
        if resp.clicked() && !sel {
            cx.act(UiAction::ChangeSetting(SettingChange::KeysByLetter(letter)));
        }
    }
    let reset = Rect::from_min_size(pos2(r.right() - 150.0, top), vec2(150.0, 26.0));
    if widgets::button(ui, Id::new("keys-reset"), reset, "Reset to defaults", ButtonKind::Normal, true).clicked() {
        cx.act(UiAction::ChangeSetting(SettingChange::ResetKeys));
    }
    let list = Rect::from_min_max(pos2(r.left(), top + 34.0), r.right_bottom());
    widgets::deep(p, list);
    let inner = list.shrink(2.0);
    let row_h = 26.0;
    let rows = s.key_bindings.clone();
    let waiting = s.key_waiting.clone();
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
        egui::ScrollArea::vertical().id_salt("keys").auto_shrink([false, false]).show(ui, |ui| {
            let (area, _) = ui.allocate_exact_size(vec2(inner.width(), rows.len() as f32 * row_h), egui::Sense::hover());
            for (i, row) in rows.iter().enumerate() {
                let rr = Rect::from_min_size(pos2(area.left(), area.top() + i as f32 * row_h), vec2(area.width(), row_h));
                let can_change = !row.fixed && !row.id.is_empty();
                let resp = ui.interact(rr, Id::new(("key-row", i)), if can_change { egui::Sense::click() } else { egui::Sense::hover() });
                let wait = can_change && waiting.as_deref() == Some(row.id.as_str());
                let p = ui.painter();
                if wait {
                    p.rect_filled(rr, CornerRadius::ZERO, color::ORANGE.gamma_multiply(0.25));
                } else if can_change && resp.hovered() {
                    p.rect_filled(rr, CornerRadius::ZERO, Color32::from_white_alpha(14));
                } else if i % 2 == 1 {
                    p.rect_filled(rr, CornerRadius::ZERO, Color32::from_white_alpha(5));
                }
                let tc = if can_change { color::TEXT } else { color::TEXT_DIM };
                p.text(pos2(rr.left() + 10.0, rr.center().y), Align2::LEFT_CENTER, &row.action, font(text::BODY), tc);
                let (key, kc) = if wait { ("Press a key (Esc: stop)", color::ORANGE) } else { (row.key.as_str(), if can_change { color::HEADING } else { color::TEXT_DIM }) };
                p.text(pos2(rr.right() - 12.0, rr.center().y), Align2::RIGHT_CENTER, key, font_bold(text::BODY), kc);
                if resp.clicked() {
                    cx.act(UiAction::ChangeSetting(SettingChange::RebindKey(row.id.clone())));
                }
                if can_change && resp.hovered() && !wait {
                    cx.tip(crate::tooltip::Tip::Text { title: row.action.clone(), body: "Click, then press the new key.".into() });
                }
            }
        });
    });
}

fn confirm_dialog(cx: &mut Cx, st: &mut UiState) {
    let Some(confirm) = st.menu.confirm.clone() else { return };
    let (title, body, ok_label) = match &confirm {
        Confirm::Overwrite(name) => ("Overwrite save?", format!("A save named \"{name}\" already exists. Replace it?"), "Overwrite"),
        Confirm::Delete(id) => {
            let name = cx.model.saves.iter().find(|s| &s.id == id).map(|s| s.name.as_str()).unwrap_or(id.as_str());
            ("Delete save?", format!("Delete \"{name}\"? You cannot undo this."), "Delete")
        }
    };
    let screen = cx.ctx.content_rect();
    let dim = Id::new("confirm-dim");
    widgets::area(cx.ctx, dim, Order::Foreground, screen, |ui| {
        ui.painter().rect_filled(screen, CornerRadius::ZERO, Color32::from_black_alpha(110));
        let _ = ui.interact(screen, Id::new("confirm-dim-block"), egui::Sense::click());
    });
    cx.ctx.move_to_top(LayerId::new(Order::Foreground, dim));
    let id = Id::new("confirm-dialog");
    let content = vec2(440.0, 70.0 + 12.0 + size::BUTTON_H);
    let outer = widgets::window_outer(content);
    let rect = widgets::place(screen, outer, Vec2::ZERO);
    let (mut cancel, mut ok) = (false, false);
    widgets::area(cx.ctx, id, Order::Foreground, rect, |ui| {
        let f = widgets::window(ui, id, rect, title, false);
        let p = ui.painter();
        let galley = p.layout(body.clone(), font(text::BODY), color::TEXT, f.content.width());
        p.galley(f.content.min + vec2(0.0, 8.0), galley, color::TEXT);
        let by = f.content.bottom() - size::BUTTON_H;
        cancel = widgets::button(ui, id.with("cancel"), Rect::from_min_size(pos2(f.content.left(), by), vec2(140.0, size::BUTTON_H)), "Cancel", ButtonKind::Normal, true).clicked();
        ok = widgets::button(ui, id.with("ok"), Rect::from_min_size(pos2(f.content.right() - 140.0, by), vec2(140.0, size::BUTTON_H)), ok_label, ButtonKind::Back, true)
            .clicked();
    });
    cx.ctx.move_to_top(LayerId::new(Order::Foreground, id));
    if cancel {
        st.menu.confirm = None;
    }
    if ok {
        match confirm {
            Confirm::Overwrite(name) => {
                cx.act(UiAction::Save { name, overwrite: true });
                st.menu.page = MenuPage::Root;
            }
            Confirm::Delete(id) => {
                cx.act(UiAction::DeleteSave(id));
                st.menu.selected_save = None;
            }
        }
        st.menu.confirm = None;
    }
}

/// The main menu background: a cut through the layers of the planet, drawn as pixel blocks.
/// Paint the main menu background. The mesh is built once for each screen size.
fn paint_strata(p: &Painter, screen: Rect, cache: &mut Option<(Rect, Arc<Mesh>)>) {
    let mesh = match cache {
        Some((r, m)) if *r == screen => m.clone(),
        _ => {
            let m = Arc::new(strata_mesh(screen));
            *cache = Some((screen, m.clone()));
            m
        }
    };
    p.add(Shape::Mesh(mesh));
    // Darken the middle so the menu is easy to read.
    let v = Rect::from_center_size(screen.center() + vec2(0.0, 60.0), vec2(screen.width() * 0.5, screen.height() * 0.8));
    for i in 0..8 {
        p.rect_filled(v.expand(i as f32 * 30.0), CornerRadius::same(120), Color32::from_black_alpha(12));
    }
}

/// A cut through the layers of the planet, drawn as pixel blocks, with a dark sky.
fn strata_mesh(screen: Rect) -> Mesh {
    // Sky gradient.
    let mut mesh = Mesh::default();
    let sky_bottom = screen.top() + screen.height() * 0.42;
    let top_c = Color32::from_rgb(18, 20, 30);
    let bot_c = Color32::from_rgb(58, 48, 44);
    mesh.colored_vertex(screen.left_top(), top_c);
    mesh.colored_vertex(screen.right_top(), top_c);
    mesh.colored_vertex(pos2(screen.right(), screen.bottom()), bot_c);
    mesh.colored_vertex(pos2(screen.left(), screen.bottom()), bot_c);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    // Layers, from the surface down: (color, relative thickness).
    let layers: [(Color32, f32); 6] = [
        (Color32::from_rgb(88, 112, 52), 0.035),
        (Color32::from_rgb(104, 76, 50), 0.10),
        (Color32::from_rgb(100, 100, 106), 0.14),
        (Color32::from_rgb(70, 66, 72), 0.14),
        (Color32::from_rgb(52, 40, 44), 0.10),
        (Color32::from_rgb(170, 64, 24), 0.2),
    ];
    let cell = 8.0;
    let cols = (screen.width() / cell).ceil() as i32 + 1;
    let hash = |x: i32, y: i32| -> u32 {
        let mut h = (x as u32).wrapping_mul(0x27d4_eb2d) ^ (y as u32).wrapping_mul(0x1656_67b1);
        h ^= h >> 15;
        h = h.wrapping_mul(0x85eb_ca6b);
        h ^ (h >> 13)
    };
    for cx_ in 0..cols {
        let x = screen.left() + cx_ as f32 * cell;
        let wave = (cx_ as f32 * 0.09).sin() * 14.0 + (cx_ as f32 * 0.023).sin() * 30.0;
        let mut y = sky_bottom + wave;
        for (li, (c, t)) in layers.iter().enumerate() {
            let h = screen.height() * t + ((cx_ as f32 * 0.05 + li as f32).sin() * 10.0);
            let y_end = if li == layers.len() - 1 { screen.bottom() + cell } else { y + h };
            let mut yy = (y / cell).floor() * cell;
            while yy < y_end {
                let n = hash(cx_, (yy / cell) as i32 + li as i32 * 997);
                let f = 0.82 + (n % 30) as f32 / 100.0;
                let depth = ((yy - sky_bottom) / screen.height()).clamp(0.0, 1.0);
                let dark = 0.55 - depth * 0.25;
                let mut col = theme::shade(*c, f * dark);
                if li == layers.len() - 1 {
                    // Magma glows more toward the bottom.
                    let g = ((yy - y) / (y_end - y)).clamp(0.0, 1.0);
                    col = theme::mix(col, Color32::from_rgb(255, 150, 40), g * 0.55 * f);
                }
                mesh.add_colored_rect(Rect::from_min_size(pos2(x, yy), Vec2::splat(cell)), col);
                yy += cell;
            }
            y = y_end;
        }
    }
    mesh
}
