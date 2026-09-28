//! Tier 1 steam production and the small bronze-pipe fluid network.
//!
//! Fluids are measured in unit-seconds: boiler and machine rates in the content data map
//! directly to these tanks, and each simulation tick moves one sixtieth of a second's flow.

use crate::buildings::Buildings;
use crate::cells;
use foundry_content::{Content, PortKind};
use foundry_core::{MaterialId, TilePos};
use serde::{Deserialize, Serialize};

pub const TANK_CAPACITY: f64 = 200.0;
const PIPE_FLOW_PER_TICK: f64 = 0.5;
const BOILER_STEAM_PER_SECOND: f64 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct FluidTank {
    pub material: Option<MaterialId>,
    pub amount: f64,
    pub capacity: f64,
}

impl FluidTank {
    pub fn new(capacity: f64) -> Self {
        Self {
            material: None,
            amount: 0.0,
            capacity,
        }
    }

    pub fn room_for(&self, material: MaterialId) -> f64 {
        if self.amount > 0.0001 && self.material != Some(material) {
            0.0
        } else {
            (self.capacity - self.amount).max(0.0)
        }
    }

    pub fn add(&mut self, material: MaterialId, amount: f64) -> f64 {
        let accepted = amount.max(0.0).min(self.room_for(material));
        if accepted > 0.0 {
            self.material = Some(material);
            self.amount += accepted;
        }
        accepted
    }

    pub fn take(&mut self, amount: f64) -> f64 {
        let taken = self.amount.min(amount.max(0.0));
        self.amount -= taken;
        if self.amount < 0.0001 {
            self.amount = 0.0;
            self.material = None;
        }
        taken
    }
}

/// Extra state for a boiler, a pipe, or a steam-powered building.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum SteamState {
    #[default]
    None,
    Boiler {
        fuel: Option<MaterialId>,
        fuel_units: u32,
        burn_ticks: u32,
        fraction: f64,
        water: f64,
        steam: f64,
    },
    Pipe(FluidTank),
    Machine(FluidTank),
}

impl SteamState {
    pub fn for_kind(kind: &str, consumes_steam: bool) -> Self {
        match kind {
            "boiler" => Self::Boiler {
                fuel: None,
                fuel_units: 0,
                burn_ticks: 0,
                fraction: 0.0,
                water: 0.0,
                steam: 0.0,
            },
            "pipe" => Self::Pipe(FluidTank::new(TANK_CAPACITY)),
            _ if consumes_steam => Self::Machine(FluidTank::new(TANK_CAPACITY)),
            _ => Self::None,
        }
    }
}

pub fn steam_rate_per_second(
    content: &foundry_content::Content,
    kind: foundry_core::BuildingKindId,
) -> f64 {
    content
        .factory
        .building_def(kind)
        .power
        .as_ref()
        .map_or(0.0, |p| p.steam_per_s as f64)
}

pub fn boiler_rate() -> f64 {
    BOILER_STEAM_PER_SECOND
}

/// Capacity displayed by a machine tank. Kept here so all steam consumers agree.
pub fn machine_tank_capacity() -> f64 {
    TANK_CAPACITY
}

/// Fluid flow along each directly connected pipe edge, per simulation tick.
pub fn pipe_flow_per_tick() -> f64 {
    PIPE_FLOW_PER_TICK
}

impl Buildings {
    /// Burn boiler fuel, then move steam along real placed pipe tiles into machine buffers.
    pub(crate) fn tick_steam_network(
        &mut self,
        content: &Content,
        sim: &mut foundry_sim::Simulation,
    ) {
        let steam = content.material("steam");
        let Some(steam) = steam else { return };
        let water = content.material("water");
        let pipe_indices: Vec<u32> = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| {
                slot.building
                    .as_ref()
                    .is_some_and(|b| matches!(b.steam, SteamState::Pipe(_)))
                    .then_some(i as u32)
            })
            .collect();

