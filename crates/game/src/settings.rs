//! The settings file: the UI scale, vertical sync, the FPS counter, the debug panel, the
//! simulation numbers, the key bindings and the learned key names. RON text in the app data
//! folder (`settings.ron` next to the `saves` folder). The game reads it at the start and writes
//! it when a setting changes.

use crate::keys::{Bindings, KeyNames};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use winit::keyboard::KeyCode;

/// Version of the file format.
const VERSION: u32 = 1;

/// Everything in the settings file. Missing fields get their defaults, so an old file still
/// loads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedSettings {
    pub version: u32,
    pub ui_scale: f32,
    pub vsync: bool,
    pub show_fps: bool,
    pub show_debug: bool,
    /// Simulation numbers by key.
    pub simulation: BTreeMap<String, f32>,
    pub bindings: Bindings,
    /// Key names learned from key presses (the player's keyboard layout).
    pub key_names: Vec<(KeyCode, String)>,
}

impl Default for SavedSettings {
    fn default() -> Self {
        Self {
            version: VERSION,
            ui_scale: 1.0,
            vsync: true,
            show_fps: true,
            show_debug: false,
            simulation: BTreeMap::new(),
            bindings: Bindings::default(),
            key_names: vec![],
        }
    }
}

impl SavedSettings {
    /// The learned key names.
    pub fn names(&self) -> KeyNames {
        let mut n = KeyNames::default();
        for (code, name) in &self.key_names {
            n.learn(*code, Some(name));
        }
        n
    }

    pub fn set_names(&mut self, names: &KeyNames) {
        self.key_names = names.all().map(|(c, n)| (*c, n.clone())).collect();
        self.key_names.sort_by_key(|(c, _)| format!("{c:?}"));
    }
}

/// The settings file in the app data folder.
pub fn default_path() -> PathBuf {
    let saves = crate::saves::default_dir();
    saves.parent().map_or_else(|| PathBuf::from("settings.ron"), |d| d.join("settings.ron"))
}

/// Read the settings file. A missing file gives the defaults. A damaged file gives the defaults
/// and a message.
pub fn load(path: &Path) -> (SavedSettings, Option<String>) {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (SavedSettings::default(), None),
        Err(e) => return (SavedSettings::default(), Some(format!("Cannot read the settings: {e}"))),
    };
    match ron::from_str::<SavedSettings>(&text) {
        Ok(s) => (s, None),
        Err(e) => (SavedSettings::default(), Some(format!("The settings file is damaged; using the defaults ({e})"))),
    }
}

/// Write the settings file (and its folder).
pub fn save(path: &Path, s: &SavedSettings) -> Result<(), String> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("Cannot save the settings: {e}"))?;
    }
    let text = ron::ser::to_string_pretty(s, ron::ser::PrettyConfig::default()).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| format!("Cannot save the settings: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{Action, KeyBind};

    #[test]
    fn round_trip_and_defaults() {
        let dir = std::env::temp_dir().join(format!("df-settings-{}", std::process::id()));
        let path = dir.join("settings.ron");
        assert_eq!(load(&path), (SavedSettings::default(), None), "no file: the defaults");
        let mut s = SavedSettings { ui_scale: 1.5, vsync: false, ..Default::default() };
        s.bindings.by_letter = true;
        s.bindings.set(Action::Rotate, KeyBind::new(KeyCode::KeyO));
        s.simulation.insert("liquid_spread".into(), 3.0);
        let mut names = KeyNames::default();
        names.learn(KeyCode::KeyF, Some("u"));
        s.set_names(&names);
        save(&path, &s).unwrap();
        let (back, err) = load(&path);
        assert_eq!(err, None);
        assert_eq!(back, s);
        assert_eq!(back.names().name(KeyCode::KeyF), "U");
        // An old file with fewer fields loads with defaults for the rest.
        std::fs::write(&path, "(ui_scale: 2.0)").unwrap();
        let (old, err) = load(&path);
        assert_eq!((old.ui_scale, old.vsync, err), (2.0, true, None));
        // A damaged file: defaults and a message.
        std::fs::write(&path, "not ron {").unwrap();
        let (bad, err) = load(&path);
        assert_eq!(bad, SavedSettings::default());
        assert!(err.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
