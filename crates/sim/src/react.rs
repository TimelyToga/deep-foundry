//! Reactions between touching cells, burning, charring and timers (technical design section 6.4).
//!
//! `try_react` runs for each updated cell that is not air, before movement.
//!
//! - **Pair rules** (`assets/data/reactions/*.ron`). The cell picks one random neighbor out of 8.
//!   The pair (cell material, neighbor material) gives a short list of rules in a dense table.
//!   Tags and `"any"` are expanded when the table is built. A rule "A + B" is also in the list of
//!   the pair (B, A), so either cell can start it. The first rule whose conditions hold and whose
//!   chance roll passes fires.
//! - **Burning** (the `burn` data of a material, see `burn.rs`).
//! - **Timers** and **charring** (see `burn.rs`).
//!
//! Sleeping: a reaction that is possible but did not happen (chance) keeps the cell awake. The
//! random neighbor can be one without a rule, so for each pair of materials with rules, one of the
//! two materials also looks at all 8 neighbors (`W_LOOK_ALL`). If any neighbor can react now, the
//! cell stays awake. So a slow reaction never stops because the chunk went to sleep. For the
//! other material of the pair the check costs only one random neighbor. Materials that fade
//! (fire, smoke) are always awake, so the other material does not need to look. A material whose
//! partners all do this (water) does not check its pairs at all; the partner starts the reaction.
//! So the `chance` of a rule is per tick for the cell that checks.
//! A rule with a temperature condition that does not hold lets the cell sleep: the heat pass
//! must wake the cell when its temperature changes (see `docs/design/requests/reactions.md`).
//!
//! The life byte of a burnable material (which cannot have a `life` range): bit 7 is set while it
//! burns; bits 0 to 6 count the steps of charring. The life byte of a timer material counts the
//! timer steps. The life byte moves with the cell, so a burning liquid keeps burning when it flows.
//!
//! Events: rules can send explosions. Each rule that fires sends `SimEvent::Reaction` for the
//! factory (discovery), at most once per reaction per job; `dedupe_reaction_events` keeps only
//! the first one per tick after the passes.

mod burn;
#[cfg(test)]
mod tests;

use crate::SimEvent;
use crate::hood::Hood;
use foundry_content::{Content, Matcher, MaterialTable, OwnChange, Phase};
use foundry_core::{CellPos, DEFAULT_TEMPERATURE, MaterialId};

/// The 8 neighbors: (dx, dy).
const DIRS: [(i32, i32); 8] = [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)];

/// Strength and heat of the explosion events that reactions send (`event` in the data).
/// The strength is compared with material hardness (technical design section 6.7).
pub const EXPLOSION_SMALL: (f32, i16) = (12.0, 400);
pub const EXPLOSION_MEDIUM: (f32, i16) = (30.0, 600);
pub const EXPLOSION_LARGE: (f32, i16) = (60.0, 900);

/// "No change" in `Rule::into_a` and `Rule::into_b`.
const KEEP: u16 = u16::MAX;

// Bits of `ReactTable::work`: what a material can do.
/// It has pair rules.
const W_PAIRS: u8 = 1 << 0;
/// It looks at all 8 neighbors when the random neighbor has no rule (see the module docs).
const W_LOOK_ALL: u8 = 1 << 1;
/// It has burn data.
const W_BURN: u8 = 1 << 2;
/// Its phase is Fire: it ignites burnable neighbors and dies in water.
const W_FIRE: u8 = 1 << 3;
/// It has a timer.
const W_TIMER: u8 = 1 << 4;

/// One rule for one pair of materials, with tags and `$` words already resolved.
#[derive(Debug, Clone, Copy)]
struct Rule {
    /// Index in `Content::reactions`.
    reaction: u16,
    /// The updated cell is input B of the reaction, and the neighbor is input A.
    swapped: bool,
    needs_air: bool,
    /// 0: none. 1, 2, 3: small, medium, large explosion.
    event: u8,
    chance: f32,
    min_temp: i16,
    max_temp: i16,
    heat: i16,
    /// Material ids, or `KEEP`.
    into_a: u16,
    into_b: u16,
    /// Chance of the second outcome (0: none), and its results.
    alt_chance: f32,
    alt_a: u16,
    alt_b: u16,
    /// Extra results: `extras[extra_start..extra_start + extra_len]`.
    extra_start: u32,
    extra_len: u8,
}

