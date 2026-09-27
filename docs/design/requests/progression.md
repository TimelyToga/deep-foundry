# Requests from the progression task

## 2026-09-27 progression — `ron` dependency in `crates/factory/Cargo.toml`

The guide loader (`crates/factory/src/progress/guide.rs`) reads `assets/data/guide/*.ron`, so
`foundry_factory` needs `ron`. I added `ron.workspace = true` to `crates/factory/Cargo.toml`.
The factory-core task may change the same file. If both add lines, keep both.

## 2026-09-27 progression — who holds the guide and calls `update_guide`

`Guide` is data, like `Content`, and is not saved. `Progress::update_guide(&guide, &content, &state)`
needs the player's items and building counts through the `GuideState` trait
(`item_count(ItemRef)`, `building_count(BuildingKindId)`).

Suggestion: load the guide once at start next to the content (`Guide::load_default()`), keep it
as `Arc<Guide>` in `Factory` (lead-owned `lib.rs`) or in the game, and call `update_guide` about
once a second from the code that owns the inventory and the building list. Until then nothing
calls it; the guide screen can still use `Progress::guide_view`.

## 2026-09-27 progression — the simulation must report reactions

Discovery needs to know when a reaction happens near the player for the first time (game design
section 7.3). `foundry_sim` has no event for this.

Suggestion: a `SimEvent::Reaction { index: u16, at: CellPos }`, sent at most once per reaction
index per second (or a "seen" bit per reaction index that the game can read and clear). The
game then calls `progress.discover_reaction(&reaction_key(&content, &content.reactions[index]))`.
`reaction_key` (in `foundry_factory::progress`) makes the key `<a>+<b>` from the data, with the
two sides in name order. Tech data names reactions as `reaction:<a>+<b>`, in any order.

## 2026-09-27 progression — scanning an ore vein

The starter tech `bronze` needs discoveries `raw_malachite` and `raw_cassiterite` (the dug
powders), but the player scans the vein cells (`malachite`, `cassiterite`) in the world.
Suggestion for the scan tool (game task): when the player scans a material, also call
`discover_material` for its `broken_into` material. Or the tech data can name the vein ids.
The guide uses the same ids as the tech data (`raw_malachite`, `raw_cassiterite`).

## 2026-09-27 progression — ids the guide uses that the data does not have yet

`Guide::check(&content)` lists guide names that the content does not have. A goal with an
unknown name is never done. The test `guide_files_load` prints the list. Now (starter data only):

- buildings: `campfire`, `kiln_controller`, `bellows`, `stamp_mill`, `sluice`, `small_boiler`,
  `steam_crusher`, `coke_oven_controller`, `blast_furnace_controller`, `iron_belt`, `steam_lab`
- parts: `steel_plate`, `glass_vial`, `rubber_sheet`, `steam_kit`
- techs: `bronze_drill_head`
- Hub stage 2

The content-data task can use these ids, or the integration step can change the guide files
(`assets/data/guide/tier0.ron`, `tier1.ron`) to the ids the data uses.

## 2026-09-27 progression — discovery point numbers for the tech data

- A new material scan gives 1 point. A new reaction gives 2 points
  (`POINTS_PER_MATERIAL`, `POINTS_PER_REACTION`). Guide goals give 1 to 5 points.
- A technology's `discovery_points` are spent when it starts for the first time (the design
  says early Tier 0 technologies "cost" discovery points).
- A technology with no `kits` needs no lab: it is done as soon as it starts. The starter techs
  `bronze` and `research` have no kits, so they work before the player has a lab.

## 2026-09-27 progression — `ItemRef` and `Stack` have no serde derives

Progress does not need them (it stores Hub deliveries as counts in the order of the milestone
list). Other saved factory state (inventories, buffers) will need them. Suggestion: derive
`Serialize, Deserialize` on `ItemRef` and `Stack` in `foundry_content::factory`.
