//! Placed buildings: the registry, the tick, ports, damage and the building window data.
//!
//! - Buildings are in a slot list. A `BuildingId` has an index and a generation; the generation
//!   changes when a slot is used again, so an old id does not find a new building.
//! - Each layer (front and back) has a map from `TilePos` to the building on that tile. A map
//!   works for a world without edges.
//! - A building's logic comes from its data `kind` (see `Logic::for_kind`).
//! - Only awake buildings run. A building that has nothing to do goes to sleep, for a time or until
//!   something wakes it (the player, a neighbor that gives it items, a new recipe).
//!
//! Tick order (technical design section 7.1): ports take input, machines work, ports give output,
//! belts move. Then every building is checked for damage once every `DAMAGE_PERIOD` ticks.
//!
//! Placement and removal are in `placement.rs`.

use crate::cells::{self, is_fluid, phase};
use crate::geometry::{Transform, neighbor_tile, opposite, tiles_to_cells};
use crate::inventory::{Inventory, TankRule};
use crate::logistics::{Belt, Hopper, Lab, steps_in_tick};
use crate::machines::{self, Conditions, Fuel, Machine, RecipeError, Status, is_fuel, output_stack};
use crate::progress_link::{self, ProgressLink};
use crate::steam::{SteamState, consume_steam, steam_ready};
use crate::views::{BufferView, BuildingView, FuelView, PortView};
use foundry_content::{Building as BuildingDef, Content, ItemRef, Layer, Phase, PortKind, Side, Stack};
use foundry_core::{BuildingId, BuildingKindId, CellPos, MaterialId, PartId, RecipeId, TILE_SIZE, TilePos};
use foundry_sim::Simulation;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

/// Ticks between two damage checks of one building.
pub const DAMAGE_PERIOD: u64 = 32;
/// Ticks between two part moves through the part ports of one building.
pub const PART_PERIOD: u64 = 10;
/// Cells that one bulk or fluid port moves in one tick, at most.
pub const PORT_CELLS_PER_TICK: u32 = 4;
/// Rows of cells outside an input port that it takes cells from.
pub const INPUT_DEPTH: i32 = 3;
/// The tallest column of powder that a belt moves.
pub const MAX_BELT_COLUMN: i32 = 32;

/// The logic of a building and its state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Logic {
    /// No logic: walls, and kinds that the factory code does not know yet.
    Passive,
    /// Only makes hand crafting faster near it.
    Workbench,
    Machine(Machine),
    /// A crate (slots) or a barrel (tanks).
    Storage(Inventory),
    Hopper(Hopper),
    Belt(Belt),
    /// Holds deliveries until the progression system takes them.
    Hub(Inventory),
    Lab(Lab),
}

impl Logic {
    /// The logic for a building type, from its data `kind`.
    /// Any kind with recipe categories (`crafts`) that is not listed here is a crafter.
    pub fn for_kind(def: &BuildingDef) -> Logic {
        let slots = def.param("slots", 8.0).max(0.0) as usize;
        let capacity = def.param("capacity", 1000.0).max(0.0) as u32;
        match def.kind.as_str() {
            "workbench" => Logic::Workbench,
            // A storage with slots (a crate): each slot holds a part stack or bulk material.
            // A storage with only tanks (a barrel) keeps liquids.
            "storage" if slots > 0 => Logic::Storage(Inventory::mixed(slots, capacity, TankRule::Bulk)),
            "storage" => {
                let mut inv = Inventory::new(0, def.param("tanks", 1.0).max(0.0) as usize, capacity);
                inv.takes = TankRule::Liquid;
                Logic::Storage(inv)
            }
            "hopper" => Logic::Hopper(Hopper::new(def.param("capacity", 64.0).max(1.0) as u32)),
            "belt" => Logic::Belt(Belt::default()),
            "hub" => Logic::Hub(Inventory::mixed(def.param("slots", 16.0).max(0.0) as usize, capacity, TankRule::Any)),
            "lab" => Logic::Lab(Lab::new(def.param("kit_buffer", 10.0) as u32)),
            _ if !def.crafts.is_empty() => {
                let m = Machine::new(def.param("buffer_crafts", 2.0) as u32);
                // A crafter with the param "fuel_capacity" has a fuel slot (the campfire).
                let capacity = def.param("fuel_capacity", 0.0);
                if capacity >= 1.0 {
                    Logic::Machine(m.with_fuel(Fuel::new(capacity as u32, def.param("fuel_ticks", 60.0).max(1.0) as u32)))
                } else {
                    Logic::Machine(m)
                }
            }
            _ => Logic::Passive,
        }
    }
}

/// A port after placement: its world tile and the side it faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacedPort {
    /// Index into the building type's `ports`. `None` for a default port (hoppers without ports
    /// in the data get one input on top and one output below).
    pub def: Option<u8>,
    pub kind: PortKind,
    pub tile: TilePos,
    pub side: Side,
}

/// The ports of a building type placed at `at` with a transform.
pub fn placed_ports(def: &BuildingDef, at: TilePos, t: Transform) -> Vec<PlacedPort> {
    let place = |tile: (u8, u8), side: Side, kind: PortKind, index: Option<u8>| {
        let (x, y) = t.tile(def.size, tile);
        PlacedPort { def: index, kind, tile: TilePos::new(at.x + x as i32, at.y + y as i32), side: t.side(side) }
    };
    let mut v: Vec<PlacedPort> =
        def.ports.iter().enumerate().map(|(i, p)| place(p.tile, p.side, p.kind, Some(i as u8))).collect();
    if v.is_empty() && def.kind == "hopper" {
        v.push(place((0, 0), Side::Up, PortKind::BulkIn, None));
        v.push(place((0, def.size.1 - 1), Side::Down, PortKind::BulkOut, None));
    }
    v
}

/// A placed building.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Building {
    pub kind: BuildingKindId,
    /// Top-left tile of the placed footprint.
    pub at: TilePos,
    pub transform: Transform,
    /// Size in tiles after the transform.
    pub size: (u8, u8),
    pub layer: Layer,
    pub ports: Vec<PlacedPort>,
    pub hit_points: f32,
    /// Average temperature of the body cells at the last check (°C).
    pub temperature: i16,
    /// The temperature that the recipe's `min_temp` was checked against in the last tick (°C):
    /// the cells in front of the heat ports, or the body.
    pub heat: i16,
    /// Body cells that are no longer the body material.
    pub lost_cells: u32,
    /// 0 to 1: the part of the needed power that the building gets. The power network sets it.
    /// Buildings that do not use electric power ignore it.
    pub power_factor: f32,
    /// Power use in the last tick (W).
    pub power_w: f32,
    pub status: Status,
    pub logic: Logic,
    /// Persisted boiler, pipe and steam-machine fluid state.
    #[serde(default)]
    pub steam: SteamState,
    /// More useful text for a steam-starved machine than the generic power status.
    #[serde(skip)]
    pub steam_reason: Option<String>,
    /// The tick at which a sleeping building wakes. `None` while awake or asleep until woken.
    pub(crate) timer: Option<u64>,
    /// Ticks in a row with nothing done.
    idle: u32,
    /// Something happened in this tick.
    busy: bool,
    /// An output port had no room in this tick.
    blocked: bool,
    /// An exhaust port had no room in this tick. The machine stops until it has room.
    exhaust_blocked: bool,
    next_pull: u64,
    next_push: u64,
}

impl Building {
    pub fn new(def: &BuildingDef, kind: BuildingKindId, at: TilePos, t: Transform) -> Self {
        Self {
            kind,
            at,
            transform: t,
            size: t.size(def.size),
            layer: def.layer,
            ports: placed_ports(def, at, t),
            hit_points: def.hit_points as f32,
            temperature: foundry_core::DEFAULT_TEMPERATURE,
            heat: foundry_core::DEFAULT_TEMPERATURE,
            lost_cells: 0,
            power_factor: 1.0,
            power_w: 0.0,
            status: Status::Idle,
            logic: Logic::for_kind(def),
            steam: SteamState::for_kind(&def.kind, def.power.as_ref().is_some_and(|p| p.steam_per_s > 0.0)),
            steam_reason: None,
            timer: None,
            idle: 0,
            busy: false,
            blocked: false,
            exhaust_blocked: false,
            next_pull: 0,
            next_push: 0,
        }
    }

