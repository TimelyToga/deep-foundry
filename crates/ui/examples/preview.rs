//! An interactive preview of all UI screens with the mock model.
//!
//! Run: `cargo run -p foundry_ui --example preview`
//!
//! The mock game applies the UI actions, so crafting, slot clicks, saves and menus work.
//! Keys of the game UI: E (character), P (statistics), Esc (close / pause), 1-0 (quickbar).
//! Extra keys of the preview are listed in the help box (F1).

use eframe::egui;
use foundry_ui::mock::{self, MockGame};
use foundry_ui::{FoundryUi, GameState, HoverView, SettingChange, Stack, UiAction};

struct Preview {
    ui: FoundryUi,
    game: MockGame,
    help: bool,
    last_time: f64,
    /// When the current message appeared.
    message_since: Option<f64>,
}

impl Preview {
    fn new(cc: &eframe::CreationContext) -> Self {
        let content = mock::content();
        Self { ui: FoundryUi::new(&cc.egui_ctx), game: MockGame::new(content), help: true, last_time: 0.0, message_since: None }
    }

    /// Keys that only the preview has. The game does these things from the world.
    fn preview_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        use egui::Key;
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k));
        let c = self.game.content();
        if pressed(Key::F1) {
            self.help = !self.help;
        }
        if pressed(Key::F2) {
            self.game.open_building(mock::steam_assembler_view(&c));
        }
        if pressed(Key::F3) {
            self.game.open_building(mock::boiler_view(&c));
        }
        if pressed(Key::F4) {
            self.game.open_building(mock::electric_furnace_view(&c));
        }
        if pressed(Key::F5) {
            self.game.open_power();
        }
        if pressed(Key::F6) {
            let hand = &mut self.game.model.player.hand;
            *hand = match hand {
                Some(_) => None,
                None => Some(Stack { item: mock::it(&c, "bronze_gear"), count: 17 }),
            };
        }
        if pressed(Key::F7) {
            let hover = &mut self.game.model.hover;
            *hover = match hover {
                Some(HoverView::Cell { .. }) => Some(mock::hover_building(&c)),
                _ => Some(HoverView::Cell {
                    pos: foundry_core::CellPos { x: 4133, y: 612 },
                    material: c.expect_material("raw_malachite"),
                    temperature: 24.0,
                }),
            };
        }
        if pressed(Key::F8) {
            let a = if self.game.model.state == GameState::MainMenu { UiAction::Continue } else { UiAction::QuitToMenu };
            self.game.apply(a);
        }
        if pressed(Key::F9) {
            let steps = [0.75, 1.0, 1.25, 1.5, 1.75, 2.0];
            let now = self.game.model.settings.ui_scale;
            let next = steps.iter().copied().find(|s| *s > now + 0.01).unwrap_or(steps[0]);
            self.game.apply(UiAction::ChangeSetting(SettingChange::UiScale(next)));
        }
        if pressed(Key::Q) {
            self.game.apply(UiAction::ClearHand);
        }
    }

    fn help_box(&self, ctx: &egui::Context) {
        if !self.help {
            return;
        }
        let lines = [
            "UI preview (mock data)",
            "E  character screen    P  statistics    Esc  close / pause",
            "1-0, Shift+1-0  quickbar    Q  empty the hand",
            "F2 steam assembler   F3 boiler   F4 electric furnace",
            "F5 power network   F6 item in hand   F7 hover cell / building",
            "F8 main menu / game   F9 UI scale   F1 hide this help",
        ];
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("preview-help")));
        let screen = ctx.content_rect();
        let w = 470.0;
        let r = egui::Rect::from_min_size(egui::pos2(screen.center().x - w * 0.5, screen.top() + 8.0), egui::vec2(w, 12.0 + lines.len() as f32 * 19.0));
        painter.rect_filled(r, 4.0, egui::Color32::from_black_alpha(170));
        for (i, l) in lines.iter().enumerate() {
            let color = if i == 0 { foundry_ui::theme::color::HEADING } else { egui::Color32::from_gray(230) };
            painter.text(r.left_top() + egui::vec2(10.0, 6.0 + i as f32 * 19.0), egui::Align2::LEFT_TOP, l, foundry_ui::theme::font(14.0), color);
        }
    }
}

impl eframe::App for Preview {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        let dt = if self.last_time == 0.0 { 0.0 } else { (now - self.last_time) as f32 };
        self.last_time = now;
        self.preview_keys(&ctx);
        if self.game.model.state != GameState::MainMenu {
            mock::paint_world(ui.painter(), ctx.content_rect());
        }
        let actions = self.ui.show(&ctx, &self.game.model);
        for a in &actions {
            println!("action: {a:?}");
        }
        self.game.apply_all(actions);
        self.game.tick(dt.min(0.1));
        // Clear the message after 3 seconds.
        if self.game.model.message.is_empty() {
            self.message_since = None;
        } else {
            let since = *self.message_since.get_or_insert(now);
            if now - since > 3.0 {
                self.game.model.message.clear();
            }
        }
        self.help_box(&ctx);
        if self.game.quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Deep Foundry UI preview").with_inner_size([1600.0, 900.0]),
        ..Default::default()
    };
    eframe::run_native("Deep Foundry UI preview", options, Box::new(|cc| Ok(Box::new(Preview::new(cc)))))
}
