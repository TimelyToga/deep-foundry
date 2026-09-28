# Requests from the heat task

Heat flow and phase changes are in `crates/sim/src/heat.rs` (read its module documentation first).
These are changes in files that the heat task does not own.

## 1. Data: cold materials start at 20 °C, and water never freezes

`ice`, `snow` and `frozen_ground` have no `temperature`, so a new cell of them is at 20 °C. Ice
melts at 0 °C, so painted or generated ice becomes water in the first tick that the heat pass
sees it. Water has no `freeze`, so water never becomes ice.

Suggestion (`assets/data/materials/terrain.ron` and `basic.ron`):

- `ice`: `temperature: Some(-10)`
- `snow`, `frozen_ground`: `temperature: Some(-5)`
- `water`: `freeze: Some((at: -2, into: "ice"))` (ice melts at 0 °C: a gap of 2 °C, so a cell
  does not change back and forth)

The scene tests set the ice temperature in the legend, so they do not depend on this.

## 2. Data: what the heat values mean now

For `assets/data/README.md` ("Value conventions"):

- Heat flows between two neighbor cells with `k = min(g_a, g_b)`, where
  `g = min(0.1 × conductivity, 0.2 × heat_capacity)` (`heat::CONDUCT_SCALE`, `heat::MAX_STEP`).
  A cell changes by `k × difference / heat_capacity` per neighbor and tick.
- So one neighbor changes a cell by at most 0.2 of the difference per tick. All metals reach this
  limit: their exact conductivity does not matter above about `2 × heat_capacity`. Stone is about
  6 times slower than metal, water about the same as stone, firebrick about 35 times slower than
  metal, air about 25 times slower.
- A material without `heat_capacity` gets 1.0, and one without `conductivity` gets 0 (it never
  takes or gives heat). Every material in the data has a conductivity now.
- There is no latent heat: melting and boiling do not use heat.

## 3. World: new chunks and the air temperature

`world.rs` makes new air (and "Air" chunks with no cells) at `DEFAULT_TEMPERATURE`. The heat pass
moves air cells toward `air_temperature[y]` (`Simulation::set_air_temperature`). Today nobody sets
the air temperature, so both are 20 °C and new chunks are in balance.

If the air temperature becomes something else (hot deep layers, a cold tundra), new air is out of
balance. Nothing happens while a chunk sleeps, but when a chunk works (for example the player digs
there), its air moves toward the air temperature, then its edges differ from the next chunk, which
wakes, and so on: the heat work spreads over the whole area, and all these chunks become changed
(not pristine), so they stay in memory and are saved.

Suggestion:

- The world gets the air temperature table (from `set_air_temperature`).
- `make_chunk` gives air cells with `ChunkCells::MATERIAL_DEFAULT` the air temperature of their
  row, and a chunk counts as "Air" (no cells stored) when all its cells are air at the air
  temperature of their rows. `Chunk::new_air()` for such a chunk fills the temperatures by row.
- Keep the air temperature change between two rows at 1 °C or less. Two neighbor cells that
  differ by 2 °C or more exchange heat, so air with a steeper change never comes to rest, and its
  chunks never sleep.

## 4. World generator: hot features

A lava pocket (or hot rock) in cold stone is not in heat balance. It sleeps until something wakes
its chunk; then the lava cools (and freezes into stone at the edge) and the stone around it warms.
If the generator wants lava to stay liquid until the player comes close, this is fine. If it wants
hot areas at rest, it can write matching temperatures into `ChunkCells::temp` around them, with at
most 1 °C difference between neighbor cells.

## 5. Tests: a change in `crates/sim/src/liquid_tests.rs` (please review)

`lava_and_mud_flow_slowly_and_rest_with_a_slope` failed with heat: lava at 1200 °C on the 20 °C
floor cools, freezes into stone (the test checks that no lava is lost), and keeps its chunks awake
for more than the 8000 ticks the test waits. The test is about movement, so `scene()` in
`liquid_tests.rs` now sets the kept liquids to 20 °C and removes their phase changes in its own
copy of the content (7 lines). Please check that this is what you want.

## 6. Tests: changes in `crates/factory/tests/` (please review)

These factory tests were written before heat existed. With heat they failed; each change keeps the
purpose of the test:

- `damage.rs`, `a_hot_body_stops_a_machine`: the wooden body at 250 °C cools to about 235 °C in
  64 ticks (heat flows into the ground and the air). The test now accepts 221 to 250 °C and builds
  the expected "Too hot" text from the building's temperature.
- `machines.rs`, `min_temp_is_read_from_the_heat_port`: one row of 8 hot stone cells cooled below
  500 °C before the oven was done. The row is now made hot again in each tick (a steady heat
  source).
- `machines.rs`, `fluid_ports_take_water_and_give_steam`: the boiler's steam (110 °C) cooled in
  the 20 °C air and condensed into water, so less steam was left. Steam does not condense in this
  test's own copy of the content.
- `machines.rs`, `a_burner_heats_the_oven_above_it`: the burner writes 800 °C at its heat port,
  then the heat pass of the same tick moves a little of it away (794 °C). The test accepts 780 to
  800 °C.

For the factory task: a boiler that should give steam that stays steam for a while needs to make
it hotter than 110 °C (steam condenses below 95 °C). A heat port that should hold a temperature
must write it in every tick (as the burner does).

## 7. Scene tests: pending checks that pass now

`assets/scenes/tests/lava_meets_water.ron`: both pending checks pass with heat alone (lava next to
water freezes into stone, water boils into steam). Remove `pending: true` if the reactions task
does not change them.

`gas_rises.ron`: with heat, the steam at 400 °C cools at the stone walls; 616 of 640 steam cells
are still in the top rows after 300 ticks (the check wants 600). It passes, but it is close.

## 8. Game: heat numbers in the debug panel (optional)

`heat::step` returns `HeatStats` (chunks worked, cells changed, phase changes, chunks that stay
heat-active). `Simulation::tick` ignores it now. It could go into `SimStats` for the timings panel.
A `SimSettings` slider for `CONDUCT_SCALE` could help to tune how fast heat moves.
