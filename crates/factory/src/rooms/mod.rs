//! Room machines: the kiln, the coke oven and the blast furnace (game design section 13,
//! technical design section 7.4).
//!
//! A room machine is a closed room of wall blocks with a controller and one or more hatches in
//! the wall. The inside of the room is real cells of the simulation.
//!
//! # The room
//!
//! - The player builds walls, a controller and hatches on the tile grid. `check` finds the room:
//!   it fills the inside tiles from the tile next to the controller (with a size limit) and checks
//!   the walls. Problems: a hole, too big, a wrong wall block, a broken wall, a second
//!   controller, no hatch.
//! - Wall blocks: buildings of kind `room_wall`. A wall must hold the heat of the machine: its
//!   `max_temp` must be at least the `max_temp` of the controller. So a kiln (1200 °C) takes
//!   clay brick walls and firebrick walls, and a blast furnace (1800 °C) only firebrick walls.
//!   Hatches (`room_port`), other controllers and bellows or blowers can also be in the wall.
//! - The room is checked again when a building is placed or removed, and every
//!   `CHECK_PERIOD` ticks (a wall that lost cells is a hole).
//!
//! # The fire
//!
//! The controller has a fuel slot (the data param `fuel_capacity`). While a recipe waits for
//! heat, the controller puts fuel from the slot as cells on the floor of the room (the fire bed),
//! lit. The fuel burns with the burning rules of the simulation and heats the cells of the room.
//! The controller also lets the smoke and other gases out (the chimney) and takes the ash out of
//! the room (the grate). It stops adding fuel when the room is `HEAT_MARGIN` °C hotter than the
//! recipe needs.
//!
//! The room temperature is the average temperature of the inside cells. Recipes with `min_temp`
//! run only when the room is hot enough. Bellows or a blower make the fire hotter
//! (`Buildings::set_blast`): the burning fuel and the air in the room get up to the fire
//! temperature of the fuel plus the blast.
//!
//! # Recipes and hatches
//!
//! The controller is a crafter (`machines::Machine`): its recipe inputs and outputs are in its
//! buffers. The player puts items in through the controller window. A hatch moves items between
//! the outside and the controller. Its role comes from its place in the wall:
//!
//! - in the roof (the room is below it): input. It takes powder that falls on it (from the end of
//!   a belt), and items from a crate, a barrel or a hopper on top of it. Gas outputs leave
//!   through it.
//! - in the floor (the room is above it): output. Products go into a crate or barrel below it,
//!   or out as cells.
//! - in a side wall: input and output.
//!
//! Products go into the building on the outer side if it takes them (a crate, a barrel, a mold),
//! or out as cells. A hatch never takes in an item that the room makes. Liquid products go out through the lowest
//! hatch first: the densest liquid (the iron) through the lowest hatch, the next one (the slag)
//! through the next hatch up. Gas products that cannot leave escape through the walls.

mod check;
mod fire;
mod hatch;
#[cfg(test)]
mod tests;

pub use check::{Problem, is_room_part};

use crate::buildings::{Building, Buildings, Logic};
use crate::machines::{self, Conditions, Status};
use crate::views::BuildingView;
use foundry_content::{Building as BuildingDef, Content, ItemRef, Side};
use foundry_core::{BuildingId, CellPos, TilePos};
use foundry_sim::Simulation;
use serde::{Deserialize, Serialize};

/// Ticks between two checks of a room (also when nothing was placed or removed).
pub const CHECK_PERIOD: u64 = 120;
/// Ticks between two runs of the fire work (fuel, gas, ash).
pub const FIRE_PERIOD: u64 = 2;
/// Ticks between two measurements of the room temperature.
pub const STATS_PERIOD: u64 = 10;
/// The controller adds fuel until the room is this much hotter than the recipe needs (°C).
pub const HEAT_MARGIN: i16 = 50;
/// The most ash the controller keeps. When it has more, new ash blows away as dust.
pub const ASH_LIMIT: u32 = 1000;
/// A blast from `set_blast` lasts this many ticks unless it is set again.
pub const BLAST_HOLD: u64 = 60;
/// Inside tiles of a room when the controller data has no `max_tiles`.
pub const DEFAULT_MAX_TILES: f32 = 24.0;

/// A hatch in the wall of a valid room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hatch {
    pub id: BuildingId,
    pub tile: TilePos,
    /// The side that faces out of the room. `None`: the hatch has no free side.
    pub outer: Option<Side>,
}

impl Hatch {
    /// A roof hatch (outer side up) only takes items in; a floor hatch (outer side down) only
    /// gives items out.
    pub fn takes_in(&self) -> bool {
        self.outer.is_some_and(|s| s != Side::Down)
    }

