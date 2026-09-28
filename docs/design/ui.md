# Deep Foundry: user interface

The UI is in the crate `crates/ui` (package `foundry_ui`). It uses egui. It looks and works like the Factorio GUI.

The crate depends only on `egui`, `foundry_core` and `foundry_content`. It does not depend on the renderer, the game or the simulation.

## 1. How the game uses the UI

The UI is a function of two things:

- a read-only `UiModel` (what to show), which the game owns, and
- a small UI state (which windows are open, where they are, the search text), which the UI owns.

Each frame the game calls `show` and gets back a list of `UiAction` values:

```rust
// Once, at the start:
let mut ui = foundry_ui::FoundryUi::new(&egui_ctx);   // installs the fonts and the style
let mut model = foundry_ui::UiModel::new(content.clone());   // content: Arc<Content>

// Each frame, inside egui_ctx.run_ui(raw_input, |egui_ui| { ... }):
update_model_from_snapshot(&mut model, &snapshot);   // the game's code
let actions = ui.show(egui_ui.ctx(), &model);
for action in actions {
    handle(action);   // the game's code: make simulation commands, open files, and so on
}
```

Rules:

- The UI never changes the model. It only returns actions.
- Update the model in place (clear and refill the lists), so that no new memory is needed each frame.
- `FoundryUi::wants_pointer(ctx)` is true when the mouse is over the UI. Then the game must not use the click in the world.
- `FoundryUi::wants_keyboard(ctx)` is true when a text field has the keyboard. Then the game must ignore key presses.
- The UI draws nothing in the first frame after `FoundryUi::new` (the new fonts become active in the second frame).

## 2. Items and ids

The UI uses the ids of `foundry_content` and `foundry_core` directly:

| Type | Meaning |
|---|---|
| `ItemRef` | An item: `Material(MaterialId)` (bulk, counted in units = cells) or `Part(PartId)` (counted in pieces). Every building is also a part. |
| `Stack` | An item and a count. |
| `RecipeId`, `TechId`, `BuildingKindId`, `BuildingId` | Recipes, technologies, building types, placed buildings. |

Names, descriptions, recipes, stack sizes and icons come from `UiModel::content` (`Arc<Content>`). The module `foundry_ui::item` has the helpers (name, kind, facts for tooltips, crafting tab, where a recipe is made).

When the game reloads the data, it puts a new `Arc<Content>` into the model. The UI then builds the icons again.

## 3. The model (`UiModel`)

| Field | What it holds |
|---|---|
| `content` | All materials, parts, buildings, recipes and technologies. |
| `state` | `MainMenu`, `Playing` or `Paused`. `Paused` means the pause menu is open. |
| `player` | Hull, heat, inventory (part slots), material tank slots, the stack in the hand, quickbar, crafting queue, crafting speed. |
| `finished_techs` | Finished technologies. A recipe that a technology unlocks shows only when that technology is here. |
| `hover` | The cell or the building under the mouse (entity info panel). |
| `research` | The technology that is researched now, and its progress. |
| `techs` | All technologies with their state, for the research window (section 3.3). The game fills it while the research window is open. |
| `discovery_points` | Discovery points that the player can spend. |
| `guide` | The guide goals of the open tiers, in order (section 3.4). |
| `alerts` | Alerts, grouped by kind, with a count and a place. |
| `building` | The open building window, or `None`. |
| `power` | The open power network window, or `None`. |
| `stats` | Production statistics: amount made and used per minute over time, per item. |
| `saves` | Saved games, newest first. |
| `settings` | UI scale, vertical sync, show FPS, the key list of the Controls section (section 3.2). |
| `hover` | The cell or the building under the mouse, for the hover box at the top center. |
| `hover_detail` | More about it (normal mode): for a cell `dig` (`CanDig`, `TooHard { needs }`, `Never`) and `undiscovered`; for a building `reason` and `hit_points`. |
| `fps` | Frames per second (shown when `settings.show_fps` is on). |
| `message` | A short line at the top of the screen, for example "Game saved". |
| `sandbox` | `Some` in the sandbox mode (section 3.1). |
| `perf` | `Some`: the HUD shows a box at the top right with FPS, tick time, ticks per second and awake chunks. |

Details of the player:

