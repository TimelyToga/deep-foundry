//! Movement rules for each phase (technical design section 6.3).
//!
//! All functions get hood coordinates of a cell in the center chunk. A cell never moves more than
//! `MAX_CELL_MOVE` cells in one tick.

use crate::chunk::{MOTION_RIGHT, MOTION_SPEED};
use crate::hood::Hood;
use foundry_content::Phase;
use foundry_core::MaterialId;

/// Density of air (kg/m³). Lighter gases rise, heavier gases sink.
pub const AIR_DENSITY: f32 = 1.2;
/// Fall speed grows by 1 each tick up to this value. The distance per tick is `1 + speed / 4`.
const MAX_FALL_SPEED: u8 = 28;
/// A liquid never flows farther than this in one tick.
const MAX_FLOW: i32 = 16;
/// How far a surface liquid cell looks to the side for a place to fall.
/// Must stay below `MAX_CELL_MOVE` (the parallel reach rule).
const LOOK_AHEAD: i32 = 31;

/// Try to move the cell. Returns true if it moved.
#[inline]
pub fn try_move(h: &mut Hood, x: i32, y: i32, m: MaterialId, phase: Phase) -> bool {
    match phase {
        Phase::Powder => powder(h, x, y, m),
        Phase::Liquid => liquid(h, x, y, m),
        Phase::Gas => gas(h, x, y, m),
        Phase::Fire => fire(h, x, y),
        Phase::Solid | Phase::Empty => false,
    }
}

/// Air, gas and fire give way to falling powder and liquid.
#[inline(always)]
fn passable(h: &Hood, m: MaterialId) -> bool {
    m.is_air() || matches!(h.mats.phase[m.index()], Phase::Gas | Phase::Fire)
}

/// Fall straight down through passable cells. Returns true if the cell moved.
#[inline]
fn fall(h: &mut Hood, x: i32, y: i32) -> bool {
    if !passable(h, h.mat(x, y + 1)) || !h.inside(x, y + 1) {
        return false;
    }
    let motion = h.motion(x, y);
    let speed = ((motion & MOTION_SPEED) + 1).min(MAX_FALL_SPEED);
    let dist = 1 + (speed as i32) / 4;
    let mut to = y + 1;
    for k in 2..=dist {
        let m = h.mat(x, y + k);
        if !passable(h, m) || !h.inside(x, y + k) {
            break;
        }
        to = y + k;
    }
    h.set_motion(x, y, (motion & !MOTION_SPEED) | speed);
    h.swap(x, y, x, to);
    true
}

/// Stop falling: clear the fall speed. Returns the old speed.
#[inline]
fn land(h: &mut Hood, x: i32, y: i32) -> u8 {
    let motion = h.motion(x, y);
    let speed = motion & MOTION_SPEED;
    if speed != 0 {
        h.set_motion(x, y, motion & !MOTION_SPEED);
    }
    speed
}

/// Chance that a cell of density `heavy` sinks through a liquid of density `light` in one tick.
#[inline(always)]
fn sink_chance(heavy: f32, light: f32) -> f32 {
    ((heavy - light) / heavy * 2.0).clamp(0.1, 0.9)
}