    pub fn gives_out(&self) -> bool {
        self.outer.is_some_and(|s| s != Side::Up)
    }
}

/// The shape of a valid room.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Shape {
    /// The inside tiles, row by row.
    pub inside: Vec<TilePos>,
    /// Inside tiles with no building on them. Their cells are the room cells.
    pub open: Vec<TilePos>,
    /// The cells of the fire bed: the bottom row of each open tile that stands on the wall.
    pub bed: Vec<CellPos>,
    pub hatches: Vec<Hatch>,
    /// The tiles of the wall around the room.
    pub walls: Vec<TilePos>,
}

impl Shape {
    /// Number of room cells.
    pub fn cell_count(&self) -> u32 {
        self.open.len() as u32 * (foundry_core::TILE_SIZE * foundry_core::TILE_SIZE) as u32
    }
}

/// The room of a controller. Only `ash` is saved; the rest is found again after a load.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoomState {
    /// Ash that the controller took out of the room. It goes out through a hatch.
    pub ash: u32,
    /// The room, if it is valid.
    #[serde(skip)]
    pub shape: Option<Shape>,
    /// What is wrong with the room, if it is not valid.
    #[serde(skip)]
    pub problem: Option<Problem>,
    /// The average temperature of the room cells (°C), from the last measurement.
    #[serde(skip)]
    pub temperature: Option<i16>,
    /// Fuel cells in the room, and how many of them are at or above their ignition temperature.
    #[serde(skip)]
    pub fuel_cells: u32,
    #[serde(skip)]
    pub hot_fuel_cells: u32,
    /// The highest fire temperature of the burning fuel in the room (°C).
    #[serde(skip)]
    pub fire_temp: i16,
    /// Extra fire temperature from bellows or a blower (°C), until the tick `blast_until`.
    #[serde(skip)]
    pub blast: i16,
    #[serde(skip)]
    pub blast_until: u64,
    /// `Buildings::layout` at the last check.
    #[serde(skip)]
    checked_layout: Option<u64>,
    #[serde(skip)]
    next_check: u64,
}

impl RoomState {
    /// A room state for a building type of kind `room_controller`.
    pub fn for_kind(def: &BuildingDef) -> Option<Box<RoomState>> {
        (def.kind == "room_controller").then(Box::default)
    }

    pub fn is_valid(&self) -> bool {
        self.shape.is_some()
    }

    /// Speed factor from the room size: the open inside tiles times the data param
    /// `speed_per_tile` of the controller (default 1). A bigger room works on more at once.
    pub fn speed(&self, controller: &BuildingDef) -> f64 {
        let per_tile = controller.param("speed_per_tile", 1.0) as f64;
        self.shape.as_ref().map_or(1.0, |s| (s.open.len() as f64 * per_tile).max(1.0))
    }

    /// The extra fire temperature now.
    pub fn blast_at(&self, now: u64) -> i16 {
        if now < self.blast_until { self.blast } else { 0 }
    }
}

/// The room part of the building window.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RoomView {
    pub valid: bool,
    /// What is wrong, as a sentence for the player. `None` if the room is valid.
    pub problem: Option<String>,
    /// The tile to mark red (a hole or a wrong wall block).
    pub problem_tile: Option<TilePos>,
    /// Average temperature of the room cells (°C).
    pub temperature: Option<i16>,
    /// The temperature that the recipe needs (°C).
    pub needs: Option<i16>,
    /// Inside tiles and the limit.
    pub tiles: u32,
    pub max_tiles: u32,
    pub hatches: u32,
    /// Fuel cells in the room, and how many are hot enough to burn.
    pub fuel_cells: u32,
    pub hot_fuel_cells: u32,
    pub ash: u32,
    /// Extra fire temperature from bellows or a blower (°C).
    pub blast: i16,
    /// The wall block names that this room takes, for example "Clay brick wall or Firebrick wall".
    pub walls: String,
    /// The recipe speed from the room size (see `RoomState::speed`).
    pub speed: f32,
}

/// The names of the wall blocks that a controller takes.
pub fn wall_names(content: &Content, controller: &BuildingDef) -> String {
    let names: Vec<&str> = content
        .factory
        .buildings
        .iter()
        .filter(|d| d.kind == "room_wall" && d.max_temp >= controller.max_temp)
        .map(|d| d.name.as_str())
        .collect();
    names.join(" or ")
}

