//! Moving items with the HUD bar (the quickbar and the tank panel) while a building window is
//! open, drag and drop, and the keep or drop buttons. The UI only reports actions; the game
//! applies them with the rules of `foundry_factory::transfer`.

use egui::{Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use foundry_content::ItemRef;
use foundry_ui::mock;
use foundry_ui::{BuildingSlots, FoundryUi, SlotClick, SlotRef, UiAction, UiModel, WindowKind};

struct State {
    ui: Option<FoundryUi>,
    model: UiModel,
    actions: Vec<UiAction>,
    open: Option<WindowKind>,
}

fn harness(model: UiModel, open: Option<WindowKind>) -> Harness<'static, State> {
    let state = State { ui: None, model, actions: vec![], open };
    Harness::builder().with_size(vec2(1920.0, 1080.0)).with_pixels_per_point(1.0).with_max_steps(12).build_ui_state(
        |ui, st: &mut State| {
            let ctx = ui.ctx().clone();
            let fui = st.ui.get_or_insert_with(|| FoundryUi::new(&ctx));
            if let Some(w) = st.open.take() {
                fui.open_window(w);
            }
            let actions = fui.show(&ctx, &st.model);
            st.actions.extend(actions);
        },
        state,
    )
}

fn settle(h: &mut Harness<State>) {
    h.run_steps(6);
}

fn take(h: &mut Harness<State>) -> Vec<UiAction> {
    std::mem::take(&mut h.state_mut().actions)
}

fn center(h: &Harness<State>, label: &str) -> Pos2 {
    h.get_by_label(label).rect().center()
}

