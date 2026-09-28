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
//!
//! How the buildings and the progression work together:
//! - Labs and the Hub call `Progress` through `progress_link::ProgressLink` in the building tick.
//! - [`Factory::is_recipe_known`] decides which recipes hand crafting and machines can use.
//! - [`Factory::tick`] checks the guide goals once every [`GUIDE_PERIOD`] ticks.
//! - The scan tool calls [`Factory::scan`]. The game calls [`Factory::observe_reaction`] for each
//!   reaction event of the simulation.

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
pub mod transfer;
pub mod views;

pub use buildings::{Building, Buildings, FactoryEvent, Logic};
pub use crafting::{CraftError, CraftJobView, HandCrafting};
pub use geometry::Transform;
pub use inventory::{Click, Inventory, InventoryView, PartStack, Place, PlaceView, SlotView, Tank, TankRule, TankView};
pub use transfer::RobotSlot;
pub use machines::{RecipeError, Status};
pub use placement::{PlaceError, RemoveError};
pub use progress::{GoalView, Guide, GuideState, Progress, ProgressEvent};
pub use views::{BufferView, BuildingView, PortView};

use foundry_content::{Content, ItemRef, Stack};
use foundry_core::{BuildingId, BuildingKindId, CellPos, MaterialId, RecipeId, TechId, TilePos};
use foundry_sim::Simulation;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Ticks between two checks of the guide goals.
pub const GUIDE_PERIOD: u64 = 30;

/// The robot sees reactions up to this many cells away (see [`Factory::observe_reaction`]).
pub const REACTION_SEE_RANGE: i32 = 96;

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
    /// Where the robot is (for the workbench speed and for seeing reactions).
    /// `None`: no robot in the world.
    pub player_pos: Option<CellPos>,
    /// The guide goals. They are data, like `content`, and are not saved. `Factory::new` starts
    /// with an empty guide; the game sets the loaded one (`Guide::load_default()`).
    pub guide: Arc<Guide>,
    /// The current research in the last tick. When it changes, the labs wake. Not saved.
    last_research: Option<TechId>,
}

/// The saved form of the factory state. (The guide is data and is not saved.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactorySave {
    pub buildings: Buildings,
    /// Research, Hub stages, discoveries and done guide goals.
    pub progress: Progress,
    pub player: Inventory,
    pub cursor: Option<PartStack>,
    pub hand: HandCrafting,
    pub player_pos: Option<CellPos>,
}

impl Factory {
    pub fn new(content: Arc<Content>) -> Self {
        let mut f = Self {
            buildings: buildings::Buildings::new(&content),
            progress: progress::Progress::new(&content),
            player: Inventory::player(),
            cursor: None,
            hand: HandCrafting::new(),
            player_pos: None,
            guide: Arc::new(Guide::default()),
            last_research: None,
            content,
        };
        f.update_hub_rule();
        f
    }

    /// Tell the buildings what the Hub takes now (the Hub repair stages and what they still
    /// need).
    fn update_hub_rule(&mut self) {
        let need = self.progress.hub_need(&self.content);
        self.buildings.set_hub_rule(buildings::HubRule { stage: self.progress.stage(), need: Some(Arc::new(need)) });
    }

    /// Run one tick. Call it before `Simulation::advance` in the same tick.
    /// Order: buildings (ports take input, machines and labs work, the Hub delivers, ports give
    /// output, belts move), then hand crafting, then progress. The guide goals are checked once
    /// every [`GUIDE_PERIOD`] ticks.
    pub fn tick(&mut self, sim: &mut Simulation) {
        let research = self.progress.current();
        if research != self.last_research {
            self.last_research = research;
            if research.is_some() {
                self.buildings.wake_labs();
            }
        }
        self.buildings.tick(&self.content, sim, &mut self.progress);
        // The Hub may have delivered items in this tick.
        self.update_hub_rule();
        if !self.hand.is_idle() {
            let speed = self.player_pos.map_or(1.0, |p| self.buildings.hand_speed(&self.content, p));
            self.hand.tick(&self.content, &mut self.player, speed);
        }
        self.progress.tick(&self.content);
        if self.buildings.now().is_multiple_of(GUIDE_PERIOD) {
            self.update_guide();
        }
    }

