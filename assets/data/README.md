# Data files

This folder holds the game content. `foundry_content` reads every `.ron` file here, in
file name order, and checks it. See `crates/content/src/defs.rs` for the exact fields.

## File layout

```
assets/data/
  materials/
    basic.ron    Air and the first materials from Milestone 0 (bedrock, stone, sand, gravel,
                 dirt, water, oil, lava, wood, ash, steam, smoke, fire). `air` must be here
                 and must come first.
    terrain.ron  More terrain and rock: grass, clay, granite, basalt, obsidian, ice, snow,
                 frozen ground, peat, leaves, rubber tree wood.
    ores.ron     Ore veins (solid) and their raw, crushed and washed powders.
    powders.ron  Fuels (charcoal, coke, wood chips), mineral powders (quicklime, slaked lime,
                 cement, salt, sulfur dust, crushed slag, rust) and metal dusts.
    liquids.ron  Water-family and molten liquids that are not metals (brine, mud, oils, resin,
                 acid, wet concrete, molten glass/slag/salt/sulfur).
    metals.ron   Molten metals and their solid block forms.
    gases.ron    Gases other than steam and smoke.
    fire.ron     Fire-family materials other than fire itself (ember).
    blocks.ron   Building blocks (wood block, brick, glass, concrete, rubber, ...).
  reactions/
    water_heat.ron  Water, ice, steam, lava, dissolving.
    burning.ron     Fire, gas explosions, fire-affecting gases.
    smelting.ron    Ore roasting and smelting.
    minerals.ron    Lime, cement, rust.
    chemistry.ron   Acids and reactive gases.
```

Each file is a RON list of `Material(...)` or `Reaction(...)` values (see `defs.rs`).
Material ids must be unique across every file, even though the files are split by topic.

## Tags

The tag list is fixed. Reactions can match `"tag:<name>"` for the `a` or `b` field.
Do not invent new tags; ask for a schema change instead (see `docs/design/requests/`).

| Tag | Meaning |
|---|---|
| `metal` | A metal dust, block or molten metal. |
| `molten` | A hot liquid form of a solid (metal, glass, slag, salt, sulfur). |
| `flammable` | Can catch fire. |
| `conductive` | Carries electricity (and sparks, once sparks exist). |
| `magnetic` | A magnet pulls it. |
| `toxic` | Harms the player. |
| `corrosive` | Attacks other materials over time. |
| `acid` | A corrosive liquid that reacts with metals and bases. |
| `oil` | A crude-oil family liquid. |
| `ore` | A vein, or a raw/crushed/washed ore powder. |
| `gangue` | Waste rock from an ore, not yet used in the Tier 0-1 data. |
| `fuel` | Burns and is meant to power furnaces and boilers. |
| `carbon` | A carbon fuel (charcoal, coke, coal) used to reduce ore in smelting. |
| `organic` | Comes from a living thing (wood, resin, rubber). |
| `soluble` | Dissolves in water. |
| `heat_proof` | Survives high heat without melting or burning. |
| `acid_proof` | Is not attacked by acid. |
| `fire_out` | Puts out fire on contact (carbon dioxide). |

## Naming

Ids are `snake_case`: lower case letters, digits and `_` only (the loader checks this).

- An ore vein keeps the ore's plain name: `malachite`, `coal`, `limestone`.
- Its powder stages are `raw_<ore>`, `crushed_<ore>`, `washed_<ore>`:
  `raw_malachite`, `crushed_malachite`, `washed_malachite`.
- A metal dust is `<metal>_dust`: `copper_dust`.
- A molten metal is `molten_<metal>`: `molten_copper`.
- A metal's solid block is `<metal>_block`: `copper_block`.
- Other molten liquids follow the same pattern: `molten_glass`, `molten_slag`, `molten_salt`,
  `molten_sulfur`.

## Value conventions

- **Density** is kg/m³. Air is 1.2. This decides what sinks and what floats.
- **Temperature** is °C. Room temperature (20 °C) is the default; only set `temperature` when
  a material is normally hot or cold (lava, molten metal, steam, fire, liquid nitrogen).
- **heat_capacity** is relative to water = 1.0. Rock is about 0.8. Metals are low, around
  0.03 to 0.13, because metal heats up and cools down fast.
- **conductivity** is 0.0 (insulator) to 1.0 (best conductor). Metals are 0.6 to 0.95, stone
  is 0.2 to 0.3, firebrick is 0.05 (it is built to block heat), air is 0.02.
- **flow** (liquids only) must be 1 or more. Water is fast (6), oil is slower (3), lava and
  molten metal are slow (1 to 3), mud is very slow (1).
- **friction** (powders only) is 0.0 to 1.0. Higher gives steeper piles: sand is loose
  (about 0.15), gravel is coarse (about 0.45), dirt piles steeply (about 0.6), clay is the
  steepest common powder (about 0.7).