fn powder(h: &mut Hood, x: i32, y: i32, m: MaterialId) -> bool {
    if fall(h, x, y) {
        return true;
    }
    let speed = land(h, x, y);
    let md = h.mats.density[m.index()];

    // Inside a liquid: flowing liquid carries light powder, and denser powder sinks.
    let below = h.mat(x, y + 1);
    if h.mats.phase[below.index()] == Phase::Liquid {
        let ld = h.mats.density[below.index()];
        if md > ld && h.rng.chance(sink_chance(md, ld)) {
            h.swap(x, y, x, y + 1);
            return true;
        }
    }
    let above = h.mat(x, y - 1);
    if h.mats.phase[above.index()] == Phase::Liquid && md < h.mats.drag_limit[above.index()] {
        // The liquid above flows to one side; move with it.
        let dir = if h.motion(x, y - 1) & MOTION_RIGHT != 0 { 1 } else { -1 };
        let side = h.mat(x + dir, y);
        let carry = 0.5 * (1.0 - md / h.mats.drag_limit[above.index()]);
        if h.mats.phase[side.index()] == Phase::Liquid && h.inside(x + dir, y) && h.rng.chance(carry) {
            h.swap(x, y, x + dir, y);
            return true;
        }
        // No keep_awake here: when the liquid around moves, it marks this cell again.
    }

    // Slide down to a side. A cell that just landed with speed slides without friction.
    if speed < 3 && h.rng.chance(h.mats.friction[m.index()]) {
        return false;
    }
    let first = if h.rng.coin() { 1 } else { -1 };
    for dx in [first, -first] {
        let d = h.mat(x + dx, y + 1);
        let side = h.mat(x + dx, y);
        if h.mats.phase[side.index()] == Phase::Solid || !h.inside(x + dx, y + 1) {
            continue;
        }
        let sinks_into = h.mats.phase[d.index()] == Phase::Liquid && md > h.mats.density[d.index()];
        if passable(h, d) || (sinks_into && h.rng.chance(0.5)) {
            h.swap(x, y, x + dx, y + 1);
            return true;
        }
    }
    false
}

/// A liquid cell left (x, y). Surface cells up to `LOOK_AHEAD` cells away on this row and the row
/// above may now find a place to fall, so check them again in the next tick.
#[inline]
fn wake_row(h: &mut Hood, x: i32, y: i32) {
    h.keep_awake_rect(x - LOOK_AHEAD, y - 1, x + LOOK_AHEAD + 1, y + 1);
}

fn liquid(h: &mut Hood, x: i32, y: i32, m: MaterialId) -> bool {
    let moved = liquid_move(h, x, y, m);
    if moved {
        wake_row(h, x, y);
    }
    moved
}

fn liquid_move(h: &mut Hood, x: i32, y: i32, m: MaterialId) -> bool {
    if fall(h, x, y) {
        return true;
    }
    land(h, x, y);
    let md = h.mats.density[m.index()];

    // Sink through a lighter liquid.
    let below = h.mat(x, y + 1);
    if h.mats.phase[below.index()] == Phase::Liquid && below != m {
        let bd = h.mats.density[below.index()];
        if md > bd {
            if h.rng.chance(sink_chance(md, bd)) {
                h.swap(x, y, x, y + 1);
                return true;
            }
            h.keep_awake(x, y);
        }
    }

    // Rise through a heavier liquid above.
    let up = h.mat(x, y - 1);
    if h.mats.phase[up.index()] == Phase::Liquid && up != m {
        let ud = h.mats.density[up.index()];
        if ud > md {
            if h.rng.chance(sink_chance(ud, md)) {
                h.swap(x, y, x, y - 1);
                return true;
            }
            h.keep_awake(x, y);
        }
    }

    let motion = h.motion(x, y);
    let mut dir = if motion & MOTION_RIGHT != 0 { 1 } else { -1 };
    // Down to a side.
    for dx in [dir, -dir] {
        if passable(h, h.mat(x + dx, y + 1)) && passable(h, h.mat(x + dx, y)) && h.inside(x + dx, y + 1) {
            h.swap(x, y, x + dx, y + 1);
            return true;
        }
    }
    // Flow sideways: keep the last direction; turn around at a wall.
    // The cell moves if it finds a place where it can fall, or if liquid above it (straight up,
    // or up on the side it leaves) can fill the place it leaves. Otherwise it stays: without this
    // rule a partial top row moves forever and never comes to rest.
    let flow = (h.mats.flow[m.index()] as i32).clamp(1, MAX_FLOW);
    let is_liquid = |h: &Hood, x: i32, y: i32| h.mats.phase[h.mat(x, y).index()] == Phase::Liquid;
    let above = is_liquid(h, x, y - 1);
    for attempt in 0..2 {
        let pressed = above || is_liquid(h, x - dir, y - 1);
        // Look up to LOOK_AHEAD cells for a place to fall; move at most `flow` cells.
        // `open`: the row is free and rests on liquid for the whole look-ahead (a wide surface).
        let mut last_free = None;
        let mut drop_at = None;
        let mut open = true;
        for d in 1..=LOOK_AHEAD {
            let tx = x + dir * d;
            let t = h.mat(tx, y);
            if !passable(h, t) || !h.inside(tx, y) {
                open = false;
                // A heavier liquid with more of itself on top pushes under a lighter one next to it.
                // (Without the weight on top, the swap gains nothing and the two mix forever.)
                if d == 1
                    && h.mats.phase[t.index()] == Phase::Liquid
                    && t != m
                    && h.mats.density[t.index()] < md
                    && h.mat(x, y - 1) == m
                    && h.rng.chance(0.3)
                {
                    last_free = Some(tx);
                    drop_at = Some(tx);
                }
                break;
            }
            if d <= flow {
                last_free = Some(tx);
            }
            let b = h.mat(tx, y + 1);
            if passable(h, b) && h.inside(tx, y + 1) {
                drop_at = Some(tx);
                break;
            }
            if h.mats.phase[b.index()] != Phase::Liquid {
                open = false;
            }
        }
        // On a wide surface the cell keeps its direction (never turns around for this reason),
        // so a partial top row spreads out and then comes to rest against a wall or other cells.
        let keep_going = attempt == 0 && open && is_liquid(h, x, y + 1);
        let target = if drop_at.is_some() || pressed || keep_going { last_free } else { None };
        if let Some(tx) = target {
            let bits = if dir > 0 { motion | MOTION_RIGHT } else { motion & !MOTION_RIGHT };
            h.set_motion(x, y, bits);
            h.swap(x, y, tx, y);
            return true;
        }
        dir = -dir;
    }
    false
}

