# TimTech

[![The TimTech trailer: click to watch the video](docs/media/timtech-trailer-preview.webp)](docs/media/timtech-trailer.mp4)

[Watch the trailer (46 seconds, 1080p, 60 fps)](docs/media/timtech-trailer.mp4). All shots are recorded from the game with `make trailer`.

A 2D side-view factory game in a world where every cell moves. Sand falls, water flows, fire burns and metal melts, as in Noita. On top of that you build a factory and work through a long production tree, as in Factorio and GregTech.

Written in Rust with wgpu and egui.

## Influences

TimTech takes ideas from these games:

- **GregTech: New Horizons** (a Minecraft modpack). A long production tree with many tiers. Each tier needs parts from the tier before it. Machines have voltage tiers, and a machine explodes if its voltage is too high. The name TimTech follows the GregTech pattern: GregTech is named after its author, Greg.
- **Factorio.** Belts, arms that move items, ghosts for buildings that are not built yet, research, and windows for the character, crafting and buildings. Factories run without the player.
- **Terraria.** A 2D side view. A small character who digs down through layers of the world. Caves, and deeper layers with harder stone, better ores and more danger.
- **Sandustry.** A factory game in a falling-sand world. The machines work with the real sand, water and heat of the world.

![A generated world](docs/screenshots/world_wide.jpg)

## Overview

You are a small robot at a broken Hub. You dig material into your tanks, craft your first tools and buildings, and repair the Hub stage by stage. Each tier opens new machines: fire and clay first, then bronze, then steam and automation, then electric power.

The machines work with the real cells of the world. A kiln is a room of brick walls with a real fire inside. The room temperature comes from the burning cells. A crucible on a campfire smelts ore and pours the molten metal into a mold. Ore falls into a hopper, moves on a belt and is washed with real water. In Tier 1, steam drills, arms, steam furnaces and assemblers run whole production lines with no work from you.

## Features

- **Cell simulation**
  - Powders, liquids, gases and fire.
  - Heat flow, melting, boiling and freezing.
  - Reactions and burning.
  - Explosions.
  - Liquids that splash and settle flat.
  - Parallel chunk update. Chunks with no change sleep.
- **Endless world**
  - The world has no end to the left or right.
  - Chunks are made when you need them and dropped when you leave.
  - Saves keep only the chunks that changed.
  - A world generator makes the surface, biomes, trees, lakes, ore veins and caves.
  - The start area has woods, clay, copper ore, two tin ore beds, coal, and shallow iron ore and limestone for Tier 1, with free flat ground right of the Hub to build on.
  - The deep: caverns lit by glow moss (some with a lake and a rich ore deposit in a wall), crystal geodes and gold in the granite (from 1800 cells down), and lava chambers in the basalt (from 3600 cells down). The first cave is about 40 tiles under the flat ground right of the Hub.
- **The robot**
  - Walk, jump and fly with a jetpack.
  - Dig material into tanks.
  - Choose per material if the robot keeps it or drops it.
  - Spray material out.
  - Scan to discover materials.
- **Construction**
  - Place buildings as ghosts that show their ports.
  - Drag lines, rotate, copy (pipette), undo and redo.
  - Remove buildings by dragging.
  - A hover box shows what is under the mouse.
- **Factory**
  - Hand crafting and a workbench.
  - Crates, barrels, hoppers and belts. Belts move powder and parts. Sorters and splitters route powder.
  - Room machines: kiln, coke oven and blast furnace.
  - Crucible, bellows and casting molds: the crucible pours into a mold, and the mold puts the plates into a crate.
  - Stamp mill and sluice.
  - Boiler, pipes and steam machines.
  - Automation (Tier 1): arms that move items between buildings (with a filter), the steam assembler, the steam furnace, the steam drill, the steam blower for the blast furnace, iron crates.
  - Electric power (Tier 2): steam turbines, copper cables and power networks; the macerator, electric furnace, electric assembler, electric drill and fast arm. Too little power slows every machine on the network. Batteries store power.
  - The deep: moss lanterns and crystal lamps light caves; steel and hard drill heads dig granite and basalt; gold smelts into plates and wire for circuits; batteries need circuits.
  - Factories keep running when you walk away.
- **Progression**
  - Research in labs.
  - Hub repair stages and milestones.
  - Material discovery.
  - A guide that always shows the next goal.
  - Every Tier 0 goal, up to the first Hub repair, has a test that plays it with real player input in the generated world. Tier 1 and Tier 2 have the same test (with the parts of the earlier tier given at its start).
- **Interface**
  - Factorio-style windows: character, crafting, buildings and research.
  - Pause, save and load menus.
- **Keys**
  - All keys can be changed in Settings.
  - By default, keys match by position on the keyboard, so movement also works on Dvorak and AZERTY.
- **Look**
  - Pixel-art robot with animations.
  - Light from hot and glowing cells.
  - Debug views for chunks, heat and the grid.

## Planned features

- Belt lifts.
- Building sprites (now buildings are blocks of their material with an outline and a faint icon).
- Blueprints, and ghosts that wait for items.
- MV machines (Tier 3 and later).
- Deeper world layers, with more ores and hazards.
- Sound.

## Screenshots

| | |
|---|---|
| ![The start in a generated world](docs/screenshots/world_gen.jpg) | ![A kiln room that fires clay bricks](docs/screenshots/kiln.jpg) |
| The start, with the guide | A kiln room that fires clay bricks |
| ![The smelting site](docs/screenshots/smelter.jpg) | ![An automated line](docs/screenshots/automation.jpg) |
| Tier 0: a crucible on a campfire pours copper into a mold | Tier 1: a steam drill, arms, a steam furnace, a mold, a gear assembler and gears on a belt |
| ![A steam crusher](docs/screenshots/steam_line.jpg) | ![The robot's animations](tools/sprites/robot_preview.png) |
| A steam crusher at work | The robot's animations |
| ![The cave under the start area](docs/screenshots/cave.jpg) | |
| The cave under the start area: glow moss, two moss lanterns and a crystal lamp | |

## Run

You need Rust 1.98 or newer ([rustup.rs](https://rustup.rs)). The game is tested on macOS.

```
git clone <this repository>
cd deep-foundry
cargo run --release -p timtech
```

New games use the world generator. The small demo world (a test world) is still there:

```
cargo run --release -p timtech -- --world demo
```

Default keys (by key position; change them in Settings > Controls):

| Key | Action |
|---|---|
| A / D, or the arrow keys | Walk |
| W, Up or Space | Jump; hold to fly |
| Left mouse | Dig |
| Right mouse | Spray the chosen tank |
| E | Character and crafting |
| G | Guide |
| T | Research |
| F | Scan |
| R | Rotate |
| Q | Copy the building under the mouse |
| Ctrl+Z / Ctrl+Y | Undo / redo |
| F3 | Debug window |
| F4 / F5 / F6 | Debug views: chunks, heat, grid |

Tests:

```
cargo test --workspace
cargo run --release -p foundry_headless -- test
```

More options (screenshots, benchmarks, test scenes) are in [crates/game/README.md](crates/game/README.md) and [crates/headless/README.md](crates/headless/README.md). The design documents are in [docs/design](docs/design/README.md).
