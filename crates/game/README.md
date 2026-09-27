# deep-foundry (the game program)

## Run the game

```sh
cargo run -p deep_foundry --release
```

The game opens a window with a demo world: stone ground with hills, a dirt layer, a sand dune,
a water pool, a small lava pocket and wooden posts. A ball of water falls into the pool and the
steep side of the dune slides at the start.

The world has no limit to the left and right. The hills go on without end, and the pool, dune,
lava pocket and posts repeat every 2048 cells. New chunks are made when the view comes near them.
Only chunks near the view update; far chunks wait until the view comes back.

The simulation runs on its own thread at 60 ticks per second. The window draws at the display
refresh rate.

## Controls

| Input | Action |
|---|---|
| W A S D or arrow keys | Move the camera (hold Shift to move faster) |
| Middle mouse drag | Move the camera |
| Mouse wheel | Zoom toward the mouse (1 to 8 screen pixels per cell) |
| Left mouse | Paint the selected material |
| Right mouse | Erase (paint air) |
| `[` and `]` | Smaller or larger brush |
| 1 to 9 | Select a material (the numbers are shown in the panel) |
| Click a material in the panel | Select it |
| Space | Pause or resume the simulation |
| `.` (period) | Run one tick while paused |
| F3 | Show or hide the stats |

## Options

```
--seed N               World seed (default 1)
--depth N              Depth below the surface in chunks of 64 x 64 cells (default 128)
--world WxH            A finite world of W x H chunks with bedrock walls (for tests)
--size WxH             Window size in screen pixels
--exit-after SECONDS   Quit after this time and print the average FPS and frame time
--no-vsync             Do not wait for the display refresh (to measure the highest FPS)
--help                 Show all options
```

## Screenshots with no window

```sh
cargo run -p deep_foundry -- --screenshot out/demo.png --ticks 120 --zoom 3 --size 1600x900
```

This builds the demo world, runs the ticks on the calling thread, draws one frame with the real
renderer into an offscreen texture, and saves a PNG file.

- `--center X,Y` sets the world cell at the image center. Any x works, for example
  `--center 64000000,1050`.
- `--ui` also draws the side panel.

## Measure the frame rate

```sh
cargo run -p deep_foundry --release -- --exit-after 10 --size 2560x1440
cargo run -p deep_foundry --release -- --exit-after 10 --size 2560x1440 --no-vsync
```

The program prints the average FPS and frame times. The numbers "after the first second" leave out
the window opening and the first upload of all visible chunks. For a debug log of slow frames, set
`RUST_LOG=deep_foundry=debug`.

## Source files

| File | Contents |
|---|---|
| `main.rs` | Reads the options and starts the window or the screenshot. |
| `args.rs` | Command line options. |
| `app.rs` | The window: events, the wgpu surface, egui, and the frame loop. |
| `controls.rs` | Camera movement and brush strokes (no window code, with unit tests). |
| `ui.rs` | The egui side panel. |
| `sim_thread.rs` | Runs the simulation at 60 ticks per second on its own thread. |
| `demo.rs` | The starting world. |
| `screenshot.rs` | `--screenshot` mode. |

The renderer is in `crates/render` and the shaders are in `assets/shaders/`. The renderer reads the
shader files at startup, so you can change a shader and start the game again without a new build.