/// A timer or charring after the table is built: it counts `steps` steps, one step per tick with
/// chance `chance`, so it takes about `steps / chance` ticks.
#[derive(Debug, Clone, Copy, Default)]
struct Steps {
    steps: u8,
    chance: f32,
}

impl Steps {
    fn new(ticks: u32) -> Steps {
        let steps = ticks.clamp(1, burn::MAX_STEPS as u32);
        Steps { steps: steps as u8, chance: steps as f32 / ticks.max(1) as f32 }
    }
}

/// `W_*` bits for every possible material id (65536 entries), so a lookup needs no range check.
/// Ids above the last material have no work.
struct WorkTable(Box<[u8; 1 << 16]>);

impl WorkTable {
    fn new(work: &[u8]) -> WorkTable {
        let mut all = vec![0u8; 1 << 16];
        all[..work.len()].copy_from_slice(work);
        WorkTable(all.into_boxed_slice().try_into().expect("65536 entries"))
    }

    #[inline(always)]
    fn get(&self, m: MaterialId) -> u8 {
        self.0[m.0 as usize]
    }
}

impl Default for WorkTable {
    fn default() -> Self {
        WorkTable::new(&[])
    }
}

impl std::fmt::Debug for WorkTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WorkTable({} materials with work)", self.0.iter().filter(|&&w| w != 0).count())
    }
}

/// Lookup tables for reactions, built once from the content.
#[derive(Debug, Default)]
pub struct ReactTable {
    /// Number of materials.
    n: usize,
    /// Index `a * n + b`: `(first rule << 8) | number of rules` for a cell of material `a` next to
    /// a cell of material `b`. 0: no rules.
    pairs: Vec<u32>,
    rules: Vec<Rule>,
    extras: Vec<(MaterialId, f32)>,
    /// `W_*` bits for each material. 0: nothing to do (the fast path).
    work: WorkTable,
    /// Air, oxygen and fire: they count as "air" for burning and for `needs_air`.
    oxidizer: Vec<bool>,
    /// Puts out burning cells and fire: cool liquids that do not burn (water) and the tag `fire_out`.
    quench: Vec<bool>,
    /// Timer steps of each material.
    timer: Vec<Steps>,
    /// Charring steps of each burnable material.
    charring: Vec<Steps>,
    /// The lowest temperature at which something can start for a cell of this material (burning,
    /// charring or a rule with a lower temperature limit). `i16::MAX`: nothing depends on heat.
    wake_temp: Vec<i16>,
    /// Number of reactions in the content.
    reactions: usize,
}

