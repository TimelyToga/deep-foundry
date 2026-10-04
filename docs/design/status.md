# Build status (lead notes)

Updated: 2026-10-03. Keep this file short. It is the lead's list of what is running and what comes next.

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
- stutter fix: sim loop on its own 1-thread pool, big ticks on an 8-thread pool (sim_pool.rs); dig circle and sprite beam use this frame's mouse (overlay::aim_point); robot drawn 1.25 ticks behind a steady clock (motion.rs); TIMTECH_PERF=1, TIMTECH_DIG_SCRIPT=1, dig_perf tests (--ignored). Worst dig tick 24-31 ms -> 0.5-2 ms under heavy load. 379 tests pass.
- walk stutter fix (fix/walk-stutter): the part of a cell the robot has moved never points into a wall, ceiling or ground, so the drawn robot does not shake when it pushes against them (walking, flying, jetpack under a ceiling); under a liquid the robot follows the ground down steps. Tests `no_shake_*`, `walks_evenly_through_water_over_a_bumpy_bottom`. Open: the robot sprite is drawn at whole cells while the camera moves in screen pixels, so at 0.5 cells/tick (wading) or jetpack speeds it shakes on screen. Fix it in the renderer after wave B merges.
  - Open: hover box shows the cell for 1-2 ticks before building data arrives; 60 Hz fix checked only by the clock-model test.
- play-test round 2: keep/drop per material when digging (crates/factory/src/digging.rs default = used by a recipe, port or reaction; dropped cells fly out behind the robot, spoil.rs; setting saved in .dfgame), HUD bar transfers and drag and drop (crates/ui/src/screens/drag.rs), guide tier0.ron rewritten with {key:ID}/{key1:ID}, "Next: the kiln. It comes in a later update.", campfire fuel slot + pit_fired_clay_brick (clay-brick loop solved). 395 tests pass.
  - Open: spoil lands 20-30 cells behind (piles near a wall); transfer hint not saved; guide texts describe the demo world (redo after worldgen).
- wave B (fully merged, with review fixes: fall pass reach, particle positions far from x = 0 (save v3), save on quit, short key presses, stable tundra): heat, reactions + burning + SimEvent::Reaction, explosions, light pass + F4-F6 debug views, worldgen ChunkSource, 195 content scenes.
- rooms: kiln, coke oven, blast furnace (crates/factory/src/rooms, crates/ui/src/screens/room.rs, docs/design/requests/rooms.md). Bellows must call Buildings::room_at + set_blast. 512 tests pass.
  - Open: molten metal from taps freezes fast; hatches have no port data.
- balance: charcoal and coke burn chance 0.02 -> 0.0025; near clay deposit 17x6; jetpack fuel 90 ticks (50 was too little to get over the Hub). 520 tests pass.
- Open: user reports stutter when mining trees. dig_wood_perf (DIG_PERF_MATERIAL=wood|leaves) shows CPU is fine (worst tick < 5 ms); suspect render (light pass, chunk uploads) or fire. Asked user for a TIMTECH_PERF=1 log.
- Codex merged (by another agent): stamp mill + sluice, crucible + bronze casting, boiler steam network + steam machines, molds, ore guide. Lead merged automation/steam and automation/ore-guide; stamp mill and wash ore goals wait for smelting; posts 5 wide. automation/progression NOT merged: it fails 4 tier0 tests on its own. 534 tests pass.
  - Review of Codex work still to do. Seen so far: the ore-line screenshot state shows an idle line with warning marks; demo-world guide texts describe the generated world.
- Public repo: https://github.com/TimelyToga/deep-foundry (main). README with screenshots in docs/screenshots.

- 2026-10-03 (lead, user away): worktrees removed. Merged ones deleted. `automation/progression` (8 commits, breaks 3 tier0 tests) and `automation/integration-wip` (uncommitted Codex work saved as a commit) are kept as branches only; not merged.
- 2026-10-03: stutter in the generated world fixed. The air temperature of the sky was 5-15 °C but new cells are 20 °C, so ~170 chunks never went to heat sleep (4 ms/tick, spikes 70 ms). Air is now 20 °C down to 600 deep. Test `the_start_area_goes_to_sleep`; measure `dig_tree_perf`; `TIMTECH_DIG_SCRIPT=left|right` walks into trees.

