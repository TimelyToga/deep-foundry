//! The steam drill: it mines the cells under it and keeps them in a hopper buffer, which gives
//! them out of its powder output like a hopper (see `Buildings::give_outputs`).
//!
//! - The area is as wide as the drill and `depth` tiles deep, under it. Each `dig_period` ticks
//!   it digs one cell: the highest cell in the area that it can dig (top row first, middle
//!   columns first). It skips air, liquids, gases, building cells and materials harder than its
//!   `hardness`.
//! - A dug cell becomes the material it breaks into (a malachite vein gives raw malachite).
//! - It needs steam (one dig uses a dig period of steam) and room in its buffer.
//! - It sorts what it dug: material that a recipe uses (ore, coal, sand, clay) goes out of its
//!   main output port; the rest (dirt, stone) goes out of its port named "Waste". A port gives
//!   to the building in front of it (a crate, a hopper, a machine) or puts cells into the world.

use crate::buildings::{Buildings, Logic, PlacedPort};
use crate::cells;
use crate::logistics::steps_in_tick;
use crate::cells::phase;
use crate::steam::{consume_steam, steam_ready};
use foundry_content::{Content, ItemRef, Phase, PortKind};
use foundry_core::{CellPos, MaterialId};
use foundry_sim::Simulation;

/// One tick of a drill. Returns true if it dug a cell.
pub(crate) fn drill_tick(b: &mut Buildings, i: u32, content: &Content, sim: &mut Simulation) -> bool {
    let now = b.now;
    let Some(d) = b.at_index(i) else { return false };
    let def = content.factory.building_def(d.kind);
    let period = def.param("dig_period", 6.0).max(1.0) as u64;
    let steam_use = def.power.as_ref().map_or(0.0, |p| p.steam_per_s as f64);
    let Logic::Hopper(h) = &d.logic else { return false };
    if h.is_full() {
        return false;
    }
    if !steam_ready(d.steam, steam_use) {
        if let Some(d) = b.at_index_mut(i) {
            d.status = crate::Status::NoPower;
            d.steam_reason = Some("Needs steam from a connected bronze pipe".into());
        }
        return false;
    }
    let electric = crate::power::is_consumer(content, d);
    if electric && d.power_factor <= 0.0 {
        if let Some(d) = b.at_index_mut(i) {
            d.status = crate::Status::NoPower;
            d.steam_reason = Some("Needs power from a connected cable".into());
        }
        return false;
    }
    if now < d.next_pull {
        // Between two digs: keep running.
        return true;
    }
    // With part of its power an electric drill digs slower.
    let period = if electric { (period as f32 / d.power_factor.max(0.05)).round() as u64 } else { period };
    let r = d.cell_rect();
    let depth = def.param("depth", 6.0).max(1.0) as i32 * foundry_core::TILE_SIZE;
    let hardness = def.param("hardness", 60.0) as u8;
    let found = find_cell(sim, content, r.x0, r.x1, r.y1, r.y1 + depth, hardness);
    let Some((p, m)) = found else {
        if let Some(d) = b.at_index_mut(i) {
            d.status = crate::Status::NoInput;
            d.steam_reason = Some("Nothing left to dig in reach: move the drill".into());
        }
        return false;
    };
    let dug = content.materials.broken_into.get(m.index()).copied().unwrap_or(m);
    sim.set_cell(p, MaterialId::AIR, None);
    let Some(d) = b.at_index_mut(i) else { return false };
    if let Logic::Hopper(h) = &mut d.logic {
        h.cells.push_back(dug);
    }
    d.next_pull = now + period;
    d.status = crate::Status::Working;
    consume_steam(&mut d.steam, steam_use * period as f64 / foundry_core::TICKS_PER_SECOND as f64);
    true
}