fn gas(h: &mut Hood, x: i32, y: i32, m: MaterialId) -> bool {
    let md = h.mats.density[m.index()];
    let dy = if md < AIR_DENSITY { -1 } else { 1 };
    let t = h.mat(x, y + dy);
    if h.inside(x, y + dy) {
        if t.is_air() {
            if h.rng.chance(0.8) {
                h.swap(x, y, x, y + dy);
                return true;
            }
            h.keep_awake(x, y);
            return false;
        }
        if t != m && h.mats.phase[t.index()] == Phase::Gas {
            let td = h.mats.density[t.index()];
            let rises_through = dy < 0 && td > md;
            let sinks_through = dy > 0 && td < md;
            if (rises_through || sinks_through) && h.rng.chance(0.5) {
                h.swap(x, y, x, y + dy);
                return true;
            }
        }
    }
    let first = if h.rng.coin() { 1 } else { -1 };
    for dx in [first, -first] {
        if h.mat(x + dx, y + dy).is_air() && h.inside(x + dx, y + dy) {
            h.swap(x, y, x + dx, y + dy);
            return true;
        }
    }
    // Spread sideways into air. Mix slowly with other gases.
    let s = h.mat(x + first, y);
    if h.inside(x + first, y) {
        if s.is_air() && h.rng.chance(0.5) {
            h.swap(x, y, x + first, y);
            return true;
        }
        if s != m && h.mats.phase[s.index()] == Phase::Gas && h.rng.chance(0.05) {
            h.swap(x, y, x + first, y);
            return true;
        }
    }
    false
}

fn fire(h: &mut Hood, x: i32, y: i32) -> bool {
    let dx = h.rng.below(3) as i32 - 1;
    for (tx, ty) in [(x + dx, y - 1), (x, y - 1)] {
        let t = h.mat(tx, ty);
        if h.inside(tx, ty) && (t.is_air() || h.mats.phase[t.index()] == Phase::Gas) {
            h.swap(x, y, tx, ty);
            return true;
        }
    }
    false
}
