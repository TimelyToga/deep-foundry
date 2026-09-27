//! Hand crafting for the player (game design section 6.5).
//!
//! - Only recipes with `hand = true` that the player knows can be crafted. The caller gives a
//!   function that says if a recipe is known, so this file does not depend on the progression code.
//! - `craft` takes the ingredients out of the inventory at once (they are held by the job).
//!   `cancel` gives them back.
//! - If an ingredient is missing but the player can hand craft it from what they have, the
//!   intermediate is queued first, as in Factorio. The intermediate's product goes straight to
//!   the job that needs it; extra pieces go to the inventory.
//! - Speed: 1 near nothing, the workbench speed near a workbench (the caller gives the factor).
//! - A finished craft that does not fit into the inventory waits until there is room.

use crate::inventory::Inventory;
use foundry_content::{Content, ItemRef, Stack};
use foundry_core::{RecipeId, Rng};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Intermediates are searched this many levels deep.
const MAX_DEPTH: u32 = 6;

/// One line of the crafting queue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CraftJob {
    /// The `craft` call this job belongs to.
    pub request: u32,
    /// A unique number for this job.
    pub uid: u32,
    pub recipe: RecipeId,
    /// Crafts still to make.
    pub count: u32,
    /// Items held for this job, for each recipe input (same order).
    pub held: Vec<u32>,
    /// The job that uses this job's product (`uid`). `None` for the job the player asked for.
    pub parent: Option<u32>,
}

/// Why a craft cannot be queued.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CraftError {
    /// The recipe is not a hand recipe.
    NotByHand,
    /// The player does not know the recipe yet.
    NotKnown,
    ZeroCount,
    /// An ingredient is missing and cannot be hand crafted from what the player has.
    Missing { item: ItemRef, name: String, count: u32 },
}

impl std::fmt::Display for CraftError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CraftError::NotByHand => write!(f, "This cannot be made by hand"),
            CraftError::NotKnown => write!(f, "Research this recipe first"),
            CraftError::ZeroCount => write!(f, "Nothing to craft"),
            CraftError::Missing { name, count, .. } => write!(f, "Needs {count} more {name}"),
        }
    }
}

impl std::error::Error for CraftError {}

/// One job of the queue, for the screen.
#[derive(Debug, Clone, PartialEq)]
pub struct CraftJobView {
    pub request: u32,
    pub recipe: RecipeId,
    pub name: String,
    pub count: u32,
    /// Progress of the current craft, 0 to 1 (only the first job has progress).
    pub progress: f32,
    /// The job was queued to make an ingredient for another job.
    pub intermediate: bool,
    /// The first job waits because its product does not fit into the inventory.
    pub waiting_for_room: bool,
}

/// The player's crafting queue.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HandCrafting {
    /// Jobs in order. The first one is crafting.
    pub jobs: Vec<CraftJob>,
    /// Work on the first job's current craft, in ticks at speed 1. A craft needs `time × 60`.
    pub work: f64,
    /// The last finished craft did not fit into the inventory.
    pub waiting_for_room: bool,
    next_request: u32,
    next_uid: u32,
    /// Crafts finished (for byproduct chances).
    crafts: u64,
}

/// A job in a plan, before it is queued.
struct Planned {
    recipe: RecipeId,
    count: u32,
    held: Vec<u32>,
    /// Index in the plan list.
    parent: Option<usize>,
}

