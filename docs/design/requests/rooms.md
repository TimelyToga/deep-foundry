# Notes and requests from the rooms task

Room machines are in `crates/factory/src/rooms/` (read the module documentation of `mod.rs`
first). The room panel of the building window is `crates/ui/src/screens/room.rs`.

## 1. For the bellows and blower task: the blast

- Bellows (`kind: "bellows"`) and blowers (`kind: "blower"`) may be in the wall of a room. The
  room check accepts them as wall parts.
- A bellows finds its room with `Buildings::room_at(tile)` (a tile inside the room or in its wall)
  and calls `Buildings::set_blast(controller, degrees)` while it works. The blast lasts
  `rooms::BLAST_HOLD` (60) ticks, so call it at least once a second. Pass 0 to stop at once.
- With a blast, the burning fuel and the air in the room get up to the fire temperature of the
  fuel plus the blast. The blast furnace needs it: a coke fire gives about 1200 °C, pig iron needs
  1400 °C. The window shows "Blast +300 °C".

## 2. For the crucible and mold task: taps

A hatch gives products into the building on its outer side if that building takes them
(`Buildings::room_for` > 0): a crate, a barrel, or a machine whose recipe takes the product (a
crucible that takes molten metal). Without a building, products leave as cells. Molten iron that
leaves as cells freezes in a few ticks next to cold air (metals have a low heat capacity and there
is no latent heat), so a mold or crucible must be right at the tap.

## 3. Fuel use (lead: please check the numbers)

The fire is real burning cells. A burning cell is used up with the `chance` of its `burn` data:
charcoal and coke 0.02 per tick (about 1 s), wood 0.005 (about 3 s). A full fire bed of a 1 tile
wide kiln uses about 7 charcoal or 2.4 wood per second; a 3 tile wide kiln about 11 charcoal per
second. The controller adds fuel only while the room is colder than the recipe needs plus 50 °C,
and only while a craft runs or can start. To make a kiln worth this fuel, a kiln works on many
bricks at once: the data param `speed_per_tile` (8 for the kiln) times the inside tiles is its
speed. If this is too much fuel, lower the burn `chance` of the fuels in the material data, or
raise `speed_per_tile`.

Room temperatures with a full fire bed (average of the inside cells): wood about 740 °C,
charcoal about 1000 °C, raw coal about 850 °C, coke about 1200 °C; with a blast of 300 °C coke
gives about 1500 °C. So: wood fire → charcoal (300 °C); charcoal fire → clay bricks (900 °C);
firebrick (1200 °C) needs charcoal and bellows; pig iron (1400 °C) needs coke and a blast.

## 4. Changes in shared files

- `buildings.rs`: `Building::room` (the room state of a controller), `Buildings::layout` (goes up
  when a building is placed or removed), a room branch at the top of `work`, and `accept` fills
  the recipe input and the fuel slot evenly when an item is both (`rooms::input_share`).
- `machines.rs`: `Status::NoRoom` ("Room not valid"). `views.rs`: `BuildingView::room`.
- `lib.rs`: `rooms::tick` after the building tick, `rooms::fill_view`, `rooms::upgrade` on load.
- `crates/ui`: `BuildingView::room` (`RoomPanel`), the panel is drawn by `screens/room.rs`.
- `crates/game`: the status and room panel mapping in `normal.rs`, red marks on the problem tile
  (`factory_host.rs` marks, `overlay.rs`), the screenshot states `kiln` and `kiln-hole`.
- `demo.rs`: the second clay deposit is now left of the Hub (it was left of the pool, where the
  robot cannot walk), so the tier 0 test has clay for the kiln.
- Data: the coke oven controller and hatch are firebrick now (the lead asked for firebrick walls);
  new recipe `pig_iron_from_hematite`; CO₂ byproduct of pig iron; coke needs 600 °C; kiln recipes
  `kiln_tin_*` and `kiln_copper_*` (raw, crushed, washed ore + charcoal, 900 °C and 1100 °C).

## 5. Not done

- Hatches have no ports in the data: their role comes from their place in the wall, so the ghost
  shows no port arrows for them.
- The room bonus of game design section 13 (better yield inside a room) and a room id per inside
  tile for reactions are not built.