    /// A copy of the state for a save file.
    pub fn save(&self) -> FactorySave {
        FactorySave {
            buildings: self.buildings.clone(),
            progress: self.progress.clone(),
            player: self.player.clone(),
            cursor: self.cursor,
            hand: self.hand.clone(),
            player_pos: self.player_pos,
        }
    }

    /// Put back the state from a save file.
    pub fn load(&mut self, save: FactorySave) {
        self.buildings = save.buildings;
        self.progress = save.progress;
        self.last_research = None;
        self.player = save.player;
        self.cursor = save.cursor;
        self.hand = save.hand;
        self.player_pos = save.player_pos;
        // Older saves have smaller robot tanks and crates for parts only.
        self.player.grow_tanks(inventory::PLAYER_TANKS, inventory::PLAYER_TANK_UNITS);
        self.buildings.upgrade_storage(&self.content);
        self.buildings.upgrade_machines(&self.content);
        self.update_hub_rule();
    }

    /// The robot dug one cell of `material`: one unit of its broken form (see
    /// `broken_into` in the data) goes into the tanks. The first dig of a material discovers it,
    /// the same as a scan. Returns false and takes nothing when the tanks have no room.
    pub fn take_dug_cell(&mut self, material: MaterialId) -> bool {
        let broken = self.content.materials.broken_into.get(material.index()).copied().unwrap_or(material);
        let item = ItemRef::Material(broken);
        if self.player.room_for(&self.content, item, 1) == 0 {
            return false;
        }
        self.player.insert(&self.content, item, 1);
        if !self.progress.is_material_discovered(material) {
            self.scan(material);
        }
        true
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
        let id = self.buildings.place(&self.content, kind, at, Transform::new(rotation, flip), sim)?;
        // A machine with a fuel slot (the campfire) that can make only one known recipe starts
        // with it, like a furnace in Factorio.
        let burner = matches!(self.buildings.get(id).map(|b| &b.logic), Some(Logic::Machine(m)) if m.fuel.is_some());
        let known: Vec<RecipeId> =
            Buildings::recipes_for(&self.content, kind).into_iter().filter(|r| self.is_recipe_known(*r)).collect();
        if let ([only], true) = (&known[..], burner) {
            let _ = self.buildings.set_recipe(&self.content, id, Some(*only));
        }
        Ok(id)
    }

    /// The ports a building would have at this place (arrows on the ghost).
    pub fn ghost_ports(&self, kind: BuildingKindId, at: TilePos, rotation: u8, flip: bool) -> Vec<PortView> {
        Buildings::ghost_ports(&self.content, kind, at, Transform::new(rotation, flip))
    }

