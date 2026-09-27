//! The guide: goals for each tier with short hints (game design section 15.4). It is also the
//! tutorial. The goals are in `assets/data/guide/*.ron`. Each file is a list of `Goal(...)`.
//! Files are read in name order, and goals show in file order.
//!
//! A goal checks a [`Condition`]. Goals of a tier show when that research tier is open. A goal
//! stays done after its condition is met once. The game gives the player's items and buildings
//! through the [`GuideState`] trait, so this module does not depend on the inventory code.

use super::{Progress, ProgressEvent};
use foundry_content::{Content, ContentError, ItemRef, default_assets_dir};
use foundry_core::BuildingKindId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// What a goal checks. Names are string ids from the data files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Condition {
    /// The player has at least this many of an item: units of a material, or parts.
    HaveItem(String, u32),
    /// A technology is done.
    Research(String),
    /// At least this many buildings of a type are placed.
    Build(String, u32),
    /// A material is scanned.
    Discover(String),
    /// This Hub repair stage is done.
    Stage(u8),
    /// All of these conditions.
    All(Vec<Condition>),
    /// At least one of these conditions.
    Any(Vec<Condition>),
}

/// One goal. RON name `Goal(...)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename = "Goal", deny_unknown_fields)]
pub struct GoalDef {
    pub id: String,
    /// The goal shows when this research tier is open.
    pub tier: u8,
    pub title: String,
    /// A short hint: what to do and how.
    pub text: String,
    pub condition: Condition,
    /// Discovery points the player gets when the goal is done.
    #[serde(default)]
    pub reward_points: u32,
}

/// What the guide needs to know from the game. The game implements it.
pub trait GuideState {
    /// How many of this item the player has (inventory and material tank).
    fn item_count(&self, item: ItemRef) -> u32;
    /// How many buildings of this type are placed in the world.
    fn building_count(&self, kind: BuildingKindId) -> u32;
}

/// One goal, for the guide screen.
#[derive(Debug, Clone, PartialEq)]
pub struct GoalView {
    pub id: String,
    pub tier: u8,
    pub title: String,
    pub text: String,
    pub done: bool,
    /// (have, need) for a goal that counts items or buildings.
    pub count: Option<(u32, u32)>,
    pub reward_points: u32,
}

/// All guide goals.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Guide {
    pub goals: Vec<GoalDef>,
}

impl Guide {
    /// Read `<assets>/data/guide/*.ron`. A missing folder gives an empty guide.
    pub fn load(assets_dir: &Path) -> Result<Guide, ContentError> {
        let dir = assets_dir.join("data").join("guide");
        let mut goals = vec![];
        if dir.is_dir() {
            let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
                .map_err(|source| ContentError::Io { path: dir.clone(), source })?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "ron"))
                .collect();
            paths.sort();
            for path in paths {
                let text =
                    std::fs::read_to_string(&path).map_err(|source| ContentError::Io { path: path.clone(), source })?;
                let list: Vec<GoalDef> =
                    ron::from_str(&text).map_err(|e| ContentError::Parse { path: path.clone(), message: e.to_string() })?;
                goals.extend(list);
            }
        }
        Self::from_goals(goals)
    }

    /// Read the guide from the default assets folder (see `foundry_content::default_assets_dir`).
    pub fn load_default() -> Result<Guide, ContentError> {
        Self::load(&default_assets_dir())
    }

    /// Read a guide from RON text: a list of `Goal(...)`. For tests.
    pub fn from_ron(text: &str) -> Result<Guide, ContentError> {
        let goals: Vec<GoalDef> =
            ron::from_str(text).map_err(|e| ContentError::Parse { path: "<guide>".into(), message: e.to_string() })?;
        Self::from_goals(goals)
    }

    fn from_goals(goals: Vec<GoalDef>) -> Result<Guide, ContentError> {
        let mut errors = vec![];
        let mut ids = BTreeSet::new();
        for g in &goals {
            if g.id.is_empty() || !g.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
                errors.push(format!("guide goal id `{}`: use only a-z, 0-9 and _", g.id));
            }
            if !ids.insert(g.id.as_str()) {
                errors.push(format!("guide goal id `{}` is used twice", g.id));
            }
        }
        if errors.is_empty() { Ok(Guide { goals }) } else { Err(ContentError::Invalid(errors)) }
    }

    /// Names in goal conditions that the content does not have. A condition with an unknown name
    /// is never met. The guide can name items that the data files do not have yet, so this is a
    /// list of warnings, not a load error.
    pub fn check(&self, content: &Content) -> Vec<String> {
        let mut out = vec![];
        for g in &self.goals {
            check_condition(content, &g.condition, &g.id, &mut out);
        }
        out
    }
}

