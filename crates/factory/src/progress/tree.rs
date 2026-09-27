//! The tech tree for the UI: one view per technology, and a graph layout.

use super::{LockReason, Progress, discovery_name};
use foundry_content::{Content, FactoryContent, Stack, Tech};
use foundry_core::{RecipeId, TechId};

/// The state of a technology in the tech tree.
#[derive(Debug, Clone, PartialEq)]
pub enum TechState {
    Done,
    /// The current research. `percent` is 0 to 100.
    Researching { percent: f32 },
    /// It can start now.
    Available,
    /// It cannot start now, for these reasons.
    Locked(Vec<LockReason>),
}

/// One discovery that a technology needs.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveryNeed {
    /// As in the data: a material id or `reaction:<a>+<b>`.
    pub key: String,
    /// A name for the player.
    pub name: String,
    pub done: bool,
}

/// What a technology costs.
#[derive(Debug, Clone, PartialEq)]
pub struct TechCost {
    /// Kits for one unit.
    pub kits: Vec<Stack>,
    pub units: u32,
    /// Seconds for one unit in a lab at speed 1.
    pub unit_time: f32,
    /// Discovery points it spends when it starts.
    pub points: u32,
    pub discoveries: Vec<DiscoveryNeed>,
}

/// The place of a technology in the tech tree graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TechPos {
    /// Depth in the `requires` graph. Lines go from left to right.
    pub column: u16,
    /// Order inside the column, from the top.
    pub row: u16,
}

/// One technology, for the tech tree screen.
#[derive(Debug, Clone, PartialEq)]
pub struct TechView {
    pub id: TechId,
    pub name: String,
    pub description: String,
    pub tier: u8,
    pub state: TechState,
    /// The lock reasons as sentences for the player. Empty unless the state is `Locked`.
    pub reasons: Vec<String>,
    /// 0 to 1. It is above 0 for a technology that started and then stopped.
    pub progress: f32,
    /// Place in the research queue (0 = next). None if it is not queued.
    pub queue_position: Option<usize>,
    pub cost: TechCost,
    /// Recipes it unlocks.
    pub unlocks: Vec<RecipeId>,
    /// Effects, for example ("belt_speed", 0.1).
    pub effects: Vec<(String, f32)>,
    /// Technologies it needs. Draw a line from each of them to this one.
    pub requires: Vec<TechId>,
    pub pos: TechPos,
}

impl Progress {
    /// All technologies, in id order, with their graph positions.
    pub fn tech_views(&self, content: &Content) -> Vec<TechView> {
        let layout = layout_techs(&content.factory);
        (0..content.factory.techs.len()).map(|i| self.tech_view(content, TechId(i as u16), layout[i])).collect()
    }

    /// One technology. `pos` comes from [`layout_techs`].
    pub fn tech_view(&self, content: &Content, tech: TechId, pos: TechPos) -> TechView {
        let def = content.factory.tech_def(tech);
        let progress = self.tech_progress(content, tech);
        let state = if self.is_researched(tech) {
            TechState::Done
        } else if self.current() == Some(tech) {
            TechState::Researching { percent: progress * 100.0 }
        } else {
            let reasons = self.lock_reasons(content, tech);
            if reasons.is_empty() { TechState::Available } else { TechState::Locked(reasons) }
        };
        let reasons = match &state {
            TechState::Locked(list) => list.iter().map(|r| r.text(content)).collect(),
            _ => vec![],
        };
        TechView {
            id: tech,
            name: def.name.clone(),
            description: def.description.clone(),
            tier: def.tier,
            state,
            reasons,
            progress,
            queue_position: self.queue().iter().position(|t| *t == tech),
            cost: TechCost {
                kits: def.kits.clone(),
                units: def.units,
                unit_time: def.unit_time,
                points: def.discovery_points,
                discoveries: def
                    .discoveries
                    .iter()
                    .map(|key| DiscoveryNeed {
                        key: key.clone(),
                        name: discovery_name(content, key),
                        done: self.has_discovery(content, key),
                    })
                    .collect(),
            },
            unlocks: def.unlocks.clone(),
            effects: def.effects.clone(),
            requires: def.requires.clone(),
            pos,
        }
    }
}

