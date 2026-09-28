//! Research, labs, Hub milestones, discovery and the guide (game design section 15).
//! Owner: task "progression".
//!
//! Rules:
//! - **Tiers.** Research tier 0 is open from the start. Each Hub repair stage (a milestone) opens
//!   a higher tier. A technology above the open tier cannot start.
//! - **Research.** One technology is the current research. Labs work on it together: each call
//!   of [`Progress::lab_tick`] adds progress. A technology has `units`. Each unit needs one set
//!   of its kits and `unit_time` seconds in a lab at speed 1. A technology with no kits (or no
//!   units) needs no lab: it is done as soon as it starts.
//! - **Queue.** As in Factorio 1.1, the player can queue technologies. Queueing a technology
//!   also queues the technologies it needs. When the current research is done, the first queued
//!   technology that can start, starts.
//! - **Switching.** A technology that stops before it is done keeps its progress.
//! - **Discovery points.** The first scan of a material gives [`POINTS_PER_MATERIAL`] points.
//!   The first time the player sees a reaction gives [`POINTS_PER_REACTION`] points. Guide goals
//!   can give points too. A technology's `discovery_points` are spent when it starts for the
//!   first time.
//! - **Recipes.** A recipe that no technology unlocks is known from the start. Other recipes are
//!   known when their technology is done.
//!
//! The game drains [`ProgressEvent`]s each tick to show notices.

mod guide;
mod kits;
mod tree;

#[cfg(test)]
mod tests;

pub use guide::{Condition, GoalDef, GoalView, Guide, GuideState};
pub use kits::KitBuffer;
pub use tree::{DiscoveryNeed, TechCost, TechPos, TechState, TechView, layout_techs};

use foundry_content::{Content, ItemRef, Matcher, Milestone, Reaction, Stack};
use foundry_core::{MaterialId, RecipeId, TICKS_PER_SECOND, TechId};
use kits::EPS;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Discovery points for the first scan of a material.
pub const POINTS_PER_MATERIAL: u32 = 1;
/// Discovery points for the first time the player sees a reaction.
pub const POINTS_PER_REACTION: u32 = 2;
/// The event list keeps at most this many events. When it is full, the oldest event is dropped.
const MAX_EVENTS: usize = 256;

/// Progress of one technology.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct UnitProgress {
    /// Finished units.
    pub units_done: u32,
    /// Progress of the current unit, 0 to 1.
    pub fraction: f64,
}

/// Something the player found for the first time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Discovered {
    Material(MaterialId),
    /// A reaction key: two material ids (or `tag:<name>`, or `any`) joined by `+`, in name order.
    /// See [`reaction_key`].
    Reaction(String),
}

/// A change the game can show as a notice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProgressEvent {
    /// A technology is done.
    TechDone(TechId),
    /// A Hub repair stage is done. `tier` is the research tier that is open now.
    StageDone { stage: u8, tier: u8 },
    /// A new material or reaction. `points` is the number of discovery points it gave.
    Discovery { found: Discovered, points: u32 },
    /// A guide goal is done (its id).
    GoalDone(String),
}

/// What a lab did in one tick.
#[derive(Debug, Clone, PartialEq)]
pub enum LabStatus {
    /// The lab added progress to the current research.
    Working,
    /// There is no current research.
    NoResearch,
    /// The lab needs these kits for one unit and does not have them. Nothing was used.
    MissingKits(Vec<Stack>),
    /// This tick finished the research of this technology.
    Done(TechId),
}

/// Why a technology cannot start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockReason {
    AlreadyDone,
    /// This technology must be done first.
    NeedsTech(TechId),
    /// The research tier is not open yet. `stage` is the Hub repair stage that opens it.
    NeedsTier { tier: u8, stage: Option<u8> },
    /// A discovery from the technology's `discoveries` list (a material id or `reaction:<a>+<b>`).
    NeedsDiscovery(String),
    /// Not enough discovery points.
    NeedsPoints { need: u32, have: u32 },
}

