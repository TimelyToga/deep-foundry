//! Electric power (Tier 2): cables, generators and the machines that use power.
//!
//! - A cable is a back-layer building of kind "cable". Cables on neighboring tiles (left, right,
//!   up, down) are one network.
//! - A building joins a network when a cable lies on the tile of one of its `Power` ports (the
//!   cable is behind the building there).
//! - A generator has `power.produce_w`. A steam turbine (a generator with `power.steam_per_s`)
//!   makes its full power while it gets that much steam, and uses steam for the power it gives.
//! - A machine with `power.use_w` (and `power.tier` 1 or more) uses that much while it works.
//! - Each tick, each network compares what its generators can make with what its machines want.
//!   Every machine on the network gets the same part of its power (`Building::power_factor`, 0 to
//!   1): its speed is that part of its full speed. A machine on no network gets nothing.
//! - The networks are made again when buildings are placed or removed (`Buildings::layout`).

use crate::buildings::{Buildings, Logic};
use crate::steam::SteamState;
use foundry_content::{Content, Layer, PortKind};
use foundry_core::{BuildingId, TICKS_PER_SECOND, TilePos};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, VecDeque};

/// One power network.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PowerNet {
    /// A number for the window (the smallest building index on it).
    pub id: u32,
    pub generators: Vec<BuildingId>,
    pub consumers: Vec<BuildingId>,
    /// Watts the generators could make in the last tick, watts the machines wanted, and watts
    /// the generators gave.
    pub supply_w: f64,
    pub demand_w: f64,
    pub used_w: f64,
    /// The part of their power the machines got in the last tick (0 to 1).
    pub satisfaction: f32,
}

/// The power networks, made from the cables. Not saved: they are made again after a load.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PowerGrid {
    #[serde(skip)]
    pub nets: Vec<PowerNet>,
    /// The `Buildings::layout` the networks were made for.
    #[serde(skip)]
    pub layout: Option<u64>,
}

/// True for a building that makes electric power.
fn is_generator(content: &Content, b: &crate::Building) -> bool {
    content.factory.building_def(b.kind).power.as_ref().is_some_and(|p| p.produce_w > 0.0)
}

/// True for a building that uses electric power.
pub fn is_consumer(content: &Content, b: &crate::Building) -> bool {
    content.factory.building_def(b.kind).power.as_ref().is_some_and(|p| p.produce_w <= 0.0 && (p.tier > 0 || p.use_w > 0.0))
}

impl Buildings {
    /// Make the networks again: flood fill over neighboring cable tiles, then find the buildings
    /// with a power port on a cable tile.
    fn rebuild_power(&mut self, content: &Content) {
        let cables: HashMap<TilePos, u32> = self
            .back
            .iter()
            .filter(|(_, id)| self.get(**id).is_some_and(|b| content.factory.building_def(b.kind).kind == "cable"))
            .map(|(t, id)| (*t, id.index))
            .collect();
        let mut net_of: HashMap<TilePos, usize> = HashMap::new();
        // The smallest cable index of each network: its number for the window.
        let mut ids: Vec<u32> = vec![];
        let mut count = 0;
        for (&start, &first) in &cables {
            if net_of.contains_key(&start) {
                continue;
            }
            let mut queue = VecDeque::from([start]);
            net_of.insert(start, count);
            ids.push(first);
            while let Some(t) = queue.pop_front() {
                ids[count] = ids[count].min(cables[&t]);
                for n in [TilePos::new(t.x + 1, t.y), TilePos::new(t.x - 1, t.y), TilePos::new(t.x, t.y + 1), TilePos::new(t.x, t.y - 1)] {
                    if cables.contains_key(&n) && !net_of.contains_key(&n) {
                        net_of.insert(n, count);
                        queue.push_back(n);
                    }
                }
            }
            count += 1;
        }
        let mut nets: Vec<PowerNet> = ids.iter().map(|&id| PowerNet { id, ..Default::default() }).collect();
        for (id, b) in self.iter() {
            if b.layer != Layer::Front {
                continue;
            }
            let generator = is_generator(content, b);
            if !generator && !is_consumer(content, b) {
                continue;
            }
            let joined: BTreeSet<usize> = b.ports.iter().filter(|p| p.kind == PortKind::Power).filter_map(|p| net_of.get(&p.tile).copied()).collect();
            // A building on two networks joins the first (the networks stay apart).
            let Some(&n) = joined.iter().next() else { continue };
            let net = &mut nets[n];
            if generator {
                net.generators.push(id);
            } else {
                net.consumers.push(id);
            }
        }
        self.power.nets = nets;
        self.power.layout = Some(self.layout);
    }

