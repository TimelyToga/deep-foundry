//! Construction on the simulation thread (game design section 18): placement from the hand,
//! drag lines that stop at the first failure, turning placed buildings, removal with the remove
//! button (one building after another, each takes a short time), copy and paste of settings, and
//! undo and redo.
//!
//! This is a child module of `factory_host`, so it can use the private fields of `FactoryHost`.
//!
//! Undo rules:
//! - Each player action (one mouse press, one key) is one undo entry. A drag line is one entry.
//! - Undo of a placement takes the building back into the inventory (with its contents).
//! - Undo of a removal places the building again with its turn and its recipe, if the building
//!   item is in the inventory or in the hand.
//! - Undo of a turn turns the building back.
//! - Undo and redo work at any distance from the robot.
//! - A part of an entry that cannot be done (the building is gone, no item) is skipped with a
//!   message. The rest is done.

use super::{DragStop, FactoryHost, Placement, RemoveView, Turn};
use crate::construct::next_rotation;
use foundry_content::{ItemRef, Layer};
use foundry_core::{BuildingId, BuildingKindId, CellPos, CellRect, RecipeId, TilePos};
use foundry_factory::{Logic, PartStack};
use foundry_sim::Simulation;
use std::collections::VecDeque;

/// Undo keeps this many actions.
pub const UNDO_LIMIT: usize = 100;
/// Ticks to remove a building: a base time plus a time for each tile of its footprint. A belt
/// takes 10 ticks, a 2 × 2 machine 22 ticks.
pub const REMOVE_TICKS_BASE: u32 = 6;
pub const REMOVE_TICKS_PER_TILE: u32 = 4;

/// A building as undo sees it: what it is, where it stands, how it is turned and its recipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    pub kind: BuildingKindId,
    pub at: TilePos,
    pub rotation: u8,
    pub flip: bool,
    pub recipe: Option<RecipeId>,
}

/// One change that undo can take back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Record {
    Placed(Plan),
    Removed(Plan),
    Turned { kind: BuildingKindId, at: TilePos, from: (u8, bool), to: (u8, bool) },
}

/// The changes of one player action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub action: u32,
    pub records: Vec<Record>,
}

/// The undo and redo lists.
#[derive(Debug, Clone, Default)]
pub struct UndoStack {
    undo: Vec<Group>,
    redo: Vec<Group>,
}

impl UndoStack {
    /// A new change by the player. It joins the last entry if that has the same action. A new
    /// change clears the redo list.
    pub fn push(&mut self, action: u32, r: Record) {
        self.redo.clear();
        match self.undo.last_mut() {
            Some(g) if g.action == action => g.records.push(r),
            _ => self.push_group(Group { action, records: vec![r] }),
        }
    }

    fn push_group(&mut self, g: Group) {
        self.undo.push(g);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
    }

    #[cfg(test)]
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    #[cfg(test)]
    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }
}

/// The buildings that the remove button takes, in order.
#[derive(Debug, Clone, Default)]
pub struct Removal {
    queue: VecDeque<BuildingId>,
    /// Ticks spent on the first building of the queue.
    ticks: u32,
    /// The press of the remove button (the undo entry).
    action: u32,
}

/// Ticks to remove a building of this size (in tiles).
pub fn remove_ticks(size: (u8, u8)) -> u32 {
    REMOVE_TICKS_BASE + REMOVE_TICKS_PER_TILE * size.0 as u32 * size.1 as u32
}

/// The recipe of a machine.
fn recipe_of(b: &foundry_factory::Building) -> Option<RecipeId> {
    match &b.logic {
        Logic::Machine(m) => m.recipe,
        _ => None,
    }
}

impl FactoryHost {
    /// The action number of a command; 0 gets a new number of the host.
    pub(super) fn action_id(&mut self, action: u32) -> u32 {
        if action != 0 {
            return action;
        }
        self.own_actions = self.own_actions.wrapping_sub(1).max(1 << 31);
        self.own_actions
    }

    // ------------------------------------------------------------ place

    /// Place the building in the hand. After a failure, other placements of the same action are
    /// ignored (a drag line stops at the first place that fails).
    pub(super) fn place(&mut self, p: Placement, sim: &mut Simulation) {
        let action = self.action_id(p.action);
        if self.drag_stop.as_ref().is_some_and(|s| s.action == action) {
            return;
        }
        match self.place_from_hand(&p, sim) {
            Ok(id) => {
                let mut recipe = None;
                if let Some(r) = p.recipe {
                    match self.factory.set_recipe(id, Some(r)) {
                        Ok(left) => {
                            recipe = Some(r);
                            self.drop_near_robot(sim, &left);
                        }
                        Err(e) => self.notice(e.to_string()),
                    }
                }
                let (rotation, flip) = (p.rotation & 3, p.flip);
                self.undo.push(action, Record::Placed(Plan { kind: p.kind, at: p.at, rotation, flip, recipe }));
                if self.drag_stop.as_ref().is_some_and(|s| s.action != action) {
                    self.drag_stop = None;
                }
            }
            Err(e) => {
                self.notice(e.clone());
                self.drag_stop = Some(DragStop { action, at: p.at, reason: e });
            }
        }
    }

