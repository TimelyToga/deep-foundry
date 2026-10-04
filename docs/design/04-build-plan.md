# TimTech: build plan

## 1. How we work

1. **Milestone 0 is serial.** One agent (or I) builds the skeleton: crate layout, core types, the main loop, and the first working sand. This sets the interfaces and the code style. Parallel work before this point is wasted, because each agent would make different choices.
2. **After Milestone 0, agents work in parallel.** Each agent works in its own git worktree, on one crate or one clear area.
3. **Each task has an owner area, tests and a done condition.** An agent changes only the files in its area. If it needs a change to a shared interface, it writes the request in `docs/design/interface-requests.md` and uses a local stub until the change is made.
4. **An integration step follows each wave of work.** It merges the branches, runs all tests and benchmarks, starts the game, and takes screenshots.
5. **A review step follows integration.** It checks correctness, the performance rules (technical design section 12), and code quality.
6. **You play-test at the end of each milestone.** Your notes decide the next changes.

Before Milestone 0:

- `git init` in the project folder. Worktrees need git.
- `rustup update stable`.

## 2. Milestones

### Milestone 0: skeleton (serial, 1 agent)

- Workspace and all crates (most are empty at first).
- `core` types (section 3).
- `content` loader with 6 materials: air, sand, water, stone, wood, fire.
- Window with wgpu. Simulation thread with a fixed 60 ticks per second.
- World of chunks. Simple single-thread movement for powder and liquid.
- Chunk texture upload and the world shader (flat palette colors).
- Camera: pan and zoom.
- Material brush: paint and erase with the mouse.
- Headless program: runs a scene and prints ms per tick.
- One scene test.

Done when:

- `cargo run -p game` opens a window. Painted sand falls and makes piles. Water flows and becomes level.
- The simulation runs at 60 ticks per second.
- `cargo test` passes.
- `cargo run -p headless -- bench sand_rain` prints timings.

### Milestone 1: simulation core (6 agents in parallel)

| Task | Owner area | Work | Done when |
|---|---|---|---|
| 1A Movement and threads | `sim/movement`, `sim/schedule` | All phases, density swaps, drag, 4-pass parallel update, dirty rectangles, chunk sleep, deterministic random numbers | 1,500 awake chunks under 5 ms per tick; determinism test passes |
| 1B Heat | `sim/heat` | Two-buffer heat pass, conductivity, air temperature, phase changes with a gap, heat skip for stable chunks | Scene: a metal bar heated at one end warms along its length; ice melts next to lava; under 2 ms per tick |
| 1C Reactions, fire, explosions, particles | `sim/react`, `sim/explode`, `sim/particles` | Pair table, rule conditions, fire rules, explosion queue, flying particles | Scenes: water + lava gives obsidian and steam; wood burns to ash; methane explodes; charcoal forms with no air |
| 1D Renderer | `render/` | Palette with shades, liquid color shift, hot glow, light map with blocking, bloom, sharp scaling | Screenshots: glowing lava in a dark cave; molten copper turns dark as it cools |
| 1E Content data | `content/`, `assets/data/` | Full data schema, validation, hot reload, about 60 materials and 60 reactions for Tier 0 and Tier 1 | The loader passes; each material has a scene or unit test |
| 1F Tools | `headless/`, `game/debug` | Scene tests from PNG, determinism test, benchmark suite with baselines, debug overlays, timings panel | The suite runs with one command |

Shared function: M0 creates the per-cell update with calls to `react::try_react` and `movement::try_move`. Task 1A owns the loop and `try_move`. Task 1C owns `try_react`.

### Milestone 2: world and player (4 agents)

| Task | Owner area | Work |
|---|---|---|
| 2A World generation | `worldgen/` | Layers, biomes, caves, ore veins, rivers with tin gravel, lakes, trees, oil pockets, the start area rules |
| 2B Player movement | `game/player` | Movement, collision with cells, jetpack with real exhaust, damage, camera follow |
| 2C Tools and inventory | `game/tools`, `game/inventory` | Dig, spray, scan, ignite, material tank, part slots, hotbar, hand crafting |
| 2D Chunk memory and saves | `sim/storage`, `game/save` | Pack and unpack chunks, save and load the world and the player |

### Milestone 3: factory core (6 agents; 3A first)

