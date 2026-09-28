//! Burning, fire cells, charring and timers. See the module documentation of `react.rs`.
//!
//! A burnable material (with `burn` data) starts to burn when:
//! - its temperature is at or above `ignite_at` and an air cell (or oxygen, or fire) is next to it
//!   (or the material does not need air), or
//! - a fire cell or a burning cell next to it sets it on fire (it must also have air next to it).
//!
//! It cannot start to burn while water or carbon dioxide (the `quench` materials) is next to it.
//!
//! A burning cell stays at least at its fire temperature. Each tick it may put a fire cell into
//! an air neighbor, make its gases (smoke, CO₂, ...) in air neighbors, and set a burnable neighbor
//! on fire. With its burn chance it is used up and becomes its burn result (ash or air). Without
//! air next to it (when it needs air) it goes out. Water next to it puts it out, cools it and
//! turns into steam. Carbon dioxide next to it puts it out.
//!
//! Charring: a burnable material with `char_into` that is at or above `ignite_at` with no air next
//! to it counts steps; after about `char_ticks` ticks it becomes `char_into` (wood becomes
//! charcoal). Air next to it resets the count.
//!
//! Timers: a material with a `timer` counts steps while it is awake (and, with `needs_air`, while
//! air is next to it) and becomes `into` after about `ticks` ticks.

use super::{DIRS, Fired, Outcome, ReactTable, Steps, W_BURN, has_oxidizer, product_temp, rules};
use crate::chunk::FLAG_BURNING;
use crate::hood::Hood;
use foundry_content::{Burn, Phase};
use foundry_core::MaterialId;

/// Life byte bit of a burnable material: the cell burns.
pub const LIFE_BURNING: u8 = 0x80;
/// Life byte bits: the step count of charring or of a timer.
const LIFE_STEPS: u8 = 0x7f;
/// Most steps of a timer or of charring. More steps make the time more exact.
pub const MAX_STEPS: u8 = 100;

/// Chance per tick that a burning cell puts a fire cell into an air neighbor.
const FIRE_CHANCE: f32 = 0.3;
/// Chance per tick that a burning cell sets the neighbor it picked on fire.
const SPREAD_CHANCE: f32 = 0.5;
/// Chance per tick that a fire cell sets the neighbor it picked on fire.
const FIRE_IGNITE_CHANCE: f32 = 0.5;

/// The neighbors of a cell that burning needs to know about.
struct Around {
    /// Air, oxygen or fire is next to it.
    oxidizer: bool,
    /// A cell that puts fire out (water, carbon dioxide).
    quench: Option<(i32, i32)>,
    /// The air neighbors, as offsets.
    air: [(i32, i32); 8],
    n_air: usize,
}

impl Around {
    fn look(h: &Hood, t: &ReactTable, x: i32, y: i32) -> Around {
        let mut a = Around { oxidizer: false, quench: None, air: [(0, 0); 8], n_air: 0 };
        for (dx, dy) in DIRS {
            if !h.inside(x + dx, y + dy) {
                continue;
            }
            let n = h.mat(x + dx, y + dy);
            a.oxidizer |= t.oxidizer[n.index()];
            if n.is_air() {
                a.air[a.n_air] = (dx, dy);
                a.n_air += 1;
            } else if a.quench.is_none() && t.quench[n.index()] {
                a.quench = Some((x + dx, y + dy));
            }
        }
        a
    }

    /// Take a random air neighbor out of the list.
    fn take_air(&mut self, h: &mut Hood) -> Option<(i32, i32)> {
        if self.n_air == 0 {
            return None;
        }
        let k = h.rng.below(self.n_air as u32) as usize;
        let d = self.air[k];
        self.n_air -= 1;
        self.air[k] = self.air[self.n_air];
        Some(d)
    }
}

/// True if a cell that puts fire out is next to (x, y).
fn has_quench(h: &Hood, t: &ReactTable, x: i32, y: i32) -> bool {
    DIRS.iter().any(|&(dx, dy)| h.inside(x + dx, y + dy) && t.quench[h.mat(x + dx, y + dy).index()])
}