impl LockReason {
    /// A short sentence for the player.
    pub fn text(&self, content: &Content) -> String {
        match self {
            LockReason::AlreadyDone => "Already researched".to_string(),
            LockReason::NeedsTech(t) => format!("Needs {}", content.factory.tech_def(*t).name),
            LockReason::NeedsTier { tier, stage: Some(stage) } => {
                format!("Needs tier {tier} (repair stage {stage} of the Hub)")
            }
            LockReason::NeedsTier { tier, stage: None } => format!("Needs tier {tier}"),
            LockReason::NeedsDiscovery(key) => format!("Needs discovery: {}", discovery_name(content, key)),
            LockReason::NeedsPoints { need, have } => format!("Needs {need} discovery points (you have {have})"),
        }
    }
}

/// The current research, for the HUD.
#[derive(Debug, Clone, PartialEq)]
pub struct ResearchStatus {
    pub tech: TechId,
    pub name: String,
    pub units_done: u32,
    pub units: u32,
    /// Progress of the current unit, 0 to 1.
    pub unit_fraction: f32,
    /// Progress of the whole technology, 0 to 1.
    pub progress: f32,
    /// Kits for one unit.
    pub kits: Vec<Stack>,
    /// Seconds for one unit in a lab at speed 1.
    pub unit_time: f32,
}

/// The next Hub repair stage, for the Hub window.
#[derive(Debug, Clone, PartialEq)]
pub struct MilestoneView {
    pub stage: u8,
    pub name: String,
    pub description: String,
    pub unlocks_tier: u8,
    pub items: Vec<DeliveryView>,
}

/// One item of a Hub repair stage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeliveryView {
    pub item: ItemRef,
    pub need: u32,
    pub delivered: u32,
}

/// The player's progress. Saved with the game.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    /// Technologies that are done.
    researched: BTreeSet<TechId>,
    /// The technology the labs work on now.
    current: Option<TechId>,
    /// Technologies that started and are not done, with their progress. The discovery points of
    /// these technologies are already spent. The current research is always in this map.
    started: BTreeMap<TechId, UnitProgress>,
    /// Technologies that wait to start, in order. The current research is not in this list.
    queue: Vec<TechId>,
    /// The highest open research tier.
    tier: u8,
    /// The last Hub repair stage that is done (0: none).
    stage: u8,
    /// Items delivered so far for the next stage, in the order of that milestone's `deliver` list.
    delivered: Vec<u32>,
    /// Scanned materials.
    materials: BTreeSet<MaterialId>,
    /// Seen reactions, as keys from [`reaction_key`].
    reactions: BTreeSet<String>,
    /// Discovery points the player can spend.
    points: u32,
    /// The sum of the effects of all done technologies, for example "dig_hardness".
    effects: BTreeMap<String, f32>,
    /// Guide goals that are done (their ids).
    goals_done: BTreeSet<String>,
    /// Notices for the game. Not saved.
    #[serde(skip)]
    events: Vec<ProgressEvent>,
}

impl Progress {
    pub fn new(_content: &Content) -> Self {
        Self::default()
    }

    /// Run one tick: start the next queued research if no research runs, and complete a Hub
    /// stage that has nothing more to receive.
    pub fn tick(&mut self, content: &Content) {
        self.try_complete_stage(content);
        self.advance_queue(content);
    }

    // ----- Research state -----

    pub fn is_researched(&self, tech: TechId) -> bool {
        self.researched.contains(&tech)
    }

