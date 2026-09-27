# Deep Foundry: technical design

## 1. Goals

- The simulation runs at 60 ticks per second at all times, with a large active factory.
- The renderer runs at the display refresh rate (120 Hz on a ProMotion display), with smooth camera movement and no input delay.
- Target machine: Apple M1 Max, 10 cores, 32 GB (your machine). Minimum: a 4-core laptop with a smaller active area.
- The simulation is **deterministic**. The same save and the same inputs give the same result, on any number of threads. This makes bugs repeatable and tests exact.
- Content is data. A new material, reaction, recipe or building needs no code change, unless it needs a new special behavior.

### 1.1 Time budget per tick

The simulation has 16.6 ms per tick. The renderer runs on a different thread (section 4), so it does not use this budget.

| Work | Budget |
|---|---|
| Cell movement and reactions | 5 ms |
| Heat | 2 ms |
| Factory (machines, networks, ports, belts) | 2 ms |
| Particles, parts, explosions | 1 ms |
| Snapshot for the renderer | 1 ms |
| Spare | 5.6 ms |

### 1.2 Scale targets

| Item | Target |
|---|---|
| Awake chunks | 1,500 (6.1 million cells) at 60 ticks per second on the M1 Max |
| Buildings | 5,000 |
| Parts in the world | 20,000 |
| Item packets in tubes | 2,000 |
| Free-flying particles | 50,000 |
| World | 8192 × 8192 cells |
| Memory | less than 2 GB |

## 2. Language and libraries

**Recommendation: Rust.**

Reasons:

- Speed is close to C and C++.
- The compiler stops data races between threads.
- `rayon` makes parallel work simple.
- `wgpu` uses Metal on macOS, and also Vulkan, DirectX 12 and WebGPU.
- Agents can check their work with the compiler and with tests.

We do not use a general game engine (such as Bevy). The simulation and the world renderer must be custom in any case. A small set of libraries is easier to control, and engine updates cannot break it.

| Need | Crate |
|---|---|
| Window and input | winit |
| GPU | wgpu |
| UI panels | egui, egui-wgpu, egui-winit (with a custom theme) |
| Math | glam |
| Parallel work | rayon |
| Data files | serde, ron |
| Saves | serde, postcard, lz4_flex |
| Noise for world generation | fastnoise-lite |
| Images | image |
| Audio (later) | kira |
| Profiling | puffin (with puffin_egui) |
| Benchmarks | criterion, plus our own headless runner |

Before we start: `rustup update stable`. The installed version (1.84) is too old for current wgpu. We keep `Cargo.lock` in git.

**Alternative: a browser game** (TypeScript for the interface, Rust compiled to WebAssembly for the simulation, WebGPU for graphics). It is easier to share with other people. But threads in WebAssembly are harder to set up and slower. If we keep the `sim` crate free of platform code, we can add a web build later.

## 3. Workspace layout

```
deep-foundry/
  Cargo.toml              workspace
  crates/
    core/       ids, positions, constants, random numbers, commands, snapshot types
    content/    data file types, loading, validation, flat lookup tables
    sim/        cell world, chunks, movement, reactions, heat, particles, explosions, parts
    worldgen/   world generation
    factory/    buildings, ports, belts, machines, room machines, networks, research, statistics
    render/     wgpu renderer, shaders, sprites, light, post effects
    ui/         egui panels, HUD, recipe browser, tech tree
    game/       the game program: main loop, threads, input, camera, build tool, player, saves
    headless/   a program with no window: scene tests, benchmarks, screenshots
  assets/
    data/       materials/*.ron, reactions/*.ron, buildings/*.ron, recipes/*.ron, tech.ron, worldgen.ron
    sprites/    png files
    shaders/    wgsl files
    scenes/     small test scenes (png + ron)
  docs/design/
```

Dependencies go one way:

- `core` depends on nothing in the workspace.
- `content` depends on `core`.
- `sim` depends on `core` and `content`.
- `worldgen` and `factory` depend on `sim`.
- `render` depends on `core` and `content`. It reads only the snapshot types from `core`. It never reads the live world.
- `ui` depends on `core` and `content`. It reads the snapshot and sends commands.
- `game` and `headless` connect everything.

