# Requests from the render-light task

Changes outside crates/render, assets/shaders and the render parts of crates/game that would
make the light better. The renderer has a local workaround for each.

## 1. Set FLAG_BUILDING on building body cells (factory)

`crates/factory/src/placement.rs` writes building body cells with `sim.set_cell(p, def.body, None)`.
The cell flag `FLAG_BUILDING` (bit 1, `crates/sim/src/chunk.rs`) stays 0.

The light pass lets building cells stop only a quarter of the light (world.wgsl,
`CELL_FLAG_BUILDING`), so that a building is lit on its whole face and does not cast a deep
shadow. Without the flag, a building is a block of solid cells: the Hub has a dark middle and a
dark band under it.

Needed: a way to set the flag on body cells (for example `Simulation::set_cell_flags` or a
`set_cell` variant), and the factory uses it when it places a building (and clears it when it
removes one).

## 2. The surface level in the snapshot (core, sim)

The sky light needs to know where the sky ends. The renderer has only the chunks near the view,
so for chunks above the view it counts "no data" as open air above the surface level and as rock
below it (`Renderer::set_surface_level`). The game guesses the level from the world size
(`render_setup::surface_level`: `DEFAULT_SKY_CHUNKS` x 64 for the world with no side limit).

Needed: `Snapshot::surface_y` (or `SimConfig::surface_y()` on `Simulation`), so that a world
from world generation or a loaded save with other settings gives the right level. For worlds
with high mountains a per-column surface height (the highest solid cell of each column, from
the chunk source) would be better still.

## 3. Heat map needs the heat module

The heat map view (F5) shows the temperature in each texel. While heat.rs is a stub, the
temperatures do not change, so the map shows only the start temperatures. Nothing to do once
the heat task is merged.
