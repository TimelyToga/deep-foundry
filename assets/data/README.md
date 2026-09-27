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
