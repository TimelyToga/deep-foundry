//! Hatches: they move items between the outside and the controller of a room.
//! See the module documentation of `rooms` for the rules.

use super::Hatch;
use crate::buildings::{Buildings, INPUT_DEPTH, Logic, PART_PERIOD, PORT_CELLS_PER_TICK};
use crate::cells::{self, phase};
use crate::geometry::neighbor_tile;
use crate::machines::output_stack;
use foundry_content::{Content, ItemRef, Layer, Phase, Stack};
use foundry_core::{BuildingId, MaterialId};
use foundry_sim::Simulation;

/// Products with a normal temperature of this or more (°C) leave hot. See `room_temp`.
const HOT_PRODUCT: i16 = 400;
/// Units of a material that a hatch moves from or to a crate or barrel in one move.
const STORAGE_UNITS: u32 = 16;

/// True if the room makes this item now (a recipe output, or ash that the recipe does not use).
fn is_product(content: &Content, buildings: &Buildings, ctrl: BuildingId, item: ItemRef) -> bool {
    let Some(b) = buildings.get(ctrl) else { return false };
    let Logic::Machine(m) = &b.logic else { return false };
    let recipe = m.recipe.map(|r| content.factory.recipe_def(r));
    let uses = recipe.is_some_and(|r| r.inputs.iter().any(|s| s.item == item));
    let makes = recipe.is_some_and(|r| (0..m.outputs.len()).any(|j| output_stack(r, j).item == item));
    makes || (!uses && content.material("ash").is_some_and(|a| item == ItemRef::Material(a)))
}

/// The building on the outer side of a hatch.
fn building_outside(buildings: &Buildings, h: &Hatch) -> Option<BuildingId> {
    buildings.at_tile(neighbor_tile(h.tile, h.outer?), Layer::Front)
}

/// The products of the room: the machine outputs, then the ash.
fn products(content: &Content, buildings: &Buildings, ctrl: BuildingId) -> Vec<Stack> {
    let mut out = buildings.outputs(content, ctrl);
    if let (Some(ash), Some(room)) = (content.material("ash"), buildings.room(ctrl))
        && room.ash > 0
    {
        out.push(Stack { item: ItemRef::Material(ash), count: room.ash });
    }
    out
}

/// Take `n` of a product out of the room.
fn take_product(content: &Content, buildings: &mut Buildings, ctrl: BuildingId, item: ItemRef, n: u32) -> u32 {
    if content.material("ash").is_some_and(|a| item == ItemRef::Material(a))
        && let Some(room) = buildings.get_mut(ctrl).and_then(|b| b.room.as_deref_mut())
        && room.ash > 0
    {
        let t = room.ash.min(n);
        room.ash -= t;
        return t;
    }
    buildings.take_output(content, ctrl, item, n)
}

/// Run the hatches of controller `i` for one tick.
pub(super) fn run(content: &Content, buildings: &mut Buildings, sim: &mut Simulation, i: u32, now: u64) {
    let ctrl = buildings.id_of(i);
    let count = buildings.room(ctrl).and_then(|r| r.shape.as_ref()).map_or(0, |s| s.hatches.len());
    let part_tick = (now + i as u64).is_multiple_of(PART_PERIOD);
    feed_ash(content, buildings, ctrl);
    for k in 0..count {
        let Some(h) = buildings.room(ctrl).and_then(|r| r.shape.as_ref()).and_then(|s| s.hatches.get(k).copied()) else { return };
        let Some(side) = h.outer else { continue };
        let outside = building_outside(buildings, &h);
        // In: from a crate or barrel, or powder cells.
        if h.takes_in() {
            match outside {
                Some(sid) if part_tick && buildings.inventory(sid).is_some() => pull_from_storage(content, buildings, ctrl, sid),
                Some(_) => {}
                None => {
                    cells::take_from_side(sim, content, h.tile, side, INPUT_DEPTH, PORT_CELLS_PER_TICK, &[Phase::Powder], |mat| {
                        let item = ItemRef::Material(mat);
                        !is_product(content, buildings, ctrl, item) && buildings.insert(content, ctrl, item, 1) == 1
                    });
                }
            }
        }
        // Out: into the building outside (a crate, a barrel, a mold), or as cells.
        for s in products(content, buildings, ctrl) {
            let ph = match s.item {
                ItemRef::Material(m) => Some(phase(content, m)),
                ItemRef::Part(_) => None,
            };
            let gas = ph == Some(Phase::Gas);
            if !(h.gives_out() || gas) {
                continue;
            }
            if let ItemRef::Material(m) = s.item
                && ph == Some(Phase::Liquid)
                && liquid_hatch(content, buildings, ctrl, m) != Some(k)
            {
                continue;
            }
            if let Some(sid) = outside {
                if !part_tick {
                    continue;
                }
                let want = if ph.is_some() { s.count.min(STORAGE_UNITS) } else { 1 };
                let room = buildings.room_for(content, sid, s.item).min(want);
                let n = take_product(content, buildings, ctrl, s.item, room);
                buildings.insert(content, sid, s.item, n);
                continue;
            }
            let ItemRef::Material(m) = s.item else { continue };
            let temp = room_temp(content, buildings, ctrl, m);
            let placed = cells::put_to_side(sim, h.tile, side, m, s.count.min(PORT_CELLS_PER_TICK), temp);
            take_product(content, buildings, ctrl, s.item, placed);
        }
    }
    vent_full_gas(content, buildings, ctrl);
}