        // A bronze pipe can take water cells along its exposed pipe ports. Taking the cell makes
        // the supply finite, and the tank then carries that water through the same network as steam.
        if let Some(water) = water {
            for &i in &pipe_indices {
                let ports = self
                    .at_index(i)
                    .map(|b| b.ports.clone())
                    .unwrap_or_default();
                for port in ports.into_iter().filter(|p| p.kind == PortKind::Pipe) {
                    let room = self
                        .at_index(i)
                        .and_then(|b| match b.steam {
                            SteamState::Pipe(t) => Some(t.room_for(water)),
                            _ => None,
                        })
                        .unwrap_or(0.0);
                    let max = room
                        .floor()
                        .min(crate::buildings::PORT_CELLS_PER_TICK as f64)
                        as u32;
                    if max == 0 {
                        continue;
                    }
                    let mut taken = 0;
                    cells::take_from_side(
                        sim,
                        content,
                        port.tile,
                        port.side,
                        3,
                        max,
                        &[foundry_content::Phase::Liquid],
                        |mat| {
                            if mat == water {
                                taken += 1;
                                true
                            } else {
                                false
                            }
                        },
                    );
                    if taken > 0 {
                        if let Some(b) = self.at_index_mut(i)
                            && let SteamState::Pipe(tank) = &mut b.steam
                        {
                            tank.add(water, taken as f64);
                        }
                    }
                }
            }
        }
        for slot in &mut self.slots {
            let Some(b) = slot.building.as_mut() else {
                continue;
            };
            let rect = b.cell_rect();
            let body = content.factory.building_def(b.kind).body;
            if let SteamState::Boiler {
                fuel_units,
                burn_ticks,
                fraction,
                water: water_tank,
                steam: steam_tank,
                ..
            } = &mut b.steam
            {
                if (*burn_ticks > 0 || *fuel_units > 0) && *steam_tank < TANK_CAPACITY {
                    if *burn_ticks == 0 {
                        *fuel_units -= 1;
                        *burn_ticks = foundry_core::TICKS_PER_SECOND;
                    }
                    *burn_ticks = burn_ticks.saturating_sub(1);
                    // Heat the boiler's actual body cells while fuel burns.
                    let mut heat_sum = 0i64;
                    let mut heat_cells = 0i64;
                    for y in rect.y0..rect.y1 {
                        for x in rect.x0..rect.x1 {
                            let p = foundry_core::CellPos::new(x, y);
                            let cell = sim.cell(p);
                            if cell.material == body {
                                sim.set_cell(
                                    p,
                                    cell.material,
                                    Some((cell.temperature + 10).min(350)),
                                );
                                heat_sum += (cell.temperature + 10).min(350) as i64;
                                heat_cells += 1;
                            }
                        }
                    }
                    let temperature = if heat_cells > 0 {
                        (heat_sum / heat_cells) as i16
                    } else {
                        20
                    };
                    if temperature >= 100 && *water_tank > 0.0 {
                        *fraction +=
                            BOILER_STEAM_PER_SECOND / foundry_core::TICKS_PER_SECOND as f64;
                        let produced = fraction.floor();
                        if produced > 0.0 {
                            let amount = produced.min(*water_tank).min(TANK_CAPACITY - *steam_tank);
                            *water_tank -= amount;
                            *steam_tank += amount;
                            *fraction -= amount;
                        }
                    }
                }
            }
        }

        // Pipes connect to another pipe on a cardinal tile, or to a machine port occupying the
        // same back-layer tile. Each edge has a finite flow, so visible sections fill in order.
        for i in pipe_indices {
            let Some(pipe_pos) = self.at_index(i).map(|b| b.at) else {
                continue;
            };
            let neighbors = [
                TilePos::new(pipe_pos.x + 1, pipe_pos.y),
                TilePos::new(pipe_pos.x, pipe_pos.y + 1),
            ];
            for pos in neighbors {
                let Some(j) = self.back.get(&pos).map(|id| id.index) else {
                    continue;
                };
                if j == i {
                    continue;
                }
                transfer_pipe_to_pipe(self, i, j);
            }
            let endpoints: Vec<u32> = self
                .slots
                .iter()
                .enumerate()
                .filter_map(|(j, s)| {
                    let b = s.building.as_ref()?;
                    let matched = b
                        .ports
                        .iter()
                        .any(|p| p.kind == PortKind::Pipe && p.tile == pipe_pos);
                    (j as u32 != i
                        && matched
                        && matches!(b.steam, SteamState::Boiler { .. } | SteamState::Machine(_)))
                    .then_some(j as u32)
                })
                .collect();
            for j in endpoints {
                transfer_endpoint(self, i, j, content, steam, water.unwrap_or(steam));
            }
        }
    }
}

pub(crate) fn steam_ready(state: SteamState, rate_per_second: f64) -> bool {
    if rate_per_second <= 0.0 {
        return true;
    }
    match state {
        SteamState::Machine(tank) => {
            tank.amount + 0.0001 >= rate_per_second / foundry_core::TICKS_PER_SECOND as f64
        }
        _ => false,
    }
}