## 4. Threads and the main loop

| Thread | Work |
|---|---|
| Main thread | Window events, input, camera, UI, rendering. Runs at the display rate. |
| Simulation thread | Runs fixed ticks at 60 per second. Owns the world, the factory and the player. Uses the worker pool for parallel parts. |
| Worker pool | rayon, with (cores − 2) threads. Also packs and unpacks chunks, and writes saves. |

Communication:

- **Main → simulation: commands.** A queue of `Command` values: place building, remove, dig, spray, set recipe, player input. The simulation applies all queued commands at the start of the next tick.
- **Simulation → main: snapshots.** After each tick, the simulation writes a `Snapshot`: the changed chunk data in the view area, the positions of the player, parts, particles and tube packets, the building states in view, the tile occupancy near the view, open panel data, alerts, statistics and timings. We use a triple buffer: the simulation writes one buffer, the main thread reads the newest complete buffer, and neither thread waits for the other.
- The main thread draws moving things at positions between the last two snapshots. This makes motion smooth at 120 Hz.

Build preview: the main thread checks ghost validity against the tile occupancy in the latest snapshot. So the ghost has no delay. The real placement is a command, and the simulation checks it again.

## 5. World data

### 5.1 Coordinates

- Cell coordinates are `i32`. **x goes right. y goes down.** y = 0 is the top of the world.
- Tile = cell / 8. Chunk = cell / 64 (use floor division for negative values).

### 5.2 Chunks

A chunk is 64 × 64 cells. Each chunk stores its cells as separate arrays (structure of arrays):

| Array | Type | Use |
|---|---|---|
| `mat` | `[u16; 4096]` | material id |
| `temp` | `[i16; 4096]` | temperature in whole °C |
| `shade` | `[u8; 4096]` | color shade, set when the cell is made |
| `life` | `[u8; 4096]` | timer for fire, gas, and slow reactions |
| `motion` | `[u8; 4096]` | fall speed (4 bits), last side direction (1 bit), free bits |
| `flags` | `[u8; 4096]` | updated-this-tick bit, building-body bit, other bits |

This is 8 bytes per cell and 32 KB per chunk. Separate arrays let the heat pass read only `mat` and `temp`.

Chunk states:

| State | Meaning |
|---|---|
| Awake | Updated every tick. |
| Asleep | In memory. Not updated. Any write into it wakes it. |
| Packed | Compressed in memory (run-length encoding per array). Unpacked when needed. |
| Not made | Not generated yet. Generated from the seed when needed. |

Each awake chunk has a **dirty rectangle**: the area where cells changed in the last tick, plus a border of one cell. The next tick updates only this area. If the rectangle stays empty for 30 ticks and the heat is stable, the chunk goes to sleep.

### 5.3 Tile grid

Each chunk also has an 8 × 8 tile grid:

- Front layer: building id (`u32`, 0 = none).
- Back layer: pipe piece id, cable piece id, tube piece id, signal wire piece id.
- Room id (`u16`, 0 = none) for tiles inside a room machine.

### 5.4 Material table

The `content` crate turns the data files into flat arrays indexed by material id: `phase[id]`, `density[id]`, `flow[id]`, `conductivity[id]`, `melt_at[id]`, and so on. Hot code reads these arrays. It never reads the data file structs.

## 6. Cell simulation

### 6.1 Update order and threads

- Each tick, awake chunks update in 4 passes. Pass *k* updates the chunks where `(chunk_x mod 2, chunk_y mod 2)` equals the *k*-th pair of (0,0), (1,0), (0,1), (1,1). In one pass, two updated chunks always have one chunk between them. So a chunk can write up to 32 cells into its neighbors without a conflict. Noita uses this method.
- A cell moves at most 32 cells in one tick.
- Inside a chunk, cells update from the bottom row to the top row. The direction in each row (left to right, or right to left) changes every tick, so there is no side bias.
- A cell that moved in this tick has its updated bit set, so it does not move twice.
- Random numbers come from a fast generator with a seed made from (world seed, tick, chunk x, chunk y). So the result does not depend on thread timing.
- The parallel pass needs write access to a 3 × 3 area of chunks. A small `unsafe` wrapper gives this access with raw pointers. The wrapper documents the no-overlap rule. Debug builds check it.

