# Design documents

Game name: **TimTech**. Crate names start with `foundry_`.

Read the files in this order:

| File | Contents |
|---|---|
| [01-game-design.md](01-game-design.md) | What the game is and how it plays: world, player, materials, heat, logistics, machines, room machines, power, research, hazards, construction, interface, graphics. |
| [02-content.md](02-content.md) | The detailed content: materials, reactions, parts, buildings, recipes, production chains, research, upgrades. |
| [03-technical-design.md](03-technical-design.md) | How we build it and how we make it fast: threads, world data, simulation rules, rendering, data files, saves, tests. |
| [04-build-plan.md](04-build-plan.md) | Milestones, the work for each agent, the shared interfaces, and how the agents work in parallel. |

## Decisions (made on 2026-09-27)

1. **Platform.** Native desktop game in Rust: wgpu for graphics, rayon for threads, egui for panels.
2. **Name.** TimTech (renamed from Deep Foundry on 2026-10-04). The crate names keep the `foundry_` start.
3. **Enemies.** None in the first version. The world itself is the danger.
4. **Overvoltage.** A machine that gets too high a voltage explodes. The build tool always warns first.
5. **First playable scope.** Surface and upper stone layers, Tier 0 and Tier 1, end at the second milestone.
6. **How we build.** The lead agent builds and tests the core crates (`foundry_core`, `foundry_content`, `foundry_sim`). Opus agents build new systems in parallel in a workflow. Smaller models only extend systems that are already built and tested (for example: more data entries, more scene tests).