- `inventory: Vec<Option<Stack>>`: part slots. The screen shows 10 slots per row.
- `tank: Vec<TankSlot>`: each tank slot holds one material (`material`, `units`, `capacity`).
- `hand: Option<Stack>`: the stack that the mouse holds, as in Factorio. The UI draws it at the mouse.
- `hotbar: Vec<Option<ItemRef>>`: 20 quickbar slots. A quickbar slot holds an item type, not items. The count shown is the number in the inventory.
- `crafting: Vec<CraftJobView>`: the hand crafting queue. Only the first job has progress.
- `dig: Vec<DigRule>`: keep or drop for each known dug material (`material`, `keep`). The game
  sends the discovered materials that can be in a tank and the materials in the tanks.
  `PlayerView::keeps(m)` finds the setting. Empty in the sandbox.

Details of a building window (`BuildingView`): status and status text, the current recipe, input, output and fuel slots (each slot can have a `filter`: the item it expects), material buffers (for example "Water in", "Steam out"), progress, speed, power use (with the voltage of the building and of its network), temperature, and `milestone` (only for the Hub, section 3.5). The name, the icon, the maximum temperature and the list of recipes that the building can make come from the content (`item::building_recipes`).

- A slot grid shows at most 10 slots in a row. More slots go into more rows (the Hub has 16 slots, a crate 8).
- Storage (a crate, the Hub) puts its slots in `inputs`. A slot can hold a material: then `BuildingSlot::capacity` is its units, and the slot shows a fill bar. A building with no recipe, no outputs and no fuel slots shows no progress arrow.
- Machine status (`MachineStatus`): Working (green); Idle, No recipe, Disabled (gray); No input, Output full, Output blocked, Too cold, Low power, No fuel (yellow); No power, Too hot, Wrong voltage, Room not valid, Broken (red).

Details of a power network window (`PowerNetworkView`): voltage tier, satisfaction, production and its maximum, consumption, energy in batteries, current and the limit of the weakest cable, producers and consumers by building type (with history for the graph lines), total production and consumption history, and warnings (overloaded cable, wrong voltage, not enough power, no generators).

### 3.1 Sandbox mode

The game runs in the sandbox mode while there is no player and no factory (like the Factorio cheat mode). The game sets `UiModel::sandbox`:

- The inventory holds every material with no limit. The slots show no counts. The character screen is called "Sandbox": the materials on the left, and a "Brush" panel (the material in the hand, the brush size, the paint keys) on the right. There is no crafting and no tank.
- The stack in the hand is the paint brush. A click on a material sends `ClickSlot`; the game puts that material in the hand.
- The line above the quickbar shows the brush (material and size) instead of the hull and heat bars.
- A click on a full quickbar slot selects it (the hand always holds something, so it does not replace the slot). A click on an empty slot puts the material in the hand there. A right click clears a slot.
- P (production statistics), T (research) and G (guide) do nothing. The HUD shows no guide tracker.

Over the world, the stack in the hand is drawn at the lower right of the mouse, so the brush center stays visible.

### 3.2 Settings

`Settings` has `ui_scale`, `vsync`, `show_fps`, `show_debug` (the debug panel, F3), the keys, and `simulation`: a list of number settings of the simulation (`SimSetting`: key, label, help, value, min, max, step). The settings screen shows one slider for each in the "Simulation" section. When the list is empty, it says that the liquid settings will be there. A slider move sends `ChangeSetting(SettingChange::Simulation { key, value })`.

The keys (the game owns the key table; see `crates/game/README.md`, "Keys and settings"):

- `key_bindings: Vec<KeyRow>`: the rows of the Controls section. A row has an action `id` (for example "rotate"), the `action` text, the `key` as the player's keyboard shows it (for example "R" or "Ctrl + Z"), and `fixed` (mouse buttons and Esc, which cannot change).
- `keys_by_letter`: keys match by the letter they type, not by their place on the keyboard.
- `key_waiting`: the id of the row that waits for a key press.
- The Controls section has "Match keys by position / letter" (sends `SettingChange::KeysByLetter`), "Reset to defaults" (`SettingChange::ResetKeys`) and the list. A click on a row sends `SettingChange::RebindKey(id)`; the game then takes the next key press as the new key (Esc stops the wait).
- `Settings::key(id)` gives the key name of an action, and `Settings::with_keys(text)` replaces each `{key:ID}` in a text with it. The HUD, the guide and the tooltips use them, so they show the player's keys. Guide texts in the data write keys as `{key:scan}`.

### 3.3 Research

