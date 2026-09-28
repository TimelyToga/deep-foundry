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

The keys are the defaults. Settings > Controls changes them (see "Keys and settings" below).

| Input | Action |
|---|---|
| A / D | Walk left / right. The robot walks up and down steps of up to 3 cells and stays on the ground. It cannot pass solid cells or powder; it wades through liquids. |
| W or Space | Jump. Release it early for a lower jump. A press just before landing, or just after walking off a ledge, still jumps. Hold it in the air: the jetpack (see below). In a liquid: swim up; with the head out of the liquid: jump out. Buried in sand: climb up. |
| Left mouse (hold) | Dig the cells at the mouse (in reach). Dug cells go into the material tanks as their broken form. Stone is too hard at the start. |
| Right mouse (hold) | Spray material from the tank at the mouse. Click a tank slot (or a material on the quickbar) to choose the material; else the first tank is used. |
| F (hold) | Scan the material under the mouse. The first scan of a material discovers it. |
| Left click on a building (empty hand) | Open its window (in reach). The window closes when the robot walks out of reach. |
| Q | On a building: take that building kind from the inventory into the hand, with the building's rotation, flip and recipe (the pipette). Elsewhere: empty the hand. |
| E | Character screen: inventory, tank and hand crafting |
| T | Research. A technology that waits for earlier technologies has a Queue button: it queues them first. |
| G | Guide (goals for each tier) |
| P | Production statistics |
| 1 to 0, Shift + 1 to 0 | Quickbar: take that building into the hand, or choose that spray material |
| Mouse wheel | Zoom |
| Esc | Close the top window, or open the pause menu |
| F3 | Debug panel |

### The robot and the jetpack

- The robot body is 8 x 16 cells. It is drawn as pixel art on the cell grid (one art pixel is one
  cell), in a frame of 16 x 20 cells: the antenna, the backpack and the tool stick out of the
  body. See "The robot picture" below.
- The jetpack has fuel for 50 ticks (a little less than one second) of flight. On the ground the
  fuel fills again after a wait of 20 ticks.
- A bar behind the robot shows the fuel while it is not full: orange while the jetpack runs,
  yellow in the air, "Jet empty" and a red blinking frame when it is empty, gray while it waits
  on the ground, cyan with an arrow while it fills.
- While a tool is used (dig, spray, scan), the robot turns to the aim point and points its tool
  arm at it. Digging shows a beam, sparks and the dug cells that fly to the tool.
- All movement numbers are in the "Tuning" block at the top of `src/player.rs`.

### Building (as in Factorio)

| Input | Action |
|---|---|
| Building in the hand | The tile grid shows. A ghost follows the mouse: the building's shape, its ports (arrows: yellow bulk, blue fluids, white parts, orange heat and exhaust; dots: pipes and power), green if it can be placed, red with the reason next to the mouse. |
| Left mouse | Place the building. Hold and move: a line along the first direction of the move, one footprint per step. Belts in a sideways line face the line direction. The line stops at the first place that fails (the red ghost shows why). |
| R / Shift + R | Turn the building in the hand (belts: left or right). With an empty hand: turn the building under the mouse. Each building kind keeps its rotation for the next placement. |
| F | Flip the building in the hand, if its mirror image is different (with no building in the hand F scans). |
| Right mouse on a building (hold) | Remove buildings: each building under the path of the mouse is taken, one after another. Each takes a short time (a ring shows the progress). Items and contents go into the inventory. The Hub cannot be removed. |
| Ctrl + Z / Ctrl + Y (Ctrl + Shift + Z) | Undo / redo the last place, remove or turn action. A drag line or a remove drag is one action. Undo of a removal needs the building item in the inventory. |
| Shift + right click / Shift + left click | Copy the recipe of a machine / paste it to another machine of the same kind. |
| Alt | Alt mode on or off: the product of each machine's recipe over it, and belt directions. Machines that do not work always show a "!" icon. |
| Mouse over a building | A thin outline. The hover box at the top center shows its name, status and reason, recipe and hit points. Over a cell: the material, its state, temperature, "Can dig" or what is needed, what it breaks into, and "Not discovered" if it was never scanned. |

Placement and removal work only in reach of the robot (80 cells, 10 tiles). Out of reach, the
ghost says "Out of reach". In machine windows, a click on an input slot with an empty hand takes
the items back (right click: half; Shift + click: into the inventory).

The Hub takes the items of the next repair stage: open it and shift + click stacks from the
inventory into it. New buildings go to a free quickbar slot the first time you get them.

