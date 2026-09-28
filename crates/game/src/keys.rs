//! Key bindings: one table from game actions to keys.
//!
//! - By default a key matches by its position on the keyboard (the winit `KeyCode`). On a Dvorak
//!   keyboard the movement keys are then where W A S D are on a QWERTY keyboard.
//! - With "match keys by letter" a key matches by the character that it types (without Shift,
//!   Ctrl or Alt). Keys that type no character (F3, Space, the arrows, Alt) match by position.
//! - The game shows each key with the name on the player's keyboard. The name of a physical key
//!   is known only after the player pressed it once (`KeyNames::learn`); before that the game
//!   uses the QWERTY name.
//! - Esc and the mouse buttons are fixed.
//!
//! Shift is not part of a binding. It changes some actions: Shift + R turns the other way, and
//! Shift + a quickbar key takes the second quickbar row.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use winit::keyboard::KeyCode;

/// Where an action works.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The normal game (the robot and the factory).
    Normal,
    /// The sandbox (the paint brush and the free camera).
    Sandbox,
    Both,
}

/// A game action that a key starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Action {
    MoveLeft,
    MoveRight,
    Jump,
    Scan,
    Rotate,
    Flip,
    Pipette,
    Undo,
    Redo,
    AltMode,
    Character,
    Research,
    Guide,
    Production,
    /// Quickbar slot 1 to 10 (0 to 9). With Shift: slot 11 to 20.
    Quickbar(u8),
    CameraLeft,
    CameraRight,
    CameraUp,
    CameraDown,
    PauseSim,
    StepSim,
    BrushSmaller,
    BrushLarger,
    DebugPanel,
    /// Debug view: awake chunks and their update rectangles.
    DebugChunks,
    /// Debug view: cells colored by temperature.
    DebugHeat,
    /// Debug view: the chunk grid.
    DebugGrid,
}

impl Action {
    /// True for actions that last while the key is down (movement, scan).
    pub fn is_held(self) -> bool {
        matches!(
            self,
            Action::MoveLeft
                | Action::MoveRight
                | Action::Jump
                | Action::Scan
                | Action::CameraLeft
                | Action::CameraRight
                | Action::CameraUp
                | Action::CameraDown
        )
    }

    /// A short stable name, for the settings screen and for `{key:NAME}` in texts.
    pub fn id(self) -> String {
        match self {
            Action::Quickbar(i) => format!("quickbar{}", i + 1),
            a => ACTIONS.iter().find(|x| x.0 == a).map_or("?", |x| x.1).to_string(),
        }
    }

    pub fn from_id(id: &str) -> Option<Action> {
        if let Some(n) = id.strip_prefix("quickbar") {
            return n.parse::<u8>().ok().filter(|n| (1..=10).contains(n)).map(|n| Action::Quickbar(n - 1));
        }
        ACTIONS.iter().find(|x| x.1 == id).map(|x| x.0)
    }

    /// What the action does, for the settings screen.
    pub fn label(self) -> String {
        match self {
            Action::Quickbar(i) => format!("Quickbar slot {} (Shift: slot {})", i + 1, i + 11),
            a => ACTIONS.iter().find(|x| x.0 == a).map_or("?", |x| x.2).to_string(),
        }
    }

    pub fn scope(self) -> Scope {
        match self {
            Action::Quickbar(_) => Scope::Both,
            a => ACTIONS.iter().find(|x| x.0 == a).map_or(Scope::Both, |x| x.3),
        }
    }

    /// Every action, in the order of the settings screen: the quickbar slots come before the
    /// debug keys.
    pub fn all() -> impl Iterator<Item = Action> {
        let split = ACTIONS.iter().position(|x| x.0 == Action::DebugPanel).unwrap_or(ACTIONS.len());
        let before = ACTIONS[..split].iter().map(|x| x.0);
        let debug = ACTIONS[split..].iter().map(|x| x.0);
        before.chain((0..10).map(Action::Quickbar)).chain(debug)
    }
}

