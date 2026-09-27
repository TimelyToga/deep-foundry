# deep-foundry (the game program)

## Run the game

```sh
cargo run -p deep_foundry --release
```

The game starts in the main menu. "New game" makes a demo world: stone ground with hills, a dirt
layer, a sand dune, a water pool, a small lava pocket and wooden posts. A ball of water falls into
the pool and the steep side of the dune slides at the start.

The world has no limit to the left and right. The hills go on without end, and the pool, dune,
lava pocket and posts repeat every 2048 cells. New chunks are made when the view comes near them.
Only chunks near the view update; far chunks wait until the view comes back.

The simulation runs on its own thread at 60 ticks per second. The window draws at the display
refresh rate.

There is no player and no factory yet, so the game runs in the **sandbox mode** (like the Factorio
cheat mode): the materials window (E) has every material with no limit, and the material in the
hand is the paint brush. The UI is `crates/ui` (see `docs/design/ui.md`).

## Controls

| Input | Action |
|---|---|
| Left mouse | Paint with the material in the hand |
| Right mouse | Erase (paint air) |
| `[` and `]` | Smaller or larger brush |
| E | Materials window: click a material to put it in the hand |
| 1 to 0, Shift + 1 to 0 | Quickbar materials (key 0 is air, the eraser) |
| Click a quickbar slot | Put that material in the hand. Holding a material, click an empty slot to put it there. Right click clears a slot. |
| Q | Empty the hand |
| W A S D or arrow keys | Move the camera (hold Shift to move faster) |
| Middle mouse drag | Move the camera |
| Mouse wheel | Zoom toward the mouse (1 to 8 screen pixels per cell) |
| Space | Pause or resume the simulation (no menu) |
| `.` (period) | Run one tick while paused |
| Esc | Close the top window, or open the pause menu (Resume, Save, Load, Settings, Quit) |
| F3 | Debug panel: simulation controls, chunk overlay, numbers |

The HUD shows the frame rate, the tick time and the awake chunks at the top right, and the cell
under the mouse (material and temperature) below them.

## Saves

Saves are in the app data folder of the system:

- macOS: `~/Library/Application Support/DeepFoundry/saves`
- Windows: `%APPDATA%\DeepFoundry\saves`
- Linux: `~/.local/share/DeepFoundry/saves`

Each save is `<name>.dfworld` (the world) and `<name>.info` (seed, size, ticks). `--saves DIR`
uses another folder. "Continue" in the main menu loads the newest save.

## Options

```
--seed N               World seed (default 1)
--depth N              Depth below the surface in chunks of 64 x 64 cells (default 128)
--world WxH            A finite world of W x H chunks with bedrock walls (for tests)
--size WxH             Window size in screen pixels
--saves DIR            Folder for saved games
--ui-state STATE       Start screen: menu, newgame, load, settings, pause, save, playing, inventory, debug
--ui-scale S           Size of the UI, 0.75 to 2
--exit-after SECONDS   Quit after this time and print the average FPS and frame time (starts in the game)
--no-vsync             Do not wait for the display refresh (to measure the highest FPS)
--smoke-test           Play new game, paint, pause, save, load, quit to menu, continue, delete, quit
--help                 Show all options
```

## Screenshots with no window

```sh
cargo run -p deep_foundry -- --screenshot out/demo.png --ticks 120 --zoom 3 --size 2560x1440 --ui-state inventory
```

This builds the demo world, runs the ticks on the calling thread, draws one frame with the real
renderer and the UI into an offscreen texture, and saves a PNG file.

- `--ui-state` picks the screen (default `playing`). The UI runs several frames first, so the fonts
  and the window sizes are ready.
- `--no-ui` draws only the world.
- `--center X,Y` sets the world cell at the image center. Any x works, for example
  `--center 64000000,1050`.

## Measure the frame rate

```sh
cargo run -p deep_foundry --release -- --exit-after 10 --size 2560x1440
cargo run -p deep_foundry --release -- --exit-after 10 --size 2560x1440 --no-vsync
```

## Check the menus and saves

```sh
cargo run -p deep_foundry -- --smoke-test --saves /tmp/deep-foundry-smoke
```

It prints each step and "smoke test passed", or exits with code 1.