/// Make a plan for `count` crafts of `recipe` from the items in `avail`. Items are taken out of
/// `avail`. Missing items are made by hand recipes first. `order` gets the plan indices with every
/// job after the jobs that make its ingredients.
#[allow(clippy::too_many_arguments)]
fn plan(
    content: &Content,
    avail: &mut BTreeMap<ItemRef, u32>,
    recipe: RecipeId,
    count: u32,
    parent: Option<usize>,
    depth: u32,
    known: &dyn Fn(RecipeId) -> bool,
    list: &mut Vec<Planned>,
    order: &mut Vec<usize>,
    visiting: &mut Vec<RecipeId>,
) -> Result<(), CraftError> {
    let r = content.factory.recipe_def(recipe);
    let me = list.len();
    list.push(Planned { recipe, count, held: vec![0; r.inputs.len()], parent });
    for (k, s) in r.inputs.iter().enumerate() {
        let need = s.count.saturating_mul(count);
        let have = avail.get(&s.item).copied().unwrap_or(0);
        let take = have.min(need);
        if take > 0 {
            *avail.get_mut(&s.item).expect("item is in the map") -= take;
        }
        list[me].held[k] = take;
        let missing = need - take;
        if missing == 0 {
            continue;
        }
        let err = CraftError::Missing { item: s.item, name: content.item_name(s.item).to_string(), count: missing };
        if depth >= MAX_DEPTH {
            return Err(err);
        }
        let mut made = false;
        for cand in content.factory.recipes_making(s.item) {
            let cr = content.factory.recipe_def(cand);
            if !cr.hand || !known(cand) || visiting.contains(&cand) {
                continue;
            }
            let per: u32 = cr.outputs.iter().filter(|o| o.item == s.item).map(|o| o.count).sum();
            if per == 0 {
                continue;
            }
            let crafts = missing.div_ceil(per);
            let (saved_avail, saved_list, saved_order) = (avail.clone(), list.len(), order.len());
            visiting.push(cand);
            let ok = plan(content, avail, cand, crafts, Some(me), depth + 1, known, list, order, visiting).is_ok();
            visiting.pop();
            if ok {
                made = true;
                break;
            }
            *avail = saved_avail;
            list.truncate(saved_list);
            order.truncate(saved_order);
        }
        if !made {
            return Err(err);
        }
    }
    order.push(me);
    Ok(())
}

impl HandCrafting {
    pub fn new() -> Self {
        Self::default()
    }

    /// True if nothing is queued.
    pub fn is_idle(&self) -> bool {
        self.jobs.is_empty()
    }

    fn make_plan(
        content: &Content,
        inv: &Inventory,
        recipe: RecipeId,
        count: u32,
        known: &dyn Fn(RecipeId) -> bool,
    ) -> Result<(Vec<Planned>, Vec<usize>), CraftError> {
        let r = content.factory.recipes.get(recipe.0 as usize).ok_or(CraftError::NotByHand)?;
        if !r.hand {
            return Err(CraftError::NotByHand);
        }
        if !known(recipe) {
            return Err(CraftError::NotKnown);
        }
        if count == 0 {
            return Err(CraftError::ZeroCount);
        }
        let mut avail: BTreeMap<ItemRef, u32> = BTreeMap::new();
        for s in inv.contents() {
            avail.insert(s.item, s.count);
        }
        let (mut list, mut order, mut visiting) = (vec![], vec![], vec![recipe]);
        plan(content, &mut avail, recipe, count, None, 0, known, &mut list, &mut order, &mut visiting)?;
        Ok((list, order))
    }

    /// Check if `count` crafts of a recipe can be queued now (with intermediates).
    pub fn can_craft(
        content: &Content,
        inv: &Inventory,
        recipe: RecipeId,
        count: u32,
        known: &dyn Fn(RecipeId) -> bool,
    ) -> Result<(), CraftError> {
        Self::make_plan(content, inv, recipe, count, known).map(|_| ())
    }

    /// Queue `count` crafts of a recipe. The ingredients leave the inventory now. Missing
    /// ingredients that can be hand crafted are queued first. Returns the request number (for
    /// `cancel`).
    pub fn craft(
        &mut self,
        content: &Content,
        inv: &mut Inventory,
        recipe: RecipeId,
        count: u32,
        known: &dyn Fn(RecipeId) -> bool,
    ) -> Result<u32, CraftError> {
        let (list, order) = Self::make_plan(content, inv, recipe, count, known)?;
        let request = self.next_request;
        self.next_request = self.next_request.wrapping_add(1);
        let uids: Vec<u32> = (0..list.len() as u32).map(|k| self.next_uid.wrapping_add(k)).collect();
        self.next_uid = self.next_uid.wrapping_add(list.len() as u32);
        for &i in &order {
            let p = &list[i];
            let r = content.factory.recipe_def(p.recipe);
            for (k, s) in r.inputs.iter().enumerate() {
                let taken = inv.remove(s.item, p.held[k]);
                debug_assert_eq!(taken, p.held[k]);
            }
            self.jobs.push(CraftJob {
                request,
                uid: uids[i],
                recipe: p.recipe,
                count: p.count,
                held: p.held.clone(),
                parent: p.parent.map(|x| uids[x]),
            });
        }
        Ok(request)
    }