| Task | Owner area | Work |
|---|---|---|
| 3A Building system (first) | `factory/buildings` | Tile grid, placement, body cells, rotation, ports, damage, release of contents |
| 3B Construction | `game/build` | Ghosts, validity and reasons, drag lines, pick, undo and redo, remove, copy and paste, back-layer view |
| 3C Bulk and parts logistics | `factory/logistics`, `sim/parts` | Belts, hoppers, chutes, splitter, sorter, screen, magnet, fan, parts in the world, arms, crates |
| 3D Networks | `factory/networks` | Pipes, pumps, outlets, drains, valves, tanks, power, signals |
| 3E Machines | `factory/machines` | Recipe system, crafter systems, boilers, steam machines, status reasons, overclocking |
| 3F Interface | `ui/` | HUD, machine panel, recipe browser, alerts |

### Milestone 4: Tier 0 and Tier 1 content (5 agents)

| Task | Owner area | Work |
|---|---|---|
| 4A Room machines | `factory/rooms` | Room check, room bonus, kiln, coke oven, blast furnace, large tank |
| 4B Content | `assets/data/` | All Tier 0 and Tier 1 buildings, parts and recipes; first balance pass |
| 4C Research | `factory/research`, `ui/tech` | Labs, kits, tech tree data and screen, Hub milestones, discovery, guide |
| 4D Hazards | `sim/`, `factory/` | Gas pockets, floods, pollution and acid rain, boiler explosion, pipe failure |
| 4E Statistics and map | `factory/stats`, `ui/` | Production statistics, power graphs, map view |

Done when: a scripted test plays from the start to milestone stage 2 with debug shortcuts, and you finish a manual play-test.

### Milestone 5: polish

Animation pass, better art, sound, blueprint library, a full performance pass.

### Milestone 6 and later

Tier 2 (LV) and the Deep Rock layer, then Tiers 3 to 5 and their layers.

## 3. Shared interfaces (made in Milestone 0)

These are sketches. Milestone 0 makes the real versions.

```rust
// core
pub const CHUNK: i32 = 64;
pub const TILE: i32 = 8;
pub const TICKS_PER_SECOND: u32 = 60;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct MaterialId(pub u16);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellPos { pub x: i32, pub y: i32 }   // x right, y down

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TilePos { pub x: i32, pub y: i32 }

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkPos { pub x: i32, pub y: i32 }

pub enum Command {
    Paint { at: CellPos, radius: u8, material: MaterialId },
    Dig { at: CellPos, radius: u8 },
    Spray { at: CellPos, material: MaterialId, amount: u16 },
    Place { kind: BuildingKindId, at: TilePos, rotation: u8, flip: bool },
    Remove { at: TilePos },
    SetRecipe { building: BuildingId, recipe: RecipeId },
    PlayerInput(PlayerInput),
    // more in later milestones
}

pub struct Snapshot {
    pub tick: u64,
    pub changed_chunks: Vec<ChunkImage>,  // only chunks in the view area
    pub player: PlayerView,
    pub particles: Vec<ParticleView>,
    pub parts: Vec<PartView>,
    pub buildings: Vec<BuildingView>,     // in the view area
    pub tiles: TileView,                  // occupancy near the view, for ghost checks
    pub alerts: Vec<Alert>,
    pub stats: StatsView,
    pub timings: Timings,
}
```

## 4. Instructions that every agent gets

- Read `docs/design/README.md` and the design sections named in the task.
- Change only the files in your owner area.
- Follow the performance rules in technical design section 12.
- Write tests first where possible. Add a scene test for each visible behavior.
- Run `cargo test`, `cargo clippy` and the benchmarks for your area before you finish.
- Write comments and docs in simple English: short sentences, no metaphors, no new jargon.
- At the end, report: what you did, test results, benchmark numbers, open problems, and interface requests.

## 5. Workflow shape

Each milestone after Milestone 0 runs as one workflow:

1. **Build.** One agent per task, in parallel, each in its own worktree.
2. **Integrate.** One agent merges the branches in a fixed order, fixes conflicts, runs all tests and benchmarks, and takes screenshots.
3. **Review.** Two or three agents review the merged code: correctness, performance, simplicity. They report findings.
4. **Fix.** Agents fix the confirmed findings.
5. **Report.** A short summary for you: what works, screenshots, benchmark numbers, known problems.

## 6. Risks of parallel work

| Risk | Plan |
|---|---|
| Merge conflicts in shared files | Small data files by topic. Clear owner areas. Shared functions made as stubs in Milestone 0. |
| Different code styles | Milestone 0 sets the examples. The review step checks style. |
| Parts that work alone but fail together | The integration step runs the full game and all scene tests after each wave. |
| Slow code that nobody notices | Benchmarks with baselines run in the integration step. |
