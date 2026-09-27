# Build status (lead notes)

Updated: 2026-09-27. Keep this file short. It is the lead's list of what is running and what comes next.

## On main
- factory (crates/factory): F1 core (registry, placement, ports, crafter, inventory, hand crafting, belts/hopper/storage/hub/lab) and progression (research, milestones, discovery, guide), joined: labs research, Hub takes deliveries, known recipes, scan, guide, save.
- ui (crates/ui: HUD, character/crafting, building, power, production, menus; mock data in crates/ui/mock_data). Docs: docs/design/ui.md.
- core, content (materials, reactions, factory data model), sim (parallel chunk update, dirty rects, sleep, save/load, event queue, stub heat/react/explode), render + game window, headless tools (scene tests, benchmarks), foundry_factory skeleton.
- Known failing: scene `water_level` (old liquid rule; fixed on branch lead/liquids).

## Running (branch or worktree)
| Work | Where | Merge notes |
|---|---|---|
| Liquids: splash, fast tunable settling, particles | `.claude/worktrees/liquids` (lead/liquids) | Merge first. Touches movement.rs, particles.rs, lib.rs tests, liquid data. |
| Infinite world: sparse chunks, awake list, on-demand ChunkSource, packing, anchors, small saves | agent worktree | Merge after liquids. Conflicts likely in sim lib.rs, schedule.rs, save.rs, particles.rs. |
| UI connected to the game window (sandbox mode: menus, save/load, material brushes) | UI agent worktree (a73ff06) | Small conflicts possible with infinite world in crates/game. |
| F1 data again (Sonnet): Tier 0-1 data, id renames, data for factory-core kinds and guide ids | agent worktree (a64c37a) | First run did nothing. Touches assets/data only (and renamed ids in tests). |

## Next
1. Merge in order as they finish: liquids, infinite world, UI-in-game, data, join.
   - Open: part/building ids `clay_brick`, `firebrick`, `wood_block` clash with material ids. F1 data renames the materials. Check wood_block too.
   - Open: `crates/ui/src/slots.rs` must move to foundry_factory (F2a) so the sim thread can apply slot clicks.
2. Wave B workflow. Reactions task must add `SimEvent::Reaction { index: u16, at: CellPos }` (max one per reaction per tick; see requests/factory-core.md). The game passes it to `Factory::observe_reaction`.
   Wave B (script: scratchpad wave_b.js): heat, reactions + burning, explosions (uses existing particles), render light + debug overlays, worldgen (must use the ChunkSource trait), content scene tests (Sonnet), then integrate, review, fix. Update the worldgen and render prompts for the infinite world first.
3. At the same time, F2a: player (movement, dig, spray, inventory), construction UX (ghosts, drag, rotate, pick, undo, remove), factory in the sim thread (make Factory, set guide = Guide::load_default(), scan tool calls Factory::scan), UI connected to real data.
4. F2b after wave B: room machines (kiln, coke oven, blast furnace), T0 special machines (crucible, mold, sluice, stamp mill), then play-test to milestone 1.
