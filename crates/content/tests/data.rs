//! Checks on the real data files in `assets/data/`.
//!
//! These tests load the default assets the same way the game does
//! (`Content::load_default`) and check the Tier 0-1 content rules from
//! `docs/design/02-content.md`.

use foundry_content::Content;

/// Ore ids that must have a raw, crushed and washed powder.
const ORES: &[&str] = &["malachite", "cassiterite", "coal", "magnetite", "hematite", "chalcopyrite", "limestone"];

/// Metal ids that must have a `molten_<name>` liquid and a `<name>_block` solid.
const METALS: &[&str] =
    &["tin", "lead", "zinc", "bronze", "copper", "pig_iron", "steel", "gold", "silver"];

#[test]
fn default_assets_load() {
    let content = Content::load_default().expect("the default assets must load and pass all checks");
    assert!(
        content.materials.len() >= 100,
        "expected at least 100 materials, found {}",
        content.materials.len()
    );
}

#[test]
fn every_ore_has_raw_crushed_and_washed_forms() {
    let content = Content::load_default().expect("assets load");
    for ore in ORES {
        for form in ["raw", "crushed", "washed"] {
            let id = format!("{form}_{ore}");
            assert!(content.material(&id).is_some(), "ore `{ore}` is missing its `{id}` form");
        }
    }
}

#[test]
fn every_molten_metal_freezes_into_its_block_and_back() {
    let content = Content::load_default().expect("assets load");
    for metal in METALS {
        let molten_id = format!("molten_{metal}");
        let block_id = format!("{metal}_block");

        let molten = content.expect_material(&molten_id);
        let block = content.expect_material(&block_id);

        let freeze = content.materials.freeze[molten.index()]
            .unwrap_or_else(|| panic!("`{molten_id}` has no `freeze`"));
        assert_eq!(freeze.into, block, "`{molten_id}` must freeze into `{block_id}`");

        let melt =
            content.materials.melt[block.index()].unwrap_or_else(|| panic!("`{block_id}` has no `melt`"));
        assert_eq!(melt.into, molten, "`{block_id}` must melt into `{molten_id}`");

        assert!(
            freeze.at < melt.at,
            "`{molten_id}`/`{block_id}`: freeze temperature ({}) must be below melt temperature ({})",
            freeze.at,
            melt.at
        );
    }
}