fn check_condition(content: &Content, c: &Condition, goal: &str, out: &mut Vec<String>) {
    let f = &content.factory;
    match c {
        Condition::HaveItem(id, _) if content.item(id).is_none() => {
            out.push(format!("guide goal `{goal}`: `{id}` is not a material or a part"));
        }
        Condition::Research(id) if f.tech(id).is_none() => {
            out.push(format!("guide goal `{goal}`: `{id}` is not a technology"));
        }
        Condition::Build(id, _) if f.building(id).is_none() => {
            out.push(format!("guide goal `{goal}`: `{id}` is not a building"));
        }
        Condition::Discover(id) if content.material(id).is_none() => {
            out.push(format!("guide goal `{goal}`: `{id}` is not a material"));
        }
        Condition::Stage(n) if !f.milestones.iter().any(|m| m.stage == *n) => {
            out.push(format!("guide goal `{goal}`: there is no Hub stage {n}"));
        }
        Condition::All(list) | Condition::Any(list) => {
            for c in list {
                check_condition(content, c, goal, out);
            }
        }
        _ => {}
    }
}

impl Progress {
    pub fn is_goal_done(&self, id: &str) -> bool {
        self.goals_done.contains(id)
    }

    /// Is a condition met now?
    pub fn condition_met(&self, content: &Content, state: &dyn GuideState, condition: &Condition) -> bool {
        let f = &content.factory;
        match condition {
            Condition::HaveItem(id, n) => content.item(id).is_some_and(|item| state.item_count(item) >= *n),
            Condition::Research(id) => f.tech(id).is_some_and(|t| self.is_researched(t)),
            Condition::Build(id, n) => f.building(id).is_some_and(|b| state.building_count(b) >= *n),
            Condition::Discover(id) => content.material(id).is_some_and(|m| self.is_material_discovered(m)),
            Condition::Stage(n) => self.stage() >= *n,
            Condition::All(list) => list.iter().all(|c| self.condition_met(content, state, c)),
            Condition::Any(list) => list.iter().any(|c| self.condition_met(content, state, c)),
        }
    }

    /// Check the shown goals that are not done yet. A goal whose condition is met is done: it
    /// gives its reward points and a `GoalDone` event. The game calls this about once a second.
    pub fn update_guide(&mut self, guide: &Guide, content: &Content, state: &dyn GuideState) {
        for g in &guide.goals {
            if g.tier > self.tier || self.goals_done.contains(&g.id) {
                continue;
            }
            if self.condition_met(content, state, &g.condition) {
                self.goals_done.insert(g.id.clone());
                self.add_discovery_points(g.reward_points);
                self.push_event(ProgressEvent::GoalDone(g.id.clone()));
            }
        }
    }

    /// The goals of all open tiers, in guide order.
    pub fn guide_view(&self, guide: &Guide, content: &Content, state: &dyn GuideState) -> Vec<GoalView> {
        guide
            .goals
            .iter()
            .filter(|g| g.tier <= self.tier)
            .map(|g| {
                let done = self.goals_done.contains(&g.id);
                let count = match &g.condition {
                    Condition::HaveItem(id, n) => {
                        let have = content.item(id).map_or(0, |item| state.item_count(item));
                        Some((if done { *n } else { have.min(*n) }, *n))
                    }
                    Condition::Build(id, n) => {
                        let have = content.factory.building(id).map_or(0, |b| state.building_count(b));
                        Some((if done { *n } else { have.min(*n) }, *n))
                    }
                    _ => None,
                };
                GoalView {
                    id: g.id.clone(),
                    tier: g.tier,
                    title: g.title.clone(),
                    text: g.text.clone(),
                    done,
                    count,
                    reward_points: g.reward_points,
                }
            })
            .collect()
    }
}