    /// Done technologies, in id order.
    pub fn researched(&self) -> impl Iterator<Item = TechId> + '_ {
        self.researched.iter().copied()
    }

    /// The technology the labs work on now.
    pub fn current(&self) -> Option<TechId> {
        self.current
    }

    /// Technologies that wait to start, in order. The current research is not in this list.
    pub fn queue(&self) -> &[TechId] {
        &self.queue
    }

    /// Units done and the fraction of the current unit, for a started technology.
    pub fn unit_progress(&self, tech: TechId) -> Option<UnitProgress> {
        self.started.get(&tech).copied()
    }

    /// Progress of a technology from 0 to 1. Done technologies give 1.
    pub fn tech_progress(&self, content: &Content, tech: TechId) -> f32 {
        if self.researched.contains(&tech) {
            return 1.0;
        }
        let Some(p) = self.started.get(&tech) else { return 0.0 };
        let units = content.factory.tech_def(tech).units;
        if units == 0 {
            return 0.0;
        }
        ((p.units_done as f64 + p.fraction) / units as f64).clamp(0.0, 1.0) as f32
    }

    /// The highest open research tier.
    pub fn unlocked_tier(&self) -> u8 {
        self.tier
    }

    /// The sum of one effect of all done technologies. 0 if no done technology has it.
    pub fn effect(&self, name: &str) -> f32 {
        self.effects.get(name).copied().unwrap_or(0.0)
    }

    /// All effects of done technologies, by name.
    pub fn effects(&self) -> &BTreeMap<String, f32> {
        &self.effects
    }

    /// True if the player can use this recipe: no technology unlocks it, or its technology is done.
    pub fn is_recipe_known(&self, content: &Content, recipe: RecipeId) -> bool {
        content.factory.recipe_def(recipe).unlocked_by.is_none_or(|t| self.researched.contains(&t))
    }

    /// All recipes the player can use, in id order.
    pub fn known_recipes(&self, content: &Content) -> Vec<RecipeId> {
        (0..content.factory.recipes.len())
            .map(|i| RecipeId(i as u16))
            .filter(|r| self.is_recipe_known(content, *r))
            .collect()
    }

    /// Kits for one unit of the current research. Empty if there is no current research.
    /// The lab building can use this to decide which kits to take in.
    pub fn current_kits<'a>(&self, content: &'a Content) -> &'a [Stack] {
        match self.current {
            Some(t) => &content.factory.tech_def(t).kits,
            None => &[],
        }
    }

    /// The current research, for the HUD.
    pub fn research_status(&self, content: &Content) -> Option<ResearchStatus> {
        let tech = self.current?;
        let def = content.factory.tech_def(tech);
        let p = self.started.get(&tech).copied().unwrap_or_default();
        Some(ResearchStatus {
            tech,
            name: def.name.clone(),
            units_done: p.units_done,
            units: def.units,
            unit_fraction: p.fraction as f32,
            progress: self.tech_progress(content, tech),
            kits: def.kits.clone(),
            unit_time: def.unit_time,
        })
    }

    // ----- Research actions -----

    /// Can this technology start now? The error is the first reason it cannot.
    /// The current research gives `Ok`.
    pub fn can_research(&self, content: &Content, tech: TechId) -> Result<(), LockReason> {
        match self.lock_reasons(content, tech).into_iter().next() {
            Some(reason) => Err(reason),
            None => Ok(()),
        }
    }

    /// All reasons this technology cannot start now. Empty if it can start.
    pub fn lock_reasons(&self, content: &Content, tech: TechId) -> Vec<LockReason> {
        let def = content.factory.tech_def(tech);
        let mut out = vec![];
        if self.researched.contains(&tech) {
            out.push(LockReason::AlreadyDone);
            return out;
        }
        if def.tier > self.tier {
            out.push(LockReason::NeedsTier { tier: def.tier, stage: stage_for_tier(content, def.tier) });
        }
        for r in &def.requires {
            if !self.researched.contains(r) {
                out.push(LockReason::NeedsTech(*r));
            }
        }
        for key in &def.discoveries {
            if !self.has_discovery(content, key) {
                out.push(LockReason::NeedsDiscovery(key.clone()));
            }
        }
        if !self.started.contains_key(&tech) && self.points < def.discovery_points {
            out.push(LockReason::NeedsPoints { need: def.discovery_points, have: self.points });
        }
        out
    }

    /// Make this technology the current research now. A research that was running goes back to the
    /// front of the queue and keeps its progress. A technology with no kits is done at once.
    pub fn start_research(&mut self, content: &Content, tech: TechId) -> Result<(), LockReason> {
        self.can_research(content, tech)?;
        if self.current == Some(tech) {
            return Ok(());
        }
        if let Some(old) = self.current.take() {
            self.queue.insert(0, old);
        }
        self.queue.retain(|t| *t != tech);
        self.begin(content, tech);
        self.advance_queue(content);
        Ok(())
    }

    /// Add a technology to the end of the queue. Technologies it needs that are not done and not
    /// queued are added before it. Fails (and changes nothing) if one of them is above the open
    /// tier. If no research runs, the first queued technology that can start, starts.
    pub fn queue_research(&mut self, content: &Content, tech: TechId) -> Result<(), LockReason> {
        if self.researched.contains(&tech) {
            return Err(LockReason::AlreadyDone);
        }
        let mut chain = vec![];
        self.missing_chain(content, tech, &mut chain);
        for t in &chain {
            let tier = content.factory.tech_def(*t).tier;
            if tier > self.tier {
                return Err(LockReason::NeedsTier { tier, stage: stage_for_tier(content, tier) });
            }
        }
        self.queue.extend(chain);
        self.advance_queue(content);
        Ok(())
    }

    /// Stop a technology: the current research or a queued one. Queued technologies that need it
    /// are removed from the queue too. The technology keeps its progress. If the current research
    /// stops, the next queued technology that can start, starts. Returns false if the technology
    /// was not running and not queued.
    pub fn cancel_research(&mut self, content: &Content, tech: TechId) -> bool {
        let was_current = self.current == Some(tech);
        if !was_current && !self.queue.contains(&tech) {
            return false;
        }
        if was_current {
            self.current = None;
        }
        // The queue is in an order where each technology comes after the ones it needs,
        // so one pass finds all technologies that need a removed one.
        let mut removed = BTreeSet::from([tech]);
        self.queue.retain(|t| {
            let drop = removed.contains(t) || content.factory.tech_def(*t).requires.iter().any(|r| removed.contains(r));
            if drop {
                removed.insert(*t);
            }
            !drop
        });
        self.advance_queue(content);
        true
    }

    /// Do one tick of work in one lab. `lab_speed` is the lab's speed (1 = normal). The lab uses
    /// `kits` per unit and adds `lab_speed / (unit_time × 60)` units of progress. Several labs can
    /// work on the same research in the same tick.
    pub fn lab_tick(&mut self, content: &Content, lab_speed: f32, kits: &mut KitBuffer) -> LabStatus {
        let Some(tech) = self.current else { return LabStatus::NoResearch };
        let def = content.factory.tech_def(tech);
        let p = self.started.get(&tech).copied().unwrap_or_default();
        let remaining = (def.units as f64 - p.units_done as f64 - p.fraction).max(0.0);
        let mut step = if def.unit_time > 0.0 {
            lab_speed.max(0.0) as f64 / (def.unit_time as f64 * TICKS_PER_SECOND as f64)
        } else {
            remaining
        };
        step = step.min(remaining);

        let missing: Vec<Stack> = def
            .kits
            .iter()
            .filter(|s| match s.item {
                ItemRef::Part(part) => kits.available(part) + EPS < s.count as f64 * step,
                // Labs hold only parts. A material kit is a data error; the lab can never have it.
                ItemRef::Material(_) => true,
            })
            .copied()
            .collect();
        if !missing.is_empty() {
            return LabStatus::MissingKits(missing);
        }
        for s in &def.kits {
            if let ItemRef::Part(part) = s.item {
                kits.use_kits(part, s.count as f64 * step);
            }
        }

        let entry = self.started.entry(tech).or_default();
        entry.fraction += step;
        while entry.fraction >= 1.0 - EPS && entry.units_done < def.units {
            entry.units_done += 1;
            entry.fraction = (entry.fraction - 1.0).max(0.0);
        }
        if entry.units_done >= def.units {
            self.finish(content, tech);
            self.advance_queue(content);
            return LabStatus::Done(tech);
        }
        LabStatus::Working
    }

    /// Debug: finish a technology and all technologies it needs, with no cost.
    pub fn debug_complete(&mut self, content: &Content, tech: TechId) {
        if self.researched.contains(&tech) {
            return;
        }
        for r in content.factory.tech_def(tech).requires.clone() {
            self.debug_complete(content, r);
        }
        self.finish(content, tech);
        self.advance_queue(content);
    }

    /// Debug: open research tiers up to `tier`.
    pub fn debug_unlock_tier(&mut self, tier: u8) {
        self.tier = self.tier.max(tier);
    }

    /// Start `tech` (it can start): spend its points the first time and make it current.
    /// A technology with no lab work is done at once.
    fn begin(&mut self, content: &Content, tech: TechId) {
        let def = content.factory.tech_def(tech);
        if !self.started.contains_key(&tech) {
            self.points = self.points.saturating_sub(def.discovery_points);
            self.started.insert(tech, UnitProgress::default());
        }
        self.current = Some(tech);
        if def.kits.is_empty() || def.units == 0 {
            self.finish(content, tech);
        }
    }

    /// Mark a technology done.
    fn finish(&mut self, content: &Content, tech: TechId) {
        self.researched.insert(tech);
        self.started.remove(&tech);
        if self.current == Some(tech) {
            self.current = None;
        }
        self.queue.retain(|t| *t != tech);
        // Sum the effects again in id order, so the result does not depend on the finish order.
        self.effects.clear();
        for t in &self.researched {
            for (name, value) in &content.factory.tech_def(*t).effects {
                *self.effects.entry(name.clone()).or_insert(0.0) += *value;
            }
        }
        self.push_event(ProgressEvent::TechDone(tech));
    }

    /// If no research runs, start the first queued technology that can start.
    fn advance_queue(&mut self, content: &Content) {
        while self.current.is_none() {
            let Some(i) = self.queue.iter().position(|t| self.can_research(content, *t).is_ok()) else { break };
            let tech = self.queue.remove(i);
            self.begin(content, tech);
        }
    }

    /// The technologies to queue for `tech`: the ones it needs that are not done, not running and
    /// not queued, each after the ones it needs, then `tech` itself.
    fn missing_chain(&self, content: &Content, tech: TechId, out: &mut Vec<TechId>) {
        if self.researched.contains(&tech) || self.current == Some(tech) || self.queue.contains(&tech) || out.contains(&tech)
        {
            return;
        }
        for r in &content.factory.tech_def(tech).requires {
            self.missing_chain(content, *r, out);
        }
        out.push(tech);
    }

    // ----- Hub milestones -----

    /// The last Hub repair stage that is done (0: none).
    pub fn stage(&self) -> u8 {
        self.stage
    }

    /// The next Hub repair stage. None when all stages are done.
    pub fn next_milestone<'a>(&self, content: &'a Content) -> Option<&'a Milestone> {
        content.factory.milestones.iter().find(|m| m.stage > self.stage)
    }

    /// What the Hub repair stages still need: the rest of the next stage and all of each later
    /// stage, one stack per item. The Hub takes no more than this.
    pub fn hub_need(&self, content: &Content) -> Vec<Stack> {
        let next = self.next_milestone(content).map(|m| m.stage);
        let mut out: Vec<Stack> = vec![];
        for m in content.factory.milestones.iter().filter(|m| m.stage > self.stage) {
            for (i, s) in m.deliver.iter().enumerate() {
                let delivered = if Some(m.stage) == next { self.delivered.get(i).copied().unwrap_or(0) } else { 0 };
                let n = s.count.saturating_sub(delivered);
                match out.iter_mut().find(|x| x.item == s.item) {
                    Some(x) => x.count += n,
                    None => out.push(Stack { item: s.item, count: n }),
                }
            }
        }
        out
    }

    /// The Hub repair stages after the next one, with nothing delivered yet (for the Hub window).
    pub fn later_milestone_views(&self, content: &Content) -> Vec<MilestoneView> {
        let next = self.next_milestone(content).map_or(u8::MAX, |m| m.stage);
        content
            .factory
            .milestones
            .iter()
            .filter(|m| m.stage > next)
            .map(|m| MilestoneView {
                stage: m.stage,
                name: m.name.clone(),
                description: m.description.clone(),
                unlocks_tier: m.unlocks_tier,
                items: m.deliver.iter().map(|s| DeliveryView { item: s.item, need: s.count, delivered: 0 }).collect(),
            })
            .collect()
    }

    /// How many more of this item the next Hub stage takes.
    pub fn hub_wants(&self, content: &Content, item: ItemRef) -> u32 {
        let Some(m) = self.next_milestone(content) else { return 0 };
        m.deliver
            .iter()
            .enumerate()
            .filter(|(_, s)| s.item == item)
            .map(|(i, s)| s.count.saturating_sub(self.delivered.get(i).copied().unwrap_or(0)))
            .sum()
    }

    /// Give items to the Hub. It takes what the next stage still needs and returns how many it
    /// took. When the stage has everything, it is done: the research tier it opens is open, and
    /// the Hub then takes items for the stage after it.
    pub fn deliver(&mut self, content: &Content, stack: Stack) -> u32 {
        let Some(m) = self.next_milestone(content) else { return 0 };
        self.delivered.resize(m.deliver.len(), 0);
        let mut left = stack.count;
        for (i, need) in m.deliver.iter().enumerate() {
            if need.item == stack.item {
                let n = need.count.saturating_sub(self.delivered[i]).min(left);
                self.delivered[i] += n;
                left -= n;
            }
        }
        let taken = stack.count - left;
        if taken > 0 {
            self.try_complete_stage(content);
        }
        taken
    }

    /// The next Hub stage with the amounts delivered so far. None when all stages are done.
    pub fn milestone_view(&self, content: &Content) -> Option<MilestoneView> {
        let m = self.next_milestone(content)?;
        Some(MilestoneView {
            stage: m.stage,
            name: m.name.clone(),
            description: m.description.clone(),
            unlocks_tier: m.unlocks_tier,
            items: m
                .deliver
                .iter()
                .enumerate()
                .map(|(i, s)| DeliveryView {
                    item: s.item,
                    need: s.count,
                    delivered: self.delivered.get(i).copied().unwrap_or(0).min(s.count),
                })
                .collect(),
        })
    }

    /// Complete the next stage (and the ones after it) while it has everything it needs.
    fn try_complete_stage(&mut self, content: &Content) {
        while let Some(m) = self.next_milestone(content) {
            let complete =
                m.deliver.iter().enumerate().all(|(i, s)| self.delivered.get(i).copied().unwrap_or(0) >= s.count);
            if !complete {
                break;
            }
            self.stage = m.stage;
            self.tier = self.tier.max(m.unlocks_tier);
            self.delivered.clear();
            self.push_event(ProgressEvent::StageDone { stage: m.stage, tier: self.tier });
        }
    }

    // ----- Discovery -----

    /// Discovery points the player can spend.
    pub fn discovery_points(&self) -> u32 {
        self.points
    }

    /// Give discovery points (guide rewards, debug).
    pub fn add_discovery_points(&mut self, points: u32) {
        self.points = self.points.saturating_add(points);
    }

    /// The player scanned a material. True the first time; that gives discovery points.
    /// Air gives nothing.
    pub fn discover_material(&mut self, material: MaterialId) -> bool {
        if material.is_air() || !self.materials.insert(material) {
            return false;
        }
        self.add_discovery_points(POINTS_PER_MATERIAL);
        self.push_event(ProgressEvent::Discovery { found: Discovered::Material(material), points: POINTS_PER_MATERIAL });
        true
    }

    /// The player saw a reaction. `key` is `<a>+<b>` or `reaction:<a>+<b>` (the order of a and b
    /// does not matter; see [`reaction_key`]). True the first time; that gives discovery points.
    pub fn discover_reaction(&mut self, key: &str) -> bool {
        let key = normalize_reaction_key(key);
        if key.is_empty() || self.reactions.contains(&key) {
            return false;
        }
        self.reactions.insert(key.clone());
        self.add_discovery_points(POINTS_PER_REACTION);
        self.push_event(ProgressEvent::Discovery { found: Discovered::Reaction(key), points: POINTS_PER_REACTION });
        true
    }

    pub fn is_material_discovered(&self, material: MaterialId) -> bool {
        self.materials.contains(&material)
    }

    pub fn is_reaction_discovered(&self, key: &str) -> bool {
        self.reactions.contains(&normalize_reaction_key(key))
    }

    /// Scanned materials, in id order.
    pub fn discovered_materials(&self) -> impl Iterator<Item = MaterialId> + '_ {
        self.materials.iter().copied()
    }

    /// Seen reactions (keys from [`reaction_key`]), in name order.
    pub fn discovered_reactions(&self) -> impl Iterator<Item = &str> + '_ {
        self.reactions.iter().map(|s| s.as_str())
    }

    /// True if a discovery from a technology's `discoveries` list is done.
    pub fn has_discovery(&self, content: &Content, key: &str) -> bool {
        match key.strip_prefix("reaction:") {
            Some(reaction) => self.reactions.contains(&normalize_reaction_key(reaction)),
            None => content.material(key).is_some_and(|m| self.materials.contains(&m)),
        }
    }

    // ----- Events -----

    /// Take the events since the last call, oldest first.
    pub fn drain_events(&mut self) -> impl Iterator<Item = ProgressEvent> + '_ {
        self.events.drain(..)
    }

    /// Events since the last `drain_events`, oldest first.
    pub fn events(&self) -> &[ProgressEvent] {
        &self.events
    }

    fn push_event(&mut self, event: ProgressEvent) {
        if self.events.len() >= MAX_EVENTS {
            self.events.remove(0);
        }
        self.events.push(event);
    }
}