/// Graph positions of all technologies. `result[i]` is the position of `TechId(i)`.
///
/// The column is the depth in the `requires` graph: a technology that needs nothing is in
/// column 0, and every other technology is one column to the right of its deepest requirement.
/// The row is the order inside the column. A few barycenter passes move each technology near the
/// average row of the technologies it is connected to, so fewer lines cross. The order with the
/// fewest crossings is kept.
pub fn layout_techs(factory: &FactoryContent) -> Vec<TechPos> {
    let techs = &factory.techs;
    let n = techs.len();
    if n == 0 {
        return vec![];
    }

    let mut depth = vec![None; n];
    let mut visiting = vec![false; n];
    for i in 0..n {
        depth_of(i, techs, &mut depth, &mut visiting);
    }
    let depth: Vec<usize> = depth.into_iter().map(|d| d.unwrap_or(0) as usize).collect();

    let mut dependents = vec![vec![]; n];
    for (i, t) in techs.iter().enumerate() {
        for r in &t.requires {
            dependents[r.0 as usize].push(i);
        }
    }

    // Start order: tier, then data order.
    let columns_count = depth.iter().max().copied().unwrap_or(0) + 1;
    let mut columns: Vec<Vec<usize>> = vec![vec![]; columns_count];
    for i in 0..n {
        columns[depth[i]].push(i);
    }
    for col in &mut columns {
        col.sort_by_key(|&i| (techs[i].tier, i));
    }

    let mut row = vec![0.0f32; n];
    for col in &columns {
        for (r, &i) in col.iter().enumerate() {
            row[i] = r as f32;
        }
    }

    let mut best = columns.clone();
    let mut best_crossings = crossings(techs, &depth, &row);
    for _ in 0..4 {
        // Left to right (column 1 to the last): sort by the average row of the requirements.
        for col in columns.iter_mut().skip(1) {
            sort_by_average(col, &mut row, |i| techs[i].requires.iter().map(|r| r.0 as usize).collect());
        }
        // Right to left (the column before the last to column 0): sort by the average row of the
        // dependents.
        for col in columns.iter_mut().rev().skip(1) {
            sort_by_average(col, &mut row, |i| dependents[i].clone());
        }
        let count = crossings(techs, &depth, &row);
        if count < best_crossings {
            best_crossings = count;
            best = columns.clone();
        }
    }

    let mut out = vec![TechPos::default(); n];
    for (c, col) in best.iter().enumerate() {
        for (r, &i) in col.iter().enumerate() {
            out[i] = TechPos { column: c as u16, row: r as u16 };
        }
    }
    out
}

/// Depth of a technology in the `requires` graph. A loop (a data error that the loader reports)
/// counts as depth 0.
fn depth_of(i: usize, techs: &[Tech], depth: &mut [Option<u16>], visiting: &mut [bool]) -> u16 {
    if let Some(d) = depth[i] {
        return d;
    }
    if visiting[i] {
        return 0;
    }
    visiting[i] = true;
    let d = techs[i].requires.iter().map(|r| depth_of(r.0 as usize, techs, depth, visiting) + 1).max().unwrap_or(0);
    visiting[i] = false;
    depth[i] = Some(d);
    d
}

/// Sort one column by the average row of each technology's neighbors. A technology with no
/// neighbors keeps its row. Equal values keep their order. Then write the new rows.
fn sort_by_average(column: &mut [usize], row: &mut [f32], neighbors: impl Fn(usize) -> Vec<usize>) {
    let mut keyed: Vec<(f32, usize)> = column
        .iter()
        .map(|&i| {
            let list = neighbors(i);
            let key = if list.is_empty() { row[i] } else { list.iter().map(|&j| row[j]).sum::<f32>() / list.len() as f32 };
            (key, i)
        })
        .collect();
    keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (r, (_, i)) in keyed.into_iter().enumerate() {
        column[r] = i;
        row[i] = r as f32;
    }
}

/// Count pairs of lines that cross. Only lines between the same two columns are compared.
fn crossings(techs: &[Tech], depth: &[usize], row: &[f32]) -> usize {
    let mut lines = vec![];
    for (i, t) in techs.iter().enumerate() {
        for r in &t.requires {
            let r = r.0 as usize;
            lines.push((depth[r], depth[i], row[r], row[i]));
        }
    }
    let mut count = 0;
    for (a, la) in lines.iter().enumerate() {
        for lb in &lines[a + 1..] {
            if la.0 == lb.0 && la.1 == lb.1 && (la.2 - lb.2) * (la.3 - lb.3) < 0.0 {
                count += 1;
            }
        }
    }
    count
}