    /// Turn or mirror a placed building where it stands (see `Buildings::set_transform`).
    pub fn set_transform(&mut self, id: BuildingId, rotation: u8, flip: bool) -> Result<(), PlaceError> {
        self.buildings.set_transform(&self.content, id, Transform::new(rotation, flip))
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

    /// Set the recipe of a machine. The player must know the recipe. The old buffer contents go
    /// to the player's inventory; what does not fit is returned.
    pub fn set_recipe(&mut self, id: BuildingId, recipe: Option<RecipeId>) -> Result<Vec<Stack>, RecipeError> {
        if let Some(r) = recipe
            && !self.is_recipe_known(r)
        {
            let name = self.content.factory.recipes.get(r.0 as usize).map_or_else(|| format!("{r:?}"), |d| d.name.clone());
            return Err(RecipeError::NotKnown { recipe: name });
        }
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

    /// True if the player can use a recipe: no technology unlocks it, or that technology is
    /// done. Hand crafting and machine recipes use this.
    pub fn is_recipe_known(&self, recipe: RecipeId) -> bool {
        recipe_known(&self.content, &self.progress, recipe)
    }

    /// Check if `count` hand crafts of a recipe can be queued now (with intermediates).
    pub fn can_craft(&self, recipe: RecipeId, count: u32) -> Result<(), CraftError> {
        let known = |r: RecipeId| self.is_recipe_known(r);
        HandCrafting::can_craft(&self.content, &self.player, recipe, count, &known)
    }

    /// Queue hand crafts of a known recipe. Missing ingredients that the player can hand craft
    /// with known recipes are queued first. Returns the request number (for `cancel_craft`).
    pub fn craft(&mut self, recipe: RecipeId, count: u32) -> Result<u32, CraftError> {
        let (content, progress) = (&*self.content, &self.progress);
        let known = |r: RecipeId| recipe_known(content, progress, r);
        self.hand.craft(content, &mut self.player, recipe, count, &known)
    }

    /// Cancel a craft request. Returns the items that did not fit back into the inventory.
    pub fn cancel_craft(&mut self, request: u32) -> Vec<Stack> {
        self.hand.cancel(&self.content, &mut self.player, request)
    }

    /// Data for the building window. `recipes` has only the recipes the player knows.
    pub fn building_view(&self, id: BuildingId) -> Option<BuildingView> {
        let mut v = self.buildings.view(&self.content, id)?;
        v.recipes.retain(|r| self.is_recipe_known(*r));
        Some(v)
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

    /// The scan tool looked at a material. The first scan of a material discovers it and gives
    /// discovery points. The material it breaks into when dug (for example the raw ore powder of
    /// an ore vein) is discovered too. Returns the materials that this scan discovered.
    pub fn scan(&mut self, material: MaterialId) -> Vec<MaterialId> {
        let Some(&broken) = self.content.materials.broken_into.get(material.index()) else { return vec![] };
        let mut found = vec![];
        for m in [material, broken] {
            if self.progress.discover_material(m) {
                found.push(m);
            }
        }
        found
    }

    /// The simulation reports that reaction `index` (into `Content::reactions`) happened at a
    /// cell. If the robot is at most [`REACTION_SEE_RANGE`] cells away (or there is no robot),
    /// the player sees it. The first time the player sees a reaction, it is discovered and gives
    /// discovery points. Returns true if the reaction is new.
    ///
    /// The simulation does not send reaction events yet. See "A reaction event from the
    /// simulation" in `docs/design/requests/factory-core.md`.
    pub fn observe_reaction(&mut self, index: u16, at: CellPos) -> bool {
        let Some(reaction) = self.content.reactions.get(index as usize) else { return false };
        if let Some(p) = self.player_pos {
            let (dx, dy) = ((at.x - p.x) as i64, (at.y - p.y) as i64);
            let range = REACTION_SEE_RANGE as i64;
            if dx * dx + dy * dy > range * range {
                return false;
            }
        }
        let key = progress::reaction_key(&self.content, reaction);
        self.progress.discover_reaction(&key)
    }

    /// Check the guide goals now. A goal whose condition is met is done and gives its reward.
    /// `tick` calls this once every [`GUIDE_PERIOD`] ticks.
    pub fn update_guide(&mut self) {
        let counts = GuideCounts { player: &self.player, cursor: self.cursor, buildings: &self.buildings };
        self.progress.update_guide(&self.guide, &self.content, &counts);
    }

    /// The goals of all open tiers, for the guide screen.
    pub fn guide_view(&self) -> Vec<GoalView> {
        self.progress.guide_view(&self.guide, &self.content, self)
    }
}

/// True if the recipe exists and the player knows it.
fn recipe_known(content: &Content, progress: &Progress, recipe: RecipeId) -> bool {
    content.factory.recipes.get(recipe.0 as usize).is_some() && progress.is_recipe_known(content, recipe)
}

/// The player's items and the placed buildings, for the guide.
struct GuideCounts<'a> {
    player: &'a Inventory,
    cursor: Option<PartStack>,
    buildings: &'a Buildings,
}

impl GuideState for GuideCounts<'_> {
    /// Items in the player's inventory (slots and material tanks), on the cursor, and in storage
    /// buildings (crates and barrels).
    fn item_count(&self, item: ItemRef) -> u32 {
        let held = match (self.cursor, item) {
            (Some(c), ItemRef::Part(p)) if c.part == p => c.count,
            _ => 0,
        };
        self.player.count(item).saturating_add(held).saturating_add(self.buildings.stored_count(item))
    }

    fn building_count(&self, kind: BuildingKindId) -> u32 {
        self.buildings.count_of(kind)
    }
}

impl GuideState for Factory {
    fn item_count(&self, item: ItemRef) -> u32 {
        GuideCounts { player: &self.player, cursor: self.cursor, buildings: &self.buildings }.item_count(item)
    }

    fn building_count(&self, kind: BuildingKindId) -> u32 {
        self.buildings.count_of(kind)
    }
}