    /// The cells of the footprint.
    pub fn cell_rect(&self) -> foundry_core::CellRect {
        tiles_to_cells(self.at, self.size)
    }

    /// The tiles of the footprint.
    pub fn tiles(&self) -> impl Iterator<Item = TilePos> + use<> {
        let (at, size) = (self.at, self.size);
        (0..size.1 as i32).flat_map(move |y| (0..size.0 as i32).map(move |x| TilePos::new(at.x + x, at.y + y)))
    }

    /// True if the building is sleeping.
    pub fn is_asleep(&self) -> bool {
        self.timer.is_some()
    }
}

/// Something that happened to a building. The game shows alerts for these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FactoryEvent {
    /// A building broke (heat or lost body cells). Its contents and some scrap are now cells.
    Broke { id: BuildingId, kind: BuildingKindId, at: TilePos },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Slot {
    pub(crate) generation: u32,
    pub(crate) building: Option<Building>,
}

/// All placed buildings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "BuildingsSave", into = "BuildingsSave")]
pub struct Buildings {
    pub(crate) slots: Vec<Slot>,
    pub(crate) free: Vec<u32>,
    /// Front-layer building on each tile.
    pub(crate) front: HashMap<TilePos, BuildingId>,
    /// Back-layer building on each tile.
    pub(crate) back: HashMap<TilePos, BuildingId>,
    /// Buildings that run in the next tick, in index order.
    pub(crate) active: BTreeSet<u32>,
    /// Sleeping buildings with a wake tick: (tick, index).
    pub(crate) timers: BTreeSet<(u64, u32)>,
    pub(crate) workbenches: BTreeSet<u32>,
    /// Placed buildings of each type.
    pub(crate) kind_counts: HashMap<BuildingKindId, u32>,
    /// What the Hub takes, from the progression at the start of each tick. Not saved.
    pub(crate) hub: HubRule,
    /// Ticks run.
    pub(crate) now: u64,
    /// Seed for random numbers (byproduct chances).
    pub(crate) seed: u64,
    pub(crate) events: Vec<FactoryEvent>,
}

/// The saved form of `Buildings`. The tile maps, the workbench list and the counts of each
/// building type are made again on load.
#[derive(Serialize, Deserialize)]
struct BuildingsSave {
    slots: Vec<Slot>,
    free: Vec<u32>,
    active: BTreeSet<u32>,
    timers: BTreeSet<(u64, u32)>,
    now: u64,
    seed: u64,
}

impl From<Buildings> for BuildingsSave {
    fn from(b: Buildings) -> Self {
        Self { slots: b.slots, free: b.free, active: b.active, timers: b.timers, now: b.now, seed: b.seed }
    }
}

impl From<BuildingsSave> for Buildings {
    fn from(s: BuildingsSave) -> Self {
        let mut b = Buildings {
            slots: s.slots,
            free: s.free,
            front: HashMap::new(),
            back: HashMap::new(),
            active: s.active,
            timers: s.timers,
            workbenches: BTreeSet::new(),
            kind_counts: HashMap::new(),
            hub: HubRule::default(),
            now: s.now,
            seed: s.seed,
            events: vec![],
        };
        for i in 0..b.slots.len() {
            let generation = b.slots[i].generation;
            let Some(bd) = &b.slots[i].building else { continue };
            let id = BuildingId { index: i as u32, generation };
            let tiles: Vec<TilePos> = bd.tiles().collect();
            let layer = bd.layer;
            if matches!(bd.logic, Logic::Workbench) {
                b.workbenches.insert(i as u32);
            }
            *b.kind_counts.entry(bd.kind).or_insert(0) += 1;
            let map = if layer == Layer::Front { &mut b.front } else { &mut b.back };
            for t in tiles {
                map.insert(t, id);
            }
        }
        b
    }
}

impl Default for Buildings {
    fn default() -> Self {
        Self {
            slots: vec![],
            free: vec![],
            front: HashMap::new(),
            back: HashMap::new(),
            active: BTreeSet::new(),
            timers: BTreeSet::new(),
            workbenches: BTreeSet::new(),
            kind_counts: HashMap::new(),
            hub: HubRule::default(),
            now: 0,
            seed: 0x6275_696c_6469_6e67,
            events: vec![],
        }
    }
}

/// Split two different slots for mutable access.
fn two_mut(slots: &mut [Slot], i: usize, j: usize) -> Option<(&mut Building, &mut Building)> {
    if i == j || i >= slots.len() || j >= slots.len() {
        return None;
    }
    let (a, b) = if i < j {
        let (l, r) = slots.split_at_mut(j);
        (&mut l[i], &mut r[0])
    } else {
        let (l, r) = slots.split_at_mut(i);
        (&mut r[0], &mut l[j])
    };
    Some((a.building.as_mut()?, b.building.as_mut()?))
}

/// True if the port's filter lets the item pass.
fn port_allows(def: &BuildingDef, port: &PlacedPort, item: ItemRef) -> bool {
    match port.def {
        Some(i) => {
            let f = &def.ports[i as usize].filter;
            f.is_empty() || f.contains(&item)
        }
        None => true,
    }
}

/// True if a part is a research kit: a technology needs it, or it is a part (not a building) in
/// the "research" category.
pub fn is_kit(content: &Content, part: PartId) -> bool {
    let p = content.factory.part_def(part);
    (p.category == "research" && p.building.is_none())
        || content.factory.techs.iter().any(|t| t.kits.iter().any(|s| s.item == ItemRef::Part(part)))
}

/// True if a Hub milestone after stage `done_stage` asks for the item. (`done_stage` is the last
/// Hub repair stage that is done; 0 if none.)
pub fn is_deliverable(content: &Content, item: ItemRef, done_stage: u8) -> bool {
    content.factory.milestones.iter().any(|m| m.stage > done_stage && m.deliver.iter().any(|s| s.item == item))
}

/// What the Hub takes: only items that a repair stage after `stage` needs, and (when `need` is
/// known) only as many as the stages still need.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HubRule {
    /// The last Hub repair stage that is done (0: none).
    pub stage: u8,
    /// What the stages still need: the rest of the next stage and all of the later stages.
    /// `None`: not known yet (before the first tick); then only the stage number counts.
    pub need: Option<Arc<Vec<Stack>>>,
}

impl HubRule {
    /// How many more of `item` the Hub takes, when it holds `held` of it.
    fn room(&self, content: &Content, item: ItemRef, held: u32) -> u32 {
        if !is_deliverable(content, item, self.stage) {
            return 0;
        }
        match &self.need {
            Some(list) => list.iter().filter(|s| s.item == item).map(|s| s.count).sum::<u32>().saturating_sub(held),
            None => u32::MAX,
        }
    }
}

/// How many of an item a building can take now.
fn accept_room(logic: &Logic, content: &Content, item: ItemRef, hub: &HubRule) -> u32 {
    match logic {
        Logic::Machine(m) => {
            let input = m.recipe.map_or(0, |r| m.input_room(content.factory.recipe_def(r), item));
            input.saturating_add(fuel_room(m, content, item))
        }
        Logic::Storage(inv) => inv.room_for(content, item, u32::MAX),
        Logic::Hub(inv) => hub.room(content, item, inv.count(item)).min(inv.room_for(content, item, u32::MAX)),
        Logic::Lab(lab) => match item {
            ItemRef::Part(p) if is_kit(content, p) => lab.kit_limit.saturating_sub(lab.kits.count(p)),
            _ => 0,
        },
        Logic::Hopper(h) => match item {
            ItemRef::Material(m) if phase(content, m) == Phase::Powder && h.accepts(m) => {
                h.capacity - h.cells.len() as u32
            }
            _ => 0,
        },
        _ => 0,
    }
}

/// How many units of `item` the fuel slot of a machine takes now (0 if it is not a fuel).
fn fuel_room(m: &Machine, content: &Content, item: ItemRef) -> u32 {
    match (item, &m.fuel) {
        (ItemRef::Material(mat), Some(f)) if is_fuel(content, mat) => f.room(mat),
        _ => 0,
    }
}

/// A fuel that a burner building burns, for the empty fuel slot: the first fuel in the filter of
/// its input ports.
fn fuel_hint(content: &Content, def: &BuildingDef) -> Option<MaterialId> {
    def.ports.iter().flat_map(|p| p.filter.iter()).find_map(|i| match *i {
        ItemRef::Material(m) if is_fuel(content, m) => Some(m),
        _ => None,
    })
}

