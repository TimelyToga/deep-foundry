//! Save files: the folder, the file names, and the list for the save and load dialogs.
//!
//! A save is `<name>.dfworld` (the world, written by the simulation thread) and `<name>.info`
//! (a few lines of text: seed, world size and ticks, written by the game when it asks for the save).

use foundry_ui::SaveInfo;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const WORLD_EXTENSION: &str = "dfworld";
const INFO_EXTENSION: &str = "info";

/// The folder for saves: the app data folder of the system, then `DeepFoundry/saves`.
///
/// - macOS: `~/Library/Application Support/DeepFoundry/saves`
/// - Windows: `%APPDATA%\DeepFoundry\saves`
/// - Linux and others: `$XDG_DATA_HOME/DeepFoundry/saves`, or `~/.local/share/DeepFoundry/saves`
pub fn default_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let base = if cfg!(target_os = "macos") {
        home.join("Library").join("Application Support")
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or(home)
    } else {
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local").join("share"))
    };
    base.join("DeepFoundry").join("saves")
}

/// A file name part for a save name: letters, digits, spaces, `-` and `_` stay, other characters
/// become `_`. At most 60 characters. An empty name becomes "save".
pub fn file_stem(name: &str) -> String {
    let s: String = name
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' })
        .take(60)
        .collect();
    let s = s.trim().to_string();
    if s.is_empty() { "save".to_string() } else { s }
}

/// The world file for a save name.
pub fn world_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{}.{WORLD_EXTENSION}", file_stem(name)))
}

/// Facts about a save that the world file does not show without loading it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SaveMeta {
    pub seed: u64,
    /// World size in chunks.
    pub chunks: (i32, i32),
    /// Simulation ticks since the world was made (60 per second).
    pub ticks: u64,
}

/// Write the info file next to a world file.
pub fn write_meta(world: &Path, meta: SaveMeta) -> std::io::Result<()> {
    let text = format!("seed={}\nchunks={}x{}\nticks={}\n", meta.seed, meta.chunks.0, meta.chunks.1, meta.ticks);
    std::fs::write(world.with_extension(INFO_EXTENSION), text)
}

/// Read the info file of a world file, if it exists.
pub fn read_meta(world: &Path) -> Option<SaveMeta> {
    let text = std::fs::read_to_string(world.with_extension(INFO_EXTENSION)).ok()?;
    let mut m = SaveMeta::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        match key.trim() {
            "seed" => m.seed = value.trim().parse().unwrap_or(0),
            "ticks" => m.ticks = value.trim().parse().unwrap_or(0),
            "chunks" => {
                if let Some((w, h)) = value.trim().split_once('x') {
                    m.chunks = (w.parse().unwrap_or(0), h.parse().unwrap_or(0));
                }
            }
            _ => {}
        }
    }
    Some(m)
}

/// Delete a save (the world file and its info file).
pub fn delete(dir: &Path, id: &str) -> std::io::Result<()> {
    let world = dir.join(id);
    std::fs::remove_file(&world)?;
    let _ = std::fs::remove_file(world.with_extension(INFO_EXTENSION));
    Ok(())
}

/// How long ago a time was, in words: "just now", "5 min ago", "3 h ago", "2 days ago".
pub fn age_text(then: SystemTime, now: SystemTime) -> String {
    let secs = now.duration_since(then).map(|d| d.as_secs()).unwrap_or(0);
    match secs {
        0..60 => "just now".to_string(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86_400 => format!("{} h ago", secs / 3600),
        86_400..172_800 => "1 day ago".to_string(),
        _ => format!("{} days ago", secs / 86_400),
    }
}

/// All saves in a folder, newest first. `SaveInfo::id` is the world file name.
pub fn list(dir: &Path) -> Vec<SaveInfo> {
    let Ok(entries) = std::fs::read_dir(dir) else { return vec![] };
    let now = SystemTime::now();
    let mut found: Vec<(SystemTime, SaveInfo)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == WORLD_EXTENSION))
        .filter_map(|e| {
            let path = e.path();
            let modified = e.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
            let id = path.file_name()?.to_string_lossy().to_string();
            let name = path.file_stem()?.to_string_lossy().to_string();
            let meta = read_meta(&path);
            let world = match meta {
                Some(m) if m.chunks.0 > 0 => format!("Seed {}, {} × {} cells", m.seed, m.chunks.0 * 64, m.chunks.1 * 64),
                Some(m) if m.chunks.1 > 0 => format!("Seed {}, endless world, {} cells deep", m.seed, m.chunks.1 * 64),
                _ => "Sandbox world".to_string(),
            };
            let info = SaveInfo {
                id,
                name,
                date: age_text(modified, now),
                play_time_s: meta.map_or(0, |m| m.ticks / 60),
                world,
            };
            Some((modified, info))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    found.into_iter().map(|(_, s)| s).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn file_stems_are_safe() {
        assert_eq!(file_stem("My factory"), "My factory");
        assert_eq!(file_stem("  a/b\\c:d  "), "a_b_c_d");
        assert_eq!(file_stem(""), "save");
        assert_eq!(file_stem("..."), "___");
        assert_eq!(file_stem(&"x".repeat(100)).len(), 60);
    }

    #[test]
    fn age_in_words() {
        let now = SystemTime::now();
        assert_eq!(age_text(now, now), "just now");
        assert_eq!(age_text(now - Duration::from_secs(300), now), "5 min ago");
        assert_eq!(age_text(now - Duration::from_secs(3 * 3600), now), "3 h ago");
        assert_eq!(age_text(now - Duration::from_secs(90_000), now), "1 day ago");
        assert_eq!(age_text(now - Duration::from_secs(5 * 86_400), now), "5 days ago");
    }

    #[test]
    fn list_is_newest_first_with_meta() {
        let dir = std::env::temp_dir().join(format!("deep-foundry-saves-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let old = world_path(&dir, "old one");
        std::fs::write(&old, b"x").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        let new = world_path(&dir, "new one");
        std::fs::write(&new, b"x").unwrap();
        write_meta(&new, SaveMeta { seed: 7, chunks: (32, 16), ticks: 600 }).unwrap();
        std::fs::write(dir.join("notes.txt"), b"not a save").unwrap();
        let list = list(&dir);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "new one");
        assert_eq!(list[0].play_time_s, 10);
        assert_eq!(list[0].world, "Seed 7, 2048 × 1024 cells");
        assert_eq!(list[1].world, "Sandbox world");
        delete(&dir, &list[0].id).unwrap();
        assert!(!new.exists() && !new.with_extension("info").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
