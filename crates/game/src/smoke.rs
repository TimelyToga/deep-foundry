//! `--smoke-test`: play a fixed list of UI actions in the real window and check the messages.
//! It checks the path a player takes: new game, paint, pause, save, resume, load, quit to the
//! main menu, continue, delete the save, quit.

use foundry_ui::{GameMode, UiAction, WorldSize};
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
    /// The test passed: close the window.
    Done,
}

/// Frames to wait for an expected message.
const EXPECT_FRAMES: u32 = 180;

pub struct Smoke {
    /// (frames to wait before the step, step)
    steps: VecDeque<(u32, Step)>,
    wait: u32,
    /// Frames spent on the current `Expect`.
    tries: u32,
    pub failure: Option<String>,
}

impl Smoke {
    pub fn new() -> Self {
        let save = "smoke test";
        let id = "smoke test.dfworld";
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
            (10, Step::Done),
        ];
        Self { steps: steps.into_iter().collect(), wait: 0, tries: 0, failure: None }
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
        if self.tries > EXPECT_FRAMES {
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