## Controls of the sandbox

The keys are the defaults (see "Keys and settings").

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

## Keys and settings

- Every game key is a binding in one table (`src/keys.rs`): action -> keys. Esc and the mouse
  buttons are fixed.
- Settings > Controls lists the keys of the mode. Click a row, then press the new key (Ctrl +
  a key binds with Ctrl; Esc stops). "Reset to defaults" restores all keys.
- "Match keys by position" (the default) uses the place of the key on the keyboard: on Dvorak
  the movement keys stay where W A S D are on QWERTY. "Match keys by letter" uses the letter
  that the key types on the keyboard layout (keys that type no letter still match by place).
- The game shows each key with its name on the player's keyboard. It learns the name of a key
  the first time the key is pressed; before that it shows the QWERTY name.
- The settings (UI scale, vertical sync, FPS, debug panel, keys, learned key names) are saved in
  `settings.ron` in the app data folder, next to the `saves` folder (macOS:
  `~/Library/Application Support/DeepFoundry/settings.ron`). `--settings FILE` uses another
  file. `--ui-scale` and `--no-vsync` win over the file.

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
                       normal mode only: building, ghost, hub, research, guide,
                       ghost-red, drag, alt, remove
--mode MODE            Mode of a world that --ui-state starts: sandbox (default) or normal
--ui-scale S           Size of the UI, 0.75 to 2 (wins over the settings file)
--settings FILE        The settings file (default: settings.ron in the app data folder)
--exit-after SECONDS   Quit after this time and print the average FPS and frame time (starts in the game)
--no-vsync             Do not wait for the display refresh (to measure the highest FPS)
--smoke-test           Play the sandbox (new game, paint, pause, save, load, quit to menu, continue,
                       delete) and the normal game (new game, dig clay, hand craft, place a
                       workbench, open its window, drag a belt line, undo, redo, remove by drag,
                       pipette, save, load, delete), then quit
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
  tick with the cells. The screens `inventory`, `building` (a crate), `ghost` (a steam crusher in
  the hand, with its ports), `ghost-red` (the same ghost in the ground, with the reason), `drag`
  (a belt line in progress), `alt` (the alt mode over two machines), `remove` (the remove button
  on a belt row), `hub` and `guide` put a few items into the inventory first, so the picture
  shows them. `--zoom 8` shows the construction shapes well.
  `--walk N` lets the robot walk N ticks first (N < 0: to the left).
- `--pose NAME[:FRAME]` (normal mode) makes the robot act for the picture: `idle`, `walk`,
  `jump`, `fall`, `land`, `fly`, `wade` show that animation (FRAME picks one frame; `fall` and
  `land` first use up the jetpack fuel, so the fuel bar shows). `dig`, `spray` and `scan` use
  that tool on a place in front of the robot. `--face left` or `--face right` sets the direction.
  `--robot X` puts the robot on the ground (or in the water) at column X first.

```sh
cargo run --release -p deep_foundry -- --screenshot out/char.png --pose dig --zoom 8 --size 800x500 --no-ui --ticks 60
cargo run --release -p deep_foundry -- --screenshot out/wade.png --robot 640 --zoom 8 --size 800x500 --no-ui --ticks 90
```

```sh
cargo run -p deep_foundry -- --screenshot out/hub.png --size 2560x1440 --mode normal --ui-state hub --ticks 60
```
- `--center X,Y` sets the world cell at the image center. Any x works, for example
  `--center 64000000,1050`.

## The robot picture

- `assets/sprites/robot.png` is the sprite sheet: one pixel is one world cell, each frame is
  16 x 20 pixels. `assets/sprites/robot.ron` describes it: the animations (idle, walk, jump,
  fall, land, fly, wade), their frame counts and speed, the arm in 16 directions, the arm
  outline, the jetpack flame and one white pixel for particles.
- `tools/sprites/make_robot.py` draws both files from parts and a small palette, and
  `tools/sprites/robot_preview.png` (all frames at 4x on a cave, sand and water background). Run
  it from the repository root:

  ```sh
  uv run --with pillow python tools/sprites/make_robot.py
  ```

- The game reads the two files at startup. If they are missing or wrong, it uses the copies built
  into the program (a warning is logged).
- `src/robot_sprite.rs` picks the animation frame from the robot state and makes the sprites. The
  renderer draws them into the world texture (`crates/render/src/sprite.rs`): the robot is behind
  liquids and gases and shows through them at 60 %, the flame, beam and sparks are in front.

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