### 6.2 Per-cell update

For each cell in the dirty rectangle:

1. Skip empty cells, static solids at rest, and cells already updated.
2. Try a reaction (section 6.4).
3. Try to move (section 6.3).
4. If the cell changed or moved, grow the dirty rectangle of its chunk.

The code has one function per step: `react::try_react` and `movement::try_move`. This lets two agents work on them at the same time.

### 6.3 Movement rules

| Phase | Rule |
|---|---|
| Powder | Try down. Then try down-left or down-right (random order). A friction chance stops the diagonal move; this sets the pile slope. It can move into gas, and it swaps with liquid or powder of lower density. Fall speed grows each tick up to a maximum. |
| Liquid | Try down, then the down diagonals, then sideways up to `flow` cells. It swaps with liquid or gas of lower density. |
| Gas | Moves up if lighter than air and down if heavier, with random sideways moves. It swaps with gases of different density to spread out. Some gases lose life each tick and disappear at zero. |
| Fire | Rises with random side moves. Loses life each tick. Heats its neighbors. Ignites flammable neighbors. |

Extra rules:

- **Drag.** When a liquid cell moves sideways into a light powder cell, the powder moves with the liquid. A powder is light if its density is less than `drag_limit` (a material property). Heavy powders stay. This makes sluices and washers work.
- **Magnets.** A magnet building writes a pull direction into a small field map around it. Magnetic powders prefer to move in that direction.
- **Float stone.** Gravity is reversed for it.
- **Special behaviors.** Each material can name a `behavior` in data. Code for special behaviors is in separate functions, one per behavior.

### 6.4 Reactions

- Each updated cell picks one random neighbor out of 8. The pair (A, B) looks up a dense table: `pair_table[A][B]` gives an index into a list of rules. With 1024 materials the table is 2 MB. It is smaller with fewer materials.
- Each rule has conditions (temperature range, air next to it, room type) and a chance. If the conditions are true and the random number is below the chance, the rule fires.
- Single-material rules (phase changes, burning start, decay) are checked against the cell's own temperature and life.
- Phase changes have a gap (for example: melt at 1085 °C, freeze at 1075 °C) so cells do not flicker between two materials.
- Reactions can send events: explosion, sound, discovery, building damage.

### 6.5 Heat

- The heat pass runs after movement, on awake chunks.
- It uses two buffers. It reads `temp` and writes `temp_next`, then swaps them. Because no thread writes to the read buffer during the pass, all chunks run in parallel in one pass.
- Formula for each cell: `new_T = T + Σ k × (T_neighbor − T)` over 4 neighbors. `k` comes from the conductivity of the two materials and the heat capacity of the cell. `k` is always ≤ 0.2, so the result is stable.
- Temperatures are whole numbers. The pass uses random rounding, so small heat flows are not lost.
- Air cells move slowly toward the air temperature of their depth.
- A chunk skips the heat pass if no cell changed by 1 °C or more in the last 30 ticks and there is no heat source in it.
- Buildings add or remove heat through heat contact ports and through their body cells.
- The loop is simple and fits SIMD. We optimize it after the first version works.

### 6.6 Particles

- Free-flying cells come from explosions, spray, splashes and digging. Each has a position (f32), a velocity, a material, a temperature and a shade.
- Each tick: apply gravity, move, and check for a hit on the grid along the path. On a hit, the particle becomes a grid cell at the nearest free position.
- Visual particles (sparks, dust, smoke puffs) never enter the grid. Only the renderer uses them.

### 6.7 Explosions

- An explosion has a center, a strength and a heat value.
- Cells in the radius with a hardness lower than the strength become particles that fly outward, or turn into smoke and fire. Hard cells stay.
- Buildings and the player in the radius take damage.
- Explosions go into a queue during the tick. One serial step after movement processes the queue, so explosions can cross chunk borders freely. A large chain of explosions is spread over several ticks, so the tick time stays stable.

### 6.8 Parts in the world

- A part is a small box (4 × 4 cells) in a separate list with a spatial hash.
- Parts collide with solid and powder cells and push each other.
- On a belt, a part takes the belt speed. In liquid, it floats or sinks by density.
- A part at rest goes to sleep.
- A part takes the temperature of the cells around it. Above its material's melting point, it becomes molten cells. A flammable part burns.

