# Requests from the task "explosions-particles"

## 2026-09-27 explosions-particles — reactions: the strength of `explosion_small`

Explosions use `SimEvent::Explosion { at, strength, heat }`. Strength uses the same scale as
material hardness (0 to 255; see `crates/sim/src/explode/mod.rs`). The radius is
`explode::radius(strength)` = 1 + 1.6 × √strength cells (at most 48).

| Event name in data | Suggested strength | Suggested heat (°C) | Radius |
|---|---|---|---|
| `explosion_small` (one gas cell, a steam burst) | 10 to 15 | 900 | 6 to 7 |
| gunpowder cell (later) | 25 | 1200 | 9 |
| boiler or machine explosion (later) | 80 to 150 | 600 to 1000 | 15 to 21 |

A burning gas pocket sends one event per reacting cell. That is fine: the queue runs them in order
and spreads the work over ticks (`explode::WORK_PER_TICK`).

## 2026-09-27 explosions-particles — core: fade and glow for visual particles

`ParticleView` has no field for visual particles. Please add, for example, `alpha: u8` (255 = new,
0 = gone) and `visual: bool`, so that the renderer can fade sparks, dust and smoke puffs and make
sparks glow. The simulation has the data: `Particles::get(i)` gives `flags` (`VISUAL`, `RISE`) and
`life` (ticks left). Until then, visual particles are sent as normal views (material color).

## 2026-09-27 explosions-particles — render: draw particles

The renderer does not draw `Snapshot::particles` yet. Explosions make many of them (debris, sparks,
smoke, dust), and liquid splashes too. One quad or point per particle, colored by material and
shade, drawn after the cells.

## 2026-09-27 explosions-particles — game: debug explosion key

Not a one-line change, so it is not built. What it needs:

1. `foundry_core::Command::Explode { center: CellPos, strength: f32, heat: i16 }`.
2. In `Simulation::apply`: `Command::Explode { center, strength, heat } => { self.explode(center, strength, heat); }`.
3. A key action in `crates/game/src/keys.rs` (for example `DebugExplode`, sandbox mode only) that
   sends `Command::Explode { center: <cell under the mouse>, strength: 60.0, heat: 1200 }`.
4. For sound and screen shake: read `SimEvent::Exploded { at, radius, strength }` from
   `Simulation::events()` after each tick.

## 2026-09-27 explosions-particles — headless: an explosion action for scene files

Scene files cannot start an explosion. A scene field such as
`explode: [(tick: 10, at: (x: 64, y: 40), strength: 40, heat: 800)]` that calls
`Simulation::explode` (positions in image pixels) would let scene tests cover explosions. Also,
`TotalUnchanged` counts only cells; with flying particles it should add
`sim.particles().count_material(m)`. The explosion scene tests are code scenes in
`crates/sim/src/explode/tests.rs` for now.

## 2026-09-27 explosions-particles — found: water can rest with holes under its top row

While changing where landing droplets go, the liquid test `dropped_ball_splashes_spreads_and_rests_flat`
(ball at x = 200) once came to rest with 13 air cells in row 247 under a partial top row 246: all
chunks slept while the surface was not flat. The droplet landing rules are now back to the old
order for liquids, and the test passes. But the liquid code can miss holes under a thin top row
when the droplets land in other places. It may be worth a look (liquids are lead-owned).

## 2026-09-27 explosions-particles — saves: the explosion queue

Queued explosions (`Explosions::queued`) are not saved. They wait only a few ticks (or until an
anchor comes near), so this is rare. If it matters later, save the queue next to the particles.