pub(crate) fn consume_steam(state: &mut SteamState, amount: f64) {
    if let SteamState::Machine(tank) = state {
        tank.take(amount);
    }
}

fn transfer_pipe_to_pipe(buildings: &mut Buildings, a: u32, b: u32) {
    let Some((left, right)) = two_mut_local(&mut buildings.slots, a as usize, b as usize) else {
        return;
    };
    let (Some(left), Some(right)) = (left.building.as_mut(), right.building.as_mut()) else {
        return;
    };
    let (SteamState::Pipe(x), SteamState::Pipe(y)) = (&mut left.steam, &mut right.steam) else {
        return;
    };
    transfer_tanks(x, y);
}

fn transfer_endpoint(
    buildings: &mut Buildings,
    pipe_index: u32,
    endpoint_index: u32,
    content: &Content,
    steam: MaterialId,
    water: MaterialId,
) {
    let Some((pipe, endpoint)) = two_mut_local(
        &mut buildings.slots,
        pipe_index as usize,
        endpoint_index as usize,
    ) else {
        return;
    };
    let (Some(pipe), Some(endpoint)) = (pipe.building.as_mut(), endpoint.building.as_mut()) else {
        return;
    };
    let port_name = |p: &crate::buildings::PlacedPort| {
        p.def
            .and_then(|n| {
                content
                    .factory
                    .building_def(endpoint.kind)
                    .ports
                    .get(n as usize)
            })
            .and_then(|p| p.name.as_deref())
    };
    let steam_out = endpoint.ports.iter().any(|p| {
        p.kind == PortKind::Pipe && p.tile == pipe.at && port_name(p) == Some("Steam out")
    });
    let water_in = endpoint
        .ports
        .iter()
        .any(|p| p.kind == PortKind::Pipe && p.tile == pipe.at && port_name(p) == Some("Water in"));
    let steam_in = endpoint
        .ports
        .iter()
        .any(|p| p.kind == PortKind::Pipe && p.tile == pipe.at && port_name(p) == Some("Steam in"));
    if let SteamState::Pipe(tank) = &mut pipe.steam {
        if steam_out && tank.room_for(steam) > 0.0 {
            if let SteamState::Boiler { steam: amount, .. } = &mut endpoint.steam {
                let moved = (*amount).min(PIPE_FLOW_PER_TICK).min(tank.room_for(steam));
                *amount -= tank.add(steam, moved);
            }
        } else if water_in && tank.material == Some(water) {
            if let SteamState::Boiler { water: amount, .. } = &mut endpoint.steam {
                let moved = tank
                    .amount
                    .min(PIPE_FLOW_PER_TICK)
                    .min(TANK_CAPACITY - *amount);
                *amount += tank.take(moved);
            }
        } else if steam_in && tank.material == Some(steam) {
            if let SteamState::Machine(machine) = &mut endpoint.steam {
                let moved = tank
                    .amount
                    .min(PIPE_FLOW_PER_TICK)
                    .min(machine.room_for(steam));
                machine.add(steam, tank.take(moved));
            }
        }
    }
}

fn transfer_tanks(a: &mut FluidTank, b: &mut FluidTank) {
    let Some(material) = a.material.or(b.material) else {
        return;
    };
    if a.material.is_some_and(|m| m != material) || b.material.is_some_and(|m| m != material) {
        return;
    }
    let ratio_a = if a.capacity > 0.0 {
        a.amount / a.capacity
    } else {
        0.0
    };
    let ratio_b = if b.capacity > 0.0 {
        b.amount / b.capacity
    } else {
        0.0
    };
    if ratio_a > ratio_b {
        let delta =
            ((ratio_a - ratio_b) * a.capacity.min(b.capacity) * 0.5).min(PIPE_FLOW_PER_TICK);
        let moved = delta.min(a.amount).min(b.room_for(material));
        b.add(material, a.take(moved));
    } else if ratio_b > ratio_a {
        let delta =
            ((ratio_b - ratio_a) * a.capacity.min(b.capacity) * 0.5).min(PIPE_FLOW_PER_TICK);
        let moved = delta.min(b.amount).min(a.room_for(material));
        a.add(material, b.take(moved));
    }
}

fn two_mut_local<T>(slice: &mut [T], a: usize, b: usize) -> Option<(&mut T, &mut T)> {
    if a == b {
        return None;
    }
    if a < b {
        let (left, right) = slice.split_at_mut(b);
        Some((&mut left[a], &mut right[0]))
    } else {
        let (left, right) = slice.split_at_mut(a);
        Some((&mut right[0], &mut left[b]))
    }
}