/// (action, id, label, scope) for every action except the quickbar slots.
const ACTIONS: &[(Action, &str, &str, Scope)] = &[
    (Action::MoveLeft, "move_left", "Walk left", Scope::Normal),
    (Action::MoveRight, "move_right", "Walk right", Scope::Normal),
    (Action::Jump, "jump", "Jump, jetpack, swim up", Scope::Normal),
    (Action::Scan, "scan", "Scan the material under the mouse (hold)", Scope::Normal),
    (Action::Rotate, "rotate", "Rotate (Shift: the other way)", Scope::Normal),
    (Action::Flip, "flip", "Flip the building in the hand", Scope::Normal),
    (Action::Pipette, "pipette", "Pick the building under the mouse / empty the hand", Scope::Both),
    (Action::Undo, "undo", "Undo", Scope::Normal),
    (Action::Redo, "redo", "Redo", Scope::Normal),
    (Action::AltMode, "alt_mode", "Alt mode: recipes and belt directions", Scope::Normal),
    (Action::Character, "character", "Character screen (sandbox: materials)", Scope::Both),
    (Action::Research, "research", "Research", Scope::Normal),
    (Action::Guide, "guide", "Guide", Scope::Normal),
    (Action::Production, "production", "Production statistics", Scope::Normal),
    (Action::CameraLeft, "camera_left", "Move the view left", Scope::Sandbox),
    (Action::CameraRight, "camera_right", "Move the view right", Scope::Sandbox),
    (Action::CameraUp, "camera_up", "Move the view up", Scope::Sandbox),
    (Action::CameraDown, "camera_down", "Move the view down", Scope::Sandbox),
    (Action::PauseSim, "pause", "Pause the simulation", Scope::Sandbox),
    (Action::StepSim, "step", "One tick (while paused)", Scope::Sandbox),
    (Action::BrushSmaller, "brush_smaller", "Smaller brush", Scope::Sandbox),
    (Action::BrushLarger, "brush_larger", "Larger brush", Scope::Sandbox),
    (Action::DebugPanel, "debug", "Debug panel", Scope::Both),
    (Action::DebugChunks, "debug_chunks", "Debug view: awake chunks", Scope::Both),
    (Action::DebugHeat, "debug_heat", "Debug view: heat map", Scope::Both),
    (Action::DebugGrid, "debug_grid", "Debug view: chunk grid", Scope::Both),
];

/// One key of a binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyBind {
    /// The key by its position.
    pub code: KeyCode,
    /// The character this key typed when it was bound, in lower case (for matching by letter).
    /// `None` for keys that type no character.
    #[serde(default)]
    pub text: Option<String>,
    /// Ctrl (or Cmd on macOS) must be down.
    #[serde(default)]
    pub ctrl: bool,
}

impl KeyBind {
    pub fn new(code: KeyCode) -> Self {
        Self { code, text: qwerty_text(code).map(str::to_string), ctrl: false }
    }

    pub fn ctrl(code: KeyCode) -> Self {
        Self { ctrl: true, ..Self::new(code) }
    }
}

/// A key press or release, as the game sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Press {
    pub code: KeyCode,
    /// The character of the key without modifiers, in lower case.
    pub text: Option<String>,
    pub ctrl: bool,
    pub shift: bool,
}

impl Press {
    #[cfg(test)]
    pub fn new(code: KeyCode) -> Self {
        Self { code, text: qwerty_text(code).map(str::to_string), ctrl: false, shift: false }
    }
}

