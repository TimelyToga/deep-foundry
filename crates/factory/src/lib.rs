//! The factory: buildings, ports, machines, inventories, crafting, logistics and progression.
//!
//! The factory runs on the simulation thread, once per tick, before the cell simulation
//! (technical design section 7.1). It changes the world only through the public API of
//! `foundry_sim::Simulation`.
//!
//! Module owners (docs/design/04-build-plan.md):
//! - `buildings`, `placement`, `geometry`, `cells`, `inventory`, `crafting`, `machines`,
//!   `logistics`, `views`, `progress_link`: task "factory-core"
//! - `progress` (technologies, labs, milestones, discovery, guide): task "progression"
//! - networks (power, fluids, signals): a later task

pub mod buildings;
pub mod cells;
pub mod crafting;
pub mod geometry;
pub mod inventory;
pub mod logistics;
pub mod machines;
pub mod placement;
pub mod progress;
pub mod progress_link;
pub mod views;

pub use buildings::{Building, Buildings, FactoryEvent, Logic};
pub use crafting::{CraftError, CraftJobView, HandCrafting};
pub use geometry::Transform;
pub use inventory::{Click, Inventory, InventoryView, PartStack, SlotView, Tank, TankView};
pub use machines::{RecipeError, Status};
pub use placement::{PlaceError, RemoveError};
pub use views::{BufferView, BuildingView, PortView};

use foundry_content::{Content, ItemRef, Stack};
use foundry_core::{BuildingId, BuildingKindId, CellPos, RecipeId, TilePos};
use foundry_sim::Simulation;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// An inventory the slot rules can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InvTarget {
    Player,
    /// A storage building or the Hub.
    Building(BuildingId),
}

/// What happened when a building was taken into the player's inventory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoveReport {
    /// Went into the player's inventory (the building's item first).
    pub taken: Vec<Stack>,
    /// Did not fit and went into the world as cells.
    pub dropped: Vec<Stack>,
    /// Did not fit and could not become cells (parts without a material). The caller keeps them.
    pub left: Vec<Stack>,
}

/// All factory state.
pub struct Factory {
    pub content: Arc<Content>,
    pub buildings: buildings::Buildings,
    pub progress: progress::Progress,
    /// The robot's inventory.
    pub player: Inventory,
    /// The stack held by the mouse cursor.
    pub cursor: Option<PartStack>,
    /// The robot's hand crafting queue.
    pub hand: HandCrafting,
    /// Where the robot is (for the workbench speed). `None`: no robot in the world.
    pub player_pos: Option<CellPos>,
}

/// The saved form of the factory state that this crate owns. (`progress` saves itself.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactorySave {
    pub buildings: Buildings,
    pub player: Inventory,
    pub cursor: Option<PartStack>,
    pub hand: HandCrafting,
    pub player_pos: Option<CellPos>,
}

impl Factory {
    pub fn new(content: Arc<Content>) -> Self {
        Self {
            buildings: buildings::Buildings::new(&content),
            progress: progress::Progress::new(&content),
            player: Inventory::player(),
            cursor: None,
            hand: HandCrafting::new(),
            player_pos: None,
            content,
        }
    }

    /// Run one tick. Call it before `Simulation::advance` in the same tick.
    /// Order: buildings (ports take input, machines work, ports give output, belts move), then
    /// hand crafting, then progress.
    pub fn tick(&mut self, sim: &mut Simulation) {
        self.buildings.tick(&self.content, sim, &mut self.progress);
        if !self.hand.is_idle() {
            let speed = self.player_pos.map_or(1.0, |p| self.buildings.hand_speed(&self.content, p));
            self.hand.tick(&self.content, &mut self.player, speed);
        }
        self.progress.tick(&self.content);
    }

    /// A copy of the state for a save file.
    pub fn save(&self) -> FactorySave {
        FactorySave {
            buildings: self.buildings.clone(),
            player: self.player.clone(),
            cursor: self.cursor,
            hand: self.hand.clone(),
            player_pos: self.player_pos,
        }
    }

    /// Put back the state from a save file.
    pub fn load(&mut self, save: FactorySave) {
        self.buildings = save.buildings;
        self.player = save.player;
        self.cursor = save.cursor;
        self.hand = save.hand;
        self.player_pos = save.player_pos;
    }

    /// Check if a building can be placed with its top-left tile at `at`.
    pub fn can_place(&self, kind: BuildingKindId, at: TilePos, rotation: u8, flip: bool, sim: &Simulation) -> Result<(), PlaceError> {
        self.buildings.check_place(&self.content, kind, at, Transform::new(rotation, flip), sim).map(|_| ())
    }

    /// Place a building. The caller takes the item from the inventory.
    pub fn place(
        &mut self,
        kind: BuildingKindId,
        at: TilePos,
        rotation: u8,
        flip: bool,
        sim: &mut Simulation,
    ) -> Result<BuildingId, PlaceError> {
        self.buildings.place(&self.content, kind, at, Transform::new(rotation, flip), sim)
    }

    /// The ports a building would have at this place (arrows on the ghost).
    pub fn ghost_ports(&self, kind: BuildingKindId, at: TilePos, rotation: u8, flip: bool) -> Vec<PortView> {
        Buildings::ghost_ports(&self.content, kind, at, Transform::new(rotation, flip))
    }

