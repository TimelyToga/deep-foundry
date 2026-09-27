# Requests from task "factory-core"

## 2026-09-27 factory-core — Join the progression stand-ins (done)

Done on 2026-09-27. `progress_link.rs` now uses the real `KitBuffer`, `LabStatus`,
`Progress::lab_tick` and `Progress::deliver`. The stand-ins are gone. The `ProgressLink` trait
stays, so tests can pass a test progress (see `tests/machines.rs`).

## 2026-09-27 factory-core — A reaction event from the simulation

For task "reactions" (`foundry_sim::react`). Discovery needs to know when a reaction happens
(game design section 7.3). `foundry_sim` has no event for this yet. Please add:

```rust
pub enum SimEvent {
    // ...
    /// A reaction happened. `index` is the index into `Content::reactions`.
    Reaction { index: u16, at: CellPos },
}
```

- Send at most one `Reaction` event for each reaction index in each tick (for example the first
  cell in the fixed update order), so the event list stays short and the result is the same on
  any number of threads.
- `at` is the cell of input A.
- The game reads `sim.events()` after each tick and calls
  `factory.observe_reaction(index, at)` for each `Reaction` event. The factory ignores reactions
  more than `REACTION_SEE_RANGE` (96) cells from the robot, and gives discovery points the first
  time the player sees a reaction.

## 2026-09-27 factory-core — Data for Tier 0 logistics buildings

The data has no hopper, belt, crate, barrel or Hub yet. The tests use their own `test_*`
buildings (`crates/factory/tests/common/mod.rs`). The factory code reads these `kind`s and
`params` (defaults in brackets):

| kind | params | notes |
|---|---|---|
| `storage` | `slots` (8), `tanks` (0), `capacity` (1000 units per tank) | crate: slots; barrel: `slots: 0, tanks: 1, capacity: 500` |
| `hopper` | `capacity` (64 cells), `rate` (16 cells/s) | without ports in the data it gets BulkIn on top and BulkOut below |
| `belt` | `belt_speed` (8 cells/s) | rotation must be 0 or 2; `flip` makes it move left |
| `hub` | `slots` (16), `tanks` (4), `capacity` (1000) | takes only items that a milestone asks for |
| `lab` | `kit_buffer` (10 of each kit) | `speed` is the lab speed |
| `workbench` | `reach` (6 tiles) | `speed` is the hand crafting speed near it |
| any kind with `crafts` | `buffer_crafts` (2), `heat_temp` (800, burners) | a crafter; a burner is a crafter with `power.burn_w > 0` and a `Heat` port |
| any kind | `needs_floor` (0) | above 0: solid cells must be under at least half of the bottom row |

A research kit is a part in category `research` that is not a building, or a part that a
technology lists in `kits`.

## 2026-09-27 factory-core — A `scrap` material

A broken building leaves scrap in the bottom quarter of its footprint. The code uses the
material `scrap` if it exists, else the broken form of the body material (for `wood_block` that
is `wood_block` itself). Please add a `scrap` powder material (for example "Scrap", grey-brown,
density about 2000).

## 2026-09-27 factory-core — World edges for placement

`Buildings::check_place` (in `placement.rs`) uses `Simulation::size_cells()` for the
"Outside the world" check. It is the only place. If the world has no edges after the storage
change, please keep `size_cells` or add `Simulation::in_world(CellPos) -> bool` (for example for
the top and bottom limits), and change that one line.

## 2026-09-27 factory-core — Building-body flag on cells

Technical design 7.2 says body cells carry the building-body flag. `set_cell` cannot set it, so
body cells are plain solid cells of the body material now. If the renderer or the cell rules need
the flag, please add `Simulation::set_cell_flags(p, flags)` or a `set_body_cell(p, material)`.

## 2026-09-27 factory-core — Serde on `ItemRef` and `Stack` (optional)

`foundry_content::ItemRef` and `Stack` do not derive `Serialize`/`Deserialize`. The factory state
avoids them (slots store `PartId`, tanks `MaterialId`, machine buffers are counts in recipe
order). Deriving serde on both would make save data and UI messages simpler later.

## 2026-09-27 factory-core — Heat pass

The lava test in `crates/factory/tests/damage.rs` checks whether heat reaches the body. While the
heat pass is a stub it sets the body temperature itself (the branch is marked in the test). When
the heat pass lands, the test uses the real heat. Burners (`heat_side`) and heat ports already
read and write cell temperatures, so they will work with the heat pass as it is.