- 2026-10-03: Tier 0 complete (plan step 1 done). `tier0_goals_can_be_done` plays all 24 goals in the generated world (seed 3), ~30 s. Scripts: crates/game/src/tier0_metal.rs (smelting site right of the kiln), ore_guide.rs. Generated world is the default (`--world demo` for the old one). Start area re-laid (no ore right of the Hub to x 170; clay left of the Hub and at 410; tin beds 232 and -525; malachite -470, -300, 330; hematite 720 and limestone -390 for Tier 1). Physics fixes: campfire body terracotta, grass does not spread fire, crucible pours straight into a mold under its tap, molds track the metal temperature.

- 2026-10-03: Tier 1 automation (plan step 3): arm (arms.rs), steam assembler, steam furnace (param internal_heat), steam drill (drill.rs; sorts ore from waste, throws waste), iron crate, steam blower; bellows/blowers blow rooms (rooms::blow_bellows); bellows body clay brick. Line tests in crates/factory/tests/automation.rs (gears, copper from a drill, steel, glass, rubber). Tier 1 guide rewritten. Far factories (step 4) were already done by Codex (factory_activity.rs: one anchor per chunk with a building). Overlay: building outlines, faint machine icons, arm arrows. Screenshots: --ui-state smelter, automation.

- 2026-10-03: parts ride belts (logistics::BeltPart); steam drill sorts ore from waste; boiler fuel 4 s per unit, 3 steam per water; steam furnace has 2 top inputs. Game tests: `a_steam_furnace_runs_on_a_boiler_with_water_from_a_sprayed_pool` (tier1_lines.rs) and `tier1_goals_can_be_done` (tier1_play.rs, ~22 s; shortcuts: Tier 0 parts given, steam put into machine tanks, big Hub-2 amounts given). 552 tests pass.
- 2026-10-03: Tier 2 LV power (power.rs): cables (back layer) make networks, a building joins with a cable on its Power port tile; generators (steam turbine 12 kW from 6 steam/s) and consumers share by satisfaction; machines with no network get factor 0. Buildings: steam_turbine, copper_cable, macerator, electric_furnace, electric_assembler, electric_drill, fast_arm; part electric_motor; techs electricity, lv_machines, electric_mining; guide tier2.ron (8 goals, in tier1_play test). Building window shows the power bar (BuildingView.power). Tests crates/factory/tests/power.rs.
- 2026-10-03: power network window with real data (FactoryCommand::OpenPower, FactoryFrame.power); sorter and splitter (kind "sorter"/"splitter", hopper buffer, drill::sort_out); arm filter (click an item on the open arm); a powder output fills an adjacent crate from any side; hoppers/sorters/splitters with cells stay awake (they used to miss output steps). 559 tests pass; smoke test passes (it uses the demo world).
- 2026-10-03: the deep (crates/worldgen/src/deep.rs, assets/data/materials/deep.ron): caverns with glow moss, stalactites, lakes and ore deposits in a wall; a fixed start cave (x 300, 330 cells down); crystal geodes; gold deposits; lava chambers. New content: moss lantern, crystal lamp, steel and hard drill heads (granite, basalt), gold smelting, gold plate and wire, circuit, battery (power.rs: batteries take surplus and give in a deficit). Guide goals t1_deep_rock, t2_gold, t2_circuits, t2_battery (play test uses shortcuts for the deep). Screenshot state `--ui-state cave`. The generator VERSION was not changed: old generated saves get the new features in chunks they never changed. `--smoke-test --world gen` fails (DigClay: no clay in reach); it failed the same way before this work; the smoke test is made for the demo world. 562 tests pass.
- Next ideas: building sprites; belt lifts; blueprints; Hub stage 3 and Tier 3; a Tier 2 Hub stage.

## Plan (2026-10-03, lead works alone while the user is away)
1. Tier 0 complete in the real game: crucible on a campfire smelts tin, tap pours into a plate mold, bellows, copper, bronze, gears, stamp mill + sluice, lab, kits, Hub repair. Remove every `waits_for`; tier0 test scripts for each goal.
2. Generated world is the default for new games. More early resources in the start area (shallow iron ore and limestone, more clay, coal). Guide texts for the generated world.
3. Tier 1 automation: steam pump (water into pipes), arm (moves parts and powder between buildings), steam assembler, steam furnace, steam hammer/ingots or molds with part output, steam drill, splitter/sorter if time. Each with a test of a full line.
4. Far factories: buildings keep running when the player is far away (sim anchors per building group).
5. Screenshots of each line; README update.

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
