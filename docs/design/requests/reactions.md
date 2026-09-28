# Requests from task "reactions"

## 2026-09-27 reactions — Small changes in lead-owned files (done, please check)

The task needs these. They are small, and each one is in its own place:

- `crates/sim/src/hood.rs`: `Hood::set_flags` is now `pub` (it was private). `react.rs` needs it to
  set and clear `FLAG_BURNING`. It keeps its old behavior.
- `crates/sim/src/lib.rs`: new event `SimEvent::Reaction { index: u16, at: CellPos }` (see
  `docs/design/requests/factory-core.md`). After `schedule::movement_tick`, `tick()` calls
  `react::dedupe_reaction_events(&mut self.events)`, which keeps the first `Reaction` event of each
  reaction. Each job already sends each reaction at most once; the events are in the fixed job
  order, so the result does not depend on the number of threads.
- `crates/sim/src/chunk.rs`: new flag `FLAG_BURNING` (bit 2).

## 2026-09-27 reactions — The heat pass must wake cells that get hot

What: when the heat pass changes the temperature of a cell so that it passes
`ReactTable::wake_temp(material)`, mark the cell for update in the next tick (the dirty rectangle
of its chunk), also in a chunk that sleeps.

Why: reactions, burning and charring only run for cells that the movement update visits. A rule
with a temperature condition (smelting at 900 °C, wood that starts to burn at 300 °C, brine that
boils at 105 °C) lets its cell sleep while the condition does not hold. Without a wake-up, a cold
pile of ore and charcoal that a furnace heats never smelts, and wood next to lava never burns.

How: `heat::step` does not get the `ReactTable` now. Either pass `&ReactTable` (it is in
`Simulation::react`), or copy `wake_temp` into the material table. `wake_temp(m)` is the lowest
temperature at which something starts for material `m` (`None`: nothing depends on heat). A
simple rule that also works: wake a cell when its temperature changes by 10 °C or more since it
was last visited. Wake the cell and its 8 neighbors (as `Hood::mark_changed` does): some
materials, for example water, leave their reactions to the neighbor material.

Stub until then: the scene and unit tests set the start temperatures in the picture, so the
cells are awake from the start.

## 2026-09-27 reactions — `set_cell` and `Hood::replace` keep old flag bits

What: `Simulation::set_cell` keeps all flag bits except the parity bit, and `Hood::replace` and
`Hood::swap` do not touch the flags. So `FLAG_BURNING` can stay on a cell after other code puts a
new material there (digging a burning cell, an explosion, a burning liquid that flows away).
Please clear `FLAG_BURNING` in `set_cell`, `fill_chunk` (it already writes only the parity) and
`Hood::replace`.

Why: the renderer may draw flames from the flag.

Stub until then: `react.rs` clears a stale flag when it visits the cell (for materials with
reaction work). The flag doc in `chunk.rs` says: use the flag only on materials with burn data,
never on air. The burning state itself is in the life byte, which moves with the cell.

## 2026-09-27 reactions — For the renderer: `FLAG_BURNING`

A burning cell has `FLAG_BURNING` in its flags (the 4th texel value). Draw it only when the
material has burn data (`MaterialTable::burn[m].is_some()`), never for air. A glow or small
flicker on these cells shows burning solids; burning liquids and powders also put fire cells
into the air above them.

## 2026-09-27 reactions — Explosion strength (for task "explosions and particles")

Reactions send `SimEvent::Explosion { at, strength, heat }` with the values in
`crates/sim/src/react.rs`: `EXPLOSION_SMALL` = (12.0, 400 °C), `EXPLOSION_MEDIUM` = (30.0, 600 °C),
`EXPLOSION_LARGE` = (60.0, 900 °C). The strength is meant to be compared with material hardness
(technical design 6.7: wood 10, stone 40, bedrock 255). If the explosion code uses another scale
(for example a radius in cells), change these three constants. A burning methane cloud sends one
small explosion for each methane cell that catches fire, so the explosion code must handle many
events in one tick (merge close events, or spread them over ticks).

## 2026-09-27 reactions — Wire reaction events into the game (integrate step)

`crates/game/src/factory_host.rs` has a comment where it should call
`self.factory.observe_reaction(index, at)` for each `SimEvent::Reaction` of `sim.events()`.

## 2026-09-27 reactions — Mud drying in air

The schema now has timers that count only with air next to the cell. Mud could dry into dirt in
open air with `timer: Some((ticks: 3600, into: "dirt", needs_air: true))`. It is left out of
`materials/liquids.ron`, because `liquid_tests::lava_and_mud_flow_slowly_and_rest_with_a_slope`
checks that no mud is lost (and a timer keeps the mud surface awake, so the test never sees the
world at rest). To enable it: use `tar` in that test, then add the timer to mud.

## 2026-09-27 reactions — Brine boiling

Brine now boils by a reaction (`reactions/water_heat.ron`: from 105 °C, 30% of cells leave salt).
Its `boil` phase change moved from 105 °C to 120 °C, so it only catches brine that was not awake
(see "The heat pass must wake cells" above). When the heat pass wakes hot cells, the `boil` of
brine can be removed.