    /// Cancel a request: its jobs leave the queue and the held items go back to the inventory.
    /// Returns the items that did not fit (the caller drops them into the world).
    pub fn cancel(&mut self, content: &Content, inv: &mut Inventory, request: u32) -> Vec<Stack> {
        let first = self.jobs.first().map(|j| j.uid);
        let mut back: Vec<Stack> = vec![];
        self.jobs.retain(|j| {
            if j.request != request {
                return true;
            }
            let r = content.factory.recipe_def(j.recipe);
            for (k, s) in r.inputs.iter().enumerate() {
                if j.held[k] > 0 {
                    back.push(Stack { item: s.item, count: j.held[k] });
                }
            }
            false
        });
        if self.jobs.first().map(|j| j.uid) != first {
            self.work = 0.0;
            self.waiting_for_room = false;
        }
        back.into_iter().filter_map(|s| inv.insert_stack(content, s)).collect()
    }

    /// Run one tick. `speed` is the crafting speed factor (1 = recipe time).
    /// Returns true if a craft is in progress.
    pub fn tick(&mut self, content: &Content, inv: &mut Inventory, speed: f32) -> bool {
        let Some(job) = self.jobs.first() else {
            self.work = 0.0;
            return false;
        };
        let recipe = content.factory.recipe_def(job.recipe);
        if recipe.inputs.iter().enumerate().any(|(k, s)| job.held[k] < s.count) {
            // An ingredient is not there (should not happen: intermediates come first).
            return false;
        }
        let time = crate::machines::work_needed(recipe);
        self.work += speed.max(0.0) as f64;
        if self.work < time {
            return true;
        }
        // The craft is done. Products go to the parent job first, the rest to the inventory.
        let parent = job.parent.and_then(|uid| self.jobs.iter().position(|j| j.uid == uid));
        let mut to_parent: Vec<(usize, u32)> = vec![];
        let mut to_inventory: Vec<Stack> = vec![];
        for s in &recipe.outputs {
            let mut n = s.count;
            if let Some(pi) = parent {
                let pj = &self.jobs[pi];
                let pr = content.factory.recipe_def(pj.recipe);
                for (k, ps) in pr.inputs.iter().enumerate() {
                    if ps.item != s.item || n == 0 {
                        continue;
                    }
                    let already: u32 = to_parent.iter().filter(|(kk, _)| *kk == k).map(|(_, c)| c).sum();
                    let room = (ps.count * pj.count).saturating_sub(pj.held[k] + already);
                    let give = room.min(n);
                    if give > 0 {
                        to_parent.push((k, give));
                        n -= give;
                    }
                }
            }
            if n > 0 {
                to_inventory.push(Stack { item: s.item, count: n });
            }
        }
        let mut rng = Rng::new(0x6861_6e64 ^ self.crafts.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        for (s, chance) in &recipe.byproducts {
            if rng.chance(*chance) {
                to_inventory.push(*s);
            }
        }
        let mut trial = inv.clone();
        if to_inventory.iter().any(|s| trial.insert_stack(content, *s).is_some()) {
            self.work = time;
            self.waiting_for_room = true;
            return false;
        }
        *inv = trial;
        self.waiting_for_room = false;
        if let Some(pi) = parent {
            for (k, n) in to_parent {
                self.jobs[pi].held[k] += n;
            }
        }
        let job = &mut self.jobs[0];
        for (k, s) in recipe.inputs.iter().enumerate() {
            job.held[k] -= s.count;
        }
        job.count -= 1;
        self.crafts += 1;
        self.work = (self.work - time).max(0.0);
        if job.count == 0 {
            self.jobs.remove(0);
            self.work = 0.0;
        }
        true
    }

    /// The queue for the screen.
    pub fn view(&self, content: &Content) -> Vec<CraftJobView> {
        self.jobs
            .iter()
            .enumerate()
            .map(|(i, j)| {
                let r = content.factory.recipe_def(j.recipe);
                CraftJobView {
                    request: j.request,
                    recipe: j.recipe,
                    name: r.name.clone(),
                    count: j.count,
                    progress: if i == 0 { (self.work / crate::machines::work_needed(r)).clamp(0.0, 1.0) as f32 } else { 0.0 },
                    intermediate: j.parent.is_some(),
                    waiting_for_room: i == 0 && self.waiting_for_room,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn content() -> Arc<Content> {
        Arc::new(Content::load_default().expect("content loads"))
    }

    fn all(_: RecipeId) -> bool {
        true
    }

    #[test]
    fn craft_reserves_and_cancel_returns() {
        let c = content();
        let gear = c.factory.recipe("bronze_gear").unwrap();
        let plate = c.item("bronze_plate").unwrap();
        let mut inv = Inventory::player();
        inv.insert(&c, plate, 7);
        let mut hc = HandCrafting::new();
        let req = hc.craft(&c, &mut inv, gear, 2, &all).unwrap();
        assert_eq!(inv.count(plate), 1);
        assert_eq!(hc.jobs.len(), 1);
        assert!(hc.cancel(&c, &mut inv, req).is_empty());
        assert_eq!(inv.count(plate), 7);
        assert!(hc.is_idle());
    }

    #[test]
    fn missing_items_and_unknown_recipes_are_refused() {
        let c = content();
        let gear = c.factory.recipe("bronze_gear").unwrap();
        let mut inv = Inventory::player();
        let mut hc = HandCrafting::new();
        match hc.craft(&c, &mut inv, gear, 1, &all) {
            Err(CraftError::Missing { count: 3, name, .. }) => assert_eq!(name, "Bronze plate"),
            other => panic!("{other:?}"),
        }
        inv.insert(&c, c.item("bronze_plate").unwrap(), 3);
        assert_eq!(hc.craft(&c, &mut inv, gear, 1, &|_| false), Err(CraftError::NotKnown));
        let alloy = c.factory.recipe("bronze_alloy").unwrap();
        assert_eq!(hc.craft(&c, &mut inv, alloy, 1, &all), Err(CraftError::NotByHand));
        assert_eq!(hc.craft(&c, &mut inv, gear, 0, &all), Err(CraftError::ZeroCount));
    }

    #[test]
    fn a_craft_takes_the_recipe_time() {
        let c = content();
        let gear = c.factory.recipe("bronze_gear").unwrap(); // 2 s
        let mut inv = Inventory::player();
        inv.insert(&c, c.item("bronze_plate").unwrap(), 3);
        let mut hc = HandCrafting::new();
        hc.craft(&c, &mut inv, gear, 1, &all).unwrap();
        for _ in 0..59 {
            hc.tick(&c, &mut inv, 2.0);
        }
        assert_eq!(inv.count(c.item("bronze_gear").unwrap()), 0);
        hc.tick(&c, &mut inv, 2.0);
        assert_eq!(inv.count(c.item("bronze_gear").unwrap()), 1, "2 s at speed 2 is 60 ticks");
        assert!(hc.is_idle());
    }

    #[test]
    fn a_full_inventory_makes_the_craft_wait() {
        let c = content();
        let gear = c.factory.recipe("bronze_gear").unwrap();
        let plate = c.item("bronze_plate").unwrap();
        let tin = c.item("tin_plate").unwrap();
        let mut inv = Inventory::new(1, 0, 0);
        inv.insert(&c, plate, 3);
        let mut hc = HandCrafting::new();
        hc.craft(&c, &mut inv, gear, 1, &all).unwrap();
        inv.insert(&c, tin, 1);
        for _ in 0..200 {
            hc.tick(&c, &mut inv, 1.0);
        }
        assert!(hc.waiting_for_room);
        inv.remove(tin, 1);
        hc.tick(&c, &mut inv, 1.0);
        assert_eq!(inv.count(c.item("bronze_gear").unwrap()), 1);
    }
}