impl ReactTable {
    pub fn new(content: &Content) -> Self {
        let mats = &content.materials;
        let n = mats.len();
        let mut entries: Vec<(usize, Rule)> = Vec::new();
        let mut extras = Vec::new();
        let mut wake_temp = vec![i16::MAX; n];
        let all: Vec<MaterialId> = mats.all().collect();
        let matching = |m: Matcher| -> Vec<MaterialId> { all.iter().copied().filter(|&x| m.matches(x, mats)).collect() };
        for (ri, r) in content.reactions.iter().enumerate() {
            let extra_start = extras.len() as u32;
            extras.extend(r.extra.iter().copied());
            let event = match r.event.as_deref() {
                Some("explosion_small") => 1,
                Some("explosion_medium") => 2,
                Some("explosion_large") => 3,
                _ => 0,
            };
            let alt = r.alt.unwrap_or(foundry_content::Alt { chance: 0.0, into_a: None, into_b: None });
            let (a_any, b_any) = (r.a == Matcher::Any, r.b == Matcher::Any);
            for a in matching(r.a) {
                let Some(into_a) = output(r.into_a, r.into_a_own, a, mats) else { continue };
                if r.min_temp > i16::MIN {
                    wake_temp[a.index()] = wake_temp[a.index()].min(r.min_temp);
                }
                for b in matching(r.b) {
                    let Some(into_b) = output(r.into_b, r.into_b_own, b, mats) else { continue };
                    let rule = Rule {
                        reaction: ri as u16,
                        swapped: false,
                        needs_air: r.needs_air,
                        event,
                        chance: r.chance,
                        min_temp: r.min_temp,
                        max_temp: r.max_temp,
                        heat: r.heat,
                        into_a,
                        into_b,
                        alt_chance: alt.chance,
                        alt_a: alt.into_a.map_or(KEEP, |m| m.0),
                        alt_b: alt.into_b.map_or(KEEP, |m| m.0),
                        extra_start,
                        extra_len: r.extra.len().min(255) as u8,
                    };
                    // "any" means "this material next to anything": only the other side looks.
                    if !a.is_air() && !a_any {
                        entries.push((a.index() * n + b.index(), rule));
                    }
                    if a != b && !b.is_air() && !b_any {
                        entries.push((b.index() * n + a.index(), Rule { swapped: true, ..rule }));
                    }
                }
            }
        }
        // Stable sort: the rules of one pair stay in reaction order.
        entries.sort_by_key(|e| e.0);
        let mut pairs = vec![0u32; n * n];
        let mut rules = Vec::with_capacity(entries.len());
        let mut i = 0;
        while i < entries.len() {
            let p = entries[i].0;
            let start = rules.len();
            while i < entries.len() && entries[i].0 == p {
                if rules.len() - start < 255 {
                    rules.push(entries[i].1);
                }
                i += 1;
            }
            pairs[p] = ((start as u32) << 8) | (rules.len() - start) as u32;
        }

        let molten = content.tags.bit("molten");
        let fire_out = content.tags.bit("fire_out");
        let oxygen = content.material("oxygen");
        let mut work = vec![0u8; n];
        let mut oxidizer = vec![false; n];
        let mut quench = vec![false; n];
        let mut timer = vec![Steps::default(); n];
        let mut charring = vec![Steps::default(); n];
        let partners: Vec<usize> = (0..n).map(|a| (0..n).filter(|&b| pairs[a * n + b] != 0).count()).collect();
        for m in mats.all() {
            let i = m.index();
            let phase = mats.phase[i];
            oxidizer[i] = m.is_air() || Some(m) == oxygen || phase == Phase::Fire;
            quench[i] = fire_out.is_some_and(|b| mats.has_tag(m, b))
                || (phase == Phase::Liquid
                    && mats.burn[i].is_none()
                    && mats.temperature[i] < 100
                    && !molten.is_some_and(|b| mats.has_tag(m, b)));
            if partners[i] > 0 {
                work[i] |= W_PAIRS;
            }
            if let Some(b) = mats.burn[i] {
                work[i] |= W_BURN;
                wake_temp[i] = wake_temp[i].min(b.ignite_at);
                if b.char_into.is_some() {
                    charring[i] = Steps::new(b.char_ticks);
                }
            }
            if phase == Phase::Fire {
                work[i] |= W_FIRE;
            }
            if let Some(t) = mats.timer[i] {
                work[i] |= W_TIMER;
                timer[i] = Steps::new(t.ticks);
            }
        }
        // For each pair with rules, one material that is not always awake looks at all its
        // neighbors (`W_LOOK_ALL`), so the pair is never left asleep. It is the only one that can
        // start the rules (the other is air, or the rule has "any"), or the one with fewer
        // partner materials (so common materials such as water do not need to look).
        for a in 1..n {
            if mats.life[a].is_some() {
                continue;
            }
            for b in 0..n {
                if pairs[a * n + b] == 0 {
                    continue;
                }
                let back = b != 0 && pairs[b * n + a] != 0;
                let look = if !back {
                    true
                } else if mats.life[b].is_some() {
                    false
                } else {
                    (partners[a], a) <= (partners[b], b)
                };
                if look {
                    work[a] |= W_LOOK_ALL;
                    break;
                }
            }
        }
        // A material whose partners all check the pair themselves (they look at all neighbors,
        // or they are always awake) does not need to check it too. Water is such a material:
        // lava, molten metal, salt, dirt and fire start their reactions with water. This keeps
        // the most common liquid free of reaction work.
        for a in 1..n {
            if work[a] & W_PAIRS == 0 || work[a] & W_LOOK_ALL != 0 || mats.life[a].is_some() {
                continue;
            }
            let others_check = (0..n).filter(|&b| pairs[a * n + b] != 0).all(|b| {
                b != 0 && (work[b] & W_LOOK_ALL != 0 || mats.life[b].is_some() || work[b] & W_FIRE != 0) && pairs[b * n + a] != 0
            });
            if others_check {
                work[a] &= !W_PAIRS;
            }
        }
        ReactTable {
            n,
            pairs,
            rules,
            extras,
            work: WorkTable::new(&work),
            oxidizer,
            quench,
            timer,
            charring,
            wake_temp,
            reactions: content.reactions.len(),
        }
    }