/// The Hub repair stage that opens a research tier. None if no stage opens it.
pub fn stage_for_tier(content: &Content, tier: u8) -> Option<u8> {
    content.factory.milestones.iter().find(|m| m.unlocks_tier >= tier).map(|m| m.stage)
}

/// The key of a reaction from the data, for [`Progress::discover_reaction`].
/// It is the two inputs joined by `+`, in name order: a material id, `tag:<name>` or `any`.
/// Example: `lava+water`.
pub fn reaction_key(content: &Content, reaction: &Reaction) -> String {
    let side = |m: Matcher| match m {
        Matcher::Material(id) => content.materials.ids[id.index()].clone(),
        Matcher::Tag(bit) => format!("tag:{}", content.tags.names.get(bit as usize).map_or("?", |s| s.as_str())),
        Matcher::Any => "any".to_string(),
    };
    normalize_reaction_key(&format!("{}+{}", side(reaction.a), side(reaction.b)))
}

/// Remove a `reaction:` prefix and put the two sides in name order.
pub fn normalize_reaction_key(key: &str) -> String {
    let key = key.trim();
    let key = key.strip_prefix("reaction:").unwrap_or(key);
    match key.split_once('+') {
        Some((a, b)) => {
            let (a, b) = (a.trim(), b.trim());
            if a <= b { format!("{a}+{b}") } else { format!("{b}+{a}") }
        }
        None => key.to_string(),
    }
}

/// A name for the player for a discovery key: a material name, or "reaction A + B".
pub fn discovery_name(content: &Content, key: &str) -> String {
    let side = |s: &str| -> String {
        if s == "any" {
            "anything".to_string()
        } else if let Some(tag) = s.strip_prefix("tag:") {
            format!("any {tag} material")
        } else {
            content.material(s).map_or_else(|| s.to_string(), |m| content.materials.names[m.index()].clone())
        }
    };
    match key.strip_prefix("reaction:") {
        Some(reaction) => match reaction.split_once('+') {
            Some((a, b)) => format!("reaction {} + {}", side(a.trim()), side(b.trim())),
            None => format!("reaction {reaction}"),
        },
        None => side(key),
    }
}