/// Set the burning flag (for the renderer).
fn set_flag(h: &mut Hood, x: i32, y: i32) {
    let f = h.flags(x, y);
    if f & FLAG_BURNING == 0 {
        h.set_flags(x, y, f | FLAG_BURNING);
    }
}

/// Clear the burning flag.
pub fn clear_flag(h: &mut Hood, x: i32, y: i32) {
    let f = h.flags(x, y);
    if f & FLAG_BURNING != 0 {
        h.set_flags(x, y, f & !FLAG_BURNING);
    }
}

/// Set the cell on fire.
fn ignite(h: &mut Hood, x: i32, y: i32, b: &Burn) {
    let life = h.life(x, y);
    h.set_life(x, y, (life & !LIFE_STEPS) | LIFE_BURNING);
    if h.temp(x, y) < b.fire_temp {
        h.set_temp(x, y, b.fire_temp);
    }
    set_flag(h, x, y);
    h.mark_changed(x, y);
}

/// Put the fire of a burning cell out. `cool`: water cooled it below its ignition point.
fn put_out(h: &mut Hood, x: i32, y: i32, b: &Burn, cool: bool) {
    let life = h.life(x, y);
    h.set_life(x, y, life & !LIFE_BURNING);
    clear_flag(h, x, y);
    if cool {
        let limit = (b.ignite_at as i32 - 1).min(100) as i16;
        if h.temp(x, y) > limit {
            h.set_temp(x, y, limit);
        }
    }
    h.mark_changed(x, y);
}

/// Count one step of charring or of a timer (with the step chance). After the last step the cell
/// becomes `into`.
fn step(h: &mut Hood, x: i32, y: i32, s: Steps, into: MaterialId) -> Outcome {
    if h.rng.chance(s.chance) {
        let life = h.life(x, y);
        let count = (life & LIFE_STEPS) + 1;
        if count >= s.steps {
            let temp = product_temp(h.mats, into, h.temp(x, y) as i32);
            h.replace(x, y, into, Some(temp));
            clear_flag(h, x, y);
            return Outcome::Changed;
        }
        h.set_life(x, y, (life & !LIFE_STEPS) | count);
    }
    Outcome::KeepAwake
}

/// A cell of a material with burn data.
pub fn burnable(h: &mut Hood, t: &ReactTable, x: i32, y: i32, m: MaterialId) -> Outcome {
    let Some(b) = h.mats.burn[m.index()] else { return Outcome::None };
    let life = h.life(x, y);
    if life & LIFE_BURNING != 0 {
        return burning(h, t, x, y, &b);
    }
    if h.flags(x, y) & FLAG_BURNING != 0 {
        // A flag left behind by a burning cell that moved away.
        clear_flag(h, x, y);
        h.mark_changed(x, y);
    }
    if h.temp(x, y) < b.ignite_at {
        return Outcome::None;
    }
    let a = Around::look(h, t, x, y);
    if a.quench.is_some() {
        return Outcome::None;
    }
    if a.oxidizer || !b.needs_air {
        ignite(h, x, y, &b);
        return Outcome::KeepAwake;
    }
    // Hot with no air next to it: charring.
    match b.char_into {
        Some(into) => step(h, x, y, t.charring[m.index()], into),
        None => Outcome::None,
    }
}