    /// True if a cell of this material can react, burn or change in any way.
    pub fn has_work(&self, m: MaterialId) -> bool {
        self.work.get(m) != 0
    }

    /// The lowest temperature (°C) at which something can start for a cell of this material by
    /// heat alone (burning, charring, a rule with a lower temperature limit). `None`: nothing.
    /// The heat pass can use it to wake cells that pass it.
    pub fn wake_temp(&self, m: MaterialId) -> Option<i16> {
        self.wake_temp.get(m.index()).copied().filter(|&t| t < i16::MAX)
    }

    /// Number of reactions in the content (the range of `SimEvent::Reaction::index`).
    pub fn reaction_count(&self) -> usize {
        self.reactions
    }

    /// Number of rules for a cell of material `a` next to a cell of material `b`.
    pub fn rule_count(&self, a: MaterialId, b: MaterialId) -> usize {
        (self.pair(a, b) & 0xff) as usize
    }

    #[inline(always)]
    fn pair(&self, a: MaterialId, b: MaterialId) -> u32 {
        // Materials outside the table (never in real data) have no rules.
        self.pairs.get(a.index() * self.n + b.index()).copied().unwrap_or(0)
    }
}

/// The result of a data file output for a matched material: `Some(KEEP)` for no change, `None` if
/// a `$` word does not apply to this material (then the rule is left out for it).
fn output(plain: Option<MaterialId>, own: Option<OwnChange>, m: MaterialId, mats: &MaterialTable) -> Option<u16> {
    match (plain, own) {
        (Some(p), _) => Some(p.0),
        (None, Some(o)) => o.of(m, mats).map(|x| x.0),
        (None, None) => Some(KEEP),
    }
}

/// What a reaction attempt did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing can happen here.
    None,
    /// The cell changed. Do not move it in this tick.
    Changed,
    /// A reaction is possible but did not happen in this tick (chance). Check again next tick.
    KeepAwake,
}

/// Try the reactions of the cell at (x, y). Called before movement.
#[inline]
pub fn try_react(h: &mut Hood, x: i32, y: i32, m: MaterialId) -> Outcome {
    let t = h.react;
    let work = t.work.get(m);
    if work == 0 {
        return Outcome::None;
    }
    react_slow(h, t, x, y, m, work)
}

/// `try_react` for a material with work to do.
fn react_slow(h: &mut Hood, t: &ReactTable, x: i32, y: i32, m: MaterialId, work: u8) -> Outcome {
    if work & (W_FIRE | W_BURN | W_TIMER) == 0 {
        // Only pair rules (water, lava, salt, ...): the common case.
        return pairs(h, t, x, y, m, work);
    }
    if work & W_FIRE != 0 {
        return burn::fire_cell(h, t, x, y, m);
    }
    let mut out = Outcome::None;
    if work & W_BURN != 0 {
        match burn::burnable(h, t, x, y, m) {
            Outcome::None => {}
            Outcome::KeepAwake => out = Outcome::KeepAwake,
            Outcome::Changed => return Outcome::Changed,
        }
    }
    if work & W_TIMER != 0 {
        match burn::timer(h, t, x, y, m) {
            Outcome::None => {}
            Outcome::KeepAwake => out = Outcome::KeepAwake,
            Outcome::Changed => return Outcome::Changed,
        }
    }
    if work & W_PAIRS != 0 {
        match pairs(h, t, x, y, m, work) {
            Outcome::None => {}
            Outcome::KeepAwake => out = Outcome::KeepAwake,
            Outcome::Changed => return Outcome::Changed,
        }
    }
    out
}