    /// Remove a building. Returns its item and its contents as stacks. The body cells become air.
    pub fn remove(&mut self, id: BuildingId, sim: &mut Simulation) -> Result<Vec<Stack>, RemoveError> {
        self.buildings.remove(&self.content, id, sim)
    }

    /// Remove a building into the player's inventory. Bulk material that does not fit goes into
    /// the world as cells where the building was.
    pub fn remove_to_player(&mut self, id: BuildingId, sim: &mut Simulation) -> Result<RemoveReport, RemoveError> {
        let area = self.buildings.get(id).ok_or(RemoveError::NotFound)?.cell_rect();
        let stacks = self.buildings.remove(&self.content, id, sim)?;
        let mut report = RemoveReport::default();
        let mut over = vec![];
        for s in stacks {
            match self.player.insert_stack(&self.content, s) {
                None => report.taken.push(s),
                Some(rest) => {
                    if rest.count < s.count {
                        report.taken.push(Stack { item: s.item, count: s.count - rest.count });
                    }
                    over.push(rest);
                }
            }
        }
        let left = Buildings::drop_as_cells(&self.content, sim, area, &over);
        for s in over {
            let not_dropped: u32 = left.iter().filter(|l| l.item == s.item).map(|l| l.count).sum();
            if s.count > not_dropped {
                report.dropped.push(Stack { item: s.item, count: s.count - not_dropped });
            }
        }
        report.left = left;
        Ok(report)
    }

    /// Set the recipe of a machine. The old buffer contents go to the player's inventory; what
    /// does not fit is returned.
    pub fn set_recipe(&mut self, id: BuildingId, recipe: Option<RecipeId>) -> Result<Vec<Stack>, RecipeError> {
        let back = self.buildings.set_recipe(&self.content, id, recipe)?;
        Ok(back.into_iter().filter_map(|s| self.player.insert_stack(&self.content, s)).collect())
    }

    /// Move up to `count` of an item from the player's inventory into a building.
    /// Returns the count moved.
    pub fn insert_from_player(&mut self, id: BuildingId, item: ItemRef, count: u32) -> u32 {
        let n = count.min(self.player.count(item)).min(self.buildings.room_for(&self.content, id, item));
        let n = self.buildings.insert(&self.content, id, item, n);
        self.player.remove(item, n);
        n
    }

    /// Move a machine's products into the player's inventory, as much as fits. What does not fit
    /// stays in the machine. Returns the stacks moved.
    pub fn take_outputs_to_player(&mut self, id: BuildingId) -> Vec<Stack> {
        let mut moved = vec![];
        for s in self.buildings.outputs(&self.content, id) {
            let room = self.player.room_for(&self.content, s.item, s.count);
            let n = self.buildings.take_output(&self.content, id, s.item, room);
            if n > 0 {
                self.player.insert(&self.content, s.item, n);
                moved.push(Stack { item: s.item, count: n });
            }
        }
        moved
    }

    /// A mouse action on a slot. `other` is the inventory that shift and ctrl clicks move to.
    /// Returns true if anything changed.
    pub fn click(&mut self, target: InvTarget, slot: usize, click: Click, other: Option<InvTarget>) -> bool {
        let content = &*self.content;
        let changed = match (target, other) {
            (InvTarget::Player, Some(InvTarget::Building(id))) => {
                let o = self.buildings.inventory_mut(id);
                self.player.click(content, slot, click, &mut self.cursor, o)
            }
            (InvTarget::Player, _) => self.player.click(content, slot, click, &mut self.cursor, None),
            (InvTarget::Building(id), o) => {
                let Some(inv) = self.buildings.inventory_mut(id) else { return false };
                let other = matches!(o, Some(InvTarget::Player)).then_some(&mut self.player);
                inv.click(content, slot, click, &mut self.cursor, other)
            }
        };
        if changed {
            for t in [Some(target), other].into_iter().flatten() {
                if let InvTarget::Building(id) = t {
                    self.buildings.wake(id);
                }
            }
        }
        changed
    }

    /// Queue hand crafts. `known` says if the player knows a recipe (from the progression).
    pub fn craft(&mut self, recipe: RecipeId, count: u32, known: &dyn Fn(RecipeId) -> bool) -> Result<u32, CraftError> {
        self.hand.craft(&self.content, &mut self.player, recipe, count, known)
    }

    /// Cancel a craft request. Returns the items that did not fit back into the inventory.
    pub fn cancel_craft(&mut self, request: u32) -> Vec<Stack> {
        self.hand.cancel(&self.content, &mut self.player, request)
    }

    /// Data for the building window.
    pub fn building_view(&self, id: BuildingId) -> Option<BuildingView> {
        self.buildings.view(&self.content, id)
    }

    /// Data for the player's inventory screen.
    pub fn player_view(&self) -> InventoryView {
        self.player.view(&self.content)
    }

    /// Data for the crafting queue.
    pub fn crafting_view(&self) -> Vec<CraftJobView> {
        self.hand.view(&self.content)
    }

    /// Building events since the last call (for alerts).
    pub fn take_events(&mut self) -> Vec<FactoryEvent> {
        self.buildings.take_events()
    }
}
