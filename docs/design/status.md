# Build status (lead notes)

Updated: 2026-09-27. Keep this file short. It is the lead's list of what is running and what comes next.

## On main
- infinite world: sparse chunks, anchors (view), packing, pristine chunks, save v2 with source name; game loads saves with `Simulation::load_file_with_resolver(.., &demo::resolve_source, ..)`. Width 0 = endless.
- normal game mode (F2a part 1): robot (walk, jump, jetpack, dig, spray, scan F), factory on the sim thread (crates/game/src/factory_host.rs, GameCommand/FactoryFrame), UI on real data, research (T), guide (G), Hub at spawn, simple ghost placement, saves <name>.dfgame beside <name>.dfworld.
- construction UX: ghost with ports, drag lines, R/F, Q pipette, drag remove, undo/redo, copy recipe, reach 10 tiles, alt mode; key binding table (by position default, by letter switch), Controls page, settings.ron; WAILA hover box.
- game window with UI in sandbox mode (menus, save/load, material brushes, F3 debug, --smoke-test, --ui-state).
- data: Tier 0-1 (49 parts, 33 buildings, 59 recipes, 12 techs, 2 milestones), completeness tests. Materials renamed: clay_brick_block, firebrick_block; new scrap.
- factory (crates/factory): F1 core (registry, placement, ports, crafter, inventory, hand crafting, belts/hopper/storage/hub/lab) and progression (research, milestones, discovery, guide), joined: labs research, Hub takes deliveries, known recipes, scan, guide, save.
- ui (crates/ui: HUD, character/crafting, building, power, production, menus; mock data in crates/ui/mock_data). Docs: docs/design/ui.md.
- core, content (materials, reactions, factory data model), sim (parallel chunk update, dirty rects, sleep, save/load, event queue, stub heat/react/explode), render + game window, headless tools (scene tests, benchmarks), foundry_factory skeleton.
- liquids: fall pass, level pass on world positions (fast settling), droplets from impact rims, viscosity, liquid settings sliders (SetSimSetting, README "Tuning liquids"), scene tests (waterfall, bowl, channel, stairs, oil, lava, mud). All 317 tests pass, water_level too.
  - Open: the liquids agent stopped before its final report. No speed numbers and no list of open items yet. A follow-up task must measure speed (flood stress case) and read commits 8969bbe..06883bd for loose ends.

- play-test fixes round 1: save-menu layer bug, tanks 8 x 6,000 + "Tanks full" box, crates (8 slots, no liquids) and barrels (liquids), click/right/ctrl/shift transfers (crates/factory/src/transfer.rs), tank trash, Hub takes only needed items and gives back, first dig discovers, guide `waits_for` (11 of 22 T0 goals wait for machines), menu click tests (crates/ui/tests/menus.rs). 346 tests pass.
  - Open: clay-brick loop (clay bricks come only from the kiln; the kiln needs 8). Lead decision: a campfire fires raw clay bricks slowly (pit firing). Goal texts describe the demo world; redo them after worldgen.
- character: robot sprite sheet 16x20 (tools/sprites/make_robot.py -> assets/sprites/robot.png/.ron), animations, tool arm in 16 directions, sprite pass crates/render/src/sprite.rs (drawn into the world texture before the scale pass), movement tuning block in player.rs (steps 1-3 cells, jump buffer, coyote time, stuck fixes), jetpack fuel 50 ticks + gauge next to the robot, screenshot --pose/--face/--robot. 373 tests pass.
  - Open: no jetpack bar in the crates/ui HUD. When wave B (light) merges: the robot must be lit by the light map; flame/beam/sparks not darkened; visor glow mask.