    /// One tick of power (see the module text). Call it before the machines work.
    pub(crate) fn tick_power(&mut self, content: &Content) {
        if self.power.layout != Some(self.layout) {
            self.rebuild_power(content);
        }
        let steam = content.material("steam");
        let mut powered: BTreeSet<u32> = BTreeSet::new();
        let mut factors: Vec<(BuildingId, f32)> = vec![];
        let mut nets = std::mem::take(&mut self.power.nets);
        for net in &mut nets {
            // What the generators can make in this tick.
            let mut caps: Vec<(BuildingId, f64)> = vec![];
            for &g in &net.generators {
                let Some(b) = self.get(g) else { continue };
                let p = content.factory.building_def(b.kind).power.as_ref().expect("a generator has power data");
                let cap = if p.steam_per_s > 0.0 {
                    let per_tick = p.steam_per_s as f64 / TICKS_PER_SECOND as f64;
                    let have = match b.steam {
                        SteamState::Machine(t) if Some(t.material) == Some(steam) || t.material.is_none() => t.amount,
                        _ => 0.0,
                    };
                    p.produce_w as f64 * (have / per_tick).min(1.0)
                } else {
                    p.produce_w as f64
                };
                caps.push((g, cap));
            }
            let supply: f64 = caps.iter().map(|c| c.1).sum();
            // What the machines want: their working power, or idle power when they have nothing
            // to do.
            let mut demand = 0.0;
            for &c in &net.consumers {
                let Some(b) = self.get(c) else { continue };
                let p = content.factory.building_def(b.kind).power.as_ref().expect("a consumer has power data");
                let over = match &b.logic {
                    Logic::Machine(m) => m.recipe.map_or(1.0, |r| crate::machines::overclock_power(content.factory.building_def(b.kind).tier, content.factory.recipe_def(r).tier)),
                    _ => 1.0,
                };
                demand += if wants_work(b) { p.use_w as f64 * over } else { p.idle_w as f64 };
            }
            let factor = if demand <= 0.0 { 1.0 } else { (supply / demand).min(1.0) } as f32;
            for &c in &net.consumers {
                powered.insert(c.index);
                factors.push((c, factor));
            }
            // The generators give the power the machines use, each its share, and turbines use
            // steam for it.
            let used = demand.min(supply);
            for (g, cap) in caps {
                let share = if supply > 0.0 { cap / supply * used } else { 0.0 };
                let Some(b) = self.get_mut(g) else { continue };
                b.power_w = share as f32;
                let p = content.factory.building_def(b.kind).power.as_ref().expect("power data");
                b.status = if share > 0.0 { crate::Status::Working } else { crate::Status::Idle };
                if p.steam_per_s > 0.0 && p.produce_w > 0.0 {
                    let steam_used = share / p.produce_w as f64 * p.steam_per_s as f64 / TICKS_PER_SECOND as f64;
                    crate::steam::consume_steam(&mut b.steam, steam_used);
                    b.steam_reason = (cap <= 0.0).then(|| "Needs steam from a connected bronze pipe".to_string());
                }
            }
            net.supply_w = supply;
            net.demand_w = demand;
            net.used_w = used;
            net.satisfaction = factor;
        }
        self.power.nets = nets;
        // Machines on no network get no power.
        let unpowered: Vec<BuildingId> = self
            .iter()
            .filter(|(id, b)| !powered.contains(&id.index) && is_consumer(content, b) && b.power_factor != 0.0)
            .map(|(id, _)| id)
            .collect();
        for id in unpowered {
            self.set_power_factor(id, 0.0);
        }
        for (id, f) in factors {
            if self.get(id).is_some_and(|b| (b.power_factor - f).abs() > 0.001) {
                self.set_power_factor(id, f);
            }
        }
    }

    /// The power networks (for the power window).
    pub fn power_nets(&self) -> &[PowerNet] {
        &self.power.nets
    }

    /// The power of a building for its window. `None` for buildings with no electric power.
    pub fn power_info(&self, content: &Content, id: BuildingId) -> Option<crate::views::PowerInfo> {
        let b = self.get(id)?;
        let p = content.factory.building_def(b.kind).power.as_ref()?;
        let generator = p.produce_w > 0.0;
        if !generator && !is_consumer(content, b) {
            return None;
        }
        let over = match &b.logic {
            Logic::Machine(m) => m.recipe.map_or(1.0, |r| crate::machines::overclock_power(content.factory.building_def(b.kind).tier, content.factory.recipe_def(r).tier)),
            _ => 1.0,
        };
        let net = self.power.nets.iter().find(|n| n.generators.contains(&id) || n.consumers.contains(&id));
        Some(crate::views::PowerInfo {
            tier: p.tier,
            max_w: if generator { p.produce_w } else { p.use_w * over as f32 },
            connected: net.is_some(),
            satisfaction: net.map_or(0.0, |n| n.satisfaction),
        })
    }
}

/// True if a machine would work now if it had power: it has a recipe and is not waiting for
/// input or for room. Arms and drills want power all the time.
fn wants_work(b: &crate::Building) -> bool {
    match &b.logic {
        Logic::Machine(m) => m.recipe.is_some() && !matches!(b.status, crate::Status::NoInput | crate::Status::OutputFull | crate::Status::OutputBlocked | crate::Status::NoRecipe),
        Logic::Lab(_) => !matches!(b.status, crate::Status::NoInput | crate::Status::Idle),
        _ => true,
    }
}
