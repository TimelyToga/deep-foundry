//! Placing, removing and breaking buildings (game design sections 10.2, 18.1 and 18.3).
//!
//! Placement rules for the front layer:
//! - Every tile of the footprint must be free in that layer.
//! - Solid cells in the footprint block the building. The player digs them first.
//! - Loose cells (powder, liquid, gas, fire) in the footprint are pushed out of the way: each one
//!   moves to the nearest air cell above or beside the footprint. Nothing is lost. If there is not
//!   enough room, the building cannot be placed.
//! - A building type with the parameter `needs_floor` above 0 needs solid cells under at least
//!   half of its bottom row.
//! - Belts can only face left or right (rotation 0 or 2).
//!
//! Placing writes the body cells (the body material) into the world. Back-layer buildings have
//! no body cells.

use crate::buildings::{Building, Buildings, FactoryEvent, Logic, Slot, placed_ports, stacks_to_cells};
use crate::cells::{self, is_loose, phase, rect_cells};
use crate::geometry::{Transform, tiles_to_cells};
use crate::views::PortView;
use foundry_content::{Content, ItemRef, Layer, Phase, Stack};
use foundry_core::{BuildingId, BuildingKindId, CellPos, CellRect, MaterialId, TilePos};
use foundry_sim::Simulation;

/// Why a building cannot be placed. `Display` gives the text for the red ghost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaceError {
    UnknownKind,
    /// Part of the footprint is outside the world.
    OutsideWorld,
    /// Another building is on a tile of the footprint.
    TileTaken { by: BuildingId, name: String },
    /// A solid cell is in the footprint. `can_dig` is false for bedrock.
    Blocked { at: CellPos, material: MaterialId, name: String, can_dig: bool },
    /// The building needs solid ground under it.
    NeedsFloor,
    /// There are not enough free cells to push the loose cells out of the footprint.
    NoRoomForLooseCells { material: MaterialId, name: String },
    /// This building cannot be turned this way (belts face only left or right).
    CannotTurn,
}

impl std::fmt::Display for PlaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlaceError::UnknownKind => write!(f, "Unknown building"),
            PlaceError::OutsideWorld => write!(f, "Outside the world"),
            PlaceError::TileTaken { name, .. } => write!(f, "Tile taken by {name}"),
            PlaceError::Blocked { name, can_dig: true, .. } => write!(f, "Blocked by {}: dig first", name.to_lowercase()),
            PlaceError::Blocked { name, can_dig: false, .. } => write!(f, "Blocked by {}", name.to_lowercase()),
            PlaceError::NeedsFloor => write!(f, "Needs a floor"),
            PlaceError::NoRoomForLooseCells { name, .. } => {
                write!(f, "No room to push the {} out of the way", name.to_lowercase())
            }
            PlaceError::CannotTurn => write!(f, "This building cannot be turned that way"),
        }
    }
}

impl std::error::Error for PlaceError {}

/// Why a building cannot be removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveError {
    /// The id is old or wrong.
    NotFound,
}

impl std::fmt::Display for RemoveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "There is no such building")
    }
}

impl std::error::Error for RemoveError {}

/// The result of a placement check: the footprint and the loose cells to move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacePlan {
    pub cells: CellRect,
    /// Loose cells in the footprint, top row first.
    pub loose: Vec<CellPos>,
    /// Where each loose cell goes (same order).
    pub to: Vec<CellPos>,
}

/// The most cells the search for free cells looks at when a building is placed.
const PUSH_SEARCH_LIMIT: usize = 16 * 1024;

/// The material of the scrap a broken building leaves: `scrap` if the data has it, else the
/// broken form of the body material (which is the body material itself if it has none).
pub fn scrap_material(content: &Content, body: MaterialId) -> MaterialId {
    content.material("scrap").unwrap_or(content.materials.broken_into[body.index()])
}

impl Buildings {
    /// The ports a building would have at this place, for the ghost preview.
    pub fn ghost_ports(content: &Content, kind: BuildingKindId, at: TilePos, t: Transform) -> Vec<PortView> {
        let def = content.factory.building_def(kind);
        placed_ports(def, at, t).iter().map(|p| PortView::new(def, p)).collect()
    }