/// The default keys of an action.
pub fn default_keys(a: Action) -> Vec<KeyBind> {
    use KeyCode as K;
    let k = KeyBind::new;
    match a {
        Action::MoveLeft | Action::CameraLeft => vec![k(K::KeyA), k(K::ArrowLeft)],
        Action::MoveRight | Action::CameraRight => vec![k(K::KeyD), k(K::ArrowRight)],
        Action::Jump => vec![k(K::KeyW), k(K::ArrowUp), k(K::Space)],
        Action::CameraUp => vec![k(K::KeyW), k(K::ArrowUp)],
        Action::CameraDown => vec![k(K::KeyS), k(K::ArrowDown)],
        Action::Scan | Action::Flip => vec![k(K::KeyF)],
        Action::Rotate => vec![k(K::KeyR)],
        Action::Pipette => vec![k(K::KeyQ)],
        Action::Undo => vec![KeyBind::ctrl(K::KeyZ)],
        Action::Redo => vec![KeyBind::ctrl(K::KeyY)],
        Action::AltMode => vec![k(K::AltLeft), k(K::AltRight)],
        Action::Character => vec![k(K::KeyE)],
        Action::Research => vec![k(K::KeyT)],
        Action::Guide => vec![k(K::KeyG)],
        Action::Production => vec![k(K::KeyP)],
        Action::Quickbar(i) => {
            const DIGITS: [KeyCode; 10] =
                [K::Digit1, K::Digit2, K::Digit3, K::Digit4, K::Digit5, K::Digit6, K::Digit7, K::Digit8, K::Digit9, K::Digit0];
            vec![k(DIGITS[(i as usize).min(9)])]
        }
        Action::PauseSim => vec![k(K::Space)],
        Action::StepSim => vec![k(K::Period)],
        Action::BrushSmaller => vec![k(K::BracketLeft)],
        Action::BrushLarger => vec![k(K::BracketRight)],
        Action::DebugPanel => vec![k(K::F3)],
        Action::DebugChunks => vec![k(K::F4)],
        Action::DebugHeat => vec![k(K::F5)],
        Action::DebugGrid => vec![k(K::F6)],
    }
}

/// The key table of the player: the changed actions, and how keys match.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Bindings {
    /// Match keys by the character they type, not by their position.
    #[serde(default)]
    pub by_letter: bool,
    /// Actions with other keys than the default.
    #[serde(default)]
    pub changed: BTreeMap<Action, Vec<KeyBind>>,
}

impl Bindings {
    pub fn keys(&self, a: Action) -> Vec<KeyBind> {
        self.changed.get(&a).cloned().unwrap_or_else(|| default_keys(a))
    }

    /// Give an action one new key.
    pub fn set(&mut self, a: Action, key: KeyBind) {
        let keys = vec![key];
        if keys == default_keys(a) {
            self.changed.remove(&a);
        } else {
            self.changed.insert(a, keys);
        }
    }

    pub fn reset(&mut self) {
        self.changed.clear();
    }

    /// True if the key matches one of the action's keys. For a press, Ctrl must match the
    /// binding. A held action ends on the release of its key with any modifiers.
    pub fn matches(&self, a: Action, p: &Press, down: bool) -> bool {
        self.keys(a).iter().any(|k| {
            let same_key = match (&k.text, &p.text, self.by_letter) {
                (Some(bound), Some(typed), true) => bound == typed,
                _ => k.code == p.code,
            };
            same_key && (a.is_held() || !down || k.ctrl == p.ctrl)
        })
    }

    /// The actions of a key in a scope.
    pub fn actions(&self, p: &Press, down: bool, normal: bool) -> Vec<Action> {
        Action::all()
            .filter(|a| match a.scope() {
                Scope::Both => true,
                Scope::Normal => normal,
                Scope::Sandbox => !normal,
            })
            .filter(|a| self.matches(*a, p, down))
            .collect()
    }
}