`techs: Vec<TechEntry>` has one entry for each technology:

- `id`: the technology. The name, the description, the tier, the kits per unit, the number of units, the discovery points, the discoveries and the unlocked recipes come from `content.factory.techs`.
- `state`: `Done`, `Researching` (the current research), `Available` (it can start now) or `Locked`.
- `progress`: 0 to 1. It can be above 0 for a technology that started and then stopped.
- `reasons`: why a locked technology cannot start, as sentences for the player (for example "Research Glass first."). Empty unless `Locked`.
- `queue_position`: the place in the research queue (0 = next), or `None`.
- `can_queue`: locked only because earlier technologies are not done. The card shows a Queue button, which sends `StartResearch`; the game queues the earlier technologies first, then this one.

### 3.4 Guide

`guide: Vec<GuideGoal>` has the goals of the open tiers, in order. A goal has an `id`, a `tier`, a `title`, a hint `text`, `done`, an optional `count` (have, need), for example (40, 64), `reward_points` (discovery points it gives), and `waits_for`: the machine that the goal needs and that the game does not have yet (for example "the kiln").

`next_goal(&guide)` gives the next step: `Goal` (the first open goal with no `waits_for`), `Waiting` (every goal that the game can do is done; this goal waits) or `AllDone`. For `Waiting`, `GuideGoal::next_text` is "Next: the kiln. It comes in a later update." The HUD tracker and the guide window use it, so the player always sees a next step.

Guide texts write keys as `{key:ID}` (all keys of the action, "A / Left") or `{key1:ID}` (the first key, "A"). `Settings::with_keys` puts in the player's keys. Never write a fixed letter.

### 3.5 Hub repair stage

`BuildingView::milestone` is `Some` only for the Hub. `MilestoneView` has the `stage` number, the `name` and the `description` of the next repair stage (from `content.factory.milestones`), and `items`: one `Delivery { item, delivered, need }` for each item that the stage needs.

Graphs use `TimeSeries`: 300 samples for each time range (5 s, 1 m, 10 m, 1 h, 10 h). The game can use `foundry_ui::graph::History` to collect them: call `push(value)` once per tick and `fill(&mut series)` when the window is open. A sample of a long range is the average of the samples of the shorter range.

## 4. The actions (`UiAction`)

| Action | What the game does |
|---|---|
| `OpenWindow(kind)` | Information only. For `Production`, the game can start to fill `stats`. For `Research`, it can start to fill `techs`. |
| `CloseWindow(kind)` | For `Building` and `PowerNetwork`: set `model.building` or `model.power` to `None`. For the others: information only. |
| `OpenPowerNetwork(building)` | Fill `model.power` with the network of this building. |
| `ClickSlot { slot, click }` | Apply the Factorio slot rules (section 5) to the real inventories. Drag and drop also sends it (section 5.1). |
| `SetKeep { material, keep }` | Keep (true) or drop (false) this material when the robot digs. |
| `SelectHotbar(i)` | Put that item in the hand or select the build tool for it. |
| `SetHotbar { index, item }` | Put an item type in a quickbar slot, or clear it (`None`). |
| `ClearHand` | Put the stack in the hand back into the inventory. |
| `Craft { recipe, count }` | Add a hand crafting job. Take the ingredients now, as in Factorio. |
| `CancelCraft { index, count }` | Remove runs from a queue job and give the ingredients back. |
| `SetRecipe { building, recipe }` | Change the recipe of a building (`None` clears it). |
| `StartResearch(tech)` | Research this technology now. Queue the technologies it needs first. |
| `ShowAlert(id)` | Move the camera to the place of the alert. |
| `NewGame { seed, size, mode }` | Make a new world. `mode` is `GameMode::Normal` (the robot, the factory, research and the Hub) or `GameMode::Sandbox`. |
| `Continue` | Load the newest save. |
| `Pause`, `Resume` | Set `state` to `Paused` or `Playing` and stop or start the simulation. |
| `Save { name, overwrite }` | Save the game. `overwrite` is true when the player said yes to replacing a save with that name. |
| `Load(id)`, `DeleteSave(id)` | Load or delete a save (`SaveInfo::id`). The player already said yes to the delete. |
| `QuitToMenu`, `QuitGame` | Go to the main menu, or close the program. |
| `ChangeSetting(change)` | Store the new setting and put it into `model.settings`. The UI applies the UI scale itself. |

