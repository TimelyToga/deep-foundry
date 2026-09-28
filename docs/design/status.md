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

## Running (branch or worktree)
| Work | Where | Merge notes |
|---|---|---|
| Play-test fixes (Opus): save-menu bug, bigger tanks, crates take bulk, Factorio transfers, tank trash, Hub take-back, dig discovers, guide goal checks | agent worktree (ac372d6) | crates/factory, crates/ui, small factory_host/app edits. Told to merge main (liquids) before its report. |
| Character (Opus): new robot sprites (user's image skill), animations, sprite pass in crates/render, smoother movement | agent worktree | crates/game player.rs + drawing, new crates/render sprite file, assets/sprites, tools/sprites. |
| Wave B workflow (run wf_7b897b9e-c25, script scratchpad/wave_b.js): heat, reactions + SimEvent::Reaction, explosions, render light + F4-F6 overlays, worldgen as ChunkSource, content scene tests (Sonnet); then integrate, review, fix | builders in worktrees; integrate/review/fix in `.claude/worktrees/wave-b` (lead/wave-b) | Lead merges lead/wave-b into main at the end. Render-light and character both touch crates/render. |

## Next
1. Merge play-test fixes when it finishes. Then tell the user: good test point.
   Then launch play-test round 2 (Opus, new agent), from the user's second play-test:
   - Digging fills the tank with dirt and stone. Make digging useful: common materials (dirt, stone, sand...) are knocked loose or dropped instead of stored, ores and wanted materials are kept; the player can set keep/drop per material in the tank UI.
   - Move resources between the HUD quickbar and the tanks (both ways) without opening the inventory: drag, click and shift-click like Factorio.
   - Guide: after the workbench the user did not know what to do. Check the whole path from spawn to the kiln step: each step says what to do, where, and with which key; the guide advances on every goal. Check the play-test fixes (dig discovers, crate storage) on the merged build.
   - Kiln, charcoal, smelting need F2b machines, which do not exist yet. The guide must not point to a step the player cannot do; mark such steps "coming soon".
   Sent to the character agent (running): no auto-jump when blocked, stuck cases, jetpack fuel gauge and recharge, draw pixel art directly.
   - Open: part/building ids `clay_brick`, `firebrick`, `wood_block` clash with material ids. F1 data renames the materials. Check wood_block too.
   - Open: `crates/ui/src/slots.rs` must move to foundry_factory (F2a) so the sim thread can apply slot clicks.
2. Wave B workflow: RUNNING (explosions need its particles; heat/react touch the same sim files). Heat should use `World::worked_chunks()`; worldgen implements `ChunkSource` (see crates/sim/src/source.rs docs). Reactions task must add `SimEvent::Reaction { index: u16, at: CellPos }` (max one per reaction per tick; see requests/factory-core.md). The game passes it to `Factory::observe_reaction`.
   Wave B (script: scratchpad wave_b.js): heat, reactions + burning, explosions (uses existing particles), render light + debug overlays, worldgen (must use the ChunkSource trait), content scene tests (Sonnet), then integrate, review, fix. Update the worldgen and render prompts for the infinite world first.
3. At the same time, F2a: player (movement, dig, spray, inventory), construction UX (ghosts, drag, rotate, pick, undo, remove), factory in the sim thread (make Factory, set guide = Guide::load_default(), scan tool calls Factory::scan), UI connected to real data.
4. F2b after wave B: room machines (kiln, coke oven, blast furnace), T0 special machines (crucible, mold, sluice, stamp mill), then play-test to milestone 1.
5. Factory areas must keep running when the player is far away: each building group adds a sim anchor (anchors exist; factory does not add them yet). Needs a spatial index for many anchors. Also fix particle f32 positions (precision past ~16.7M cells).