/// The QWERTY character of a key (lower case), for keys that type one.
pub fn qwerty_text(code: KeyCode) -> Option<&'static str> {
    use KeyCode as K;
    const LETTERS: [(KeyCode, &str); 26] = [
        (K::KeyA, "a"),
        (K::KeyB, "b"),
        (K::KeyC, "c"),
        (K::KeyD, "d"),
        (K::KeyE, "e"),
        (K::KeyF, "f"),
        (K::KeyG, "g"),
        (K::KeyH, "h"),
        (K::KeyI, "i"),
        (K::KeyJ, "j"),
        (K::KeyK, "k"),
        (K::KeyL, "l"),
        (K::KeyM, "m"),
        (K::KeyN, "n"),
        (K::KeyO, "o"),
        (K::KeyP, "p"),
        (K::KeyQ, "q"),
        (K::KeyR, "r"),
        (K::KeyS, "s"),
        (K::KeyT, "t"),
        (K::KeyU, "u"),
        (K::KeyV, "v"),
        (K::KeyW, "w"),
        (K::KeyX, "x"),
        (K::KeyY, "y"),
        (K::KeyZ, "z"),
    ];
    const OTHERS: [(KeyCode, &str); 21] = [
        (K::Digit0, "0"),
        (K::Digit1, "1"),
        (K::Digit2, "2"),
        (K::Digit3, "3"),
        (K::Digit4, "4"),
        (K::Digit5, "5"),
        (K::Digit6, "6"),
        (K::Digit7, "7"),
        (K::Digit8, "8"),
        (K::Digit9, "9"),
        (K::Minus, "-"),
        (K::Equal, "="),
        (K::BracketLeft, "["),
        (K::BracketRight, "]"),
        (K::Backslash, "\\"),
        (K::Semicolon, ";"),
        (K::Quote, "'"),
        (K::Backquote, "`"),
        (K::Comma, ","),
        (K::Period, "."),
        (K::Slash, "/"),
    ];
    LETTERS.iter().chain(OTHERS.iter()).find(|(c, _)| *c == code).map(|(_, t)| *t)
}

/// The QWERTY name of a key (for keys that type no character: the key name).
pub fn qwerty_name(code: KeyCode) -> String {
    use KeyCode as K;
    if let Some(t) = qwerty_text(code) {
        return t.to_uppercase();
    }
    match code {
        K::Space => "Space".into(),
        K::AltLeft | K::AltRight => "Alt".into(),
        K::ControlLeft | K::ControlRight => "Ctrl".into(),
        K::ShiftLeft | K::ShiftRight => "Shift".into(),
        K::ArrowLeft => "Left".into(),
        K::ArrowRight => "Right".into(),
        K::ArrowUp => "Up".into(),
        K::ArrowDown => "Down".into(),
        K::Tab => "Tab".into(),
        K::Enter => "Enter".into(),
        K::Backspace => "Backspace".into(),
        K::Escape => "Esc".into(),
        other => {
            // F1, Home, Numpad1 and so on: the winit name.
            let s = format!("{other:?}");
            s.strip_prefix("Key").map(str::to_string).unwrap_or(s)
        }
    }
}

/// The names of keys on the player's keyboard, learned from key presses.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KeyNames {
    learned: HashMap<KeyCode, String>,
}

impl KeyNames {
    /// Remember what a key typed. Returns true if the name is new or changed.
    pub fn learn(&mut self, code: KeyCode, text: Option<&str>) -> bool {
        let Some(t) = text.filter(|t| !t.trim().is_empty() && t.chars().count() == 1) else { return false };
        let name = t.to_uppercase();
        if self.learned.get(&code) == Some(&name) {
            return false;
        }
        self.learned.insert(code, name);
        true
    }

    pub fn all(&self) -> impl Iterator<Item = (&KeyCode, &String)> {
        self.learned.iter()
    }

    /// The name of a key on the player's keyboard.
    pub fn name(&self, code: KeyCode) -> String {
        self.learned.get(&code).cloned().unwrap_or_else(|| qwerty_name(code))
    }

