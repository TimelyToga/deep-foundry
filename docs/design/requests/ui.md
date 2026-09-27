# Interface requests from the UI task

## 2026-09-27 ui — part ids that are also material ids

What: the design names the parts "Clay brick" and "Firebrick" (02-content section 4) and the building
"Wood block" (section 5.1). The material ids `clay_brick`, `firebrick` and `wood_block` already exist
(wall blocks in `materials/blocks.ron`). Material ids and part ids share one name space, so a part with
the id `clay_brick` fails the content check.

Why: the content task will hit this when it adds the Tier 0 parts.

Stub until then: the UI mock data (`crates/ui/mock_data/`) uses the part ids `brick`, `fire_brick` and
the building id `wood_wall`. Suggestion: rename the block materials (for example `clay_brick_block`) or
pick part ids like mine.

## 2026-09-27 ui — Factorio slot rules outside the UI crate

What: `foundry_ui::slots` has the Factorio click rules (pick up, put down, swap, half, one, shift-click,
ctrl-click) and unit tests. It uses only `foundry_content::{ItemRef, Stack}`, no egui. The UI sends
`UiAction::ClickSlot`; the owner of the items applies `slots::apply`.

Why: if the building inventories live in `crates/factory` and the factory applies the clicks on the
simulation thread, the factory would need this code, but it should not depend on `foundry_ui`.

Stub until then: the game crate can call `foundry_ui::slots::apply`. If the factory needs it, move
`crates/ui/src/slots.rs` to `foundry_content` (or `foundry_factory`) as is.

## 2026-09-27 ui — (optional) order of recipes in the crafting menu

What: an optional `order: Option<String>` field on `RecipeDef` (or `PartDef`), like the Factorio
subgroup order.

Why: the crafting grid puts recipes of the same kind next to each other. Today it groups them by the
icon name of the result (plates with plates, gears with gears) in data file order.

Stub until then: grouping by icon name.

## 2026-09-27 ui — (optional) material descriptions

What: keep a short `description` for each material in `MaterialTable` (the RON `note` field is for
designers and is not kept).

Why: item tooltips show a description. For materials the UI now writes one sentence from the phase
("A powder. It falls and makes piles.").

Stub until then: the sentence from the phase.