/// The result of the rules of one pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fired {
    /// No rule has its conditions now.
    NotReady,
    /// A rule has its conditions, but its chance roll failed.
    Waiting,
    /// A rule fired. `true` if the updated cell changed its material.
    Yes(bool),
}

/// Pair rules with one random neighbor. A material with `W_LOOK_ALL` looks at all neighbors if that one has no rule
/// that can fire now.
#[inline]
fn pairs(h: &mut Hood, t: &ReactTable, x: i32, y: i32, m: MaterialId, work: u8) -> Outcome {
    let (dx, dy) = DIRS[h.rng.below(8) as usize];
    let (nx, ny) = (x + dx, y + dy);
    let range = t.pair(m, h.mat(nx, ny));
    if range != 0 {
        match rules(h, t, x, y, nx, ny, range, true) {
            Fired::Yes(true) => return Outcome::Changed,
            Fired::Yes(false) => return Outcome::None,
            Fired::Waiting => return Outcome::KeepAwake,
            Fired::NotReady => {}
        }
    }
    if work & W_LOOK_ALL != 0 {
        for (dx, dy) in DIRS {
            let (nx, ny) = (x + dx, y + dy);
            let range = t.pair(m, h.mat(nx, ny));
            if range != 0 && rules(h, t, x, y, nx, ny, range, false) != Fired::NotReady {
                return Outcome::KeepAwake;
            }
        }
    }
    Outcome::None
}

/// Check the rules `range` (a `pairs` entry) for the updated cell (x, y) and its neighbor
/// (nx, ny). With `roll`, roll the chances and apply the first rule that passes; without it, only
/// report whether a rule has its conditions now.
#[allow(clippy::too_many_arguments)]
fn rules(h: &mut Hood, t: &ReactTable, x: i32, y: i32, nx: i32, ny: i32, range: u32, roll: bool) -> Fired {
    let (start, len) = ((range >> 8) as usize, (range & 0xff) as usize);
    let mut result = Fired::NotReady;
    for rule in &t.rules[start..start + len] {
        let ((ax, ay), (bx, by)) = if rule.swapped { ((nx, ny), (x, y)) } else { ((x, y), (nx, ny)) };
        let temp = h.temp(ax, ay);
        if temp < rule.min_temp || temp > rule.max_temp {
            continue;
        }
        if rule.needs_air && !has_oxidizer(h, t, ax, ay) {
            continue;
        }
        if !roll {
            return Fired::Waiting;
        }
        result = Fired::Waiting;
        if h.rng.chance(rule.chance) {
            let before = h.mat(x, y);
            apply(h, t, rule, ax, ay, bx, by);
            return Fired::Yes(h.mat(x, y) != before);
        }
    }
    result
}

/// Apply a rule: A is the cell at (ax, ay), B at (bx, by).
fn apply(h: &mut Hood, t: &ReactTable, rule: &Rule, ax: i32, ay: i32, bx: i32, by: i32) {
    let (mut into_a, mut into_b) = (rule.into_a, rule.into_b);
    if rule.alt_chance > 0.0 && h.rng.chance(rule.alt_chance) {
        (into_a, into_b) = (rule.alt_a, rule.alt_b);
    }
    let heat = rule.heat as i32;
    let (ta, tb) = (h.temp(ax, ay) as i32 + heat, h.temp(bx, by) as i32 + heat);
    set_result(h, ax, ay, into_a, ta);
    set_result(h, bx, by, into_b, tb);
    let start = rule.extra_start as usize;
    for &(m, chance) in &t.extras[start..start + rule.extra_len as usize] {
        if h.rng.chance(chance)
            && let Some((fx, fy)) = free_neighbor(h, ax, ay)
        {
            let temp = product_temp(h.mats, m, ta);
            h.replace(fx, fy, m, Some(temp));
        }
    }
    let at = h.world_pos(ax, ay);
    let explosion = match rule.event {
        1 => Some(EXPLOSION_SMALL),
        2 => Some(EXPLOSION_MEDIUM),
        3 => Some(EXPLOSION_LARGE),
        _ => None,
    };
    if let Some((strength, heat)) = explosion {
        h.emit(SimEvent::Explosion { at, strength, heat });
    }
    send_reaction(h, rule.reaction, at);
}