### 6.9 Rigid bodies

Not in the first versions. (Noita has falling rigid bodies. We can add them later.)

## 7. Factory systems

### 7.1 Tick order

1. Apply commands (build, remove, dig, spray, settings).
2. Update the player (movement, tool actions).
3. Update the factory:
   1. Networks: power, fluid, item tubes, signals.
   2. Machines: advance recipes; use and fill buffers.
   3. Ports and belts: take cells from input areas, write cells to output areas, move cells on belts.
   4. Room machines: read room state from their cells.
4. Cell simulation: movement and reactions (4 passes), explosion queue, heat.
5. Particles and parts.
6. Damage: apply simulation events to buildings (heat, corrosion, explosions).
7. Write the snapshot for the renderer.

Step 3.3 touches only cells near each building. At first it runs on one thread. If it becomes slow, it can run in the same 4-pass chunk groups as the cell simulation.

### 7.2 Buildings

- Buildings are in a slot map with generation-checked ids.
- A building type (from data) has: size, ports, body material, hit points, maximum temperature, tier, recipes, and a logic kind.
- Placing a building writes its body cells into the world as a solid "body material" with the building-body flag.
- Rotation and flip transform the port positions.
- Machine logic is grouped by kind (crafter, boiler, pump, belt, hopper, sorter, lab, and so on). Each kind is a system that runs over an array of its buildings. We do not use one virtual call per building.

### 7.3 Networks

- Networks are graphs built from the back-layer tile grid. When a piece changes, only the network that contains it is rebuilt.
- **Power.** Each network, each tick: add all supply and all demand. Satisfaction = min(1, supply ÷ demand). Machines run at that speed. Batteries fill or empty. The voltage check happens when a connection changes. The current limit of a network is the limit of its weakest cable (game design section 14).
- **Fluid.** Each connected pipe system is one fluid box: fluid type, amount, temperature, capacity (the sum of its pipe capacities). Inputs push into it. Outputs pull from it. The maximum flow per tick depends on the pipe type and the system length. This is similar to the Factorio 2.0 fluid system. It is cheap and stable.
- **Item tubes.** A graph of tube nodes. Packets (a stack of parts) move along edges at tube speed. Each packet goes to the nearest connection that accepts it. Routes are cached per network and rebuilt when the network changes.
- **Signals.** Each wire network, each tick: value = sum of the sensor outputs. Consumers read the value from the previous tick.

### 7.4 Room machines

- The controller fills outward from the inside tile next to it, up to the maximum room size. It checks that the walls are closed and of the correct block, and it lists the port blocks in the walls.
- Room walls are on the tile grid, so the room is a set of tiles. Each inside tile stores the room id.
- Reactions look up the room id of the tile to apply the room bonus.
- The room is checked again when a wall tile changes, and every 2 seconds.
- Room statistics (average temperature, amount of each material, levels) are computed every 10 ticks.

### 7.5 Research, milestones, statistics

- Research state: finished technologies, current technology, progress, discoveries.
- Statistics: counters per material per tick, collected into ring buffers for 1 minute, 10 minutes, 1 hour and 10 hours.

## 8. Rendering

### 8.1 World

- The GPU holds a texture array. Each visible chunk gets one 64 × 64 layer. The format is `Rgba16Uint`: red = material, green = temperature, blue = shade and life, alpha = flags.
- Each tick, only the changed chunks upload.
- The world shader draws one quad per visible chunk (instanced). For each cell it:
  1. Gets the base color from a palette texture (material × shade).
  2. Adds a small color shift over time for liquids.
  3. Adds a hot glow from the temperature above about 500 °C, from a black-body color lookup texture.
  4. Writes an emission value for the light pass.
- The world is drawn at 1 texel per cell into an offscreen target. Then it is scaled to the screen with sharp bilinear filtering (nearest-pixel look with smooth edges), so all zoom levels look clean.

### 8.2 Light

