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

"New game" offers two modes. The UI is `crates/ui` (see `docs/design/ui.md`).

- **Normal game**: the robot, the factory, research and the Hub. The broken Hub (the landing pod)
  stands at the start, and the robot stands next to it. The start inventory is 30 wood and a
  crate. Near the start there is clay (right of the robot, and at the left bank of the pool),
  malachite (copper ore, left of the Hub) and a gravel bed with cassiterite (tin ore, before the
  dune). The camera follows the robot.
- **Sandbox**: no robot and no factory (like the Factorio cheat mode). The materials window (E)
  has every material with no limit, and the material in the hand is the paint brush. The camera
  moves freely.

## Controls of the normal game

| Input | Action |
|---|---|
| A / D | Walk left / right. The robot walks up low steps. It cannot pass solid cells or powder; it wades through liquids. |
| W or Space | Jump. Hold it in the air: a small jetpack (the fuel fills on the ground). In a liquid: swim up. |
| Left mouse (hold) | Dig the cells at the mouse (in reach). Dug cells go into the material tanks as their broken form. Stone is too hard at the start. |
| Right mouse (hold) | Spray material from the tank at the mouse. Click a tank slot (or a material on the quickbar) to choose the material; else the first tank is used. |
| F (hold) | Scan the material under the mouse. The first scan of a material discovers it. |
| Building in the hand | A ghost follows the mouse: green if it can be placed, red with the reason. Left click places it. R rotates it. |
| Left click on a building (empty hand) | Open its window. |
| Right click on a building | Take it back into the inventory (not the Hub). |
| Q | Empty the hand |
| E | Character screen: inventory, tank and hand crafting |
| T | Research |
| G | Guide (goals for each tier) |
| 1 to 0, Shift + 1 to 0 | Quickbar: take that building into the hand, or choose that spray material |
| Mouse wheel | Zoom |
| Esc | Close the top window, or open the pause menu |
| F3 | Debug panel |

The Hub takes the items of the next repair stage: open it and shift + click stacks from the
inventory into it. New buildings go to a free quickbar slot the first time you get them.

## Controls of the sandbox

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

Each save is `<name>.dfworld` (the world) and `<name>.info` (seed, size, ticks). A normal game
also has `<name>.dfgame` (the factory, the robot and the quickbar, as RON text). A save without
it loads in the sandbox mode. `--saves DIR` uses another folder. "Continue" in the main menu loads
the newest save.

## Options

```
--seed N               World seed (default 1)
--depth N              Depth below the surface in chunks of 64 x 64 cells (default 128)
--world WxH            A finite world of W x H chunks with bedrock walls (for tests)
--size WxH             Window size in screen pixels
--saves DIR            Folder for saved games
--ui-state STATE       Start screen: menu, newgame, load, settings, pause, save, playing, inventory, debug;
                       normal mode only: building, ghost, hub, research, guide
--mode MODE            Mode of a world that --ui-state starts: sandbox (default) or normal
--ui-scale S           Size of the UI, 0.75 to 2
--exit-after SECONDS   Quit after this time and print the average FPS and frame time (starts in the game)
--no-vsync             Do not wait for the display refresh (to measure the highest FPS)
--smoke-test           Play the sandbox (new game, paint, pause, save, load, quit to menu, continue,
                       delete) and the normal game (new game, dig clay, hand craft, place a
                       workbench, open its window, save, load, delete), then quit
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
- `--mode normal` (or a normal-mode screen) makes a normal game: the Hub, the robot and the factory
  tick with the cells. The screens `inventory`, `building` (a crate), `ghost` (a workbench in the
  hand), `hub` and `guide` put a few items into the inventory first, so the picture shows them.
  `--walk N` lets the robot walk N ticks first (N < 0: to the left).

```sh
cargo run -p deep_foundry -- --screenshot out/hub.png --size 2560x1440 --mode normal --ui-state hub --ticks 60
```
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