/// Write one result of a rule. `temp` is the cell's temperature plus the rule's heat.
fn set_result(h: &mut Hood, x: i32, y: i32, into: u16, temp: i32) {
    if into == KEEP {
        let clamped = temp.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        if clamped != h.temp(x, y) {
            h.set_temp(x, y, clamped);
            h.keep_awake(x, y);
        }
        return;
    }
    let m = MaterialId(into);
    let temp = product_temp(h.mats, m, temp);
    h.replace(x, y, m, Some(temp));
    burn::clear_flag(h, x, y);
}

/// The temperature of a new cell of material `m` made from a cell at `temp`: a normally hot
/// material (steam, fire) is at least its own temperature, a normally cold one at most. The result
/// stays inside the range where `m` does not change phase at once (a copper block made from molten
/// copper is just below the melting point).
pub fn product_temp(mats: &MaterialTable, m: MaterialId, temp: i32) -> i16 {
    let i = m.index();
    let own = mats.temperature[i] as i32;
    let mut t = temp;
    if own > DEFAULT_TEMPERATURE as i32 {
        t = t.max(own);
    } else if own < DEFAULT_TEMPERATURE as i32 {
        t = t.min(own);
    }
    if let Some(c) = mats.melt[i] {
        t = t.min(c.at as i32 - 1);
    }
    if let Some(c) = mats.boil[i] {
        t = t.min(c.at as i32 - 1);
    }
    if let Some(c) = mats.freeze[i] {
        t = t.max(c.at as i32);
    }
    if let Some(c) = mats.condense[i] {
        t = t.max(c.at as i32);
    }
    t.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

/// A free neighbor of (x, y) for a new cell: an air cell, or else a gas cell that did not change
/// in this tick. The search starts at a random neighbor.
fn free_neighbor(h: &mut Hood, x: i32, y: i32) -> Option<(i32, i32)> {
    let start = h.rng.below(8) as usize;
    let mut gas = None;
    for k in 0..8 {
        let (dx, dy) = DIRS[(start + k) & 7];
        let (nx, ny) = (x + dx, y + dy);
        if !h.inside(nx, ny) {
            continue;
        }
        let n = h.mat(nx, ny);
        if n.is_air() {
            return Some((nx, ny));
        }
        if gas.is_none() && h.mats.phase[n.index()] == Phase::Gas && !h.is_updated(nx, ny) {
            gas = Some((nx, ny));
        }
    }
    gas
}

/// True if an air cell (or oxygen, or fire) is next to (x, y).
#[inline]
fn has_oxidizer(h: &Hood, t: &ReactTable, x: i32, y: i32) -> bool {
    DIRS.iter().any(|&(dx, dy)| h.inside(x + dx, y + dy) && t.oxidizer[h.mat(x + dx, y + dy).index()])
}

/// Send a `Reaction` event, once per reaction in this job.
fn send_reaction(h: &mut Hood, index: u16, at: CellPos) {
    let sent = h.events.iter().any(|e| matches!(e, SimEvent::Reaction { index: i, .. } if *i == index));
    if !sent {
        h.emit(SimEvent::Reaction { index, at });
    }
}

/// Keep only the first `Reaction` event of each reaction. The events are in the fixed job order,
/// so the result does not depend on the number of threads.
pub fn dedupe_reaction_events(events: &mut Vec<SimEvent>) {
    let Some(max) = events.iter().filter_map(|e| if let SimEvent::Reaction { index, .. } = e { Some(*index) } else { None }).max() else {
        return;
    };
    let mut seen = vec![0u64; max as usize / 64 + 1];
    events.retain(|e| match e {
        SimEvent::Reaction { index, .. } => {
            let (w, bit) = (*index as usize / 64, 1u64 << (index % 64));
            let first = seen[w] & bit == 0;
            seen[w] |= bit;
            first
        }
        _ => true,
    });
}
