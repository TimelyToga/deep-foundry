//! Click tests for the main menu, the pause menu and their dialogs.
//!
//! The harness changes the game state as the game does (Pause, Resume, New game, Quit to menu),
//! so the tests go through the menus in the same order as a player.
//!
//! Bug found in play: after the Save page, the full-screen "Paused" dim was on top of the menu
//! buttons and took all clicks. These tests click every button after every other page.

use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use foundry_ui::mock;
use foundry_ui::{FoundryUi, GameState, UiAction, UiModel};

struct State {
    ui: Option<FoundryUi>,
    model: UiModel,
    actions: Vec<UiAction>,
}

fn harness(model: UiModel) -> Harness<'static, State> {
    let state = State { ui: None, model, actions: vec![] };
    Harness::builder().with_size(vec2(1920.0, 1080.0)).with_pixels_per_point(1.0).with_max_steps(12).build_ui_state(
        |ui, st: &mut State| {
            let ctx = ui.ctx().clone();
            let fui = st.ui.get_or_insert_with(|| FoundryUi::new(&ctx));
            let actions = fui.show(&ctx, &st.model);
            // Change the state as the game does.
            for a in &actions {
                match a {
                    UiAction::Pause => st.model.state = GameState::Paused,
                    UiAction::Resume | UiAction::NewGame { .. } | UiAction::Continue | UiAction::Load(_) => st.model.state = GameState::Playing,
                    UiAction::QuitToMenu => st.model.state = GameState::MainMenu,
                    _ => {}
                }
            }
            st.actions.extend(actions);
        },
        state,
    )
}

fn settle(h: &mut Harness<State>) {
    h.run_steps(6);
}

fn click(h: &mut Harness<State>, label: &str) {
    h.get_by_label(label).click();
    settle(h);
}

/// Take the actions of the last clicks.
fn take(h: &mut Harness<State>) -> Vec<UiAction> {
    std::mem::take(&mut h.state_mut().actions)
}

fn state(h: &Harness<State>) -> GameState {
    h.state().model.state
}

/// True if the button with this label is on the screen.
fn shown(h: &Harness<State>, label: &str) -> bool {
    h.query_by_label(label).is_some()
}

fn playing_normal() -> UiModel {
    mock::model(mock::content())
}

fn playing_sandbox() -> UiModel {
    let mut m = mock::sandbox_model(mock::content());
    m.saves = playing_normal().saves;
    m
}

/// Esc -> Save game -> Save, with a new name and with the name of an old save (overwrite).
fn save_from_pause_menu(model: UiModel) {
    let mut h = harness(model);
    settle(&mut h);
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    assert_eq!(state(&h), GameState::Paused);

    // A new name: the game saves at once.
    click(&mut h, "Save game");
    // The name starts as the name of the newest save.
    let field = h.get_by_role(egui::accesskit::Role::TextInput);
    field.click();
    field.type_text(" 2");
    settle(&mut h);
    click(&mut h, "Save");
    let saved: Vec<UiAction> = take(&mut h).into_iter().filter(|a| matches!(a, UiAction::Save { .. })).collect();
    assert_eq!(saved, [UiAction::Save { name: "Autosave 2".into(), overwrite: false }]);
    assert!(shown(&h, "Resume"), "after a save, the pause menu shows its buttons again");

    // The name of an old save: a yes/no question first. Cancel, then yes.
    click(&mut h, "Save game");
    click(&mut h, "Copper valley");
    click(&mut h, "Save");
    assert!(shown(&h, "Overwrite"), "the overwrite question is on the screen");
    click(&mut h, "Cancel");
    assert!(!shown(&h, "Overwrite"));
    click(&mut h, "Save");
    click(&mut h, "Overwrite");
    assert!(take(&mut h).contains(&UiAction::Save { name: "Copper valley".into(), overwrite: true }));

    // Back on the pause menu: every page opens and closes, then Resume works.
    for (button, page_button) in [("Save game", "Save"), ("Load game", "Load"), ("Settings", "Back"), ("Save game", "Back")] {
        click(&mut h, button);
        assert!(shown(&h, page_button), "{button}: the page did not open");
        click(&mut h, "Back");
        assert!(shown(&h, "Resume"), "{button}: Back did not go to the pause menu");
    }
    click(&mut h, "Resume");
    assert!(take(&mut h).contains(&UiAction::Resume));
    assert_eq!(state(&h), GameState::Playing);
}

#[test]
fn save_from_pause_menu_normal_mode() {
    save_from_pause_menu(playing_normal());
}

#[test]
fn save_from_pause_menu_sandbox() {
    save_from_pause_menu(playing_sandbox());
}

/// The load dialog and its delete question, in the pause menu.
#[test]
fn load_and_delete_from_pause_menu() {
    let mut h = harness(playing_normal());
    settle(&mut h);
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    click(&mut h, "Save game");
    click(&mut h, "Back");
    click(&mut h, "Load game");
    click(&mut h, "First kiln");
    click(&mut h, "Delete");
    assert!(shown(&h, "Cancel"), "the delete question is on the screen");
    click(&mut h, "Cancel");
    click(&mut h, "Delete");
    // The question has a second "Delete" button (the yes button), on top.
    h.get_all_by_label("Delete").last().unwrap().click();
    settle(&mut h);
    assert!(take(&mut h).iter().any(|a| matches!(a, UiAction::DeleteSave(_))));
    click(&mut h, "Copper valley");
    click(&mut h, "Load");
    assert!(take(&mut h).iter().any(|a| matches!(a, UiAction::Load(_))));
    assert_eq!(state(&h), GameState::Playing);
}

/// Dialogs first opened in the main menu still work in the pause menu, and the quit buttons
/// work.
#[test]
fn main_menu_dialogs_then_pause_menu() {
    let mut model = playing_normal();
    model.state = GameState::MainMenu;
    let mut h = harness(model);
    settle(&mut h);
    for (button, page_button) in [("Settings", "Back"), ("Load game", "Load"), ("New game", "Play")] {
        click(&mut h, button);
        assert!(shown(&h, page_button), "main menu {button}: the page did not open");
        click(&mut h, "Back");
        assert!(shown(&h, "Quit game"), "main menu {button}: Back did not work");
    }
    click(&mut h, "New game");
    click(&mut h, "Play");
    assert_eq!(state(&h), GameState::Playing);
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    for (button, page_button) in [("Settings", "Back"), ("Load game", "Load"), ("Save game", "Save")] {
        click(&mut h, button);
        assert!(shown(&h, page_button), "pause menu {button}: the page did not open");
        click(&mut h, "Back");
        assert!(shown(&h, "Resume"), "pause menu {button}: Back did not work");
    }
    // Esc goes back from a page, then closes the pause menu.
    click(&mut h, "Settings");
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    assert!(shown(&h, "Resume"));
    click(&mut h, "Quit to main menu");
    assert_eq!(state(&h), GameState::MainMenu);
    click(&mut h, "Quit game");
    assert!(take(&mut h).contains(&UiAction::QuitGame));
}
