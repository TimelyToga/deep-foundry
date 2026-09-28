//! `--smoke-test`: play a fixed list of UI actions in the real window and check the messages.
//! It checks the path a player takes:
//! - sandbox: new game, paint, pause, save, resume, load, quit to the main menu, continue, delete
//!   the save;
//! - normal mode: new game, dig clay, hand craft a clay brick and a workbench, place the
//!   workbench, open its window, dig until the tanks are full, place a crate and move a tank
//!   into it, save from the pause menu with mouse clicks, load, check the inventory, delete the
//!   save;
//! - quit.
//!
//! The steps `Key` and `Click` go through egui like the player's keys and clicks, so they find
//! menus that do not take clicks.

use foundry_ui::{GameMode, UiAction, WorldSize};
use egui::Key;
use std::collections::VecDeque;

/// One step of the test.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Do what a click in the UI does.
    Act(UiAction),
    /// Paint sand at the camera center.
    PaintSand,
    /// A message with this text must appear (within about 3 seconds).
    Expect(&'static str),
    /// Normal mode: aim the dig tool at clay near the robot and hold the dig button.
    DigClay,
    /// Normal mode: let go of the tool buttons and aim at the mouse again.
    StopTools,
    /// Normal mode: at least this many of an item (a material or part id) must be in the
    /// inventory (within about 15 seconds).
    Have(&'static str, u32),
    /// Normal mode: hand craft a recipe (by id).
    Craft(&'static str, u32),
    /// Normal mode: take a building (part id) into the hand and place it near the robot.
    PlaceNear(&'static str),
    /// Normal mode: aim at the building placed last and click it with the empty hand.
    OpenPlaced,
    /// Normal mode: the window of a building of this type must be open.
    WindowOf(&'static str),
    /// Normal mode: fill the tanks so that only the tank of this material has this many units
    /// of room (the test does not dig for minutes).
    FillTanks(&'static str, u32),
    /// Normal mode: click the tank that holds this material (in the open building window).
    ClickTankOf(&'static str),
    /// Normal mode: the open building window holds some of this item.
    InBuilding(&'static str),
    /// Press a key (through egui, as the player does).
    Key(Key),
    /// Click the button with this label (through egui). Waits until the button is shown.
    Click(&'static str),
    /// Click the button with this label if it is shown now, else go on.
    ClickIfShown(&'static str),
    /// Type this name into the save dialog.
    SaveName(&'static str),
    /// The test passed: close the window.
    Done,
}

/// Frames to wait for an expected message.
const EXPECT_FRAMES: u32 = 180;
/// Frames to wait for `Have` (hand crafting takes a few seconds).
const HAVE_FRAMES: u32 = 900;

pub struct Smoke {
    /// (frames to wait before the step, step)
    steps: VecDeque<(u32, Step)>,
    wait: u32,
    /// Frames spent on the current `Expect`.
    tries: u32,
    pub failure: Option<String>,
    /// egui input for the next frame (keys and clicks of the steps).
    pub events: Vec<egui::Event>,
}

impl Smoke {
    pub fn new() -> Self {
        let save = "smoke test";
        let id = "smoke test.dfworld";
        let normal_save = "smoke normal";
        let normal_id = "smoke normal.dfworld";
        let steps = [
            (10, Step::Act(UiAction::NewGame { seed: 3, size: WorldSize::Small, mode: GameMode::Sandbox })),
            (30, Step::PaintSand),
            (30, Step::Act(UiAction::Pause)),
            (5, Step::Act(UiAction::Save { name: save.into(), overwrite: false })),
            (1, Step::Expect("Game saved: smoke test")),
            (5, Step::Act(UiAction::Resume)),
            (30, Step::Act(UiAction::Load(id.into()))),
            (1, Step::Expect("Game loaded: smoke test")),
            (30, Step::Act(UiAction::QuitToMenu)),
            (10, Step::Act(UiAction::Continue)),
            (1, Step::Expect("Game loaded: smoke test")),
            (30, Step::Act(UiAction::QuitToMenu)),
            (10, Step::Act(UiAction::DeleteSave(id.into()))),
            (1, Step::Expect("Deleted: smoke test")),
            // The normal mode.
            (10, Step::Act(UiAction::NewGame { seed: 3, size: WorldSize::Small, mode: GameMode::Normal })),
            (1, Step::Expect("New game")),
            (60, Step::DigClay),
            (1, Step::Have("clay", 32)),
            (1, Step::StopTools),
            (5, Step::Craft("raw_clay_brick", 1)),
            (1, Step::Have("raw_clay_brick", 1)),
            (5, Step::Craft("workbench", 1)),
            (1, Step::Have("workbench", 1)),
            (5, Step::PlaceNear("workbench")),
            (10, Step::OpenPlaced),
            (1, Step::WindowOf("workbench")),
            // Tanks: dig until they are full, then put the clay into a crate.
            (5, Step::Key(Key::Escape)),
            (5, Step::FillTanks("clay", 30)),
            (5, Step::DigClay),
            (1, Step::Expect(foundry_ui::TANKS_FULL)),
            (1, Step::StopTools),
            (5, Step::PlaceNear("crate")),
            (10, Step::OpenPlaced),
            (1, Step::WindowOf("crate")),
            (5, Step::ClickTankOf("clay")),
            (1, Step::InBuilding("clay")),
            // Save from the pause menu with mouse clicks.
            (5, Step::Key(Key::Escape)),
            (5, Step::Key(Key::Escape)),
            (5, Step::SaveName(normal_save)),
            (5, Step::Click("Save game")),
            (5, Step::Click("Save")),
            (10, Step::ClickIfShown("Overwrite")),
            (1, Step::Expect("Game saved: smoke normal")),
            (5, Step::Click("Resume")),
            (30, Step::Act(UiAction::Load(normal_id.into()))),
            (1, Step::Expect("Game loaded: smoke normal")),
            (30, Step::Have("raw_clay_brick", 1)),
            (10, Step::Act(UiAction::QuitToMenu)),
            (10, Step::Act(UiAction::DeleteSave(normal_id.into()))),
            (1, Step::Expect("Deleted: smoke normal")),
            (10, Step::Done),
        ];
        Self { steps: steps.into_iter().collect(), wait: 0, tries: 0, failure: None, events: vec![] }
    }

    /// The step to run in this frame, if its wait is over.
    pub fn next(&mut self) -> Option<Step> {
        let (wait, _) = self.steps.front()?;
        if self.wait < *wait {
            self.wait += 1;
            return None;
        }
        self.wait = 0;
        self.steps.pop_front().map(|(_, s)| s)
    }

    /// An `Expect` step did not see its message yet: try again in the next frame.
    /// Returns false when the time is over (then `failure` is set).
    pub fn retry(&mut self, step: Step) -> bool {
        self.tries += 1;
        let limit = if matches!(step, Step::Have(..)) { HAVE_FRAMES } else { EXPECT_FRAMES };
        if self.tries > limit {
            self.failure = Some(format!("{step:?} did not happen"));
            return false;
        }
        self.steps.push_front((0, step));
        true
    }

    /// An `Expect` step saw its message.
    pub fn passed_expect(&mut self) {
        self.tries = 0;
    }
}

/// The egui events of a key press.
pub fn key_events(key: Key) -> Vec<egui::Event> {
    [true, false]
        .map(|pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE })
        .into()
}

/// The egui events of a left click at `pos` (in points).
pub fn click_events(pos: egui::Pos2) -> Vec<egui::Event> {
    let button = |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    vec![egui::Event::PointerMoved(pos), button(true), button(false)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_wait_and_retry() {
        let mut s = Smoke::new();
        for _ in 0..10 {
            assert_eq!(s.next(), None);
        }
        assert!(matches!(s.next(), Some(Step::Act(UiAction::NewGame { .. }))));
        // Run to the first Expect and make it time out.
        let expect = loop {
            if let Some(step @ Step::Expect(_)) = s.next() {
                break step;
            }
        };
        let mut n = 0;
        while s.retry(expect.clone()) {
            n += 1;
            assert_eq!(s.next(), Some(expect.clone()));
        }
        assert_eq!(n, EXPECT_FRAMES);
        assert!(s.failure.is_some());
    }
}