/// How many of `n` units of an item go into the recipe input first, when the item is both a
/// recipe input and a fuel of the machine (wood in a kiln that makes charcoal, coal in a coke
/// oven). The input buffer and the fuel slot then fill to about the same part of their size, so
/// the fire gets fuel too. For other items: all `n`.
pub fn input_share(m: &machines::Machine, content: &Content, item: ItemRef, n: u32) -> u32 {
    let (Some(r), Some(f), ItemRef::Material(mat)) = (m.recipe, m.fuel.as_ref(), item) else { return n };
    let recipe = content.factory.recipe_def(r);
    let Some(k) = recipe.inputs.iter().position(|s| s.item == item) else { return n };
    if !machines::is_fuel(content, mat) || f.room(mat) == 0 {
        return n;
    }
    let (input, input_cap) = (m.inputs[k] as u64, m.input_capacity(recipe, k).max(1) as u64);
    let (fuel, fuel_cap) = (f.units as u64, f.capacity.max(1) as u64);
    let n64 = n as u64;
    // (input + a) / input_cap = (fuel + n - a) / fuel_cap, solved for a.
    let a = ((fuel + n64) * input_cap).saturating_sub(input * fuel_cap) / (input_cap + fuel_cap);
    a.min(n64) as u32
}

/// Run one tick of a room controller (called from `Buildings::work`). Returns the status.
pub(crate) fn work(b: &mut Building, content: &Content, def: &BuildingDef, seed: u64) -> Status {
    b.power_w = 0.0;
    let too_hot = b.temperature > def.max_temp;
    let (valid, temp, speed) = match b.room.as_deref() {
        Some(r) => (r.is_valid(), r.temperature.unwrap_or(b.temperature), r.speed(def)),
        None => (false, b.temperature, 1.0),
    };
    let Logic::Machine(m) = &mut b.logic else {
        b.status = Status::Idle;
        return Status::Idle;
    };
    let Some(r) = m.recipe else {
        b.status = Status::NoRecipe;
        return Status::NoRecipe;
    };
    if !valid {
        b.status = Status::NoRoom;
        return Status::NoRoom;
    }
    let recipe = content.factory.recipe_def(r);
    b.heat = temp;
    let cond = Conditions {
        speed: def.speed as f64 * machines::overclock_speed(def.tier, recipe.tier) * speed,
        has_power: true,
        too_hot,
        heat: temp,
    };
    // The fuel slot feeds the fire in the room. The recipe itself burns no fuel.
    let fuel = m.fuel.take();
    let status = m.step(recipe, &cond, seed);
    m.fuel = fuel;
    b.status = status;
    status
}

/// Run all room controllers for one tick: check the rooms, move items through the hatches, and
/// tend the fire. Call it after `Buildings::tick`.
pub fn tick(content: &Content, buildings: &mut Buildings, sim: &mut Simulation) {
    let now = buildings.now;
    let layout = buildings.layout;
    blow_bellows(content, buildings);
    for i in 0..buildings.slots.len() as u32 {
        let Some(room) = buildings.at_index(i).and_then(|b| b.room.as_deref()) else { continue };
        let id = buildings.id_of(i);
        if room.checked_layout != Some(layout) || now >= room.next_check {
            let result = check::check(content, buildings, id);
            let b = buildings.at_index_mut(i).expect("the controller exists");
            let room = b.room.as_deref_mut().expect("the controller has a room");
            let was_valid = room.is_valid();
            let old_problem = room.problem.clone();
            match result {
                Ok(shape) => {
                    room.shape = Some(shape);
                    room.problem = None;
                }
                Err(p) => {
                    room.shape = None;
                    room.problem = Some(p);
                    room.temperature = None;
                }
            }
            room.checked_layout = Some(layout);
            // Spread the checks of many rooms over the ticks.
            room.next_check = now + CHECK_PERIOD + (i as u64 % 16);
            let changed = was_valid != room.is_valid() || old_problem != room.problem;
            if changed {
                buildings.wake_index(i);
            }
        }
        let valid = buildings.at_index(i).and_then(|b| b.room.as_deref()).is_some_and(|r| r.is_valid());
        if !valid {
            continue;
        }
        hatch::run(content, buildings, sim, i, now);
        if now.is_multiple_of(FIRE_PERIOD) {
            fire::run(content, buildings, sim, i, now);
        }
    }
}

/// After a load: controllers from an older save get their room state.
pub fn upgrade(content: &Content, buildings: &mut Buildings) {
    for slot in &mut buildings.slots {
        let Some(b) = slot.building.as_mut() else { continue };
        if b.room.is_none() {
            b.room = RoomState::for_kind(content.factory.building_def(b.kind));
        }
    }
}

