//! The steam drill: it mines the cells under it and keeps them in a hopper buffer, which gives
//! them out of its powder output like a hopper (see `Buildings::give_outputs`).
//!
//! - The area is as wide as the drill and `depth` tiles deep, under it. Each `dig_period` ticks
//!   it digs one cell: the highest cell in the area that it can dig (top row first, middle
//!   columns first). It skips air, liquids, gases, building cells and materials harder than its
//!   `hardness`.
//! - A dug cell becomes the material it breaks into (a malachite vein gives raw malachite).
//! - It needs steam (one dig uses a dig period of steam) and room in its buffer.

use crate::buildings::{Buildings, Logic};
use crate::cells::phase;
use crate::steam::{consume_steam, steam_ready};
use foundry_content::{Content, Phase};
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
    if now < d.next_pull {
        // Between two digs: keep running.
        return true;
    }
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
