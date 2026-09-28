//! Screenshots of every screen with the mock model, rendered with wgpu.
//!
//! The PNG files are in `tests/snapshots/`. A test fails if the picture changes.
//! After a wanted change, run `UPDATE_SNAPSHOTS=1 cargo test -p foundry_ui --test snapshots`
//! and look at the new pictures.

use egui::{Vec2, vec2};
use egui_kittest::kittest::Queryable;
use egui_kittest::{Harness, SnapshotOptions};
use foundry_ui::mock;
use foundry_ui::{FoundryUi, GameMode, GameState, MenuPage, UiAction, UiModel, WindowKind};

type Setup = Box<dyn FnOnce(&mut FoundryUi)>;

struct State {
    ui: Option<FoundryUi>,
    model: UiModel,
    actions: Vec<UiAction>,
    setup: Option<Setup>,
}

fn harness(size: Vec2, model: UiModel, setup: impl FnOnce(&mut FoundryUi) + 'static) -> Harness<'static, State> {
    let state = State { ui: None, model, actions: vec![], setup: Some(Box::new(setup)) };
    Harness::builder().with_size(size).with_pixels_per_point(1.0).wgpu().with_max_steps(12).build_ui_state(
        |ui, st: &mut State| {
            let ctx = ui.ctx().clone();
            let fui = st.ui.get_or_insert_with(|| FoundryUi::new(&ctx));
            if let Some(setup) = st.setup.take() {
                setup(fui);
            }
            if st.model.state != GameState::MainMenu {
                mock::paint_world(ui.painter(), ctx.content_rect());
            }
            let actions = fui.show(&ctx, &st.model);
            st.actions.extend(actions);
        },
        state,
    )
}

fn options() -> SnapshotOptions {
    // Allow a few pixels to differ between GPU drivers.
    SnapshotOptions::new().max_failed_pixels(2000)
}

fn settle(h: &mut Harness<State>) {
    h.run_steps(6);
}

const HD: Vec2 = vec2(1920.0, 1080.0);
const QHD: Vec2 = vec2(2560.0, 1440.0);

fn playing() -> UiModel {
    mock::model(mock::content())
}

#[test]
fn hud() {
    let mut h = harness(HD, playing(), |_| {});
    settle(&mut h);
    h.snapshot_options("hud_1920", &options());
}

#[test]
fn hud_building_hover_and_hand() {
    let mut model = playing();
    let c = model.content.clone();
    model.hover = Some(mock::hover_building(&c));
    model.hover_detail = mock::hover_building_detail();
    model.player.hand = Some(foundry_ui::Stack { item: mock::it(&c, "bronze_gear"), count: 17 });
    let mut h = harness(HD, model, |_| {});
    settle(&mut h);
    h.hover_at(egui::pos2(900.0, 420.0));
    settle(&mut h);
    h.snapshot_options("hud_hover_building_1920", &options());
}

#[test]
fn character_screen() {
    let mut h = harness(HD, playing(), |ui| ui.open_window(WindowKind::Character));
    settle(&mut h);
    h.get_by_label("Craft Bronze gear").hover();
    settle(&mut h);
    h.snapshot_options("character_1920", &options());
}

#[test]
fn item_tooltip() {
    let mut h = harness(HD, playing(), |ui| ui.open_window(WindowKind::Character));
    settle(&mut h);
    h.get_by_label("Bronze plate").hover();
    settle(&mut h);
    h.snapshot_options("item_tooltip_1920", &options());
}

#[test]
fn character_screen_missing_ingredients() {
    let mut h = harness(HD, playing(), |ui| ui.open_window(WindowKind::Character));
    settle(&mut h);
    h.get_by_label("Craft Vacuum tube").hover();
    settle(&mut h);
    h.snapshot_options("character_missing_1920", &options());
}

#[test]
fn character_screen_production_tab() {
    let mut h = harness(HD, playing(), |ui| {
        ui.open_window(WindowKind::Character);
        ui.select_craft_tab(foundry_ui::CraftGroup::Production);
    });
    settle(&mut h);
    h.get_by_label("Craft Steam crusher").hover();
    settle(&mut h);
    h.snapshot_options("character_production_1920", &options());
}

#[test]
fn building_window() {
    let mut model = playing();
    model.building = Some(mock::steam_assembler_view(&model.content));
    let mut h = harness(HD, model, |_| {});
    settle(&mut h);
    h.snapshot_options("building_1920", &options());
}

#[test]
fn building_recipe_picker() {
    let mut model = playing();
    model.building = Some(mock::steam_assembler_view(&model.content));
    let mut h = harness(HD, model, |_| {});
    settle(&mut h);
    h.get_by_label("Select recipe").click();
    settle(&mut h);
    h.get_by_label("Use recipe Steel gear").hover();
    settle(&mut h);
    h.snapshot_options("building_picker_1920", &options());
}

#[test]
fn building_boiler() {
    let mut model = playing();
    model.building = Some(mock::boiler_view(&model.content));
    let mut h = harness(HD, model, |_| {});
    settle(&mut h);
    h.snapshot_options("building_boiler_1920", &options());
}

#[test]
fn building_electric_furnace() {
    let mut model = playing();
    model.building = Some(mock::electric_furnace_view(&model.content));
    let mut h = harness(HD, model, |_| {});
    settle(&mut h);
    h.get_by_label("Power network").hover();
    settle(&mut h);
    h.snapshot_options("building_furnace_1920", &options());
}

#[test]
fn power_network() {
    let mut model = playing();
    model.power = Some(mock::power_view(&model.content));
    let mut h = harness(HD, model, |_| {});
    settle(&mut h);
    h.snapshot_options("power_1920", &options());
}

#[test]
fn production_statistics() {
    let mut h = harness(HD, playing(), |ui| ui.open_window(WindowKind::Production));
    settle(&mut h);
    h.snapshot_options("production_1920", &options());
}

#[test]
fn research_window() {
    let mut h = harness(HD, playing(), |ui| ui.open_window(WindowKind::Research));
    settle(&mut h);
    h.get_all_by_label("Unlocks Small boiler").next().unwrap().hover();
    settle(&mut h);
    h.snapshot_options("research_1920", &options());
}

#[test]
fn research_window_empty() {
    let mut model = playing();
    model.techs.clear();
    let mut h = harness(HD, model, |ui| ui.open_window(WindowKind::Research));
    settle(&mut h);
    h.snapshot_options("research_empty_1920", &options());
}

#[test]
fn guide_window() {
    let mut h = harness(HD, playing(), |ui| ui.open_window(WindowKind::Guide));
    settle(&mut h);
    h.snapshot_options("guide_1920", &options());
}

#[test]
fn building_hub() {
    let mut model = playing();
    model.building = Some(mock::hub_view(&model.content));
    let mut h = harness(HD, model, |_| {});
    settle(&mut h);
    h.snapshot_options("building_hub_1920", &options());
}

/// With no research, the guide tracker is at the top left. The mouse is on it.
#[test]
fn hud_guide_tracker() {
    let mut model = playing();
    model.research = None;
    let mut h = harness(HD, model, |_| {});
    settle(&mut h);
    h.get_by_label("Open guide").hover();
    settle(&mut h);
    h.snapshot_options("hud_guide_1920", &options());
}

/// Every goal that the game can do is done: the tracker says what comes next.
#[test]
fn hud_guide_tracker_next() {
    let mut model = playing();
    model.research = None;
    for g in &mut model.guide {
        g.done |= g.waits_for.is_none();
    }
    let mut h = harness(HD, model, |_| {});
    settle(&mut h);
    h.snapshot_options("hud_guide_next_1920", &options());
}

/// The tank HUD at 2560 × 1440: the spray material has an orange frame, and the tanks are full.
#[test]
fn tank_hud_2560() {
    let mut model = playing();
    model.player.tanks_full = true;
    let mut h = harness(QHD, model, |_| {});
    settle(&mut h);
    h.snapshot_options("tank_hud_2560", &options());
}

/// A crate with bulk materials and parts, next to the inventory with its tanks.
#[test]
fn crate_window_2560() {
    let mut model = playing();
    model.building = Some(mock::crate_view(&model.content));
    let mut h = harness(QHD, model, |_| {});
    settle(&mut h);
    h.get_by_label("Sand").hover();
    settle(&mut h);
    h.snapshot_options("crate_2560", &options());
}

/// The Hub: the next repair stage, the later stages, and the items it holds.
#[test]
fn hub_window_2560() {
    let mut model = playing();
    model.building = Some(mock::hub_view(&model.content));
    let mut h = harness(QHD, model, |_| {});
    settle(&mut h);
    h.snapshot_options("hub_2560", &options());
}

/// Esc, then Save game in the pause menu.
#[test]
fn save_from_pause_menu_2560() {
    let mut h = harness(QHD, menu_model(GameState::Paused), |_| {});
    settle(&mut h);
    h.get_by_label("Save game").click();
    settle(&mut h);
    h.snapshot_options("save_from_pause_2560", &options());
}

/// The yes/no question before a full tank is emptied.
#[test]
fn empty_tank_question() {
    let mut h = harness(HD, playing(), |ui| {
        ui.open_window(WindowKind::Character);
        ui.confirm_empty_tank(0);
    });
    settle(&mut h);
    h.snapshot_options("empty_tank_1920", &options());
}

fn menu_model(state: GameState) -> UiModel {
    let mut m = playing();
    m.state = state;
    m
}

#[test]
fn main_menu() {
    let mut h = harness(HD, menu_model(GameState::MainMenu), |_| {});
    settle(&mut h);
    h.snapshot_options("main_menu_1920", &options());
}

#[test]
fn new_game_dialog() {
    let mut h = harness(HD, menu_model(GameState::MainMenu), |_| {});
    settle(&mut h);
    h.get_by_label("New game").click();
    settle(&mut h);
    h.snapshot_options("new_game_1920", &options());
}

#[test]
fn pause_menu() {
    let mut h = harness(HD, menu_model(GameState::Paused), |_| {});
    settle(&mut h);
    h.snapshot_options("pause_1920", &options());
}

#[test]
fn save_dialog() {
    let mut h = harness(HD, menu_model(GameState::Paused), |ui| ui.open_menu(MenuPage::Save));
    settle(&mut h);
    h.get_by_label("Copper valley").click();
    settle(&mut h);
    h.snapshot_options("save_1920", &options());
}

#[test]
fn save_overwrite_confirmation() {
    let mut h = harness(HD, menu_model(GameState::Paused), |ui| ui.confirm_overwrite("Copper valley"));
    settle(&mut h);
    h.snapshot_options("save_overwrite_1920", &options());
}

#[test]
fn load_dialog() {
    let mut h = harness(HD, menu_model(GameState::MainMenu), |ui| ui.open_menu(MenuPage::Load));
    settle(&mut h);
    h.get_by_label("First kiln").click();
    settle(&mut h);
    h.snapshot_options("load_1920", &options());
}

#[test]
fn load_delete_confirmation() {
    let mut h = harness(HD, menu_model(GameState::MainMenu), |ui| ui.confirm_delete("desert-test"));
    settle(&mut h);
    h.snapshot_options("load_delete_1920", &options());
}

#[test]
fn settings_dialog() {
    let mut h = harness(HD, menu_model(GameState::MainMenu), |ui| ui.open_menu(MenuPage::Settings));
    settle(&mut h);
    h.snapshot_options("settings_1920", &options());
}

#[test]
fn sandbox_hud() {
    let mut h = harness(HD, mock::sandbox_model(mock::content()), |_| {});
    settle(&mut h);
    h.snapshot_options("sandbox_hud_1920", &options());
}

#[test]
fn sandbox_materials_window() {
    let mut h = harness(HD, mock::sandbox_model(mock::content()), |ui| ui.open_window(WindowKind::Character));
    settle(&mut h);
    h.get_by_label("Water").hover();
    settle(&mut h);
    h.snapshot_options("sandbox_materials_1920", &options());
}

#[test]
fn sandbox_click_puts_material_in_hand_action() {
    let model = mock::sandbox_model(mock::content());
    let lava = mock::it(&model.content, "lava");
    let index = model.player.inventory.iter().position(|s| s.is_some_and(|s| s.item == lava)).unwrap();
    let mut h = harness(HD, model, |ui| ui.open_window(WindowKind::Character));
    settle(&mut h);
    h.get_by_label("Lava").click();
    settle(&mut h);
    let want = UiAction::ClickSlot { slot: foundry_ui::SlotRef::Inventory(index), click: foundry_ui::SlotClick::LEFT };
    assert!(h.state().actions.contains(&want), "{:?}", h.state().actions);
    // In the sandbox, a click on a full quickbar slot selects it (the hand always holds the brush).
    h.get_by_label("Quickbar Water").click();
    settle(&mut h);
    assert!(h.state().actions.contains(&UiAction::SelectHotbar(1)), "{:?}", h.state().actions);
}

#[test]
fn settings_with_simulation_sliders() {
    let mut model = menu_model(GameState::MainMenu);
    model.settings.simulation = mock::example_sim_settings();
    let mut h = harness(HD, model, |ui| ui.open_menu(MenuPage::Settings));
    settle(&mut h);
    h.snapshot_options("settings_simulation_1920", &options());
}

#[test]
fn large_screen_hud_and_character() {
    let mut h = harness(QHD, playing(), |ui| ui.open_window(WindowKind::Character));
    settle(&mut h);
    h.snapshot_options("character_2560", &options());
}

#[test]
fn large_screen_scaled_building() {
    let mut model = playing();
    model.settings.ui_scale = 1.25;
    model.building = Some(mock::electric_furnace_view(&model.content));
    let mut h = harness(QHD, model, |_| {});
    settle(&mut h);
    h.snapshot_options("building_2560_scale125", &options());
}

/// All icons at 32 and 64 pixels, to check the generated pixel art.
#[test]
fn icon_sheet() {
    let content = mock::content();
    let items: Vec<foundry_ui::ItemRef> = content
        .materials
        .all()
        .skip(1)
        .map(foundry_ui::ItemRef::Material)
        .chain((0..content.factory.parts.len()).map(|i| foundry_ui::ItemRef::Part(foundry_core::PartId(i as u16))))
        .collect();
    let mut atlas: Option<foundry_ui::icons::IconAtlas> = None;
    let cols = 24usize;
    let rows = items.len().div_ceil(cols);
    let size = vec2(cols as f32 * 72.0 + 16.0, rows as f32 * 72.0 + 16.0);
    let mut h = Harness::builder().with_size(size).with_pixels_per_point(1.0).wgpu().build_ui(move |ui| {
        let a = atlas.get_or_insert_with(|| foundry_ui::icons::IconAtlas::build(ui.ctx(), &content));
        let p = ui.painter();
        p.rect_filled(ui.ctx().content_rect(), 0.0, foundry_ui::theme::color::DEEP);
        for (i, it) in items.iter().enumerate() {
            let x = 8.0 + (i % cols) as f32 * 72.0;
            let y = 8.0 + (i / cols) as f32 * 72.0;
            let slot = egui::Rect::from_min_size(egui::pos2(x, y), egui::Vec2::splat(64.0));
            p.rect_filled(slot, 0.0, foundry_ui::theme::color::SLOT);
            a.paint(p, *it, slot, egui::Color32::WHITE);
        }
    });
    h.run_steps(2);
    h.snapshot_options("icons_64", &options());
}

// ---------------------------------------------------------------- interaction

#[test]
fn clicking_a_recipe_crafts_it() {
    let model = playing();
    let bronze_gear = mock::rid(&model.content, "bronze_gear");
    let mut h = harness(HD, model, |ui| ui.open_window(WindowKind::Character));
    settle(&mut h);
    h.get_by_label("Craft Bronze gear").click();
    settle(&mut h);
    h.get_by_label("Craft Bronze gear").click_secondary();
    settle(&mut h);
    let crafts: Vec<&UiAction> = h.state().actions.iter().filter(|a| matches!(a, UiAction::Craft { .. })).collect();
    assert_eq!(crafts, [&UiAction::Craft { recipe: bronze_gear, count: 1 }, &UiAction::Craft { recipe: bronze_gear, count: 5 }]);
}

#[test]
fn keys_open_and_close_windows() {
    let mut h = harness(HD, playing(), |_| {});
    settle(&mut h);
    h.key_press(egui::Key::E);
    settle(&mut h);
    assert!(h.state().ui.as_ref().unwrap().is_open(WindowKind::Character));
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    assert!(!h.state().ui.as_ref().unwrap().is_open(WindowKind::Character));
    // Esc with no window open asks for the pause menu.
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    assert!(h.state().actions.contains(&UiAction::Pause));
    h.key_press(egui::Key::Num3);
    settle(&mut h);
    assert!(h.state().actions.contains(&UiAction::SelectHotbar(2)));
}

#[test]
fn research_and_guide_keys_and_clicks() {
    let mut h = harness(HD, playing(), |_| {});
    settle(&mut h);
    let is_open = |h: &Harness<State>, kind| h.state().ui.as_ref().unwrap().is_open(kind);
    h.key_press(egui::Key::T);
    settle(&mut h);
    assert!(is_open(&h, WindowKind::Research));
    // The main windows replace each other.
    h.key_press(egui::Key::G);
    settle(&mut h);
    assert!(is_open(&h, WindowKind::Guide) && !is_open(&h, WindowKind::Research));
    assert!(h.state().actions.contains(&UiAction::CloseWindow(WindowKind::Research)));
    h.key_press(egui::Key::E);
    settle(&mut h);
    assert!(is_open(&h, WindowKind::Character) && !is_open(&h, WindowKind::Guide));
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    // A click on the HUD research box opens the research window.
    h.get_by_label("Open research").click();
    settle(&mut h);
    assert!(is_open(&h, WindowKind::Research));
    assert!(h.state().actions.contains(&UiAction::OpenWindow(WindowKind::Research)));
    // The first Research button is the first available technology.
    h.get_all_by_label("Research").next().unwrap().click();
    settle(&mut h);
    let drill = mock::tid(&h.state().model.content, "bronze_drill_head");
    assert!(h.state().actions.contains(&UiAction::StartResearch(drill)), "{:?}", h.state().actions);
    // A building window closes the research window.
    let hub = mock::hub_view(&h.state().model.content);
    h.state_mut().model.building = Some(hub);
    settle(&mut h);
    assert!(!is_open(&h, WindowKind::Research) && is_open(&h, WindowKind::Building));
}

#[test]
fn sandbox_has_no_research_or_guide() {
    let mut h = harness(HD, mock::sandbox_model(mock::content()), |_| {});
    settle(&mut h);
    for key in [egui::Key::T, egui::Key::G, egui::Key::P] {
        h.key_press(key);
        settle(&mut h);
    }
    let ui = h.state().ui.as_ref().unwrap();
    assert!(!ui.is_open(WindowKind::Research) && !ui.is_open(WindowKind::Guide) && !ui.is_open(WindowKind::Production));
}

#[test]
fn new_game_sends_the_mode() {
    let mut h = harness(HD, menu_model(GameState::MainMenu), |_| {});
    settle(&mut h);
    h.get_by_label("New game").click();
    settle(&mut h);
    h.get_by_label("Play").click();
    settle(&mut h);
    assert!(h.state().actions.iter().any(|a| matches!(a, UiAction::NewGame { mode: GameMode::Normal, .. })), "{:?}", h.state().actions);
    h.get_by_label("New game").click();
    settle(&mut h);
    h.get_by_label("Sandbox").click();
    settle(&mut h);
    h.get_by_label("Play").click();
    settle(&mut h);
    assert!(h.state().actions.iter().any(|a| matches!(a, UiAction::NewGame { mode: GameMode::Sandbox, .. })), "{:?}", h.state().actions);
}

#[test]
fn shift_click_on_inventory_slot_is_reported() {
    let mut model = playing();
    model.building = Some(mock::steam_assembler_view(&model.content));
    let wire = mock::it(&model.content, "copper_wire");
    let index = model.player.inventory.iter().position(|s| s.is_some_and(|s| s.item == wire)).unwrap();
    let mut h = harness(HD, model, |_| {});
    settle(&mut h);
    // The first match is the inventory slot (the building input slot has the same item).
    h.get_all_by_label("Copper wire").next().unwrap().click_modifiers(egui::Modifiers::SHIFT);
    settle(&mut h);
    let want = UiAction::ClickSlot { slot: foundry_ui::SlotRef::Inventory(index), click: foundry_ui::SlotClick::SHIFT_LEFT };
    assert!(h.state().actions.contains(&want), "{:?}", h.state().actions);
}

#[test]
fn closing_the_building_window_tells_the_game() {
    let mut model = playing();
    model.building = Some(mock::steam_assembler_view(&model.content));
    let mut h = harness(HD, model, |_| {});
    settle(&mut h);
    h.get_by_label("Close").click();
    settle(&mut h);
    assert!(h.state().actions.contains(&UiAction::CloseWindow(WindowKind::Building)));
}

#[test]
fn tank_trash_button_asks_before_a_large_amount() {
    let mut model = playing();
    // Tank 3 has 420 units (no question), tank 0 has 4,240 (a question first).
    assert!(model.player.tank[3].units < foundry_ui::EMPTY_CONFIRM_UNITS && model.player.tank[0].units >= foundry_ui::EMPTY_CONFIRM_UNITS);
    model.player.tanks_full = false;
    let mut h = harness(HD, model, |ui| ui.open_window(WindowKind::Character));
    settle(&mut h);
    h.get_by_label("Empty tank 4").click();
    settle(&mut h);
    assert!(h.state().actions.contains(&UiAction::EmptyTank(3)), "{:?}", h.state().actions);
    h.get_by_label("Empty tank 1").click();
    settle(&mut h);
    assert!(!h.state().actions.contains(&UiAction::EmptyTank(0)), "a large amount asks first");
    // Cancel keeps the material; Empty deletes it.
    h.get_by_label("Cancel").click();
    settle(&mut h);
    assert!(h.query_by_label("Cancel").is_none());
    h.get_by_label("Empty tank 1").click();
    settle(&mut h);
    h.get_by_label("Empty").click();
    settle(&mut h);
    assert!(h.state().actions.contains(&UiAction::EmptyTank(0)), "{:?}", h.state().actions);
}

#[test]
fn tank_and_storage_clicks_are_reported() {
    let mut model = playing();
    model.building = Some(mock::crate_view(&model.content));
    let crate_id = model.building.as_ref().unwrap().id;
    let mut h = harness(HD, model, |_| {});
    settle(&mut h);
    // A click on a tank (with a building open, the game moves it into the building).
    h.get_by_label("Tank Clay").click();
    settle(&mut h);
    let want = UiAction::ClickSlot { slot: foundry_ui::SlotRef::Tank(0), click: foundry_ui::SlotClick::LEFT };
    assert!(h.state().actions.contains(&want), "{:?}", h.state().actions);
    // Ctrl + click: every tank of that material.
    h.get_by_label("Tank Clay").click_modifiers(egui::Modifiers::CTRL);
    settle(&mut h);
    let want = UiAction::ClickSlot { slot: foundry_ui::SlotRef::Tank(0), click: foundry_ui::SlotClick::CTRL_LEFT };
    assert!(h.state().actions.contains(&want), "{:?}", h.state().actions);
    // Shift + click on a crate slot takes it back.
    h.get_by_label("Sand").click_modifiers(egui::Modifiers::SHIFT);
    settle(&mut h);
    let slot = foundry_ui::SlotRef::Building { building: crate_id, group: foundry_ui::BuildingSlots::Input, index: 2 };
    let want = UiAction::ClickSlot { slot, click: foundry_ui::SlotClick::SHIFT_LEFT };
    assert!(h.state().actions.contains(&want), "{:?}", h.state().actions);
    // The HUD tanks are buttons too (a click chooses the spray material).
    h.get_by_label("HUD tank Sand").click();
    settle(&mut h);
    let want = UiAction::ClickSlot { slot: foundry_ui::SlotRef::Tank(4), click: foundry_ui::SlotClick::LEFT };
    assert!(h.state().actions.contains(&want), "{:?}", h.state().actions);
}

#[test]
fn pause_menu_buttons() {
    let mut h = harness(HD, menu_model(GameState::Paused), |_| {});
    settle(&mut h);
    h.get_by_label("Resume").click();
    settle(&mut h);
    assert!(h.state().actions.contains(&UiAction::Resume));
}