## 5. Slot rules (Factorio)

The UI only reports the click. The owner of the items applies the rules with `foundry_ui::slots::apply`. This module has no egui code, so the game or the factory can use it.

| Click | Hand empty | Hand holds items |
|---|---|---|
| Left | Pick up the whole stack. | Put the stack down. Same item: add to the slot. Other item or full slot: swap. |
| Right | Take half (rounded up). | Put one item down. |
| Shift + left | Move the whole stack to the other inventory. | (same) |
| Shift + right | Move half of the stack to the other inventory. | (same) |
| Ctrl + left | Move all items of this type to the other inventory. | (same) |
| Ctrl + right | Move half of all items of this type to the other inventory. | (same) |

Items move into stacks of the same item first, then into empty slots, in slot order. Each slot list has a `limit(slot, item)` function: the most of an item that the slot can hold (0 = the slot does not take it). So a tank slot takes only materials, a part slot takes only parts, an output slot takes nothing, and a filtered input slot takes only its item.

"The other inventory": for a player slot, the open building (fuel slots first, then input slots). For a building slot, the player inventory, then the tank. With no building open, shift-click and ctrl-click do nothing. On macOS, Cmd counts as Ctrl.

### 5.1 Moving items with the HUD bar

With a building window open, the HUD bar (the quickbar and the tank panel) works as the robot's side of the transfer, so the player does not need the inventory:

| Input | Action sent |
|---|---|
| Click a HUD tank | `ClickSlot(Tank)`: the tank moves into the building (right click: half). |
| Click a material on the quickbar | Ctrl + click on a tank of it: that material from every tank. |
| Shift + click a part on the quickbar | Ctrl + click on an inventory slot of it: all of that part. A plain click takes it in the hand. |
| Drag a tank, a quickbar slot or an inventory slot onto the building window | The same as the click above (an inventory slot: Shift + click). |
| Drag a building slot onto the HUD bar or the inventory | Shift + click on the building slot: it goes back to the robot. |
| Drag a tank or an inventory slot onto a quickbar slot | `SetHotbar`: the quickbar shows that item. |
| Drag a quickbar slot onto another quickbar slot | Two `SetHotbar`: the slots change places. |

The game applies these actions with the rules of `foundry_factory::transfer`; the UI has no rules of its own (`screens/drag.rs`). While the player drags, the item is drawn at the mouse and the place where it can go has an orange frame. The first building window of a game shows a hint above the quickbar: "Move items with the bar below".

## 6. Crafting clicks

In the character screen, on a recipe: left click makes 1, right click makes 5, shift + click makes all you can. A click never asks for more than the player can make now (`crafting::craftable_count`). On the crafting queue: left click cancels 1, right click cancels 5, shift + click cancels the whole job.

Recipes that the player cannot make now have a red slot. The tooltip shows missing ingredients in red.

## 7. Keys

These keys act on the UI. The game reads them with its key bindings and gives them to the UI with `FoundryUi::press_key(UiKey)` (after `use_game_keys()`, the UI no longer reads them itself; the preview and the tests without a game still use the default keys below). Esc stays with the UI. While the Controls list waits for a key, the UI ignores Esc (the game uses it to stop the wait).

| Key | Action |
|---|---|
| E | Open or close the character screen. If a building or power window is open, close it (as in Factorio). |
| P | Open or close the production statistics (not in the sandbox mode). |
| T | Open or close the research window (not in the sandbox mode). |
| G | Open or close the guide (not in the sandbox mode). |
| Esc | Close the top window. With no window open, send `Pause`. In the pause menu: go back one page, or send `Resume`. |
| 1 to 0 | Select quickbar slots 1 to 10 (the bottom row). |
| Shift + 1 to 0 | Select quickbar slots 11 to 20 (the top row). |

The UI ignores keys while a text field has the keyboard.

## 8. Windows

- The character screen, the statistics, the research window and the guide replace each other: only one of them is open at a time, as in Factorio. The UI sends `CloseWindow` for the window that it closes.
- A new building window closes these windows too, because the building window shows the inventory next to it.
- The power window can be open together with a building window.
- The player moves a window by its title bar. The [X] button closes it. Esc closes the top window. A click on a window puts it on top.
- The building window has the player inventory on its left. The two move together.

## 9. Screens