    /// Check if a building can be placed with its top-left tile at `at`.
    pub fn check_place(
        &self,
        content: &Content,
        kind: BuildingKindId,
        at: TilePos,
        t: Transform,
        sim: &Simulation,
    ) -> Result<PlacePlan, PlaceError> {
        let def = content.factory.buildings.get(kind.0 as usize).ok_or(PlaceError::UnknownKind)?;
        if matches!(Logic::for_kind(def), Logic::Belt(_)) && t.rotation & 1 == 1 {
            return Err(PlaceError::CannotTurn);
        }
        let size = t.size(def.size);
        let rect = tiles_to_cells(at, size);
        // The world size comes from the simulation. Width 0: no limit to the left and right.
        let (w, h) = sim.size_cells();
        let outside_x = w > 0 && (rect.x0 < 0 || rect.x1 > w);
        if outside_x || rect.y0 < 0 || rect.y1 > h {
            return Err(PlaceError::OutsideWorld);
        }
        let map = if def.layer == Layer::Front { &self.front } else { &self.back };
        for ty in 0..size.1 as i32 {
            for tx in 0..size.0 as i32 {
                if let Some(&id) = map.get(&TilePos::new(at.x + tx, at.y + ty)) {
                    let name = self.get(id).map(|b| content.factory.building_def(b.kind).name.clone()).unwrap_or_default();
                    return Err(PlaceError::TileTaken { by: id, name });
                }
            }
        }
        if def.layer == Layer::Back {
            return Ok(PlacePlan { cells: rect, loose: vec![], to: vec![] });
        }
        let mut loose = vec![];
        for p in rect_cells(rect) {
            let m = sim.cell(p).material;
            let ph = phase(content, m);
            if ph == Phase::Solid {
                return Err(PlaceError::Blocked {
                    at: p,
                    material: m,
                    name: content.materials.names[m.index()].clone(),
                    can_dig: content.materials.hardness[m.index()] < 255,
                });
            }
            if is_loose(ph) {
                loose.push(p);
            }
        }
        if def.param("needs_floor", 0.0) > 0.0 {
            let solid = (rect.x0..rect.x1)
                .filter(|&x| phase(content, sim.cell(CellPos::new(x, rect.y1)).material) == Phase::Solid)
                .count() as i32;
            if solid * 2 < rect.width() {
                return Err(PlaceError::NeedsFloor);
            }
        }
        let to = cells::find_free_cells(
            sim,
            content,
            cells::border_above_and_sides(rect),
            rect,
            false,
            loose.len(),
            PUSH_SEARCH_LIMIT,
        );
        if to.len() < loose.len() {
            let m = sim.cell(loose[to.len()]).material;
            return Err(PlaceError::NoRoomForLooseCells { material: m, name: content.materials.names[m.index()].clone() });
        }
        Ok(PlacePlan { cells: rect, loose, to })
    }

    /// Place a building. It is ready at once (the build tool checks the items first).
    pub fn place(
        &mut self,
        content: &Content,
        kind: BuildingKindId,
        at: TilePos,
        t: Transform,
        sim: &mut Simulation,
    ) -> Result<BuildingId, PlaceError> {
        let plan = self.check_place(content, kind, at, t, sim)?;
        let def = content.factory.building_def(kind);
        for (from, to) in plan.loose.iter().zip(&plan.to) {
            cells::move_cell(sim, *from, *to);
        }
        if def.layer == Layer::Front {
            for p in rect_cells(plan.cells) {
                sim.set_cell(p, def.body, None);
            }
        }
        let mut b = Building::new(def, kind, at, t);
        b.temperature = content.materials.temperature[def.body.index()];
        let is_workbench = matches!(b.logic, Logic::Workbench);
        let tiles: Vec<TilePos> = b.tiles().collect();
        let index = match self.free.pop() {
            Some(i) => {
                self.slots[i as usize].building = Some(b);
                i
            }
            None => {
                self.slots.push(Slot { generation: 0, building: Some(b) });
                (self.slots.len() - 1) as u32
            }
        };
        let id = self.id_of(index);
        let map = if def.layer == Layer::Front { &mut self.front } else { &mut self.back };
        for tile in tiles {
            map.insert(tile, id);
        }
        if is_workbench {
            self.workbenches.insert(index);
        }
        *self.kind_counts.entry(kind).or_insert(0) += 1;
        self.wake_index(index);
        Ok(id)
    }