/// The temperature of a product that leaves as cells. A hot product (molten metal, slag: its
/// normal temperature is `HOT_PRODUCT` or more) leaves at the room temperature, but at least at
/// its normal temperature, so it stays molten. Other products leave at their normal temperature
/// (hot coke or creosote would burn in the air).
fn room_temp(content: &Content, buildings: &Buildings, ctrl: BuildingId, m: MaterialId) -> Option<i16> {
    let base = content.materials.temperature[m.index()];
    if base < HOT_PRODUCT {
        return None;
    }
    let room = buildings.room(ctrl).and_then(|r| r.temperature)?;
    Some(room.max(base))
}

/// The hatch (index into `Shape::hatches`) that gives out a liquid product. The taps are the
/// hatches that give out and can give this liquid (no building outside, or a building that
/// takes it). The taps, lowest first, take the liquid products by density, densest first.
fn liquid_hatch(content: &Content, buildings: &Buildings, ctrl: BuildingId, m: MaterialId) -> Option<usize> {
    let shape = buildings.room(ctrl)?.shape.as_ref()?;
    let can_give = |h: &Hatch| match building_outside(buildings, h) {
        None => true,
        Some(b) => buildings.room_for(content, b, ItemRef::Material(m)) > 0,
    };
    let mut taps: Vec<(i32, usize)> =
        shape.hatches.iter().enumerate().filter(|(_, h)| h.gives_out() && can_give(h)).map(|(k, h)| (-h.tile.y, k)).collect();
    taps.sort_unstable();
    let mut liquids: Vec<MaterialId> = products(content, buildings, ctrl)
        .iter()
        .filter_map(|s| match s.item {
            ItemRef::Material(x) if phase(content, x) == Phase::Liquid => Some(x),
            _ => None,
        })
        .collect();
    // Also the liquid outputs that are empty now, so the order does not change.
    if let Some(b) = buildings.get(ctrl)
        && let Logic::Machine(mach) = &b.logic
        && let Some(r) = mach.recipe
    {
        let recipe = content.factory.recipe_def(r);
        for j in 0..mach.outputs.len() {
            if let ItemRef::Material(x) = output_stack(recipe, j).item
                && phase(content, x) == Phase::Liquid
                && !liquids.contains(&x)
            {
                liquids.push(x);
            }
        }
    }
    let dens = |x: &MaterialId| content.materials.density[x.index()];
    liquids.sort_by(|a, b| dens(b).total_cmp(&dens(a)).then(a.0.cmp(&b.0)));
    let rank = liquids.iter().position(|&x| x == m)?;
    let last = taps.len().checked_sub(1)?;
    Some(taps[rank.min(last)].1)
}

/// A hatch takes the items the room needs from a crate or barrel on its outer side.
fn pull_from_storage(content: &Content, buildings: &mut Buildings, ctrl: BuildingId, sid: BuildingId) {
    let Some(inv) = buildings.inventory(sid) else { return };
    for s in inv.contents() {
        if is_product(content, buildings, ctrl, s.item) {
            continue;
        }
        let want = if matches!(s.item, ItemRef::Material(_)) { STORAGE_UNITS } else { 1 };
        let n = s.count.min(want).min(buildings.room_for(content, ctrl, s.item));
        if n == 0 {
            continue;
        }
        let Some(inv) = buildings.inventory_mut(sid) else { return };
        let removed = inv.remove(s.item, n);
        let taken = buildings.insert(content, ctrl, s.item, removed);
        if taken < removed
            && let Some(inv) = buildings.inventory_mut(sid)
        {
            inv.insert(content, s.item, removed - taken);
        }
        buildings.wake(sid);
        return;
    }
}

/// A recipe that uses ash (glass) takes the ash of the fire first.
fn feed_ash(content: &Content, buildings: &mut Buildings, ctrl: BuildingId) {
    let Some(ash) = content.material("ash").map(ItemRef::Material) else { return };
    let have = buildings.room(ctrl).map_or(0, |r| r.ash);
    if have == 0 {
        return;
    }
    let n = buildings.room_for(content, ctrl, ash).min(have);
    let uses = buildings.get(ctrl).is_some_and(|b| match &b.logic {
        Logic::Machine(m) => m.recipe.is_some_and(|r| content.factory.recipe_def(r).inputs.iter().any(|s| s.item == ash)),
        _ => false,
    });
    if n == 0 || !uses {
        return;
    }
    let taken = buildings.insert(content, ctrl, ash, n);
    if let Some(room) = buildings.get_mut(ctrl).and_then(|b| b.room.as_deref_mut()) {
        room.ash -= taken;
    }
}

/// Gas products that no hatch can let out escape when their buffer is full, so gas never stops
/// the machine.
fn vent_full_gas(content: &Content, buildings: &mut Buildings, ctrl: BuildingId) {
    let Some(b) = buildings.get_mut(ctrl) else { return };
    let Logic::Machine(m) = &mut b.logic else { return };
    let Some(r) = m.recipe else { return };
    let recipe = content.factory.recipe_def(r);
    let mut vented = false;
    for j in 0..m.outputs.len() {
        let s = output_stack(recipe, j);
        let ItemRef::Material(x) = s.item else { continue };
        if phase(content, x) == Phase::Gas && m.outputs[j] + s.count > m.output_capacity(recipe, j) {
            m.outputs[j] = 0;
            vented = true;
        }
    }
    if vented {
        buildings.wake(ctrl);
    }
}