| Screen | Contents |
|---|---|
| HUD | Hover box at the top center (as in the Minecraft mod WAILA): the icon and name of the cell or building under the mouse, then for a cell its state, temperature, "Can dig" or what is needed, what it breaks into and "Not discovered: scan with F"; for a building its status and reason, recipe with progress, hit points and temperature. Nothing for air. It stays right of the boxes on the left; the message line moves under it. Quickbar (2 rows of 10) with hull and heat bars at the bottom center. Tank summary on its right. Crafting queue at the bottom left. Research box at the top left (a click opens the research window). Guide tracker under the research box, or at the top left when there is no research: the first 2 open goals that the game can do, with the title, the count and the text. When the game can do no more goals, it says "You did every goal for now" and "Next: the kiln. It comes in a later update." (a click opens the guide; not in the sandbox mode; nothing when every goal is done). Alerts at the bottom right. FPS at the top right corner. |
| Character screen (E) | Inventory grid and tank on the left. Crafting on the right: 5 tabs (Logistics, Production, Intermediate products, Power, Research), a search field, and the recipe grid. |
| Building window | Status line with a colored dot, tier, picture, Hub repair stage (only the Hub), recipe selector (a click opens a grid of recipes), input slots, progress arrow, output slots, fuel slots, material buffers, power bar (a click opens the power network), temperature bar. |
| Hub repair stage | In the Hub window: "Repair stage N: name", the description, one row for each item (icon, name, a bar with "delivered / need"), and a hint: shift + click moves a stack from the inventory into the Hub. Under it, "Later repair stages" (`BuildingView::later_stages`) and "Held items": the items the Hub keeps for a later stage. A click on a held item gives it back to the robot. The Hub takes only what the stages still need. |
| Tanks | HUD: a slot for each tank (icon, amount, fill bar), the spray material with an orange frame and "Hold right mouse to spray it out"; a click chooses the spray material. `PlayerView::tanks_full` shows a red box: "Tanks full: put material in a crate, spray it out, or empty a tank". Inventory panel: a trash button under each tank (`UiAction::EmptyTank`; 1,000 units or more asks first). With a building window open, a click on a tank moves it into the building, and the text next to the HUD tanks says so (section 5.1). |
| Keep or drop | Each tank slot (HUD and inventory) has a small button in its top-left corner: a green check (keep: dug units go into the tanks) or a red arrow down (drop: the robot throws them out behind itself). One click changes it (`UiAction::SetKeep`). The character screen has the list "Digging: keep or drop" under the inventory with every known material and a key of the two marks. The hover box of a diggable cell says "When dug: kept in the tanks" or "thrown out". |
| Fuel slot | A machine with a fuel slot (the campfire) shows it under "Fuel" as a material slot with a fill bar (`BuildingView::fuel`). A click on it gives the fuel back to the robot. |
| Menus | All menu windows (the pause dim, the page, the yes/no question) are in one egui layer, drawn in that order. Separate layers kept an old order, and the pause dim covered the menu buttons. |
| Research (T) | Discovery points at the top. A list with a scroll bar, grouped by tier. Each technology: icon, name, a state badge (Done, Researching, Available, Locked), the queue place, the cost (kits per unit × units, discovery points, discoveries to scan), a progress bar when progress is above 0, the lock reasons in red, the icons of the recipes it unlocks (with recipe tooltips), and a Research button for available technologies (a Queue button for technologies that wait only for earlier technologies). An empty list shows "No technologies yet." |
| Guide (G) | Goals done and discovery points at the top. The goals grouped by tier. A done goal is one dim line with a check mark. An open goal shows the title, the reward, the text, and a bar with "have / need" when it counts something. The next goal has an orange frame and a "Next" mark. A goal that waits is gray with "Waits for the kiln. It comes in a later update." When no goal can be done now, a yellow line at the top says "You did every goal for now. Next: the kiln. It comes in a later update." |
| Power network | Voltage tier, satisfaction, production, storage and current bars, warnings, consumers and producers by building type, graphs of consumption and production with time ranges 5s / 1m / 10m / 1h / 10h. |
| Production statistics (P) | Time range tabs, graphs of made and used per minute, and lists of items with bars and rates. |
| Main menu | Continue, New game, Load game, Settings, Quit game. |
| New game | Game mode ("Normal game" or "Sandbox", default Normal game), seed (with a Random button) and world size. The sandbox sizes are Small 2048 × 1024, Normal 4096 × 2048 and Large 8192 × 4096 cells (`WorldSize::chunks`). |
| Pause menu (Esc) | Resume, Save game, Load game, Settings, Quit to main menu, Quit game. The world is dimmed. |
| Save game | List of saves (name, date, play time), name field, Save. A save with the same name asks "Overwrite save?". |
| Load game | List of saves, Delete (asks first), Load. A double click loads. |
| Settings | UI scale (75 % to 200 %), vertical sync, show FPS, debug panel, simulation settings (sliders), Controls: match keys by position or letter, Reset to defaults, and the list of keys (click a row, then press the new key). |