    /// Take a building out of the registry. Its cells stay as they are.
    fn unregister(&mut self, id: BuildingId) -> Option<Building> {
        self.get(id)?;
        let i = id.index;
        let slot = &mut self.slots[i as usize];
        let b = slot.building.take()?;
        slot.generation = slot.generation.wrapping_add(1);
        self.free.push(i);
        let map = if b.layer == Layer::Front { &mut self.front } else { &mut self.back };
        for tile in b.tiles() {
            if map.get(&tile) == Some(&id) {
                map.remove(&tile);
            }
        }
        self.active.remove(&i);
        if let Some(t) = b.timer {
            self.timers.remove(&(t, i));
        }
        self.workbenches.remove(&i);
        if let Some(n) = self.kind_counts.get_mut(&b.kind) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                self.kind_counts.remove(&b.kind);
            }
        }
        Some(b)
    }

    /// Turn the body cells that are still the body material into air.
    fn clear_body(content: &Content, sim: &mut Simulation, b: &Building) {
        if b.layer != Layer::Front {
            return;
        }
        let body = content.factory.building_def(b.kind).body;
        for p in rect_cells(b.cell_rect()) {
            if sim.cell(p).material == body {
                sim.set_cell(p, MaterialId::AIR, None);
            }
        }
    }

    /// Remove a building. The body cells become air. Returns the building's item first, then
    /// everything it held (inventory, buffers, kits).
    pub fn remove(&mut self, content: &Content, id: BuildingId, sim: &mut Simulation) -> Result<Vec<Stack>, RemoveError> {
        let mut b = self.unregister(id).ok_or(RemoveError::NotFound)?;
        Self::clear_body(content, sim, &b);
        let def = content.factory.building_def(b.kind);
        let mut out = vec![Stack { item: ItemRef::Part(def.part), count: 1 }];
        out.extend(b.take_all(content));
        Ok(out)
    }

    /// Put material stacks into the world as cells near a place (for example where a removed
    /// building stood). Parts become cells of their material. Returns the stacks that had no
    /// cells or no room.
    pub fn drop_as_cells(content: &Content, sim: &mut Simulation, near: CellRect, stacks: &[Stack]) -> Vec<Stack> {
        let mut left: Vec<Stack> = vec![];
        let mut cell_stacks = vec![];
        for s in stacks {
            if stacks_to_cells(content, std::slice::from_ref(s)).is_empty() {
                left.push(*s);
            } else {
                cell_stacks.push(*s);
            }
        }
        let cells = stacks_to_cells(content, &cell_stacks);
        for (m, n) in cells::release_cells(sim, content, near, &cells) {
            left.push(Stack { item: ItemRef::Material(m), count: n });
        }
        left
    }

    /// A building at 0 hit points falls apart: the body becomes air, the bottom quarter of the
    /// footprint becomes scrap, and the contents go into the world as cells.
    pub(crate) fn break_building(&mut self, content: &Content, sim: &mut Simulation, id: BuildingId) {
        let Some(mut b) = self.unregister(id) else { return };
        Self::clear_body(content, sim, &b);
        let r = b.cell_rect();
        if b.layer == Layer::Front {
            let scrap = scrap_material(content, content.factory.building_def(b.kind).body);
            let rows = (r.height() / 4).max(1);
            for y in r.y1 - rows..r.y1 {
                for x in r.x0..r.x1 {
                    let p = CellPos::new(x, y);
                    if sim.cell(p).material.is_air() {
                        sim.set_cell(p, scrap, None);
                    }
                }
            }
        }
        let stacks = b.take_all(content);
        Self::drop_as_cells(content, sim, r, &stacks);
        self.events.push(FactoryEvent::Broke { id, kind: b.kind, at: b.at });
    }
}

impl Building {
    /// Everything the building holds, as stacks. The building is emptied.
    pub fn take_all(&mut self, content: &Content) -> Vec<Stack> {
        crate::buildings::take_contents(&mut self.logic, content)
    }
}