/// A burning cell.
fn burning(h: &mut Hood, t: &ReactTable, x: i32, y: i32, b: &Burn) -> Outcome {
    let mut a = Around::look(h, t, x, y);
    if let Some((qx, qy)) = a.quench {
        let q = h.mat(qx, qy);
        let liquid = h.mats.phase[q.index()] == Phase::Liquid;
        put_out(h, x, y, b, liquid);
        // Water that puts out a fire boils.
        if let Some(c) = h.mats.boil[q.index()] {
            let temp = product_temp(h.mats, c.into, h.temp(qx, qy) as i32);
            h.replace(qx, qy, c.into, Some(temp));
        }
        return Outcome::None;
    }
    if b.needs_air && !a.oxidizer {
        put_out(h, x, y, b, false);
        return Outcome::None;
    }
    // A burning liquid or powder moved here: its flag stayed behind.
    if h.flags(x, y) & FLAG_BURNING == 0 {
        set_flag(h, x, y);
        h.mark_changed(x, y);
    }
    let temp = h.temp(x, y).max(b.fire_temp);
    if temp != h.temp(x, y) {
        h.set_temp(x, y, temp);
    }
    if h.rng.chance(b.chance) {
        // Used up.
        let into_temp = product_temp(h.mats, b.into, temp as i32);
        h.replace(x, y, b.into, Some(into_temp));
        clear_flag(h, x, y);
        return Outcome::Changed;
    }
    if h.rng.chance(FIRE_CHANCE)
        && let Some((dx, dy)) = a.take_air(h)
    {
        let fire_temp = product_temp(h.mats, b.fire, b.fire_temp as i32);
        h.replace(x + dx, y + dy, b.fire, Some(fire_temp));
        clear_flag(h, x + dx, y + dy);
    }
    for (gas, chance) in b.gases.into_iter().flatten() {
        if h.rng.chance(chance)
            && let Some((dx, dy)) = a.take_air(h)
        {
            let gas_temp = product_temp(h.mats, gas, h.temp(x + dx, y + dy) as i32);
            h.replace(x + dx, y + dy, gas, Some(gas_temp));
            clear_flag(h, x + dx, y + dy);
        }
    }
    // Set a burnable neighbor on fire.
    let (dx, dy) = DIRS[h.rng.below(8) as usize];
    let (nx, ny) = (x + dx, y + dy);
    let n = h.mat(nx, ny);
    if t.work.get(n) & W_BURN != 0 && h.life(nx, ny) & LIFE_BURNING == 0 && h.rng.chance(SPREAD_CHANCE) {
        ignite_neighbor(h, t, nx, ny, n, false);
    }
    Outcome::KeepAwake
}

/// Set the burnable cell (x, y) of material `m` on fire, if it has air next to it (when it needs
/// air; `by_fire`: a fire cell is next to it, which counts as air) and no water next to it.
fn ignite_neighbor(h: &mut Hood, t: &ReactTable, x: i32, y: i32, m: MaterialId, by_fire: bool) {
    let Some(b) = h.mats.burn[m.index()] else { return };
    if b.needs_air && !by_fire && !has_oxidizer(h, t, x, y) {
        return;
    }
    if has_quench(h, t, x, y) {
        return;
    }
    ignite(h, x, y, &b);
}

/// A fire cell (phase Fire): it ignites burnable neighbors and dies in water. The pair rules of
/// fire (fire + water, methane + fire, ...) come first.
pub fn fire_cell(h: &mut Hood, t: &ReactTable, x: i32, y: i32, m: MaterialId) -> Outcome {
    // A flag left behind by a burning cell (fire moves into such places).
    clear_flag(h, x, y);
    let (dx, dy) = DIRS[h.rng.below(8) as usize];
    let (nx, ny) = (x + dx, y + dy);
    let n = h.mat(nx, ny);
    let range = t.pair(m, n);
    if range != 0 {
        match rules(h, t, x, y, nx, ny, range, true) {
            Fired::Yes(true) => return Outcome::Changed,
            Fired::NotReady => {}
            Fired::Yes(false) | Fired::Waiting => return Outcome::None,
        }
    }
    if t.work.get(n) & W_BURN != 0 {
        if h.life(nx, ny) & LIFE_BURNING == 0 && h.rng.chance(FIRE_IGNITE_CHANCE) {
            ignite_neighbor(h, t, nx, ny, n, true);
        }
    } else if t.quench[n.index()] {
        // Fire dies in water.
        h.replace(x, y, MaterialId::AIR, None);
        return Outcome::Changed;
    }
    // Fire fades, so it is always awake.
    Outcome::None
}

/// A cell of a material with a timer.
pub fn timer(h: &mut Hood, t: &ReactTable, x: i32, y: i32, m: MaterialId) -> Outcome {
    let Some(tm) = h.mats.timer[m.index()] else { return Outcome::None };
    if tm.needs_air && !has_oxidizer(h, t, x, y) {
        return Outcome::None;
    }
    step(h, x, y, t.timer[m.index()], tm.into)
}
