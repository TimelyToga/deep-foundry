# foundry_headless

Scene tests, benchmarks and PNG pictures of the cell world. No window and no GPU.

## Commands

Run from the workspace folder. Use `--release` (debug builds are slow and cannot save baselines).

```
cargo run -p foundry_headless --release -- test [filter]
cargo run -p foundry_headless --release -- scene <name> [--ticks N] [--png out.png] [--scale S] [--every K] [--heat]
cargo run -p foundry_headless --release -- bench [name] [--ticks N] [--repeat N] [--save-baseline] [--compare]
cargo run -p foundry_headless --release -- determinism <scene> [--ticks N] [--every K]
```

| Command | What it does |
|---|---|
| `test` | Runs every scene in `assets/scenes/tests/` (only names that contain `filter`). Prints one block per scene and one row per check. Exit code 1 if a check that is not pending fails. |
| `scene` | Runs one scene and its checks. Writes a PNG of the whole world after the last tick (default `out/<name>.png`). Each cell gets the first color of its material. `--scale S` makes each cell S × S pixels (default: fits in 1024 pixels). `--every K` also writes `out/<name>_t000100.png` and so on every K ticks. `--heat` draws temperatures (20 °C is near black, cold is blue, hot is red to yellow to white). `--ticks N` replaces the scene's tick count. |
| `bench` | Runs the benchmark worlds (all, or one by name) and prints ms per tick (mean, p50, p95, max) and awake chunks. `--repeat N` runs each benchmark N times and keeps the run with the lowest mean (default 3 with `--save-baseline` or `--compare`, else 1). `--save-baseline` writes `bench/baseline.ron`. `--compare` exits with code 1 if a benchmark's mean is more than 10% above its baseline. Only the same tick count is compared. |
| `determinism` | Runs a scene twice with the same seed and prints the world hash every K ticks (default 100) for both runs. |

`<name>` is a scene name in `assets/scenes/` or `assets/scenes/tests/`, or a path to a `.ron` file.

`cargo test -p foundry_headless` also runs all scene tests (`tests/scenes.rs`).

## Make a scene

A scene is two files with the same name: `<name>.png` and `<name>.ron`. Put test scenes in `assets/scenes/tests/`. The `test` command runs them all.

1. Draw the PNG in any paint program, or in code (see `examples/make_test_scenes.rs`, run it with `cargo run -p foundry_headless --example make_test_scenes`). One pixel is one cell. Transparent pixels and pure black (`#000000`) pixels are air. Keep scenes small (about 64 to 200 pixels on a side), so the tests stay fast.
2. Write the RON file:

```ron
// Say what the picture has, with pixel positions, so that others can check the rectangles.
Scene(
    note: "Sand falls and forms a pile; nothing is lost.",
    legend: {
        "#d9c38c": "sand",                                   // color -> material id
        "#ff5a1a": (material: "lava", temperature: 1500),    // with a start temperature (°C)
    },
    ticks: 500,                 // ticks to run before the checks
    // Optional, with their defaults:
    // size: Fit,               // Fit, or Chunks(width, height)
    // margin: 16,              // air cells around the image when size is Fit
    // at: (x: 10, y: 10),      // world position of the top-left pixel; default: image in the center
    // seed: 1,
    // bedrock_border: true,    // bedrock on the left, right and bottom world edges (2 cells)
    checks: [
        TotalUnchanged(material: "sand"),
    ],
)
```

A color that is not in the legend is an error. The error names the color and the pixel.
Any material id from `assets/data/materials/*.ron` works in the legend.

## Checks

Checks run after the ticks. Positions and rectangles are in image pixels: (0, 0) is the top-left pixel of the PNG. A rectangle is `(x: 0, y: 40, w: 96, h: 20)` (top-left corner, width, height).

| Check | Passes when |
|---|---|
| `Count(material: "sand", rect: (x: 0, y: 40, w: 96, h: 20), min: 200, max: 240)` | The number of cells of the material in the rectangle is in the range. Use `exact: 240`, or `min`, `max`, or both. |
| `Total(material: "smoke", exact: 0)` | The number of cells of the material in the whole world is in the range (`exact`, `min`, `max`). |
| `TotalUnchanged(material: "water")` | The world has as many cells of the material at the end as at the start. Nothing is lost. |
| `CellIs(at: (x: 48, y: 59), material: "sand")` | The cell at the pixel has the material. |
| `Temperature(material: "iron", rect: (x: 0, y: 0, w: 10, h: 4), min: 100, max: 500, of: Mean)` | The temperature of the cells is in the range. `material` and `rect` are both optional: without `material`, all cells in the rectangle; without `rect`, the whole world. `of: Mean` (default) tests the mean. `of: Each` tests every cell. Give `min`, `max`, or both. |
| `NoChange(last_ticks: 60)` | The world hash did not change in the last 60 ticks. The hash includes materials and temperatures. |
| `Deterministic(every: 100)` | A second run of the scene with the same seed, on a rayon pool with one thread, gives the same world hash every 100 ticks and at the end. |

Add `pending: true` to any check for behavior that is not built yet (for example heat or reactions). A pending check runs and shows in the table, but it does not fail the test. When a pending check passes, the table says `pending, passes`: then remove `pending: true`.

## Benchmarks

The benchmark worlds are made in code in `src/bench.rs`. Only `Simulation::tick` is timed.

| Name | World |
|---|---|
| `sand_rain` | 32 × 8 chunks. 256 sand cells are added near the top every tick over a wide area. |
| `ocean` | 32 × 16 chunks. About 1 million water cells with air on the right side, as if a wall was just removed. |
| `lava_water` | 16 × 8 chunks. Lava and water flow toward each other and meet. |
| `pile_collapse` | 16 × 16 chunks. A sand column 200 cells wide and 958 cells tall falls into a pile. |
| `settled_world` | 64 × 32 chunks. Stone ground, a flat sand layer and closed water caves. Only one sand cell per tick moves. This shows the cost of chunks where nothing moves. |
| `mixed` | 64 × 32 chunks. All of the above in one world. About 1,500 chunks with material. |

`bench/baseline.ron` records the machine that made it. Numbers from different machines do not compare well: save a new baseline on your machine before you use `--compare`. Other programs that use the CPU (for example other builds) make the numbers slower. Save and compare baselines when the machine is quiet.

## Use as a library

Other crates and tests can use `foundry_headless` directly:

- `Scene::find(name, &content)` or `Scene::load(path, &content)`, then `scene.build_sim(content)`.
- `runner::run_scene(&scene, &content, None, &mut |sim, tick| { ... })` returns the simulation and a `SceneReport`.
- `image::render_cells(&sim, area, scale).save_png(path)` writes a picture.
- `bench::find("ocean").unwrap().make(&content)` gives the benchmark world, for profiling.
