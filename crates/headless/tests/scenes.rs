//! Runs every scene test in `assets/scenes/tests/`. Checks that are not pending must pass.
//! The same as `cargo run -p foundry_headless --release -- test`.

use foundry_content::Content;
use foundry_headless::runner::{format_table, run_tests};
use std::sync::Arc;

#[test]
fn all_scene_tests_pass() {
    let content = Arc::new(Content::load_default().expect("data files load"));
    let reports = run_tests(&content, None).expect("scene folder exists");
    assert!(!reports.is_empty(), "no scene tests found");
    let table = format_table(&reports);
    println!("{table}");
    let failed: Vec<&str> = reports.iter().filter(|r| r.failed()).map(|r| r.name.as_str()).collect();
    assert!(failed.is_empty(), "scene tests failed: {failed:?}\n\n{table}");
}