/// Add the room part to a building window.
pub fn fill_view(content: &Content, buildings: &Buildings, v: &mut BuildingView) {
    let Some(b) = buildings.get(v.id) else { return };
    let Some(room) = b.room.as_deref() else { return };
    let def = content.factory.building_def(b.kind);
    let needs = match &b.logic {
        Logic::Machine(m) => m.recipe.and_then(|r| content.factory.recipe_def(r).min_temp),
        _ => None,
    };
    let fuel_units = match &b.logic {
        Logic::Machine(m) => m.fuel.map_or(0, |f| f.units),
        _ => 0,
    };
    let problem = room.problem.as_ref().map(|p| p.text(content, def, b.at));
    if b.status == Status::NoRoom
        && let Some(p) = &room.problem
    {
        v.reason = p.short().to_string();
    }
    if b.status == Status::TooCold
        && let (Some(t), Some(n)) = (room.temperature, needs)
    {
        v.reason = if fuel_units == 0 && room.hot_fuel_cells == 0 {
            format!("Room {t} °C, needs {n} °C. No fuel.")
        } else {
            format!("Room {t} °C, needs {n} °C. Heating.")
        };
    }
    let shape = room.shape.as_ref();
    v.room = Some(RoomView {
        valid: room.is_valid(),
        problem,
        problem_tile: room.problem.as_ref().and_then(|p| p.tile()),
        temperature: room.temperature,
        needs,
        tiles: shape.map_or(0, |s| s.inside.len() as u32),
        max_tiles: def.param("max_tiles", DEFAULT_MAX_TILES) as u32,
        hatches: shape.map_or(0, |s| s.hatches.len() as u32),
        fuel_cells: room.fuel_cells,
        hot_fuel_cells: room.hot_fuel_cells,
        ash: room.ash,
        blast: room.blast_at(buildings.now),
        walls: wall_names(content, def),
        speed: room.speed(def) as f32,
    });
}

impl Buildings {
    /// The room of a controller.
    pub fn room(&self, controller: BuildingId) -> Option<&RoomState> {
        self.get(controller)?.room.as_deref()
    }

    /// The controller of the valid room that has this tile inside it or in its wall.
    /// Bellows and blowers in a room wall use it to find their room.
    pub fn room_at(&self, tile: TilePos) -> Option<BuildingId> {
        for i in 0..self.slots.len() as u32 {
            let Some(shape) = self.at_index(i).and_then(|b| b.room.as_deref()).and_then(|r| r.shape.as_ref()) else { continue };
            if shape.inside.contains(&tile) || shape.walls.contains(&tile) {
                return Some(self.id_of(i));
            }
        }
        None
    }

    /// Make the fire of a room hotter by `temperature` °C for the next `BLAST_HOLD` ticks
    /// (bellows or a blower call this while they work). Returns false if the building is not a
    /// controller.
    pub fn set_blast(&mut self, controller: BuildingId, temperature: i16) -> bool {
        let now = self.now;
        let Some(room) = self.get_mut(controller).and_then(|b| b.room.as_deref_mut()) else { return false };
        room.blast = temperature.max(0);
        room.blast_until = now + BLAST_HOLD;
        true
    }
}

/// Extra fire temperature that bellows give a room (°C).
pub const BELLOWS_BLAST: i16 = 300;

/// Bellows in the wall of a valid room, or right next to it, blow air into its fire: they set the
/// blast of the room now and then (it holds for `BLAST_HOLD` ticks).
fn blow_bellows(content: &Content, buildings: &mut Buildings) {
    if !buildings.now.is_multiple_of(BLAST_HOLD / 2) {
        return;
    }
    let bellows: Vec<(BuildingId, TilePos, f64)> = buildings
        .iter()
        .filter(|(_, b)| content.factory.building_def(b.kind).kind == "bellows")
        .map(|(id, b)| (id, b.at, content.factory.building_def(b.kind).power.as_ref().map_or(0.0, |p| p.steam_per_s as f64)))
        .collect();
    let hold = BLAST_HOLD / 2;
    for (id, at, steam_use) in bellows {
        let tiles = [at, TilePos::new(at.x - 1, at.y), TilePos::new(at.x + 1, at.y), TilePos::new(at.x, at.y - 1), TilePos::new(at.x, at.y + 1)];
        let Some(controller) = tiles.iter().find_map(|t| buildings.room_at(*t)) else { continue };
        // A steam blower blows only while it gets steam (it uses it for the time the blast holds).
        if steam_use > 0.0 {
            let Some(b) = buildings.get_mut(id) else { continue };
            let need = steam_use * hold as f64 / foundry_core::TICKS_PER_SECOND as f64;
            let has = matches!(b.steam, crate::steam::SteamState::Machine(t) if t.amount >= need);
            b.status = if has { crate::Status::Working } else { crate::Status::NoPower };
            b.steam_reason = (!has).then(|| "Needs steam from a connected bronze pipe".to_string());
            if !has {
                continue;
            }
            crate::steam::consume_steam(&mut b.steam, need);
        }
        buildings.set_blast(controller, BELLOWS_BLAST);
    }
}