Tooltips appear at once, next to the mouse, and follow the Factorio layout: a title bar with the name and the kind, the description, facts, the recipe (ingredients with icons, red when the player has too few), crafting time, "Made in", and "Used in".

## 10. Icons

There is no art yet. `foundry_ui::icons` draws small pixel-art icons (32 × 32 art pixels) once, into egui textures at 32 px and 64 px.

`icons::spec_for(content, item)` is the one place that maps an item to its picture:

- Materials: a pile (powder), a drop (liquid), a cloud (gas), a cube (solid) or a flame, in the material colors.
- Parts: the `icon` name of the part in the data files. Names: `gear`, `plate`, `ingot`, `rod`, `wire`, `pipe`, `brick`, `circuit`, `kit`, `vial`, `tube`, `pane`, `sheet`, `bolt`, `block`, `machine`, `belt`, `crate`, `barrel`, `wall`, `ladder`, `campfire`, `workbench`, `hopper`, `mold`, `crucible`, `tank`, `cable`, `solar`, `battery`. The color comes from the material of the part.
- Buildings with the icon `machine` (the default): a machine box in the color of the body material, with a band in the tier color and a small symbol for the building kind (flame for furnaces, gear for assemblers, and so on).

When real art exists, change `IconAtlas::build` to load it by the string id of the item.

## 11. Look

- Font: Titillium Web (SemiBold for normal text, Bold for titles and counts), the font of Factorio. The files and the OFL license are in `assets/fonts/`. They are built into the program.
- Colors and sizes: `foundry_ui::theme`. Dark gray windows with a light top edge and a dark bottom edge; darker sunken frames for slot grids; light gray buttons with dark text; orange for hover and selection; green for confirm, red for back and delete.
- A slot is 40 × 40 points with a 32 × 32 icon. Counts use the Factorio number style: 999, 1.2k, 45k, 1.2M (rounded down).
- UI scale: 0.75 to 2.0. The UI sets the egui zoom factor from `settings.ui_scale`.

## 12. Preview and tests

- `cargo run -p foundry_ui --example preview` opens a window with the mock data. The mock game applies the actions, so crafting, slot clicks, saves and menus work. Extra keys: F2 steam assembler, F3 boiler, F4 electric furnace, F5 power network, F6 item in the hand, F7 hover a cell or a building, F8 main menu, F9 UI scale, F10 the Hub, F1 help. The game keys (E, P, T, G, Esc, 1 to 0) also work.
- `cargo test -p foundry_ui` runs the unit tests (slot rules, number formats, crafting counts, graph math, mock game) and the screenshot tests.
- The screenshot tests render every screen at 1920 × 1080 (and some at 2560 × 1440) to `crates/ui/tests/snapshots/*.png`. A test fails when a picture changes. After a wanted change, run `UPDATE_SNAPSHOTS=1 cargo test -p foundry_ui --test snapshots` and look at the new pictures. A picture with fewer than 2000 changed pixels passes the test and is not written again; delete its file first to get the new picture.

## 13. Mock data

`foundry_ui::mock` loads the real content from `assets/data`. The real factory data is still a small starter set, so the mock adds Tier 0 to Tier 2 parts, buildings, recipes and technologies from `crates/ui/mock_data/*.ron`, only for ids that the real data does not have. These files are test data for the UI, not game data. The mock invents only the state: inventory, queue, machine state, power numbers, saves, the state of each technology (`mock::tech_entries`: done, researching, available and locked with reasons), discovery points, guide goals (texts from `assets/data/guide`, some done) and the Hub (`mock::hub_view`: 16 slots, the first repair stage from the real milestone data and the later stages), and a crate with materials (`mock::crate_view`). `MockGame` applies `StartResearch` and makes a sandbox model for `NewGame` with `GameMode::Sandbox`.