    /// The name of one key of a binding.
    pub fn bind_name(&self, k: &KeyBind, by_letter: bool) -> String {
        let key = match (&k.text, by_letter) {
            (Some(t), true) => t.to_uppercase(),
            _ => self.name(k.code),
        };
        if k.ctrl { format!("Ctrl + {key}") } else { key }
    }

    /// The names of all keys of an action, for example "A / Left".
    pub fn action_name(&self, b: &Bindings, a: Action) -> String {
        let mut names: Vec<String> = vec![];
        for k in b.keys(a) {
            let n = self.bind_name(&k, b.by_letter);
            if !names.contains(&n) {
                names.push(n);
            }
        }
        names.join(" / ")
    }
}

/// The rows of the Controls list in the settings: the mouse and Esc (fixed), then the keys of the
/// mode. `normal`: `Some(true)` in the normal game, `Some(false)` in the sandbox, `None` in the
/// main menu (all keys).
pub fn rows(b: &Bindings, names: &KeyNames, normal: Option<bool>) -> Vec<foundry_ui::KeyRow> {
    use foundry_ui::KeyRow;
    let fixed = |action: &str, key: &str| KeyRow::new("", action, key, true);
    let mut out = match normal {
        Some(false) => vec![
            fixed("Paint with the material in the hand", "Left mouse"),
            fixed("Erase (paint air)", "Right mouse"),
            fixed("Move the view with the mouse (Shift: faster keys)", "Middle mouse drag"),
        ],
        _ => vec![
            fixed("Dig; place the building in the hand (drag: a line)", "Left mouse"),
            fixed("Spray; remove buildings (hold and drag)", "Right mouse"),
            fixed("Open a building", "Left mouse on it"),
            fixed("Copy / paste the recipe of a machine", "Shift + right / left click"),
        ],
    };
    out.push(fixed("Zoom", "Mouse wheel"));
    out.push(fixed("Close the window / pause menu", "Esc"));
    for a in Action::all() {
        let show = match (a.scope(), normal) {
            (Scope::Both, _) | (_, None) => true,
            (Scope::Normal, Some(n)) => n,
            (Scope::Sandbox, Some(n)) => !n,
        };
        if show {
            out.push(KeyRow::new(&a.id(), &a.label(), &names.action_name(b, a), false));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode, text: &str) -> Press {
        Press { code, text: Some(text.into()), ctrl: false, shift: false }
    }

    #[test]
    fn by_position_the_dvorak_key_at_the_w_place_moves() {
        let b = Bindings::default();
        // On Dvorak the key at the QWERTY W place types ",".
        assert_eq!(b.actions(&press(KeyCode::KeyW, ","), true, true), vec![Action::Jump]);
        // The Dvorak "w" key is at the QWERTY comma place: not the jump key.
        assert!(b.actions(&press(KeyCode::Comma, "w"), true, true).is_empty());
    }

    #[test]
    fn by_letter_the_key_that_types_w_moves() {
        let b = Bindings { by_letter: true, ..Default::default() };
        assert_eq!(b.actions(&press(KeyCode::Comma, "w"), true, true), vec![Action::Jump]);
        assert!(b.actions(&press(KeyCode::KeyW, ","), true, true).is_empty());
        // Keys that type nothing still match by position.
        let space = Press { code: KeyCode::Space, text: None, ctrl: false, shift: false };
        assert_eq!(b.actions(&space, true, true), vec![Action::Jump]);
        assert_eq!(b.actions(&space, true, false), vec![Action::PauseSim], "the sandbox has other actions");
    }

    #[test]
    fn ctrl_must_match_for_a_press_but_not_for_a_held_key() {
        let b = Bindings::default();
        let z = Press { ctrl: true, ..Press::new(KeyCode::KeyZ) };
        assert_eq!(b.actions(&z, true, true), vec![Action::Undo]);
        assert!(b.actions(&Press::new(KeyCode::KeyZ), true, true).is_empty());
        // Ctrl + R does not rotate; Ctrl + A still walks (a held key).
        assert!(b.actions(&Press { ctrl: true, ..Press::new(KeyCode::KeyR) }, true, true).is_empty());
        assert_eq!(b.actions(&Press { ctrl: true, ..Press::new(KeyCode::KeyA) }, true, true), vec![Action::MoveLeft]);
    }

    #[test]
    fn rebind_and_reset() {
        let mut b = Bindings::default();
        b.set(Action::Rotate, KeyBind { code: KeyCode::KeyO, text: Some("r".into()), ctrl: false });
        assert!(b.matches(Action::Rotate, &Press::new(KeyCode::KeyO), true));
        assert!(!b.matches(Action::Rotate, &Press::new(KeyCode::KeyR), true));
        // Setting the default again removes the change.
        b.set(Action::Rotate, KeyBind::new(KeyCode::KeyR));
        assert!(b.changed.is_empty());
        b.set(Action::Jump, KeyBind::new(KeyCode::KeyK));
        b.reset();
        assert_eq!(b, Bindings::default());
        // The file form reads back.
        b.set(Action::Undo, KeyBind::ctrl(KeyCode::KeyU));
        let text = ron::to_string(&b).unwrap();
        assert_eq!(ron::from_str::<Bindings>(&text).unwrap(), b);
    }

    #[test]
    fn names_use_the_learned_layout() {
        let mut n = KeyNames::default();
        let b = Bindings::default();
        assert_eq!(n.action_name(&b, Action::MoveLeft), "A / Left");
        assert_eq!(n.action_name(&b, Action::Undo), "Ctrl + Z");
        // Dvorak: the key at the QWERTY F place types "u".
        assert!(n.learn(KeyCode::KeyF, Some("u")));
        assert!(!n.learn(KeyCode::KeyF, Some("u")), "nothing new");
        assert_eq!(n.action_name(&b, Action::Scan), "U");
        // By letter the name is the letter of the binding.
        let by_letter = Bindings { by_letter: true, ..Default::default() };
        assert_eq!(n.action_name(&by_letter, Action::Scan), "F");
        assert_eq!(n.action_name(&b, Action::DebugPanel), "F3");
    }

    #[test]
    fn ids_and_labels() {
        for a in Action::all() {
            assert_eq!(Action::from_id(&a.id()), Some(a), "{a:?}");
            assert!(!a.label().is_empty());
        }
        assert_eq!(Action::all().count(), ACTIONS.len() + 10);
    }

    #[test]
    fn no_two_actions_of_a_mode_share_a_key() {
        // Actions that may share a key on purpose: they work in different situations.
        let shared = [
            (Action::Scan, Action::Flip),
            (Action::Jump, Action::CameraUp),
            (Action::MoveLeft, Action::CameraLeft),
            (Action::MoveRight, Action::CameraRight),
            (Action::Jump, Action::PauseSim),
        ];
        let b = Bindings::default();
        for normal in [true, false] {
            let actions: Vec<Action> = Action::all()
                .filter(|a| match a.scope() {
                    Scope::Both => true,
                    Scope::Normal => normal,
                    Scope::Sandbox => !normal,
                })
                .collect();
            for (i, a) in actions.iter().enumerate() {
                for c in &actions[i + 1..] {
                    let clash = b.keys(*a).iter().any(|k| b.keys(*c).contains(k));
                    let allowed = shared.iter().any(|&(x, y)| (x, y) == (*a, *c) || (y, x) == (*a, *c));
                    assert!(!clash || allowed, "{a:?} and {c:?} have the same key");
                }
            }
        }
        // The debug keys.
        let f = |code| b.actions(&Press::new(code), true, true);
        assert_eq!(f(KeyCode::F4), vec![Action::DebugChunks]);
        assert_eq!(f(KeyCode::F5), vec![Action::DebugHeat]);
        assert_eq!(f(KeyCode::F6), vec![Action::DebugGrid]);
    }
}