/// Press at `from`, move to `to` in steps, and release there.
fn drag(h: &mut Harness<State>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.run_steps(2);
    h.drag_at(from);
    h.run_steps(2);
    for k in 1..=10 {
        h.hover_at(from.lerp(to, k as f32 / 10.0));
        h.run_steps(1);
    }
    h.event(egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
    h.run_steps(3);
}

fn with_crate() -> UiModel {
    let mut m = mock::model(mock::content());
    m.building = Some(mock::crate_view(&m.content));
    m
}

fn material(m: &UiModel, id: &str) -> ItemRef {
    ItemRef::Material(m.content.expect_material(id))
}

/// The index of the first tank with this material in the mock model.
fn tank_of(m: &UiModel, id: &str) -> usize {
    let mat = m.content.expect_material(id);
    m.player.tank.iter().position(|t| t.material == Some(mat)).unwrap()
}

#[test]
fn a_hud_tank_click_moves_it_into_the_open_building() {
    let model = with_crate();
    let clay = tank_of(&model, "clay");
    let mut h = harness(model, None);
    settle(&mut h);
    h.get_by_label("HUD tank Clay").click();
    settle(&mut h);
    let want = UiAction::ClickSlot { slot: SlotRef::Tank(clay), click: SlotClick::LEFT };
    assert!(take(&mut h).contains(&want));
}

#[test]
fn a_hud_tank_dragged_onto_the_building_window_moves_into_it() {
    let model = with_crate();
    let sand = tank_of(&model, "sand");
    let mut h = harness(model, None);
    settle(&mut h);
    let (from, to) = (center(&h, "HUD tank Sand"), center(&h, "Raw malachite"));
    drag(&mut h, from, to);
    let actions = take(&mut h);
    let want = UiAction::ClickSlot { slot: SlotRef::Tank(sand), click: SlotClick::LEFT };
    assert!(actions.contains(&want), "{actions:?}");
    // A drag is not a click on the tank as well.
    assert_eq!(actions.iter().filter(|a| matches!(a, UiAction::ClickSlot { .. })).count(), 1, "{actions:?}");
}

#[test]
fn a_building_slot_dragged_onto_the_hud_goes_back_to_the_robot() {
    let model = with_crate();
    let id = model.building.as_ref().unwrap().id;
    let mut h = harness(model, None);
    settle(&mut h);
    // Crate slot 3 holds raw malachite.
    let (from, to) = (center(&h, "Raw malachite"), center(&h, "HUD tank Clay"));
    drag(&mut h, from, to);
    let want = UiAction::ClickSlot { slot: SlotRef::Building { building: id, group: BuildingSlots::Input, index: 3 }, click: SlotClick::SHIFT_LEFT };
    let actions = take(&mut h);
    assert!(actions.contains(&want), "{actions:?}");
    // Onto the quickbar works too.
    let (from, to) = (center(&h, "Raw malachite"), center(&h, "Quickbar Hopper"));
    drag(&mut h, from, to);
    assert!(take(&mut h).contains(&want));
}

#[test]
fn quickbar_clicks_move_items_into_the_open_building() {
    let mut model = with_crate();
    model.player.hotbar[9] = Some(material(&model, "clay"));
    let clay = tank_of(&model, "clay");
    let belts = model.player.inventory.iter().position(|s| s.is_some_and(|s| s.item == model.player.hotbar[0].unwrap())).unwrap();
    let mut h = harness(model, None);
    settle(&mut h);
    // A material: a click moves it from every tank.
    h.get_by_label("Quickbar Clay").click();
    settle(&mut h);
    assert!(take(&mut h).contains(&UiAction::ClickSlot { slot: SlotRef::Tank(clay), click: SlotClick::CTRL_LEFT }));
    // A part: Shift + click moves all of it; a plain click takes it in the hand.
    h.get_by_label("Quickbar Wood belt").click_modifiers(egui::Modifiers::SHIFT);
    settle(&mut h);
    assert!(take(&mut h).contains(&UiAction::ClickSlot { slot: SlotRef::Inventory(belts), click: SlotClick::CTRL_LEFT }));
    h.get_by_label("Quickbar Wood belt").click();
    settle(&mut h);
    assert!(take(&mut h).contains(&UiAction::SelectHotbar(0)));
}

#[test]
fn drag_a_tank_or_a_quickbar_slot_onto_the_quickbar() {
    let model = mock::model(mock::content());
    let sand = material(&model, "sand");
    let (belt, hopper) = (model.player.hotbar[0], model.player.hotbar[1]);
    let mut h = harness(model, None);
    settle(&mut h);
    // Slot 10 (index 9) is empty: it is right of slot 9 (the ladder).
    let empty = center(&h, "Quickbar Ladder") + vec2(40.0, 0.0);
    let from = center(&h, "HUD tank Sand");
    drag(&mut h, from, empty);
    assert!(take(&mut h).contains(&UiAction::SetHotbar { index: 9, item: Some(sand) }));
    // Two quickbar slots change places.
    let (from, to) = (center(&h, "Quickbar Wood belt"), center(&h, "Quickbar Hopper"));
    drag(&mut h, from, to);
    let actions = take(&mut h);
    assert!(actions.contains(&UiAction::SetHotbar { index: 1, item: belt }), "{actions:?}");
    assert!(actions.contains(&UiAction::SetHotbar { index: 0, item: hopper }), "{actions:?}");
}

#[test]
fn keep_or_drop_is_one_click_on_a_tank_or_in_the_list() {
    let model = mock::model(mock::content());
    let (clay, dirt) = (model.content.expect_material("clay"), model.content.expect_material("dirt"));
    let mut h = harness(model, None);
    settle(&mut h);
    // The mark on a HUD tank.
    h.get_by_label("Keep Clay").click();
    settle(&mut h);
    assert!(take(&mut h).contains(&UiAction::SetKeep { material: clay, keep: false }));
    // The list in the character screen.
    h.state_mut().open = Some(WindowKind::Character);
    settle(&mut h);
    h.get_by_label("Dig list Drop Dirt").click();
    settle(&mut h);
    assert!(take(&mut h).contains(&UiAction::SetKeep { material: dirt, keep: true }));
}

#[test]
fn the_first_building_window_shows_the_transfer_hint_once() {
    let mut h = harness(mock::model(mock::content()), None);
    settle(&mut h);
    let hint = "Move items with the bar below";
    assert!(h.query_by_label(hint).is_none());
    let crate_view = mock::crate_view(&h.state().model.content);
    h.state_mut().model.building = Some(crate_view.clone());
    settle(&mut h);
    assert!(h.query_by_label(hint).is_some(), "the hint shows with the first building window");
    h.state_mut().model.building = None;
    settle(&mut h);
    h.state_mut().model.building = Some(crate_view);
    settle(&mut h);
    assert!(h.query_by_label(hint).is_none(), "only the first time");
}