- stutter fix: sim loop on its own 1-thread pool, big ticks on an 8-thread pool (sim_pool.rs); dig circle and sprite beam use this frame's mouse (overlay::aim_point); robot drawn 1.25 ticks behind a steady clock (motion.rs); DEEP_FOUNDRY_PERF=1, DEEP_FOUNDRY_DIG_SCRIPT=1, dig_perf tests (--ignored). Worst dig tick 24-31 ms -> 0.5-2 ms under heavy load. 379 tests pass.
- walk stutter fix (fix/walk-stutter): the part of a cell the robot has moved never points into a wall, ceiling or ground, so the drawn robot does not shake when it pushes against them (walking, flying, jetpack under a ceiling); under a liquid the robot follows the ground down steps. Tests `no_shake_*`, `walks_evenly_through_water_over_a_bumpy_bottom`. Open: the robot sprite is drawn at whole cells while the camera moves in screen pixels, so at 0.5 cells/tick (wading) or jetpack speeds it shakes on screen. Fix it in the renderer after wave B merges.
  - Open: hover box shows the cell for 1-2 ticks before building data arrives; 60 Hz fix checked only by the clock-model test.
- play-test round 2: keep/drop per material when digging (crates/factory/src/digging.rs default = used by a recipe, port or reaction; dropped cells fly out behind the robot, spoil.rs; setting saved in .dfgame), HUD bar transfers and drag and drop (crates/ui/src/screens/drag.rs), guide tier0.ron rewritten with {key:ID}/{key1:ID}, "Next: the kiln. It comes in a later update.", campfire fuel slot + pit_fired_clay_brick (clay-brick loop solved). 395 tests pass.
  - Open: spoil lands 20-30 cells behind (piles near a wall); transfer hint not saved; guide texts describe the demo world (redo after worldgen).
- wave B (fully merged, with review fixes: fall pass reach, particle positions far from x = 0 (save v3), save on quit, short key presses, stable tundra): heat, reactions + burning + SimEvent::Reaction, explosions, light pass + F4-F6 debug views, worldgen ChunkSource, 195 content scenes.
- rooms: kiln, coke oven, blast furnace (crates/factory/src/rooms, crates/ui/src/screens/room.rs, docs/design/requests/rooms.md). Bellows must call Buildings::room_at + set_blast. 512 tests pass.
  - Open: molten metal from taps freezes fast; hatches have no port data.
- balance: charcoal and coke burn chance 0.02 -> 0.0025; near clay deposit 17x6; jetpack fuel 90 ticks (50 was too little to get over the Hub). 520 tests pass.
- Open: user reports stutter when mining trees. dig_wood_perf (DIG_PERF_MATERIAL=wood|leaves) shows CPU is fine (worst tick < 5 ms); suspect render (light pass, chunk uploads) or fire. Asked user for a DEEP_FOUNDRY_PERF=1 log.

## Running (branch or worktree)
| Work | Where | Merge notes |
|---|---|---|

| Codex agents (started by the user): automation/hot-metal, ore-processing, steam, ore-guide, progression, integration | `../sand-game-*` worktrees | The user runs these. automation/integration already merged lead/wave-b at be09994. |

- The user's second stutter session is merged (fix/walk-stutter, see "walk stutter fix" above). It changed only player.rs, player_tests.rs and this file.

Rule from the user: after Codex branches merge, the lead reviews them against the design and fixes or improves what does not meet it.
Rule from the user: the lead does not start agents or workflows. Write task text for the user instead.

## Next
1. Play-test rounds 1 and 2, character and stutter fix are merged. Wait for wave B.
2. When wave B finishes: read its result, merge lead/wave-b into main (check the robot sprite is lit by the light map), tell the user it is a test point. Liquids follow-up: measure speed (flood stress case) and check commits 8969bbe..06883bd for loose ends.
3. After the merge: redo guide goal texts for the generated world; decide if worldgen becomes the default for new games.
4. F2b after wave B: room machines (kiln, coke oven, blast furnace), T0 special machines (crucible, mold, sluice, stamp mill), then play-test to milestone 1.
5. Factory areas must keep running when the player is far away: each building group adds a sim anchor (anchors exist; factory does not add them yet). Needs a spatial index for many anchors. Also fix particle f32 positions (precision past ~16.7M cells).
