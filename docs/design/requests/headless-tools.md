## 2026-09-27 headless-tools — the parallel simulation should use the current rayon pool

The `Deterministic` check and the `determinism` command run a scene a second time inside
`rayon::ThreadPoolBuilder::new().num_threads(1).build()?.install(...)`. This gives the
"1 thread against many threads" test from the technical design (section 11), but only if
`Simulation::tick` runs its parallel work on the current rayon pool (`rayon::join`, `par_iter`).
If the simulation makes its own thread pool, please add `SimConfig::threads: Option<usize>`
so the tools can ask for one thread. Until then, the tools use the rayon pool as described.

## 2026-09-27 headless-tools — found: cells that do not move update only every other tick

In `foundry_sim::naive`, a cell is skipped when its parity flag equals `tick & 1`. The flag is only
written when a cell moves. So a cell at rest is skipped on every second tick. For materials with a
life (smoke, fire), life counts down at half speed: smoke with life 120..255 lasts up to 510 ticks.
The scene `assets/scenes/tests/gas_rises.ron` has a pending check `Total(material: "smoke", exact: 0)`
at tick 300 for this. Please check that the new movement code counts life down once per tick.

## 2026-09-27 headless-tools — low priority: fast read of a whole chunk

The tools read cells one at a time with `Simulation::cell`. This is fast enough for scenes and for
benchmark worlds up to 64 × 32 chunks, but a picture of an 8192 × 8192 world takes 67 million calls.
A read-only call such as `Simulation::chunk_materials(ChunkPos) -> Option<&[u16]>` would make
pictures and material counts of large worlds fast. No stub is needed now.
