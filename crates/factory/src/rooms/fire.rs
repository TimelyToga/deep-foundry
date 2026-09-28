//! The fire of a room: gas out, ash out, bellows heat, fuel in, and the room temperature.
//!
//! The work runs every `FIRE_PERIOD` ticks over the room cells. It makes no heap allocation.

use super::{ASH_LIMIT, HEAT_MARGIN, STATS_PERIOD};
use crate::buildings::{Buildings, Logic};
use crate::cells::phase;
use foundry_content::{Content, Phase};
use foundry_core::{CellPos, MaterialId, TILE_SIZE};
use foundry_sim::Simulation;

/// New fuel cells start this much above the ignition temperature of the fuel, so they burn.
const LIGHT_ABOVE: i16 = 20;
/// With a blast, the air in the room gets this much hotter in each run (°C), up to the fire
/// temperature plus the blast.
const BLAST_STEP: i16 = 20;

/// True if the machine of the controller waits for heat: its recipe needs a temperature, a craft
/// runs or can start, and the room is below the recipe temperature plus `HEAT_MARGIN`.
fn wants_heat(content: &Content, logic: &Logic, temperature: Option<i16>) -> bool {
    let Logic::Machine(m) = logic else { return false };
    let Some(r) = m.recipe else { return false };
    let recipe = content.factory.recipe_def(r);
    let Some(min) = recipe.min_temp else { return false };
    let ready = m.running || (m.missing_input(recipe).is_none() && m.full_output(recipe).is_none());
    ready && temperature.is_none_or(|t| t < min.saturating_add(HEAT_MARGIN))
}

/// Run the fire work of controller `i`.
pub(super) fn run(content: &Content, buildings: &mut Buildings, sim: &mut Simulation, i: u32, now: u64) {
    let ash = content.material("ash");
    let Some(b) = buildings.at_index_mut(i) else { return };
    let Some(room) = b.room.as_deref_mut() else { return };
    let Some(shape) = room.shape.as_ref() else { return };
    let mats = &content.materials;
    let blast = room.blast_at(now);
    // Bellows or a blower blow hot air in while the fire burns.
    let blast_air = if blast > 0 && room.hot_fuel_cells > 0 { room.fire_temp.saturating_add(blast) } else { i16::MIN };
    let mut fire_temp = i16::MIN;
    let stats = now.is_multiple_of(STATS_PERIOD) || room.temperature.is_none();
    let (mut sum, mut n, mut fuel_cells, mut hot) = (0i64, 0i64, 0u32, 0u32);
    let mut ash_taken = 0u32;
    for &t in &shape.open {
        let o = t.origin();
        for dy in 0..TILE_SIZE {
            for dx in 0..TILE_SIZE {
                let p = CellPos::new(o.x + dx, o.y + dy);
                let c = sim.cell(p);
                let m = c.material;
                let mut temp = c.temperature;
                if m.is_air() {
                    if temp < blast_air {
                        temp = temp.saturating_add(BLAST_STEP).min(blast_air);
                        sim.set_cell(p, m, Some(temp));
                    }
                } else if phase(content, m) == Phase::Gas {
                    // The chimney: gas leaves the room, fresh air comes in.
                    sim.set_cell(p, MaterialId::AIR, Some(temp));
                } else if Some(m) == ash {
                    // The grate: ash falls out of the fire.
                    sim.set_cell(p, MaterialId::AIR, Some(temp));
                    ash_taken += 1;
                } else if let Some(burn) = mats.burn[m.index()] {
                    fuel_cells += 1;
                    if temp >= burn.ignite_at {
                        hot += 1;
                        fire_temp = fire_temp.max(burn.fire_temp);
                        let blast_temp = burn.fire_temp.saturating_add(blast);
                        if blast > 0 && temp < blast_temp {
                            sim.set_cell(p, m, Some(blast_temp));
                            temp = blast_temp;
                        }
                    }
                }
                sum += temp as i64;
                n += 1;
            }
        }
    }
    room.ash = (room.ash + ash_taken).min(ASH_LIMIT);
    if stats {
        room.temperature = Some((sum / n.max(1)) as i16);
        room.fuel_cells = fuel_cells;
        room.hot_fuel_cells = hot;
        room.fire_temp = fire_temp;
    }
    if !wants_heat(content, &b.logic, room.temperature) {
        return;
    }
    // Feed the fire: fuel from the slot onto the free cells of the bed.
    let Logic::Machine(m) = &mut b.logic else { return };
    let Some(fuel) = m.fuel.as_mut() else { return };
    for &p in &shape.bed {
        let Some(f) = fuel.material else { break };
        let c = sim.cell(p);
        if !c.material.is_air() {
            continue;
        }
        if fuel.take(1).is_none() {
            break;
        }
        let light = mats.burn[f.index()].map_or(c.temperature, |burn| c.temperature.max(burn.ignite_at.saturating_add(LIGHT_ABOVE)));
        sim.set_cell(p, f, Some(light));
    }
}