/// The highest cell that a drill can dig in columns `x0..x1`, rows `y0..y1`: (position, material).
fn find_cell(sim: &Simulation, content: &Content, x0: i32, x1: i32, y0: i32, y1: i32, hardness: u8) -> Option<(CellPos, MaterialId)> {
    let mid = (x0 + x1) / 2;
    let w = x1 - x0;
    for y in y0..y1 {
        for k in 0..w {
            // Middle columns first: k = 0, 1, 2, ... gives mid, mid - 1, mid + 1, ...
            let x = if k % 2 == 0 { mid + k / 2 } else { mid - 1 - k / 2 };
            if x < x0 || x >= x1 {
                continue;
            }
            let p = CellPos::new(x, y);
            let c = sim.cell(p);
            if c.material.is_air() || sim.is_building_cell(p) {
                continue;
            }
            if !matches!(phase(content, c.material), Phase::Solid | Phase::Powder) {
                continue;
            }
            if content.materials.hardness[c.material.index()] > hardness {
                continue;
            }
            return Some((p, c.material));
        }
    }
    None
}

/// Give the dug cells out: useful material through the main output, the rest through the waste
/// output (see the module text). Up to "rate" cells per second.
pub(crate) fn give_outputs(b: &mut Buildings, i: u32, content: &Content, sim: &mut Simulation) {
    if b.useful.is_empty() {
        b.useful = crate::digging::useful_materials(content);
    }
    let now = b.now;
    let Some(d) = b.at_index(i) else { return };
    let def = content.factory.building_def(d.kind);
    let n = steps_in_tick(now, def.param("rate", 16.0));
    let is_waste = |p: &PlacedPort| p.def.and_then(|k| def.ports[k as usize].name.as_deref()) == Some("Waste");
    let outs: Vec<PlacedPort> = d.ports.iter().filter(|p| p.kind == PortKind::BulkOut).copied().collect();
    let main = outs.iter().find(|p| !is_waste(p)).copied();
    let waste = outs.iter().find(|p| is_waste(p)).copied().or(main);
    let (mut given, mut blocked) = (0, [false, false]);
    let mut k = 0;
    while given < n {
        let Some(Logic::Hopper(h)) = b.at_index(i).map(|d| &d.logic) else { return };
        let Some(&m) = h.cells.get(k) else { break };
        let useful = b.useful.get(m.index()).copied().unwrap_or(true);
        let slot = if useful { 0 } else { 1 };
        let Some(port) = (if useful { main } else { waste }) else { break };
        if blocked[slot] {
            k += 1;
            continue;
        }
        let pushed = b.push_bulk(i, port, ItemRef::Material(m), 1, content);
        let ok = match pushed {
            Some((1, dst)) => {
                b.wake_index(dst);
                true
            }
            Some(_) => false,
            // No building at the waste port: throw the waste away, so it lands 15 to 30 cells
            // off and does not pile up in front of the port.
            None if !useful && port.side != foundry_content::Side::Down => {
                throw_waste(sim, port, m, now as u32 ^ (given << 8));
                true
            }
            None => cells::put_to_side(sim, port.tile, port.side, m, 1, None) == 1,
        };
        if ok {
            if let Some(Logic::Hopper(h)) = b.at_index_mut(i).map(|d| &mut d.logic) {
                h.cells.remove(k);
            }
            given += 1;
        } else {
            blocked[slot] = true;
            k += 1;
        }
    }
    if let Some(d) = b.at_index_mut(i) {
        d.busy |= given > 0;
        let full = matches!(&d.logic, Logic::Hopper(h) if h.is_full());
        if full && given == 0 {
            d.status = crate::Status::OutputBlocked;
            d.steam_reason = Some("Output blocked: no room for what it dug".into());
        }
    }
}

/// Throw one waste cell out of a port, up and away from the drill, as a flying particle.
fn throw_waste(sim: &mut Simulation, port: PlacedPort, m: MaterialId, n: u32) {
    let (dx, _) = crate::geometry::side_step(port.side);
    let r = crate::geometry::tiles_to_cells(port.tile, (1, 1));
    let x = if dx < 0 { r.x0 - 2 } else { r.x1 + 1 };
    let mut h = n.wrapping_mul(0x9e37_79b9) ^ (r.x0 as u32).wrapping_mul(0x85eb_ca6b);
    h ^= h >> 15;
    let (j1, j2) = ((h & 0xff) as f32 / 255.0, ((h >> 8) & 0xff) as f32 / 255.0);
    let v = (dx as f32 * (0.8 + 0.7 * j1), -(1.0 + 0.6 * j2));
    sim.spawn_particle((x as f64 + 0.5, r.y0 as f64 + 1.5), v, m, None);
}
