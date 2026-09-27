# Build status (lead notes)

Updated: 2026-09-27. Keep this file short. It is the lead's list of what is running and what comes next.

## On main
- core, content (materials, reactions, factory data model), sim (parallel chunk update, dirty rects, sleep, save/load, event queue, stub heat/react/explode), render + game window, headless tools (scene tests, benchmarks), foundry_factory skeleton.
- Known failing: scene `water_level` (old liquid rule; fixed on branch lead/liquids).

## Running (branch or worktree)
| Work | Where | Merge notes |
|---|---|---|
| Liquids: splash, fast tunable settling, particles | `.claude/worktrees/liquids` (lead/liquids) | Merge first. Touches movement.rs, particles.rs, lib.rs tests, liquid data. |
| Infinite world: sparse chunks, awake list, on-demand ChunkSource, packing, anchors, small saves | agent worktree | Merge after liquids. Conflicts likely in sim lib.rs, schedule.rs, save.rs, particles.rs. |
| UI (Factorio-style, crates/ui) | agent worktree | Merge any time; later connect to the game window. |
| F1 workflow: factory-core, progression, factory data (Sonnet) | 3 worktrees | Merge after the sim branches. Data renames clay_brick/firebrick block materials. |

## Next
1. Merge in order: liquids, infinite world, UI, F1.
2. Wave B workflow (script: scratchpad wave_b.js): heat, reactions + burning, explosions (uses existing particles), render light + debug overlays, worldgen (must use the ChunkSource trait), content scene tests (Sonnet), then integrate, review, fix. Update the worldgen and render prompts for the infinite world first.
3. At the same time, F2a: player (movement, dig, spray, inventory), construction UX (ghosts, drag, rotate, pick, undo, remove), factory in the sim thread, UI connected to real data.
4. F2b after wave B: room machines (kiln, coke oven, blast furnace), T0 special machines (crucible, mold, sluice, stamp mill), then play-test to milestone 1.
