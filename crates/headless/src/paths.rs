//! Where the tools find scenes and write their files.

use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

/// The `assets/` folder. See `foundry_content::default_assets_dir`.
pub fn assets_dir() -> PathBuf {
    let dir = foundry_content::default_assets_dir();
    dir.canonicalize().unwrap_or(dir)
}

/// The workspace folder (the parent of `assets/`).
pub fn workspace_dir() -> PathBuf {
    assets_dir().parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
}

/// `assets/scenes/`.
pub fn scenes_dir() -> PathBuf {
    assets_dir().join("scenes")
}

/// `assets/scenes/tests/`. The `test` command runs every scene in it.
pub fn test_scenes_dir() -> PathBuf {
    scenes_dir().join("tests")
}

/// `out/` in the workspace. Git ignores it. Pictures go here.
pub fn out_dir() -> PathBuf {
    workspace_dir().join("out")
}

/// `bench/baseline.ron` in the workspace.
pub fn baseline_path() -> PathBuf {
    workspace_dir().join("bench").join("baseline.ron")
}

/// Find a scene RON file. `name` is a path to a `.ron` file, or a scene name in
/// `assets/scenes/` or `assets/scenes/tests/`.
pub fn find_scene(name: &str) -> Result<PathBuf> {
    let as_path = PathBuf::from(name);
    if as_path.extension().is_some_and(|e| e == "ron") && as_path.is_file() {
        return Ok(as_path);
    }
    let stem = name.trim_end_matches(".ron");
    for dir in [scenes_dir(), test_scenes_dir()] {
        let p = dir.join(format!("{stem}.ron"));
        if p.is_file() {
            return Ok(p);
        }
    }
    bail!("no scene `{name}` in {} or {}", scenes_dir().display(), test_scenes_dir().display())
}

/// All `.ron` files in a folder, sorted by name.
pub fn ron_files(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        bail!("folder {} does not exist", dir.display());
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "ron"))
        .collect();
    files.sort();
    Ok(files)
}

/// The file name without folder and extension.
pub fn stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}