/// Give up to `n` of an item to a building. Returns the count taken.
fn accept(logic: &mut Logic, content: &Content, item: ItemRef, n: u32, hub: &HubRule) -> u32 {
    let n = n.min(accept_room(logic, content, item, hub));
    if n == 0 {
        return 0;
    }
    match logic {
        Logic::Machine(m) => {
            // Recipe inputs first, then the fuel slot.
            let taken = m.recipe.map_or(0, |r| m.add_input(content.factory.recipe_def(r), item, n));
            let fuel = match (item, m.fuel.as_mut()) {
                (ItemRef::Material(mat), Some(f)) if taken < n && is_fuel(content, mat) => f.add(mat, n - taken),
                _ => 0,
            };
            taken + fuel
        }
        Logic::Storage(inv) | Logic::Hub(inv) => n - inv.insert(content, item, n),
        Logic::Lab(lab) => match item {
            ItemRef::Part(p) => {
                lab.kits.add(p, n);
                n
            }
            ItemRef::Material(_) => 0,
        },
        Logic::Hopper(h) => match item {
            ItemRef::Material(m) => (0..n).take_while(|_| h.push(m)).count() as u32,
            ItemRef::Part(_) => 0,
        },
        _ => 0,
    }
}

/// Everything a building holds, as stacks. The building is emptied.
pub(crate) fn take_contents(logic: &mut Logic, content: &Content) -> Vec<Stack> {
    match logic {
        Logic::Machine(m) => {
            let mut out = m.take_contents(content);
            out.extend(m.take_fuel());
            out
        }
        Logic::Storage(inv) | Logic::Hub(inv) => inv.take_all(),
        Logic::Hopper(h) => {
            let out = h.counts().into_iter().map(|(m, n)| Stack { item: ItemRef::Material(m), count: n }).collect();
            h.cells.clear();
            out
        }
        Logic::Lab(lab) => {
            let list: Vec<(PartId, u32)> = lab.kits.iter().collect();
            list.into_iter()
                .map(|(p, n)| {
                    let taken = lab.kits.take(p, n);
                    Stack { item: ItemRef::Part(p), count: taken }
                })
                .filter(|s| s.count > 0)
                .collect()
        }
        Logic::Passive | Logic::Workbench | Logic::Belt(_) => vec![],
    }
}

/// Stacks as world cells. A part becomes `units` cells of its material; a part without a
/// material has no cells.
pub fn stacks_to_cells(content: &Content, stacks: &[Stack]) -> Vec<(MaterialId, u32)> {
    let mut out: Vec<(MaterialId, u32)> = vec![];
    for s in stacks {
        let (m, n) = match s.item {
            ItemRef::Material(m) => (m, s.count),
            ItemRef::Part(p) => {
                let part = content.factory.part_def(p);
                match part.material {
                    Some(m) if part.units > 0 => (m, s.count.saturating_mul(part.units as u32)),
                    _ => continue,
                }
            }
        };
        if n == 0 || m.is_air() {
            continue;
        }
        match out.iter_mut().find(|(x, _)| *x == m) {
            Some((_, c)) => *c += n,
            None => out.push((m, n)),
        }
    }
    out
}

/// The temperature a machine's `min_temp` is checked against: the average of the cells in front
/// of its heat ports, or the body temperature if it has no heat port.
fn heat_reading(sim: &Simulation, ports: &[PlacedPort], body: i16) -> i16 {
    let mut sum = 0i32;
    let mut n = 0i32;
    for p in ports.iter().filter(|p| p.kind == PortKind::Heat) {
        sum += cells::side_temperature(sim, p.tile, p.side, 1) as i32;
        n += 1;
    }
    if n == 0 { body } else { (sum / n) as i16 }
}

/// The belt direction: +1 right, -1 left.
pub fn belt_direction(t: Transform) -> i32 {
    if t.side(Side::Right) == Side::Left { -1 } else { 1 }
}

/// Move each column of powder that rests on the belt top one cell in `dir`.
/// Returns the number of cells moved.
fn move_belt_cells(sim: &mut Simulation, content: &Content, r: foundry_core::CellRect, dir: i32) -> u32 {
    let top = r.y0 - 1;
    let mut moved = 0;
    for k in 0..r.width() {
        // The leading column moves first, so every column moves into a free place.
        let x = if dir > 0 { r.x1 - 1 - k } else { r.x0 + k };
        for up in 0..MAX_BELT_COLUMN {
            let from = CellPos::new(x, top - up);
            let c = sim.cell(from);
            if phase(content, c.material) != Phase::Powder {
                break;
            }
            let to = CellPos::new(x + dir, top - up);
            if !sim.cell(to).material.is_air() {
                break;
            }
            sim.set_cell(to, c.material, Some(c.temperature));
            sim.set_cell(from, MaterialId::AIR, None);
            moved += 1;
        }
    }
    moved
}

impl Buildings {
    pub fn new(_content: &Content) -> Self {
        Self::default()
    }

    /// Ticks run so far.
    pub fn now(&self) -> u64 {
        self.now
    }

    /// How many buildings of a type are placed.
    pub fn count_of(&self, kind: BuildingKindId) -> u32 {
        self.kind_counts.get(&kind).copied().unwrap_or(0)
    }

    /// Wake all labs (for example when the current research changes).
    pub fn wake_labs(&mut self) {
        let labs: Vec<u32> = (0..self.slots.len() as u32)
            .filter(|&i| matches!(self.at_index(i).map(|b| &b.logic), Some(Logic::Lab(_))))
            .collect();
        for i in labs {
            self.wake_index(i);
        }
    }

    /// The building with this id, if it still exists.
    pub fn get(&self, id: BuildingId) -> Option<&Building> {
        let s = self.slots.get(id.index as usize)?;
        if s.generation != id.generation {
            return None;
        }
        s.building.as_ref()
    }

    pub fn get_mut(&mut self, id: BuildingId) -> Option<&mut Building> {
        let s = self.slots.get_mut(id.index as usize)?;
        if s.generation != id.generation {
            return None;
        }
        s.building.as_mut()
    }

    pub(crate) fn at_index(&self, i: u32) -> Option<&Building> {
        self.slots.get(i as usize)?.building.as_ref()
    }

    pub(crate) fn at_index_mut(&mut self, i: u32) -> Option<&mut Building> {
        self.slots.get_mut(i as usize)?.building.as_mut()
    }

    pub(crate) fn id_of(&self, i: u32) -> BuildingId {
        BuildingId { index: i, generation: self.slots[i as usize].generation }
    }