- **melt** / **freeze** / **boil** / **condense** are a temperature plus the material it
  becomes. Keep freeze below melt, and condense below boil, so a cell does not flicker
  between two materials at the same temperature. This project uses a 10 °C gap for metals.
- **life** is a `(min, max)` tick count for a material that fades away on its own (smoke,
  fire, ember). It is a `u8`, so it can only count up to 255 ticks (about 4.25 seconds at
  60 ticks/s). A longer timer needs a low-chance reaction with `b: "any"` instead (see
  `wet_concrete` in `liquids.ron` and `reactions/minerals.ron`).
- **drag_limit** (liquids only) lets light powders move with the liquid when it flows
  sideways. Water uses 1700, so sand and lighter powders wash along with it.
- **colors** are 3 to 4 shades that are close to each other, so a pile of the material
  looks textured but still reads as one thing. Similar materials (the ores, especially)
  use clearly different colors so the player can tell them apart at a glance. Liquids and
  gases may add an alpha channel: `"#rrggbbaa"`.

## Factory data

The factory data model and loader are in `crates/content/src/factory_defs.rs` (file format)
and `crates/content/src/factory.rs` (after loading). See that file for the exact fields.

### File layout

```
assets/data/
  parts/*.ron        Part(...): discrete items (gears, plates, research kits, ...).
  buildings/*.ron     Building(...): building types. Every building is also a part with the
                      same id (the item that places it).
  recipes/*.ron       Recipe(...): what turns inputs into outputs, and where.
  tech/*.ron          Tech(...): the research tree.
  milestones/*.ron    Milestone(...): Hub repair stages.
```

`assets/data/parts/starter.ron` and `assets/data/buildings/starter.ron` hold the Tier 0-1
parts and buildings; `assets/data/recipes/starter.ron` and `assets/data/tech/starter.ron`
hold the matching recipes and technologies.

### Id naming rules

- Material ids and part ids share one name space: an id cannot be both. When a part is
  named after the block material it comes from, the material takes the `_block` suffix so
  the part can use the plain name: the material `clay_brick_block` gives the part
  `clay_brick`; the material `firebrick_block` gives the part `firebrick`. The material
  `wood_block` keeps its name, so the wood wall building is `wood_wall`, not `wood_block`.
- A building's id is also its part id (the item that places it), so it must not clash with
  any material or other part id either.
- Recipe ids usually match their main output's id (`bronze_gear` makes `bronze_gear`), except
  when several recipes make the same output in different ways.

### Units

- Materials are bulk: recipe counts are in units, which are cells (so 16 units of molten
  copper is 16 cells' worth).
- Parts are discrete: recipe counts are in pieces.

### Ports and params

Ports (`kind`, `tile`, `side`, `filter`) connect a building to the world or to other
buildings: `BulkIn`/`BulkOut` for powder, `FluidIn`/`FluidOut` for liquids and gases,
`Pipe` for a back-layer pipe connection, `PartIn`/`PartOut` for pieces, `Heat` for
temperature, `Exhaust` for waste gas. Put input ports on the side cells or fluid naturally
arrive from (usually `Up`), and output ports where they should leave (usually `Down` or a
named side tap).

The factory code reads these `kind`s and `params` (see `docs/design/requests/factory-core.md`
for the full table):

| kind | params | notes |
|---|---|---|
| `storage` | `slots`, `tanks`, `capacity` | a crate uses slots, a barrel uses tanks |
| `hopper` | `capacity`, `rate` | ports default to BulkIn on top, BulkOut below |
| `belt` | `belt_speed` | rotation must be 0 or 2; `flip` moves left |
| `hub` | `slots`, `tanks`, `capacity` | takes only items a milestone still needs |
| `lab` | `kit_buffer` | `speed` is the lab speed |
| `workbench` | `reach` | `speed` is the hand crafting speed near it |
| any kind with `crafts` | `buffer_crafts`, `heat_temp` (burners) | a crafter; a burner also has `power.burn_w > 0` and a `Heat` port |
| any kind | `needs_floor` | above 0: solid cells must be under at least half the bottom row |

Only the kind strings above (plus `workbench`) change the building's behavior in code.
Other kind names (`furnace`, `crafter`, `boiler`, `mold`, `wall`, `room_wall`,
`room_controller`, `room_port`, `pipe`, `bellows`, `stamp_mill`, `sluice`, ...) are labels for
players and the UI; a building with any of them and a non-empty `crafts` list runs as a
generic crafter.

### How techs unlock recipes

A recipe is known from the start unless some technology lists its id in `unlocks`. Give a
tech that reference to gate the recipe behind research; leave it out of every tech's
`unlocks` to make the recipe free from the start (this project uses that for early Tier 0
recipes such as bricks, the campfire and the crucible, so a new player has something to do
before any research is done).