    /// Check and place a building from the hand, and take it from the hand. When the hand is
    /// empty, the next stack of this building comes from the inventory (as in Factorio).
    fn place_from_hand(&mut self, p: &Placement, sim: &mut Simulation) -> Result<BuildingId, String> {
        let content = self.factory.content.clone();
        let part = content.factory.buildings.get(p.kind.0 as usize).ok_or("Unknown building")?.part;
        if !self.factory.cursor.is_some_and(|c| c.part == part) {
            return Err(format!("No {} in the hand", content.factory.part_def(part).name));
        }
        self.check_place(p.kind, p.at, p.rotation, p.flip, sim)?;
        let id = self.factory.place(p.kind, p.at, p.rotation, p.flip, sim).map_err(|e| e.to_string())?;
        self.last_placed = self.factory.buildings.get(id).map(|b| (p.kind, b.cell_rect()));
        self.ghost_check = None;
        if let Some(c) = self.factory.cursor.as_mut() {
            c.count -= 1;
            if c.count == 0 {
                self.factory.cursor = None;
                let size = foundry_factory::inventory::stack_size(&content, part);
                let n = self.factory.player.remove(ItemRef::Part(part), size);
                if n > 0 {
                    self.factory.cursor = Some(PartStack::new(part, n));
                }
            }
        }
        Ok(id)
    }

    // ------------------------------------------------------------ remove

    /// The building at a cell, if the player may remove it (not the Hub, in reach). Says why not.
    pub(super) fn removable_at(&mut self, p: CellPos) -> Option<BuildingId> {
        let id = self.building_at(p)?;
        let b = self.factory.buildings.get(id)?;
        if self.factory.content.factory.building_def(b.kind).kind == "hub" {
            self.notice("The Hub cannot be removed");
            return None;
        }
        if self.out_of_reach(b.cell_rect()) {
            self.notice("Out of reach");
            return None;
        }
        Some(id)
    }

    /// Take a building into the inventory now, as part of an action.
    pub(super) fn remove_building(&mut self, id: BuildingId, action: u32, sim: &mut Simulation) {
        let Some(b) = self.factory.buildings.get(id) else { return };
        let plan = Plan { kind: b.kind, at: b.at, rotation: b.transform.rotation, flip: b.transform.flip, recipe: recipe_of(b) };
        match self.factory.remove_to_player(id, sim) {
            Ok(report) => {
                if self.open == Some(id) {
                    self.open = None;
                }
                if !report.dropped.is_empty() || !report.left.is_empty() {
                    self.notice("The inventory is full: some items fell out");
                }
                self.ghost_check = None;
                self.undo.push(action, Record::Removed(plan));
            }
            Err(e) => self.notice(e.to_string()),
        }
    }