    /// All buildings, in index order.
    pub fn iter(&self) -> impl Iterator<Item = (BuildingId, &Building)> + '_ {
        self.slots.iter().enumerate().filter_map(|(i, s)| {
            s.building.as_ref().map(|b| (BuildingId { index: i as u32, generation: s.generation }, b))
        })
    }

    /// Number of placed buildings.
    pub fn len(&self) -> usize {
        self.slots.iter().filter(|s| s.building.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Number of buildings that run in the next tick.
    pub fn awake_count(&self) -> usize {
        self.active.len()
    }

    /// The building on a tile in a layer.
    pub fn at_tile(&self, tile: TilePos, layer: Layer) -> Option<BuildingId> {
        let map = if layer == Layer::Front { &self.front } else { &self.back };
        map.get(&tile).copied()
    }

    /// Events since the last call.
    pub fn take_events(&mut self) -> Vec<FactoryEvent> {
        std::mem::take(&mut self.events)
    }

    /// Run the building in the next tick.
    pub fn wake(&mut self, id: BuildingId) {
        if self.get(id).is_some() {
            self.wake_index(id.index);
        }
    }

    pub(crate) fn wake_index(&mut self, i: u32) {
        let Some(b) = self.at_index_mut(i) else { return };
        b.idle = 0;
        if let Some(t) = b.timer.take() {
            self.timers.remove(&(t, i));
        }
        self.active.insert(i);
    }

    /// Stop running the building: for `ticks` ticks, or until woken if `ticks` is `None`.
    fn sleep(&mut self, i: u32, ticks: Option<u64>) {
        self.active.remove(&i);
        let now = self.now;
        let Some(b) = self.at_index_mut(i) else { return };
        let old = b.timer.take();
        let new = ticks.map(|d| now + d.max(1));
        b.timer = new;
        if let Some(t) = old {
            self.timers.remove(&(t, i));
        }
        if let Some(t) = new {
            self.timers.insert((t, i));
        }
    }

    /// Set the power factor (0 to 1) of a building. The power network calls this.
    pub fn set_power_factor(&mut self, id: BuildingId, factor: f32) {
        if let Some(b) = self.get_mut(id) {
            let changed = b.power_factor != factor;
            b.power_factor = factor.clamp(0.0, 1.0);
            if changed {
                self.wake(id);
            }
        }
    }

    /// Recipes a building can run: its categories, up to its tier, in data order.
    pub fn recipes_for(content: &Content, kind: BuildingKindId) -> Vec<RecipeId> {
        let def = content.factory.building_def(kind);
        if !matches!(Logic::for_kind(def), Logic::Machine(_)) {
            return vec![];
        }
        content
            .factory
            .recipes
            .iter()
            .enumerate()
            .filter(|(_, r)| def.crafts.contains(&r.category) && r.tier <= def.tier)
            .map(|(i, _)| RecipeId(i as u16))
            .collect()
    }

    /// Set the recipe of a machine (or clear it with `None`). Returns what was in its buffers.
    pub fn set_recipe(&mut self, content: &Content, id: BuildingId, recipe: Option<RecipeId>) -> Result<Vec<Stack>, RecipeError> {
        let Some(b) = self.get_mut(id) else { return Err(RecipeError::NotAMachine) };
        let def = content.factory.building_def(b.kind);
        let Logic::Machine(m) = &mut b.logic else { return Err(RecipeError::NotAMachine) };
        if let Some(r) = recipe {
            let rd = content.factory.recipe_def(r);
            if !def.crafts.contains(&rd.category) {
                return Err(RecipeError::WrongCategory { recipe: rd.name.clone(), building: def.name.clone() });
            }
            if rd.tier > def.tier {
                return Err(RecipeError::TierTooLow { needs: rd.tier, has: def.tier });
            }
        }
        if m.recipe == recipe {
            return Ok(vec![]);
        }
        let back = m.set_recipe(content, recipe);
        self.wake(id);
        Ok(back)
    }

    /// Put up to `count` of an item into a building (machine input, storage, hub, lab kits,
    /// hopper). Returns the count taken.
    pub fn insert(&mut self, content: &Content, id: BuildingId, item: ItemRef, count: u32) -> u32 {
        let hub = self.hub.clone();
        let Some(b) = self.get_mut(id) else { return 0 };
        let n = match (&mut b.steam, item) {
            (SteamState::Boiler { fuel, fuel_units, .. }, ItemRef::Material(material))
                if is_fuel(content, material) && fuel.is_none_or(|old| old == material) =>
            {
                let moved = count.min(32u32.saturating_sub(*fuel_units));
                if moved > 0 {
                    *fuel = Some(material);
                    *fuel_units += moved;
                }
                moved
            }
            (SteamState::Boiler { water, .. }, ItemRef::Material(material))
                if content.material("water") == Some(material) =>
            {
                let moved = count.min((200.0 - *water).floor().max(0.0) as u32);
                *water += moved as f64;
                moved
            }
            _ => accept(&mut b.logic, content, item, count, &hub),
        };
        if n > 0 {
            self.wake(id);
        }
        n
    }

    /// How many of an item a building can take now.
    pub fn room_for(&self, content: &Content, id: BuildingId, item: ItemRef) -> u32 {
        self.get(id).map_or(0, |b| match (b.steam, item) {
            (SteamState::Boiler { fuel, fuel_units, .. }, ItemRef::Material(material))
                if is_fuel(content, material) && fuel.is_none_or(|old| old == material) =>
            {
                32u32.saturating_sub(fuel_units)
            }
            (SteamState::Boiler { water, .. }, ItemRef::Material(material))
                if content.material("water") == Some(material) =>
            {
                (200.0 - water).floor().max(0.0) as u32
            }
            _ => accept_room(&b.logic, content, item, &self.hub),
        })
    }

    /// The finished products in a machine (outputs and byproducts that are not empty).
    pub fn outputs(&self, content: &Content, id: BuildingId) -> Vec<Stack> {
        let Some(b) = self.get(id) else { return vec![] };
        let Logic::Machine(m) = &b.logic else { return vec![] };
        let Some(r) = m.recipe else { return vec![] };
        let recipe = content.factory.recipe_def(r);
        (0..m.outputs.len())
            .filter(|&j| m.outputs[j] > 0)
            .map(|j| Stack { item: output_stack(recipe, j).item, count: m.outputs[j] })
            .collect()
    }

    /// Take up to `n` of one product out of a machine. Returns the count taken.
    pub fn take_output(&mut self, content: &Content, id: BuildingId, item: ItemRef, n: u32) -> u32 {
        let Some(b) = self.get_mut(id) else { return 0 };
        if let (SteamState::Boiler { steam, .. }, ItemRef::Material(material)) = (&mut b.steam, item)
            && content.material("steam") == Some(material)
        {
            let taken = (*steam).floor().min(n as f64) as u32;
            *steam -= taken as f64;
            if taken > 0 {
                self.wake(id);
            }
            return taken;
        }
        let Logic::Machine(m) = &mut b.logic else { return 0 };
        let Some(r) = m.recipe else { return 0 };
        let recipe = content.factory.recipe_def(r);
        let mut taken = 0;
        for j in 0..m.outputs.len() {
            if output_stack(recipe, j).item == item {
                taken += m.take_output(j, n - taken);
            }
        }
        if taken > 0 {
            self.wake(id);
        }
        taken
    }

    /// Take up to `n` of an item back out of the input buffers of a machine (or the kits of a
    /// lab). Returns the count taken.
    pub fn take_input(&mut self, content: &Content, id: BuildingId, item: ItemRef, n: u32) -> u32 {
        let Some(b) = self.get_mut(id) else { return 0 };
        let taken = match &mut b.logic {
            Logic::Machine(m) => {
                let Some(r) = m.recipe else { return 0 };
                let recipe = content.factory.recipe_def(r);
                let mut taken = 0;
                for (k, s) in recipe.inputs.iter().enumerate() {
                    if s.item == item && taken < n {
                        let t = m.inputs[k].min(n - taken);
                        m.inputs[k] -= t;
                        taken += t;
                    }
                }
                taken
            }
            Logic::Lab(lab) => match item {
                ItemRef::Part(p) => lab.kits.take(p, n),
                ItemRef::Material(_) => 0,
            },
            _ => match (&mut b.steam, item) {
                (SteamState::Boiler { water, .. }, ItemRef::Material(material))
                    if content.material("water") == Some(material) =>
                {
                    let taken = water.floor().min(n as f64) as u32;
                    *water -= taken as f64;
                    taken
                }
                _ => 0,
            },
        };
        if taken > 0 {
            self.wake(id);
        }
        taken
    }

    /// Take up to `n` units out of the fuel slot of a machine. Returns the material and the count.
    pub fn take_fuel(&mut self, id: BuildingId, n: u32) -> Option<(MaterialId, u32)> {
        let b = self.get_mut(id)?;
        if let SteamState::Boiler { fuel, fuel_units, .. } = &mut b.steam {
            let Some(material) = *fuel else { return None };
            let taken = n.min(*fuel_units);
            *fuel_units -= taken;
            if *fuel_units == 0 {
                *fuel = None;
            }
            if taken > 0 {
                self.wake(id);
                return Some((material, taken));
            }
            return None;
        }
        let Logic::Machine(m) = &mut b.logic else { return None };
        let taken = m.fuel.as_mut()?.take(n);
        if taken.is_some() {
            self.wake(id);
        }
        taken
    }

    /// Take all finished products out of a machine.
    pub fn take_outputs(&mut self, content: &Content, id: BuildingId) -> Vec<Stack> {
        let Some(b) = self.get_mut(id) else { return vec![] };
        let Logic::Machine(m) = &mut b.logic else { return vec![] };
        let Some(r) = m.recipe else { return vec![] };
        let recipe = content.factory.recipe_def(r);
        let mut out = vec![];
        for j in 0..m.outputs.len() {
            let n = m.take_output(j, u32::MAX);
            if n > 0 {
                out.push(Stack { item: output_stack(recipe, j).item, count: n });
            }
        }
        if !out.is_empty() {
            self.wake(id);
        }
        out
    }

    /// The slot inventory of a storage building or the Hub, for the slot rules.
    /// Call `wake` after changing it.
    pub fn inventory_mut(&mut self, id: BuildingId) -> Option<&mut Inventory> {
        match &mut self.get_mut(id)?.logic {
            Logic::Storage(inv) | Logic::Hub(inv) => Some(inv),
            _ => None,
        }
    }

    pub fn inventory(&self, id: BuildingId) -> Option<&Inventory> {
        match &self.get(id)?.logic {
            Logic::Storage(inv) | Logic::Hub(inv) => Some(inv),
            _ => None,
        }
    }

    /// How many of an item the storage buildings (crates and barrels) hold.
    pub fn stored_count(&self, item: ItemRef) -> u32 {
        self.slots
            .iter()
            .filter_map(|s| s.building.as_ref())
            .map(|b| match &b.logic {
                Logic::Storage(inv) => inv.count(item),
                _ => 0,
            })
            .fold(0u32, u32::saturating_add)
    }

    /// True if the building is the Hub.
    pub fn is_hub(&self, id: BuildingId) -> bool {
        self.get(id).is_some_and(|b| matches!(b.logic, Logic::Hub(_)))
    }

    /// Set what the Hub takes (the game does this at the start of each tick; the factory also
    /// after a load).
    pub fn set_hub_rule(&mut self, rule: HubRule) {
        self.hub = rule;
    }

    /// After a load: give storage buildings and the Hub the slots and tanks that their data
    /// asks for now, and keep their items. (Older saves have crates with part slots only.)
    /// A building keeps its old inventory if its items do not fit into the new one.
    pub fn upgrade_storage(&mut self, content: &Content) {
        for slot in &mut self.slots {
            let Some(b) = slot.building.as_mut() else { continue };
            let (Logic::Storage(old) | Logic::Hub(old)) = &mut b.logic else { continue };
            let (Logic::Storage(mut new) | Logic::Hub(mut new)) = Logic::for_kind(content.factory.building_def(b.kind)) else { continue };
            let same_shape = old.mixed == new.mixed
                && old.takes == new.takes
                && old.slots.len() == new.slots.len()
                && old.tanks.len() == new.tanks.len()
                && old.tanks.iter().zip(&new.tanks).all(|(a, n)| a.capacity == n.capacity);
            if same_shape {
                continue;
            }
            // Part stacks keep their slot when they can.
            let mut fits = true;
            for (i, s) in old.slots.iter().enumerate() {
                let Some(s) = *s else { continue };
                if i < new.slots.len() && new.slots[i].is_none() {
                    new.slots[i] = Some(s);
                } else {
                    fits &= new.insert(content, ItemRef::Part(s.part), s.count) == 0;
                }
            }
            for t in &old.tanks {
                if let Some(m) = t.material {
                    fits &= new.insert(content, ItemRef::Material(m), t.units) == 0;
                }
            }
            if fits {
                *old = new;
            }
        }
    }

    /// Buildings from an older save whose kind has new logic now: a passive building that is a
    /// machine now (the campfire), and a machine that has a fuel slot now.
    pub fn upgrade_machines(&mut self, content: &Content) {
        for i in 0..self.slots.len() {
            let mut wake = false;
            {
                let Some(b) = self.slots[i].building.as_mut() else { continue };
                if b.steam == SteamState::None {
                    let def = content.factory.building_def(b.kind);
                    let steam = SteamState::for_kind(&def.kind, def.power.as_ref().is_some_and(|p| p.steam_per_s > 0.0));
                    if steam != SteamState::None {
                        b.steam = steam;
                        wake = true;
                    }
                }
                let new = Logic::for_kind(content.factory.building_def(b.kind));
                match (&mut b.logic, new) {
                    (Logic::Passive, new @ Logic::Machine(_)) => { b.logic = new; wake = true; }
                    (Logic::Machine(old), Logic::Machine(new)) if old.fuel.is_none() && new.fuel.is_some() => { old.fuel = new.fuel; wake = true; }
                    _ => {}
                }
            }
            if wake {
                self.wake_index(i as u32);
            }
        }
    }

    /// Set the material filter of a hopper (`None`: all powders).
    pub fn set_hopper_filter(&mut self, id: BuildingId, filter: Option<MaterialId>) -> bool {
        let Some(b) = self.get_mut(id) else { return false };
        let Logic::Hopper(h) = &mut b.logic else { return false };
        h.filter = filter;
        self.wake(id);
        true
    }

    /// Hand crafting speed at a cell: the best workbench speed in reach, or 1.
    pub fn hand_speed(&self, content: &Content, at: CellPos) -> f32 {
        let mut best = 1.0f32;
        for &i in &self.workbenches {
            let Some(b) = self.at_index(i) else { continue };
            let def = content.factory.building_def(b.kind);
            let reach = def.param("reach", 6.0) * TILE_SIZE as f32;
            let r = b.cell_rect();
            let dx = (r.x0 - at.x).max(at.x - (r.x1 - 1)).max(0) as f32;
            let dy = (r.y0 - at.y).max(at.y - (r.y1 - 1)).max(0) as f32;
            if (dx * dx + dy * dy).sqrt() <= reach {
                best = best.max(def.speed);
            }
        }
        best
    }

    /// Run all building logic for one tick.
    pub fn tick<P: ProgressLink>(&mut self, content: &Content, sim: &mut Simulation, progress: &mut P) {
        self.now += 1;
        self.hub = HubRule { stage: progress.hub_stage(), need: progress.hub_need(content).map(Arc::new) };
        let now = self.now;
        while let Some(&(t, i)) = self.timers.first() {
            if t > now {
                break;
            }
            self.timers.pop_first();
            if let Some(b) = self.at_index_mut(i)
                && b.timer == Some(t)
            {
                b.timer = None;
                self.active.insert(i);
            }
        }
        let list: Vec<u32> = self.active.iter().copied().collect();
        for &i in &list {
            if let Some(b) = self.at_index_mut(i) {
                b.busy = false;
                b.blocked = false;
            }
        }
        for &i in &list {
            self.take_inputs(i, content, sim);
        }
        self.tick_steam_network(content, sim);
        for &i in &list {
            self.work(i, content, sim, progress);
        }
        for &i in &list {
            self.give_outputs(i, content, sim);
        }
        self.move_belts(&list, content, sim);
        for &i in &list {
            self.after_tick(i);
        }
        self.check_damage(content, sim);
    }

    /// Ports take cells from the world, and part inputs take parts from a storage next to them.
    fn take_inputs(&mut self, i: u32, content: &Content, sim: &mut Simulation) {
        let (now, hub) = (self.now, self.hub.clone());
        let Some(b) = self.at_index_mut(i) else { return };
        let def = content.factory.building_def(b.kind);
        let mut pull_ports: Vec<PlacedPort> = vec![];
        for pi in 0..b.ports.len() {
            let port = b.ports[pi];
            let phases: &[Phase] = match port.kind {
                PortKind::BulkIn => &[Phase::Powder],
                PortKind::FluidIn => &[Phase::Liquid, Phase::Gas],
                PortKind::PartIn => {
                    if now >= b.next_pull {
                        pull_ports.push(port);
                    }
                    continue;
                }
                _ => continue,
            };
            let taken = match &mut b.logic {
                Logic::Machine(m) => {
                    let recipe = m.recipe.map(|r| content.factory.recipe_def(r));
                    // Do not read the world when no input of this phase (and no fuel slot) has room.
                    let wanted = recipe.is_some_and(|recipe| {
                        recipe.inputs.iter().any(|s| match s.item {
                            ItemRef::Material(mat) => phases.contains(&phase(content, mat)) && m.input_room(recipe, s.item) > 0,
                            ItemRef::Part(_) => false,
                        })
                    });
                    let fuel_room = m.fuel.is_some_and(|f| f.units < f.capacity);
                    if !wanted && !fuel_room {
                        continue;
                    }
                    cells::take_from_side(sim, content, port.tile, port.side, INPUT_DEPTH, PORT_CELLS_PER_TICK, phases, |mat| {
                        let item = ItemRef::Material(mat);
                        if !port_allows(def, &port, item) {
                            return false;
                        }
                        if recipe.is_some_and(|r| m.add_input(r, item, 1) == 1) {
                            return true;
                        }
                        is_fuel(content, mat) && m.fuel.as_mut().is_some_and(|f| f.add(mat, 1) == 1)
                    })
                }
                Logic::Hopper(h) => {
                    if h.is_full() {
                        continue;
                    }
                    cells::take_from_side(sim, content, port.tile, port.side, INPUT_DEPTH, PORT_CELLS_PER_TICK, phases, |mat| {
                        port_allows(def, &port, ItemRef::Material(mat)) && h.push(mat)
                    })
                }
                Logic::Storage(inv) => {
                    cells::take_from_side(sim, content, port.tile, port.side, INPUT_DEPTH, PORT_CELLS_PER_TICK, phases, |mat| {
                        let item = ItemRef::Material(mat);
                        port_allows(def, &port, item) && inv.insert(content, item, 1) == 0
                    })
                }
                Logic::Hub(inv) => {
                    cells::take_from_side(sim, content, port.tile, port.side, INPUT_DEPTH, PORT_CELLS_PER_TICK, phases, |mat| {
                        let item = ItemRef::Material(mat);
                        port_allows(def, &port, item)
                            && hub.room(content, item, inv.count(item)) > 0
                            && inv.insert(content, item, 1) == 0
                    })
                }
                Logic::Passive if def.kind == "boiler" => {
                    if let SteamState::Boiler { fuel, fuel_units, .. } = &mut b.steam {
                        if port.kind == PortKind::BulkIn && *fuel_units < 32 {
                            cells::take_from_side(sim, content, port.tile, port.side, INPUT_DEPTH, PORT_CELLS_PER_TICK, &[Phase::Powder], |mat| {
                                if !port_allows(def, &port, ItemRef::Material(mat)) || !is_fuel(content, mat) || fuel.is_some_and(|f| f != mat) {
                                    return false;
                                }
                                *fuel = Some(mat);
                                *fuel_units += 1;
                                true
                            })
                        } else { 0 }
                    } else { 0 }
                }
                _ => 0,
            };
            if taken > 0 {
                b.busy = true;
            }
        }
        let mut moved = false;
        for port in pull_ports {
            moved |= self.pull_part(i, port, content);
        }
        if moved && let Some(b) = self.at_index_mut(i) {
            b.busy = true;
            b.next_pull = now + PART_PERIOD;
        }
    }

    /// A part input takes one part from a storage building on the other side of the port.
    fn pull_part(&mut self, i: u32, port: PlacedPort, content: &Content) -> bool {
        let hub = self.hub.clone();
        let Some(&nid) = self.front.get(&neighbor_tile(port.tile, port.side)) else { return false };
        let Some((me, src)) = two_mut(&mut self.slots, i as usize, nid.index as usize) else { return false };
        let Logic::Storage(inv) = &mut src.logic else { return false };
        let def = content.factory.building_def(me.kind);
        for s in 0..inv.slots.len() {
            let Some(st) = inv.slots[s] else { continue };
            let item = ItemRef::Part(st.part);
            if !port_allows(def, &port, item) || accept(&mut me.logic, content, item, 1, &hub) == 0 {
                continue;
            }
            let left = st.count - 1;
            inv.slots[s] = (left > 0).then_some(crate::inventory::PartStack::new(st.part, left));
            return true;
        }
        false
    }

    /// A part output gives one part to the building on the other side of the port, if that
    /// building is a storage or has a part input facing this port.
    fn push_part(&mut self, i: u32, port: PlacedPort, content: &Content) -> Option<u32> {
        let hub = self.hub.clone();
        let n_tile = neighbor_tile(port.tile, port.side);
        let &nid = self.front.get(&n_tile)?;
        let (me, dst) = two_mut(&mut self.slots, i as usize, nid.index as usize)?;
        let their_port =
            dst.ports.iter().find(|p| p.kind == PortKind::PartIn && p.tile == n_tile && p.side == opposite(port.side)).copied();
        if their_port.is_none() && !matches!(dst.logic, Logic::Storage(_)) {
            return None;
        }
        let my_def = content.factory.building_def(me.kind);
        let their_def = content.factory.building_def(dst.kind);
        let passes = |item: ItemRef| {
            port_allows(my_def, &port, item) && their_port.is_none_or(|tp| port_allows(their_def, &tp, item))
        };
        match &mut me.logic {
            Logic::Machine(m) => {
                let recipe = content.factory.recipe_def(m.recipe?);
                for j in 0..m.outputs.len() {
                    let item = output_stack(recipe, j).item;
                    if m.outputs[j] == 0 || !matches!(item, ItemRef::Part(_)) || !passes(item) {
                        continue;
                    }
                    if accept(&mut dst.logic, content, item, 1, &hub) == 1 {
                        m.outputs[j] -= 1;
                        return Some(nid.index);
                    }
                }
            }
            Logic::Storage(inv) => {
                for s in 0..inv.slots.len() {
                    let Some(st) = inv.slots[s] else { continue };
                    let item = ItemRef::Part(st.part);
                    if passes(item) && accept(&mut dst.logic, content, item, 1, &hub) == 1 {
                        inv.remove(item, 1);
                        return Some(nid.index);
                    }
                }
            }
            _ => {}
        }
        None
    }

    /// Machines work, labs research, the Hub delivers.
    fn work<P: ProgressLink>(&mut self, i: u32, content: &Content, sim: &mut Simulation, progress: &mut P) {
        let seed = self.seed ^ ((i as u64) << 32) ^ self.slots[i as usize].generation as u64;
        let Some(b) = self.at_index_mut(i) else { return };
        b.steam_reason = None;
        let def = content.factory.building_def(b.kind);
        let power = def.power.as_ref();
        let electric = power.is_some_and(|p| p.tier > 0 || p.use_w > 0.0);
        let has_power = !electric || b.power_factor > 0.0;
        let power_scale = if electric { b.power_factor.max(0.0) as f64 } else { 1.0 };
        let too_hot = b.temperature > def.max_temp;
        let idle_w = power.map_or(0.0, |p| p.idle_w);
        match &mut b.logic {
            Logic::Machine(m) => {
                let Some(r) = m.recipe else {
                    b.status = Status::NoRecipe;
                    b.power_w = idle_w;
                    return;
                };
                let recipe = content.factory.recipe_def(r);
                if b.exhaust_blocked {
                    b.status = Status::OutputBlocked;
                    b.power_w = idle_w;
                    return;
                }
                let heat = if recipe.min_temp.is_some() { heat_reading(sim, &b.ports, b.temperature) } else { b.temperature };
                b.heat = heat;
                let cond = Conditions {
                    speed: def.speed as f64 * machines::overclock_speed(def.tier, recipe.tier) * power_scale,
                    has_power: has_power && steam_ready(b.steam, power.map_or(0.0, |p| p.steam_per_s as f64)),
                    too_hot,
                    heat,
                };
                if has_power && !steam_ready(b.steam, power.map_or(0.0, |p| p.steam_per_s as f64)) {
                    b.steam_reason = Some("Needs steam from a connected bronze pipe".into());
                }
                let status = m.step(recipe, &cond, seed);
                b.status = status;
                if status == Status::Working {
                    b.busy = true;
                    consume_steam(&mut b.steam, power.map_or(0.0, |p| p.steam_per_s as f64) / foundry_core::TICKS_PER_SECOND as f64);
                    b.power_w = power.map_or(0.0, |p| p.use_w) * machines::overclock_power(def.tier, recipe.tier) as f32;
                    // A burner heats the cells at its heat ports while it works.
                    if let Some(p) = power
                        && p.burn_w > 0.0
                    {
                        let target = def.param("heat_temp", 800.0) as i16;
                        let step = (p.burn_w / 1000.0).clamp(1.0, 100.0) as i16;
                        for port in b.ports.iter().filter(|p| p.kind == PortKind::Heat) {
                            cells::heat_side(sim, port.tile, port.side, target, step);
                        }
                    }
                } else {
                    b.power_w = idle_w;
                }
            }
            Logic::Lab(lab) => {
                if !has_power || !steam_ready(b.steam, power.map_or(0.0, |p| p.steam_per_s as f64)) {
                    b.status = Status::NoPower;
                    if has_power { b.steam_reason = Some("Needs steam from a connected bronze pipe".into()); }
                    lab.last = None;
                    return;
                }
                let speed = def.speed * if electric { b.power_factor } else { 1.0 };
                let st = progress.lab_tick(content, speed, &mut lab.kits);
                let status = progress_link::lab_status(&st);
                b.status = status;
                b.steam_reason = None;
                lab.last = Some(st);
                if status == Status::Working {
                    b.busy = true;
                    consume_steam(&mut b.steam, power.map_or(0.0, |p| p.steam_per_s as f64) / foundry_core::TICKS_PER_SECOND as f64);
                    b.power_w = power.map_or(0.0, |p| p.use_w);
                } else {
                    b.power_w = idle_w;
                }
            }
            Logic::Hub(inv) => {
                let mut delivered = false;
                for s in inv.contents() {
                    let n = progress.deliver(content, s).min(s.count);
                    if n > 0 {
                        inv.remove(s.item, n);
                        delivered = true;
                    }
                }
                b.status = if delivered { Status::Working } else { Status::Idle };
                b.busy |= delivered;
            }
            Logic::Passive if def.kind == "boiler" => {
                match b.steam {
                    SteamState::Boiler { fuel_units: 0, burn_ticks: 0, water, .. } if water > 0.0 => {
                        b.status = Status::NoFuel;
                    }
                    SteamState::Boiler { water, .. } if water <= 0.0 => {
                        b.status = Status::NoInput;
                        b.steam_reason = Some("Waiting for water from a connected bronze pipe".into());
                    }
                    SteamState::Boiler { steam, .. } if steam >= crate::steam::TANK_CAPACITY => {
                        b.status = Status::OutputFull;
                        b.steam_reason = Some("Steam tank full".into());
                    }
                    SteamState::Boiler { .. } if b.temperature < 100 => {
                        b.status = Status::Working;
                        b.steam_reason = Some(format!("Heating water: {} °C / 100 °C", b.temperature));
                    }
                    _ => {
                        b.status = Status::Working;
                        b.steam_reason = Some("Making steam".into());
                    }
                }
            }
            Logic::Storage(_) | Logic::Workbench | Logic::Passive => b.status = Status::Idle,
            Logic::Hopper(_) | Logic::Belt(_) => {}
        }
    }

    /// Output ports put cells into the world; part outputs give parts to neighbors.
    fn give_outputs(&mut self, i: u32, content: &Content, sim: &mut Simulation) {
        let now = self.now;
        let Some(b) = self.at_index_mut(i) else { return };
        let def = content.factory.building_def(b.kind);
        match &mut b.logic {
            Logic::Machine(m) => {
                let Some(r) = m.recipe else { return };
                let recipe = content.factory.recipe_def(r);
                let mut exhaust_failed = false;
                for j in 0..m.outputs.len() {
                    let item = output_stack(recipe, j).item;
                    let ItemRef::Material(mat) = item else { continue };
                    if m.outputs[j] == 0 {
                        continue;
                    }
                    let ph = phase(content, mat);
                    for port in &b.ports {
                        let fits = match port.kind {
                            PortKind::BulkOut => ph == Phase::Powder,
                            PortKind::FluidOut => is_fluid(ph),
                            PortKind::Exhaust => ph == Phase::Gas,
                            _ => false,
                        };
                        if !fits || !port_allows(def, port, item) {
                            continue;
                        }
                        let want = m.outputs[j].min(PORT_CELLS_PER_TICK);
                        let placed = cells::put_to_side(sim, port.tile, port.side, mat, want, None);
                        m.outputs[j] -= placed;
                        b.busy |= placed > 0;
                        if placed < want {
                            b.blocked = true;
                            exhaust_failed |= port.kind == PortKind::Exhaust;
                        }
                        if m.outputs[j] == 0 {
                            break;
                        }
                    }
                }
                b.exhaust_blocked = exhaust_failed;
                if b.status == Status::OutputFull && b.blocked {
                    b.status = Status::OutputBlocked;
                }
            }
            Logic::Hopper(h) => {
                let n = steps_in_tick(now, def.param("rate", 16.0));
                let mut released = 0;
                'ports: for port in b.ports.iter().filter(|p| p.kind == PortKind::BulkOut) {
                    while released < n {
                        let Some(&mat) = h.cells.front() else { break 'ports };
                        if cells::put_to_side(sim, port.tile, port.side, mat, 1, None) == 0 {
                            b.blocked = true;
                            continue 'ports;
                        }
                        h.cells.pop_front();
                        released += 1;
                    }
                }
                b.busy |= released > 0;
                b.status = if h.cells.is_empty() {
                    Status::NoInput
                } else if b.blocked && released == 0 {
                    Status::OutputBlocked
                } else {
                    Status::Working
                };
            }
            _ => {}
        }
        if now < b.next_push {
            return;
        }
        let push_ports: Vec<PlacedPort> = b.ports.iter().filter(|p| p.kind == PortKind::PartOut).copied().collect();
        let mut woken = vec![];
        for port in push_ports {
            if let Some(j) = self.push_part(i, port, content) {
                woken.push(j);
            }
        }
        if !woken.is_empty() {
            if let Some(b) = self.at_index_mut(i) {
                b.busy = true;
                b.next_push = now + PART_PERIOD;
            }
            for j in woken {
                self.wake_index(j);
            }
        }
    }

    /// Belts that step in this tick move the powder on them. The belt at the front of a line
    /// moves first, so the powder behind it has room.
    fn move_belts(&mut self, list: &[u32], content: &Content, sim: &mut Simulation) {
        let now = self.now;
        let mut stepping: Vec<(i32, i32, i32, u32, u32)> = vec![];
        for &i in list {
            let Some(b) = self.at_index(i) else { continue };
            if !matches!(b.logic, Logic::Belt(_)) {
                continue;
            }
            let def = content.factory.building_def(b.kind);
            let steps = steps_in_tick(now, def.param("belt_speed", 8.0));
            if steps == 0 {
                // Not a step tick. A belt with powder on it stays awake for the next step.
                let r = b.cell_rect();
                let loaded = (r.x0..r.x1).any(|x| phase(content, sim.cell(CellPos::new(x, r.y0 - 1)).material) == Phase::Powder);
                if loaded && let Some(b) = self.at_index_mut(i) {
                    b.busy = true;
                }
                continue;
            }
            let dir = belt_direction(b.transform);
            let r = b.cell_rect();
            let order = if dir > 0 { -r.x1 } else { r.x0 };
            stepping.push((dir, order, r.y0, i, steps));
        }
        stepping.sort_unstable();
        for (dir, _, _, i, steps) in stepping {
            let Some(b) = self.at_index(i) else { continue };
            let r = b.cell_rect();
            let (at, size) = (b.at, b.size);
            let mut moved = 0;
            for _ in 0..steps {
                moved += move_belt_cells(sim, content, r, dir);
            }
            let b = self.at_index_mut(i).expect("belt exists");
            b.busy |= moved > 0;
            b.status = if moved > 0 { Status::Working } else { Status::Idle };
            if let Logic::Belt(belt) = &mut b.logic {
                belt.idle_steps = if moved > 0 { 0 } else { belt.idle_steps + 1 };
            }
            if moved > 0 {
                // Wake the next belt of the line: the powder now reaches it.
                let next = TilePos::new(if dir > 0 { at.x + size.0 as i32 } else { at.x - 1 }, at.y);
                if let Some(&nid) = self.front.get(&next)
                    && matches!(self.at_index(nid.index).map(|n| &n.logic), Some(Logic::Belt(_)))
                {
                    self.wake_index(nid.index);
                }
            }
        }
    }

    /// Decide if the building sleeps after this tick.
    fn after_tick(&mut self, i: u32) {
        let Some(b) = self.at_index_mut(i) else { return };
        if b.busy {
            b.idle = 0;
        } else {
            b.idle = b.idle.saturating_add(1);
        }
        let poll = if b.idle < 60 { 8 } else { 30 };
        let has_inputs = b.ports.iter().any(|p| matches!(p.kind, PortKind::BulkIn | PortKind::FluidIn | PortKind::PartIn));
        let has_outputs =
            b.ports.iter().any(|p| matches!(p.kind, PortKind::BulkOut | PortKind::FluidOut | PortKind::Exhaust | PortKind::PartOut));
        // None: stay awake. Some(None): sleep until woken. Some(Some(n)): sleep n ticks.
        let wait: Option<Option<u64>> = match &b.logic {
            Logic::Passive | Logic::Workbench => Some(None),
            Logic::Storage(_) => {
                if !b.ports.is_empty() && b.idle >= 2 {
                    Some(Some(poll))
                } else if b.ports.is_empty() {
                    Some(None)
                } else {
                    None
                }
            }
            Logic::Machine(m) => {
                if m.recipe.is_none() {
                    Some(None)
                } else if b.busy || b.idle < 2 {
                    None
                } else {
                    match b.status {
                        Status::NoInput if !has_inputs => Some(None),
                        Status::OutputFull if !has_outputs => Some(None),
                        _ => Some(Some(poll)),
                    }
                }
            }
            Logic::Hopper(_) | Logic::Belt(_) => (!b.busy && b.idle >= 2).then_some(Some(poll)),
            Logic::Hub(_) | Logic::Lab(_) => {
                (!b.busy && b.status != Status::Working && b.idle >= 2).then_some(Some(poll))
            }
        };
        if let Some(t) = wait {
            self.sleep(i, t);
        }
    }

    /// Check the body cells of every front-layer building once every `DAMAGE_PERIOD` ticks.
    /// Body cells hotter than the building's `max_temp` cause damage (more when far above it).
    /// Body cells that are no longer the body material cause damage once.
    fn check_damage(&mut self, content: &Content, sim: &mut Simulation) {
        let start = (self.now % DAMAGE_PERIOD) as usize;
        let mut broken = vec![];
        for i in (start..self.slots.len()).step_by(DAMAGE_PERIOD as usize) {
            let Some(b) = self.slots[i].building.as_mut() else { continue };
            if b.layer != Layer::Front {
                continue;
            }
            let def = content.factory.building_def(b.kind);
            let r = b.cell_rect();
            let total = (r.width() * r.height()).max(1) as f32;
            let (mut lost, mut sum, mut n, mut heat) = (0u32, 0i64, 0i64, 0f32);
            let max = def.max_temp as f32;
            for p in cells::rect_cells(r) {
                let c = sim.cell(p);
                if c.material != def.body {
                    lost += 1;
                    continue;
                }
                sum += c.temperature as i64;
                n += 1;
                if c.temperature > def.max_temp {
                    heat += ((c.temperature as f32 - max) / max.max(100.0)).min(1.0);
                }
            }
            if n > 0 {
                b.temperature = (sum / n) as i16;
            }
            let new_lost = lost.saturating_sub(b.lost_cells);
            b.lost_cells = lost;
            let damage = def.hit_points as f32 * (0.5 * heat / total + 2.0 * new_lost as f32 / total);
            if damage > 0.0 {
                b.hit_points -= damage;
                if b.hit_points <= 0.0 {
                    b.hit_points = 0.0;
                    b.status = Status::Broken;
                    broken.push(i as u32);
                }
            }
        }
        for i in broken {
            let id = self.id_of(i);
            self.break_building(content, sim, id);
        }
    }

    /// Data for the building window.
    pub fn view(&self, content: &Content, id: BuildingId) -> Option<BuildingView> {
        let b = self.get(id)?;
        let def = content.factory.building_def(b.kind);
        let mut v = BuildingView {
            id,
            kind: b.kind,
            name: def.name.clone(),
            logic: def.kind.clone(),
            at: b.at,
            size: b.size,
            rotation: b.transform.rotation,
            flip: b.transform.flip,
            status: b.status,
            reason: b.status.text().to_string(),
            recipe: None,
            recipe_name: None,
            recipes: Self::recipes_for(content, b.kind),
            inputs: vec![],
            outputs: vec![],
            fuel: None,
            inventory: None,
            progress: 0.0,
            power_w: b.power_w,
            temperature: b.temperature,
            max_temp: def.max_temp,
            hit_points: b.hit_points.ceil() as u32,
            max_hit_points: def.hit_points,
            ports: b.ports.iter().map(|p| PortView::new(def, p)).collect(),
        };
        match &b.logic {
            Logic::Machine(m) => {
                v.fuel = m.fuel.map(|f| FuelView {
                    material: f.material,
                    units: f.units,
                    capacity: f.capacity,
                    hint: fuel_hint(content, def),
                    burning: f.burn as f32 / f.ticks_per_unit.max(1) as f32,
                });
                if let Some(r) = m.recipe {
                    let recipe = content.factory.recipe_def(r);
                    v.recipe = Some(r);
                    v.recipe_name = Some(recipe.name.clone());
                    v.progress = m.progress(recipe);
                    v.inputs = recipe
                        .inputs
                        .iter()
                        .enumerate()
                        .map(|(k, s)| BufferView::new(content, s.item, m.inputs[k], m.input_capacity(recipe, k), s.count))
                        .collect();
                    v.outputs = (0..m.outputs.len())
                        .map(|j| {
                            let s = output_stack(recipe, j);
                            BufferView::new(content, s.item, m.outputs[j], m.output_capacity(recipe, j), s.count)
                        })
                        .collect();
                    v.reason = match b.status {
                        Status::NoInput => match m.missing_input(recipe) {
                            Some(s) => format!("Needs {} more {}", s.count, content.item_name(s.item)),
                            None => "No input".into(),
                        },
                        Status::OutputFull => match m.full_output(recipe) {
                            Some(j) => format!("Output full: take out the {}", content.item_name(output_stack(recipe, j).item)),
                            None => "Output full".into(),
                        },
                        Status::OutputBlocked => "Output blocked: no free cells in front of the port".into(),
                        Status::TooCold => {
                            format!("Too cold: {} °C, needs {} °C", b.heat, recipe.min_temp.unwrap_or(0))
                        }
                        Status::TooHot => format!("Too hot: {} °C, the limit is {} °C", b.temperature, def.max_temp),
                        Status::NoFuel => match fuel_hint(content, def) {
                            Some(f) => format!("No fuel: put {} in the fuel slot", content.materials.names[f.index()].to_lowercase()),
                            None => "No fuel".into(),
                        },
                        s => s.text().to_string(),
                    };
                } else {
                    v.reason = "Choose a recipe".into();
                }
            }
            Logic::Storage(inv) | Logic::Hub(inv) => v.inventory = Some(inv.view(content)),
            Logic::Hopper(h) => {
                v.inputs = h
                    .counts()
                    .into_iter()
                    .map(|(m, n)| BufferView::new(content, ItemRef::Material(m), n, h.capacity, 0))
                    .collect();
                if b.status == Status::NoInput {
                    v.reason = "Empty".into();
                }
            }
            Logic::Lab(lab) => {
                v.inputs = lab
                    .kits
                    .iter()
                    .map(|(p, n)| BufferView::new(content, ItemRef::Part(p), n, lab.kit_limit, 1))
                    .collect();
                if let Some(st) = &lab.last {
                    v.reason = progress_link::lab_reason(content, st);
                }
            }
            Logic::Belt(_) | Logic::Workbench | Logic::Passive => {}
        }
        match b.steam {
            SteamState::Boiler { fuel, fuel_units, burn_ticks, water, steam, .. } => {
                v.fuel = Some(FuelView {
                    material: fuel,
                    units: fuel_units,
                    capacity: 32,
                    hint: content.material("coal").or_else(|| content.material("charcoal")),
                    burning: burn_ticks as f32 / foundry_core::TICKS_PER_SECOND as f32,
                });
                if let (Some(water_id), Some(steam_id)) = (content.material("water"), content.material("steam")) {
                    v.inputs.push(BufferView::new(content, ItemRef::Material(water_id), water.ceil() as u32, 200, 1));
                    v.outputs.push(BufferView::new(content, ItemRef::Material(steam_id), steam.ceil() as u32, 200, 1));
                }
            }
            SteamState::Pipe(tank) | SteamState::Machine(tank) => {
                if let Some(material) = tank.material {
                    v.inputs.push(BufferView::new(content, ItemRef::Material(material), tank.amount.ceil() as u32, tank.capacity as u32, 1));
                }
            }
            SteamState::None => {}
        }
        if let Some(reason) = &b.steam_reason { v.reason = reason.clone(); }
        if b.status == Status::TooHot {
            v.reason = format!("Too hot: {} °C, the limit is {} °C", b.temperature, def.max_temp);
        }
        Some(v)
    }
}