- The light map is at half or quarter resolution.
- Light sources: emissive cells, hot cells, sky light on the surface (blocked by solid cells), the robot's lamp, machine lights.
- First version: put the emission into the light map. Then run several blur passes in which solid cells block light.
- Later: radiance cascades for better light and shadows.
- Final color = base color × (ambient + light) + emission. Then bloom.

### 8.3 Other passes

- Back layer (pipes, cables, tubes, wires): a sprite batch behind the cell layer. In the back-layer view, cells are drawn partly transparent.
- Buildings: a sprite batch with a texture atlas. The animation frame comes from the building state.
- Parts and particles: instanced quads.
- Gas: drawn with transparency.
- Heat shimmer: a screen offset map from temperature.
- Overlays: heat, gas, power, dirty rectangles and chunk states (debug).
- Background: parallax images for each world layer.
- UI: egui on top of everything.

### 8.4 Map view

Each chunk keeps a small average-color image (16 × 16). The map view draws these images. A chunk updates its image when it changes.

## 9. Content data

- Data files are RON files in `assets/data/`, split into many small files by topic (for example `materials/metals.ron`). Small files reduce merge conflicts between agents.
- Ids in files are strings (`"molten_copper"`). The loader gives each one a `u16` id and builds the flat lookup arrays.
- The loader checks everything: each reference exists, reaction outputs are valid, ids are unique. Each error names the file and the line.
- In debug builds, the game reloads data files when they change. Colors, properties, reactions and recipes update without a restart.

Example material:

```ron
Material(
    id: "molten_copper",
    name: "Molten copper",
    phase: Liquid,
    density: 8000,
    flow: 3,
    colors: ["#ff8a3d", "#ff7a2a", "#ffa04f"],
    heat_capacity: 0.39,
    conductivity: 0.8,
    freeze: Some((below: 1075, into: "copper_block")),
    tags: ["metal", "molten", "conductive"],
)
```

Example reaction:

```ron
Reaction(
    a: "water",
    b: "lava",
    chance: 0.5,
    into_a: "steam",
    into_b: "obsidian",
)
```

## 10. Saves

- A save has: a header with a version, the id tables (string → number), the world seed, the tick, the chunks, buildings, networks, player, research and statistics.
- Chunks that the player never changed are not saved. They are made again from the seed.
- Chunks are compressed with lz4.
- Autosave runs on a worker thread from a copy of the state, so the game stops only for a very short time.

## 11. Tests and tools

| Tool | Purpose |
|---|---|
| Unit tests | One test for each movement rule and each reaction, on small grids. |
| Scene tests | A small PNG where each color is a material (a RON file next to it lists the colors). The test runs N ticks and checks conditions: the amount of a material in an area, a temperature range, a building status. Agents can make new scenes easily. |
| Determinism test | Run a scene with 1 thread and with 8 threads. The world hash must be the same. |
| Benchmarks | Headless scenes: sand rain, ocean, forest fire, lava and water, large factory. The runner prints ms per tick. A benchmark fails if it is more than 10% slower than the saved baseline. |
| Screenshots | The headless program renders scenes to PNG files. People and agents can look at them. |
| In-game debug | Material brush (paint any material), pause and step one tick, overlays (dirty rectangles, chunk states, heat), a timings panel (ms per system), free buildings. |

## 12. Performance rules for all code

- No heap allocation in per-cell code.
- Flat arrays and number ids. No `Rc<RefCell<…>>` in hot paths.
- Read material properties from the flat arrays.
- Do work only in awake chunks and inside dirty rectangles.
- Measure before and after each optimization with the benchmark runner.
- The main thread never does simulation work.

## 13. Risks

| Risk | Plan |
|---|---|
| The heat pass is too slow. | Chunks skip heat when stable. SIMD. Run heat at 30 Hz with a double step. Move heat to a GPU compute shader if needed. |
| Belts and machines keep too many chunks awake. | Belts move cells once every few ticks. Ports exchange cells in batches. A chunk with an empty belt sleeps. |
| Physical logistics becomes messy for the player. | Sealed transport from Tier 1. Cleanup tools (vacuum, sweeper drones). Clear alerts. |
| Room machines are hard to balance. | The controller can fix some values, for example the wall temperature at each height in the distillation tower. |
| Too much scope. | Build the first playable version first (game design section 21). Content is data, so many agents can add content in parallel. |