    /// A new player input. While the remove button is down, the buildings under the path of the
    /// mouse since the last input join the removal queue.
    pub(super) fn set_input(&mut self, input: super::PlayerInput) {
        let old = std::mem::replace(&mut self.input, input);
        // Keep the presses until a tick uses them (see `Taps`).
        let t = &mut self.taps;
        t.jump |= input.movement.jump && !old.movement.jump;
        t.dig |= input.dig && !old.dig;
        t.scan |= input.scan && !old.scan;
        if input.spray.is_some() && old.spray.is_none() {
            t.spray = input.spray;
        }
        let Some(action) = input.remove else {
            self.removal = Removal::default();
            return;
        };
        if self.removal.action != action {
            self.removal = Removal { action, ..Default::default() };
        }
        let from = if old.remove == Some(action) { old.aim } else { input.aim };
        // Points along the path, at most half a tile apart, so a fast mouse misses no tile.
        let (dx, dy) = ((input.aim.x - from.x) as f32, (input.aim.y - from.y) as f32);
        let steps = ((dx.abs().max(dy.abs())) / 4.0).ceil().max(1.0) as i32;
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let p = CellPos::new(from.x + (dx * t).round() as i32, from.y + (dy * t).round() as i32);
            let Some(id) = self.building_at(p) else { continue };
            if self.removal.queue.contains(&id) {
                continue;
            }
            if let Some(id) = self.removable_at(p) {
                self.removal.queue.push_back(id);
            }
        }
    }

    /// One tick of the remove button: work on the first building of the queue.
    pub(super) fn tick_removal(&mut self, sim: &mut Simulation) {
        if self.input.remove.is_none() {
            return;
        }
        let (id, size) = loop {
            let Some(&id) = self.removal.queue.front() else { return };
            match self.factory.buildings.get(id) {
                Some(b) if !self.out_of_reach(b.cell_rect()) => break (id, b.size),
                _ => {
                    self.removal.queue.pop_front();
                    self.removal.ticks = 0;
                }
            }
        };
        self.removal.ticks += 1;
        if self.removal.ticks >= remove_ticks(size) {
            self.removal.queue.pop_front();
            self.removal.ticks = 0;
            let action = self.removal.action;
            self.remove_building(id, action, sim);
        }
    }

    /// The building that the remove button takes now, with its progress.
    pub(super) fn removal_view(&self) -> Option<RemoveView> {
        self.input.remove?;
        let b = self.factory.buildings.get(*self.removal.queue.front()?)?;
        Some(RemoveView { rect: b.cell_rect(), progress: self.removal.ticks as f32 / remove_ticks(b.size) as f32 })
    }

    /// The buildings that wait in the removal queue after the first one.
    pub(super) fn removal_queue_rects(&self) -> Vec<CellRect> {
        if self.input.remove.is_none() {
            return vec![];
        }
        self.removal.queue.iter().skip(1).filter_map(|id| self.factory.buildings.get(*id)).map(|b| b.cell_rect()).collect()
    }

    // ------------------------------------------------------------ turn

    /// Turn the placed building at a cell (R on a building with an empty hand).
    pub(super) fn turn_at(&mut self, at: CellPos, turn: Turn, action: u32) {
        let action = self.action_id(action);
        if self.drag_stop.as_ref().is_some_and(|s| s.action == action) {
            return;
        }
        let content = self.factory.content.clone();
        let Some(id) = self.building_at(at) else { return };
        let Some(b) = self.factory.buildings.get(id) else { return };
        let def = content.factory.building_def(b.kind);
        if def.kind == "hub" {
            self.notice("The Hub cannot be turned");
            return;
        }
        if self.out_of_reach(b.cell_rect()) {
            self.notice("Out of reach");
            return;
        }
        let from = (b.transform.rotation, b.transform.flip);
        let to = match turn {
            Turn::Clockwise => (next_rotation(def, from.0, false, true), from.1),
            Turn::CounterClockwise => (next_rotation(def, from.0, true, true), from.1),
            Turn::To { rotation, flip } => (rotation & 3, flip),
        };
        if to == from {
            return;
        }
        let (kind, tile) = (b.kind, b.at);
        match self.factory.set_transform(id, to.0, to.1) {
            Ok(()) => {
                self.ghost_check = None;
                self.undo.push(action, Record::Turned { kind, at: tile, from, to });
            }
            Err(e) => self.notice(e.to_string()),
        }
    }

    // ------------------------------------------------------------ copy and paste

    /// Shift + right click: remember the recipe of a building.
    pub(super) fn copy_settings(&mut self, p: CellPos) {
        let content = self.factory.content.clone();
        let Some(id) = self.building_at(p) else { return };
        let Some(b) = self.factory.buildings.get(id) else { return };
        let name = content.factory.building_def(b.kind).name.clone();
        if !matches!(b.logic, Logic::Machine(_)) {
            self.notice(format!("{name} has no settings to copy"));
            return;
        }
        let recipe = recipe_of(b);
        self.copied = Some((b.kind, recipe));
        let what = recipe.map_or("no recipe".to_string(), |r| content.factory.recipe_def(r).name.clone());
        self.notice(format!("Copied from {name}: {what}"));
    }

    /// Shift + left click: give the remembered recipe to a building of the same kind.
    pub(super) fn paste_settings(&mut self, p: CellPos, sim: &mut Simulation) {
        let content = self.factory.content.clone();
        let Some(id) = self.building_at(p) else { return };
        let Some(b) = self.factory.buildings.get(id) else { return };
        let Some((kind, recipe)) = self.copied else {
            self.notice("Nothing to paste: Shift + right click a machine first");
            return;
        };
        if b.kind != kind {
            self.notice(format!("Paste works only on a {}", content.factory.building_def(kind).name));
            return;
        }
        if self.out_of_reach(b.cell_rect()) {
            self.notice("Out of reach");
            return;
        }
        if recipe_of(b) == recipe {
            return;
        }
        match self.factory.set_recipe(id, recipe) {
            Ok(left) => {
                self.drop_near_robot(sim, &left);
                let what = recipe.map_or("no recipe".to_string(), |r| content.factory.recipe_def(r).name.clone());
                self.notice(format!("Pasted: {what}"));
            }
            Err(e) => self.notice(e.to_string()),
        }
    }

    // ------------------------------------------------------------ undo and redo

    /// Take back the last action of the player.
    pub(super) fn undo(&mut self, sim: &mut Simulation) {
        let Some(g) = self.undo.undo.pop() else {
            self.notice("Nothing to undo");
            return;
        };
        let mut done = vec![];
        let mut failed = None;
        for r in g.records.iter().rev() {
            match self.revert(r, sim) {
                Ok(()) => done.push(*r),
                Err(e) => {
                    failed.get_or_insert(e);
                }
            }
        }
        done.reverse();
        if !done.is_empty() {
            self.undo.redo.push(Group { action: g.action, records: done });
        }
        if let Some(e) = failed {
            self.notice(format!("Undo: {e}"));
        }
        self.ghost_check = None;
    }

    /// Do again the last action that undo took back.
    pub(super) fn redo(&mut self, sim: &mut Simulation) {
        let Some(g) = self.undo.redo.pop() else {
            self.notice("Nothing to redo");
            return;
        };
        let mut done = vec![];
        let mut failed = None;
        for r in &g.records {
            match self.redo_record(r, sim) {
                Ok(()) => done.push(*r),
                Err(e) => {
                    failed.get_or_insert(e);
                }
            }
        }
        if !done.is_empty() {
            self.undo.push_group(Group { action: g.action, records: done });
        }
        if let Some(e) = failed {
            self.notice(format!("Redo: {e}"));
        }
        self.ghost_check = None;
    }

    fn revert(&mut self, r: &Record, sim: &mut Simulation) -> Result<(), String> {
        match *r {
            Record::Placed(p) => self.take_away(p, sim),
            Record::Removed(p) => self.put_back(p, sim),
            Record::Turned { kind, at, from, .. } => self.set_turn(kind, at, from),
        }
    }

    fn redo_record(&mut self, r: &Record, sim: &mut Simulation) -> Result<(), String> {
        match *r {
            Record::Placed(p) => self.put_back(p, sim),
            Record::Removed(p) => self.take_away(p, sim),
            Record::Turned { kind, at, to, .. } => self.set_turn(kind, at, to),
        }
    }

    /// The placed building of this kind with its top-left tile at `at`.
    fn find(&self, kind: BuildingKindId, at: TilePos) -> Option<BuildingId> {
        let layer = self.factory.content.factory.buildings.get(kind.0 as usize).map_or(Layer::Front, |d| d.layer);
        let id = self.factory.buildings.at_tile(at, layer)?;
        self.factory.buildings.get(id).filter(|b| b.kind == kind && b.at == at).map(|_| id)
    }

    fn name(&self, kind: BuildingKindId) -> String {
        self.factory.content.factory.building_def(kind).name.clone()
    }

    /// Take a building back into the inventory (undo of a placement, redo of a removal).
    fn take_away(&mut self, p: Plan, sim: &mut Simulation) -> Result<(), String> {
        let id = self.find(p.kind, p.at).ok_or_else(|| format!("the {} is not there any more", self.name(p.kind)))?;
        let report = self.factory.remove_to_player(id, sim).map_err(|e| e.to_string())?;
        if self.open == Some(id) {
            self.open = None;
        }
        if !report.dropped.is_empty() || !report.left.is_empty() {
            self.notice("The inventory is full: some items fell out");
        }
        Ok(())
    }

    /// Place a building again from the inventory or the hand (undo of a removal, redo of a
    /// placement), with its turn and its recipe.
    fn put_back(&mut self, p: Plan, sim: &mut Simulation) -> Result<(), String> {
        let content = self.factory.content.clone();
        let part = content.factory.building_def(p.kind).part;
        let item = ItemRef::Part(part);
        let in_hand = self.factory.cursor.filter(|c| c.part == part).map_or(0, |c| c.count);
        if self.factory.player.count(item) + in_hand == 0 {
            return Err(format!("no {} in the inventory", self.name(p.kind)));
        }
        self.check_place_reach(p.kind, p.at, p.rotation, p.flip, false, sim)?;
        let id = self.factory.place(p.kind, p.at, p.rotation, p.flip, sim).map_err(|e| e.to_string())?;
        if self.factory.player.remove(item, 1) == 0
            && let Some(c) = self.factory.cursor.as_mut()
        {
            c.count -= 1;
            if c.count == 0 {
                self.factory.cursor = None;
            }
        }
        if let Some(r) = p.recipe
            && let Ok(left) = self.factory.set_recipe(id, Some(r))
        {
            self.drop_near_robot(sim, &left);
        }
        Ok(())
    }

    fn set_turn(&mut self, kind: BuildingKindId, at: TilePos, to: (u8, bool)) -> Result<(), String> {
        let id = self.find(kind, at).ok_or_else(|| format!("the {} is not there any more", self.name(kind)))?;
        self.factory.set_transform(id, to.0, to.1).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
#[path = "host_build_tests.rs"]
mod tests;
