# Requests from the content-data task

Rules from `docs/design/02-content.md` section 3 that the current `MaterialDef` /
`ReactionDef` schema (`crates/content/src/defs.rs`) cannot express. Listed here instead of
being forced into the data files, as the task instructions asked.

**Status (2026-09-27, task "reactions"):** items 1 to 6 are done. The schema has `extra`
(item 1), `burn.gases` (item 2), `burn.char_into` / `char_ticks` (item 3), `timer` with
`needs_air` (item 4), the `"$freeze"` result words (item 5) and `alt` (item 6). The data files
use them (smelting slag, coal gases, wood charring, wet concrete timer, one rule for molten
metal in water, brine boiling). Mud drying in air is not in the data yet; see
`docs/design/requests/reactions.md`. The field docs are in `assets/data/README.md`.

## 1. A reaction can only change two cells

`into_a` and `into_b` are each a single material. Several rules in the design need three
outputs: a metal, a slag byproduct and a CO₂ gas at the same time (rules 27, 28-30, 33).

What we did instead: the ore-smelting reactions in `reactions/smelting.ron` turn the ore into
the molten metal and turn the fuel into carbon dioxide, and drop the separate slag output.

Suggestion: let a reaction optionally spawn one extra cell in a named empty neighbor slot
(for example `spawn: Some("molten_slag")`), or add a small `outputs: Vec<(MaterialId, f32)>`
list with a chance per output.

## 2. `BurnDef` can only produce one kind of smoke gas

Coal burning is supposed to make ash, smoke, CO₂ and sometimes SO₂ together (rule 17). The
`burn` field has one `smoke` slot, so `raw_coal`, `crushed_coal` and `washed_coal` only emit
`smoke` in `materials/ores.ron`, not CO₂ and SO₂ as well.

Suggestion: allow a small list of `(gas, chance)` pairs instead of one `smoke` field.

## 3. No "no air for a while" condition

Rule 16: wood only turns into charcoal if it has had no air next to it for about 10 seconds.
`BurnDef.needs_air` only checks the current tick, and there is no field for "this condition
must hold for N ticks in a row". We left this rule out of the wood material.

Suggestion: a `char_into` field on `BurnDef` with a tick count, checked by the simulation
with a small per-cell counter (for example, reset the counter whenever air is present).

## 4. No time-based change while a condition holds, only a fixed timer or a fixed temperature

Rule 14 says mud also dries into dirt "slowly in air", separately from the ≥ 60 °C rule we
did add (`mud.melt`). Similarly, `life` only counts ticks unconditionally; it cannot mean
"only while exposed to open air". We left the "in air" half of the mud rule out.

Also, `life` is a `u8` (0-255 ticks, about 4.25 s at 60 ticks/s), so it cannot express a
30-second timer such as wet concrete setting (rule 45). We worked around this for
`wet_concrete` with a low-chance `Reaction(a: "wet_concrete", b: "any", chance: 0.00056, ...)`
in `reactions/minerals.ron`, which behaves like a slow, always-on timer, but a wider `life`
field (`u16` or `u32`) or a real timer field would be more direct and less roundabout for any
future case like this.

## 5. A tag match cannot produce a result specific to the matched material

Rule 7: any molten metal that touches water should freeze into its own block and make a
small steam explosion. Because `into_a` must be one fixed material id, `"tag:molten"` cannot
mean "freeze into whatever this cell's own freeze target is". We wrote nine near-identical
reactions in `reactions/water_heat.ron` instead (one per metal), which will need a tenth
whenever a new molten metal is added.

Suggestion: a result keyword such as `into_a: Some("$freeze")` that means "use this
material's own `freeze.into`", so one reaction can cover every material with the `molten` tag.

## 6. Reactions cannot split into two different outcomes by chance

Rule 12: brine boiling into steam sometimes leaves the salt behind and sometimes does not.
The schema (and `boil`/`condense`) always produce exactly one result. We modeled brine
boiling fully into steam every time, with a note in `materials/liquids.ron`.

## 7. Growth and spreading are behaviors, not two-material reactions

Rules 64 and 65 (grass spreading onto nearby dirt in light; glow fungus growing on wood in
dark, wet places) depend on light, wetness and open neighbor cells over time, not on two
touching cells. These need a coded behavior, similar to the existing `behavior` field used
for things like `float_up`, for example `behavior: "spread_on_dirt"` for grass. We left both
out of the reaction files and put a note on `grass` in `materials/terrain.ron` instead.

## 8. Materials intentionally not added yet (later tiers)

These rules from section 3 reference materials outside the Tier 0-1 list this task covers, so
the reactions are left out until those materials exist:

- Rule 46: sulfuric acid + iron/steel/zinc/tin needs metal sulfate solution materials.
- Rule 47: sulfuric acid + limestone needs a gypsum material.
- Rule 48: acid + sodium hydroxide solution needs a sodium hydroxide solution material.
- Rules 51-53: mercury and gold amalgam.
- Rules 54-57: hydrogen chloride, hydrochloric acid, liquid nitrogen.
- Rule 59-60: spark and bare cable are Tier 2 electrical parts, not materials.
- Rule 61-63, 66: nanites, uraninite, corium are later-tier materials.
- Rule 31: gangue (ore waste) plus flux making slag needs a gangue byproduct material, which
  none of the Tier 0-1 ores produce yet in this data set.

Rule 49 (sulfur dioxide + water makes a weak acid) is approximated in
`reactions/chemistry.ron` using `sulfuric_acid` as a stand-in, since we do not have a
separate weak "acid water" material yet. A real fix is to add that material in a later task.

## 9. Electrolysis and other powered reactions

Copper sulfate electrolytic refining and the aluminium cell (mentioned in `02-content.md`
section 2.2) need a reaction condition on nearby electrical power, which does not exist in
`ReactionDef`. Not needed for Tier 0-1, but worth knowing about before Tier 2 chemistry is
designed.
