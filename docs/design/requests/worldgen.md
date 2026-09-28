# Requests from the worldgen task

Things the world generator (`crates/worldgen`) needs from files that other tasks own. For each,
the local workaround that the generator uses now.

## 1. Air temperature per column (biome), not only per row

The design wants cold air in the tundra (−20 °C at the surface). `Simulation::set_air_temperature`
takes one value for each row, so the tundra air is 15 °C like everywhere else at the surface.

What the generator does now:

- `WorldGen::air_temperature_rows()` gives the value for each row (5 °C at the top of the sky,
  15 °C at the surface, 20 °C at 600 cells deep, 40 to 80 °C in the deep rock, and so on). The
  game sets it with `set_air_temperature` when it makes a generated world.
- `WorldGen::air_temperature_at(seed, x, y)` gives the value with the biome (−20 °C near the
  tundra surface). Nothing uses it yet.
- Tundra cells are made cold: snow, ice and frozen ground at −12 to −2 °C, lake water at −1 °C
  (see item 3). Ground of other biomes near a tundra zone is as cold at the border and gets
  warmer over 160 cells.
  When the heat pass runs, the 15 °C air will warm them, and snow and frozen ground (both melt
  at 1 °C) will slowly melt.

Suggestion: let the heat pass read the air temperature from the chunk source for each chunk
column (for example an optional `ChunkSource::air_temperature(x, y) -> Option<i16>`, sampled once
per chunk column and row band), or accept a second table by chunk column.

## 2. Ore colors close to black

`magnetite` (`#15161c`) and `coal` (`#181818`) are almost black. In a cave, the player cannot
tell them from the dark air. A little more color (for example a blue-gray for magnetite) would
help. (content-data task)

## 3. Frozen lakes and the water `freeze` rule (done)

Water now freezes below -2 °C. The generator gives the water of a frozen lake -1 °C, and the ice,
the lake bed and the shore 1 °C less for each cell away from the water (the ice is 11 to 13 rows
thick, -12 °C at the top). The heat code moves no heat between neighbors that differ by less than
2 °C, so the lake does not change when it wakes. Over a long time the warm air (item 1) still
melts the top of the ice and the snow.

## 4. World generation settings in a data file

`docs/design/03-technical-design.md` lists `assets/data/worldgen.ron`. The numbers are constants
in the code for now (in `surface.rs` and `chunk.rs`, each with a comment). The settings that are
saved in world files are only the version and the size (`v1 surface=1024 bottom=9216`). If the
numbers move to a data file, their values must also go into the saved settings text, or a world
file loads with other cells than it had.
