//! Movement rules for each phase (technical design section 6.3).
//!
//! All functions get hood coordinates of a cell in the center chunk. A cell never moves more than
//! `MAX_CELL_MOVE` cells in one tick.

use crate::chunk::{MOTION_MOMENTUM, MOTION_MOMENTUM_SHIFT, MOTION_RIGHT, MOTION_SPEED};
use crate::hood::Hood;
use foundry_content::{MaterialTable, Phase};
use foundry_core::{CellPos, MaterialId};

/// Density of air (kg/m³). Lighter gases rise, heavier gases sink.
pub const AIR_DENSITY: f32 = 1.2;
/// Fall speed grows by 1 each tick up to this value. The distance per tick is `1 + speed / 4`.
const MAX_FALL_SPEED: u8 = 28;
/// A liquid never flows farther than this in one tick.
const MAX_FLOW: i32 = 16;
/// How far a surface liquid cell looks to the side for a place to fall.
/// Must stay below `MAX_CELL_MOVE` (the parallel reach rule).
const LOOK_AHEAD: i32 = 31;
/// The level pass looks at most this far (cells) along a liquid surface.
pub const MAX_LEVEL_SCAN: i32 = 1024;

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

/// The fall pass: a powder or liquid cell that is already falling moves straight down.
/// It touches only cells in its own column. Returns true if it moved.
#[inline]
pub fn fall_only(h: &mut Hood, x: i32, y: i32, phase: Phase) -> bool {
    let top = phase == Phase::Liquid && h.mat(x, y - 1).is_air();
    if !fall(h, x, y) {
        return false;
    }
    if phase == Phase::Liquid {
        wake_row(h, x, y);
        opened(h, x, y, top);
    }
    true
}

/// A liquid cell left (x, y). If it was a top cell and (x, y) is now air, the row above has a new
/// place to fall there; the level pass wakes the far ends of that row (see `wake_row_ends`).
#[inline]
fn opened(h: &mut Hood, x: i32, y: i32, was_top: bool) {
    if was_top && h.mat(x, y).is_air() && h.mat(x, y - 1).is_air() {
        let p = h.world_pos(x, y);
        h.opened.push(p);
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
    let mut speed = ((motion & MOTION_SPEED) + 1).min(MAX_FALL_SPEED);
    let dist = 1 + (speed as i32) / 4;
    let mut to = y + 1;
    for k in 2..=dist {
        let m = h.mat(x, y + k);
        if !passable(h, m) || !h.inside(x, y + k) {
            break;
        }
        to = y + k;
    }
    // A one-cell gap to a falling cell below: close it, so a falling body stays in one piece.
    if to == y + dist && passable(h, h.mat(x, to + 1)) && h.inside(x, to + 1) {
        let b = h.mat(x, to + 2);
        if matches!(h.mats.phase[b.index()], Phase::Powder | Phase::Liquid) && h.motion(x, to + 2) & MOTION_SPEED != 0 {
            to += 1;
        }
    }
    // A falling cell just below: take at least its speed, so a part of a body that started to
    // fall one tick later catches up and the body falls as one piece. (If the cell below has not
    // moved yet in this tick, its speed goes up by one when it moves.)
    let b = h.mat(x, to + 1);
    if matches!(h.mats.phase[b.index()], Phase::Powder | Phase::Liquid) {
        let below = h.motion(x, to + 1) & MOTION_SPEED;
        if below > 0 {
            let next = if h.is_updated(x, to + 1) { below } else { below + 1 };
            speed = speed.max(next.min(MAX_FALL_SPEED));
        }
    }
    h.set_motion(x, y, (motion & !MOTION_SPEED) | speed);
    h.swap(x, y, x, to);
    true
}

/// True if the cell should wait because the cell below will still fall in this tick:
/// - the cell below waited in this tick (it is updated, has a fall speed, and is still there); or
/// - the cell below has not been updated yet in this tick (it is in a chunk that comes later),
///   and the powder and liquid cells under it end on air, so they will fall.
///
/// A waiting cell does nothing else, so a falling body does not land or flow sideways in mid-air
/// at a chunk border. It gets fall speed 1 (if it had none), so the fall pass moves it in the
/// next tick and the cells on top of it wait too.
#[inline]
fn waits_on_fall(h: &mut Hood, x: i32, y: i32) -> bool {
    let b = h.mat(x, y + 1);
    if !matches!(h.mats.phase[b.index()], Phase::Powder | Phase::Liquid) {
        return false;
    }
    let wait = if h.is_updated(x, y + 1) {
        h.motion(x, y + 1) & MOTION_SPEED != 0
    } else {
        column_falls(h, x, y + 1)
    };
    if wait {
        let motion = h.motion(x, y);
        if motion & MOTION_SPEED == 0 {
            h.set_motion(x, y, motion | 1);
        }
        h.keep_awake(x, y);
    }
    wait
}

/// True if the powder and liquid cells from (x, y) down, which have not moved in this tick, end
/// on air (so they will fall in this tick). Looks at most `LOOK_AHEAD` cells down.
#[inline]
fn column_falls(h: &Hood, x: i32, y: i32) -> bool {
    for k in 0..LOOK_AHEAD {
        let m = h.mat(x, y + k);
        if passable(h, m) {
            return h.inside(x, y + k);
        }
        if !matches!(h.mats.phase[m.index()], Phase::Powder | Phase::Liquid) || h.is_updated(x, y + k) {
            return false;
        }
    }
    false
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
    if waits_on_fall(h, x, y) {
        return false;
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

#[inline(always)]
fn is_liquid(h: &Hood, x: i32, y: i32) -> bool {
    h.mats.phase[h.mat(x, y).index()] == Phase::Liquid
}

/// Motion bits of a liquid cell that is not falling: flow direction and momentum.
#[inline(always)]
fn flow_bits(dir: i32, energy: u8) -> u8 {
    (if dir > 0 { MOTION_RIGHT } else { 0 }) | (energy << MOTION_MOMENTUM_SHIFT)
}

fn liquid(h: &mut Hood, x: i32, y: i32, m: MaterialId) -> bool {
    if waits_on_fall(h, x, y) {
        return false;
    }
    let top = h.mat(x, y - 1).is_air();
    if !(fall(h, x, y) || spread(h, x, y, m)) {
        return false;
    }
    wake_row(h, x, y);
    opened(h, x, y, top);
    true
}

/// How far a liquid cell looks to the side for a place to fall: 4 × flow, at most
/// `SimSettings::liquid_look_ahead`.
#[inline(always)]
fn look_ahead(h: &Hood, flow: i32) -> i32 {
    h.settings.liquid_look_ahead.min(4 * flow).clamp(1, LOOK_AHEAD)
}

/// Momentum level (1 to 3) of a cell that lands with this fall speed.
#[inline(always)]
fn landing_energy(speed: u8) -> u8 {
    (speed / 8 + 1).min(3)
}

/// The first free cell on row `y` in direction `dir`, through cells of the same liquid, at most
/// `range` cells away.
#[inline]
fn rim(h: &Hood, x: i32, y: i32, m: MaterialId, dir: i32, range: i32) -> Option<i32> {
    for d in 1..=range {
        let tx = x + dir * d;
        let t = h.mat(tx, y);
        if passable(h, t) {
            return h.inside(tx, y).then_some(tx);
        }
        if t != m {
            return None;
        }
    }
    None
}

/// A falling liquid cell hit something. Its fall turns into sideways movement: it goes through
/// the liquid to the first free cell to one side (farther for a harder landing) and keeps
/// momentum there. With the material's `splash` chance it flies off from there as a droplet.
/// A falling column delivers `1 + speed / 4` cells per tick, so the next falling cells above take
/// the same way in the same tick. Returns true if the cell moved.
fn impact(h: &mut Hood, x: i32, y: i32, m: MaterialId, speed: u8) -> bool {
    let mi = m.index();
    if h.mats.momentum[mi] == 0 {
        return false;
    }
    let energy = landing_energy(speed);
    let flow = (h.mats.flow[mi] as i32).clamp(1, MAX_FLOW);
    let range = (flow * (1 + energy as i32)).min(look_ahead(h, flow));
    let arrivals = 1 + speed as i32 / 4;
    let v = arrivals as f32;
    let splash = if h.splash_ok && speed >= h.settings.splash_min_speed { h.mats.splash[mi] } else { 0.0 };
    let mut moved = false;
    for k in 0..arrivals {
        let first = if h.rng.coin() { 1 } else { -1 };
        let Some((dir, tx)) = [first, -first].into_iter().find_map(|d| rim(h, x, y, m, d, range).map(|tx| (d, tx))) else {
            break;
        };
        h.set_motion(x, y, flow_bits(dir, energy));
        h.swap(x, y, tx, y);
        if splash > 0.0 && h.mat(tx, y - 1).is_air() && h.rng.chance(splash) {
            let vx = dir as f32 * v * (0.3 + 0.4 * h.rng.unit());
            let vy = -v * (0.25 + 0.35 * h.rng.unit());
            h.launch(tx, y, vx, vy);
        }
        moved = true;
        if k + 1 == arrivals {
            break;
        }
        // The next falling cell of the column comes down to this place.
        let Some(j) = (1..=arrivals).find(|&j| !h.mat(x, y - j).is_air()) else { break };
        if h.mat(x, y - j) != m || h.motion(x, y - j) & MOTION_SPEED < 2 {
            break;
        }
        h.swap(x, y - j, x, y);
    }
    moved
}

/// Everything a liquid cell does when it does not fall: land, sink or rise, flow down to a side,
/// flow sideways. Returns true if the cell moved.
///
/// Sideways, a cell moves at most `flow` cells, for one of these reasons:
/// - drop: it sees a place to fall within the look-ahead (4 × flow cells, at most
///   `SimSettings::liquid_look_ahead`);
/// - pressed: liquid above it will fill its place;
/// - push: it is pressed or has momentum, and the same liquid is next to it: it goes through
///   the liquid to the first free cell (up to `flow` cells away). This is how pressure moves
///   water: a column of water pushes out at its foot, and a landing stream pushes the pool aside;
/// - momentum: it is still moving (from a landing or an earlier move);
/// - walk: it is on top of liquid and the surface ahead is free (liquids with a viscosity
///   below 0.5 only). This spreads a thin top layer until it is flat.
///
/// Momentum (0 to 3) comes from landings (more for a faster landing) and from drop, pressed and
/// push moves (at least 1). A momentum move off a liquid surface keeps its momentum with the
/// material's `momentum` chance. A cell that is blocked turns around and loses one level (it
/// bounces); a cell with no momentum does not turn around. A walk costs nothing but never turns.
/// So the total movement is finite and the liquid always comes to rest.
fn spread(h: &mut Hood, x: i32, y: i32, m: MaterialId) -> bool {
    let mi = m.index();
    let motion = h.motion(x, y);
    let speed = motion & MOTION_SPEED;
    let keep = h.mats.momentum[mi];
    let mut dir = if motion & MOTION_RIGHT != 0 { 1 } else { -1 };
    let mut energy = (motion & MOTION_MOMENTUM) >> MOTION_MOMENTUM_SHIFT;

    // Landing: the cell pushes out sideways (see `impact`). If it cannot, its fall speed becomes
    // momentum in a random direction, so falling liquid spreads out instead of piling up.
    if speed >= 2 {
        if impact(h, x, y, m, speed) {
            return true;
        }
        if keep > 0 {
            energy = energy.max((speed / 8 + 1).min(3));
            dir = if h.rng.coin() { 1 } else { -1 };
        }
    }
    h.set_motion(x, y, flow_bits(dir, energy));
    let md = h.mats.density[mi];

    // Sink through a lighter liquid below.
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

    // A viscous cell that can move waits with this chance (it stays awake).
    let viscosity = h.mats.viscosity[mi];
    let sticky = viscosity > 0.0 && h.rng.chance(viscosity);

    // Down to a side.
    for dx in [dir, -dir] {
        if passable(h, h.mat(x + dx, y + 1)) && passable(h, h.mat(x + dx, y)) && h.inside(x + dx, y + 1) {
            if sticky {
                h.keep_awake(x, y);
                return false;
            }
            h.set_motion(x, y, flow_bits(dx, energy));
            h.swap(x, y, x + dx, y + 1);
            return true;
        }
    }

    // Sideways.
    let flow = (h.mats.flow[mi] as i32).clamp(1, MAX_FLOW);
    let look = look_ahead(h, flow);
    let levels = viscosity < 0.5;
    let pressed = is_liquid(h, x, y - 1);
    let on_liquid = is_liquid(h, x, y + 1);
    let mut saw_rim = false;
    for _ in 0..2 {
        let side = look_side(h, x, y, m, dir, look, on_liquid);
        saw_rim |= side.rim.is_some();
        let mut go: Option<Move> = None;
        if side.heavier_under {
            go = Some(Move { to: x + dir, from_top: false, releases: true });
        } else if side.through {
            // The same liquid is next to this cell.
            if let Some(d) = side.rim {
                if pressed {
                    // Pressure pushes to a free cell up to the look-ahead away; a far push happens
                    // only with the chance range / distance, so the speed is about `range`.
                    let range = push_range(liquid_depth(h, x, y), flow);
                    if d <= range || h.rng.chance(range as f32 / d as f32) {
                        go = Some(Move { to: x + dir * d, from_top: true, releases: true });
                    } else {
                        h.keep_awake(x, y);
                    }
                } else if side.drop.is_some() || (energy > 0 && d <= flow) {
                    go = Some(Move { to: x + dir * d, from_top: false, releases: side.drop.is_some() });
                }
            }
        } else if side.free > 0 {
            // Free cells next to this cell: move up to `flow` cells, and stop at a place to fall.
            let reach = side.drop.map_or(side.free, |d| d.min(side.free));
            if pressed {
                let d = reach.min(push_range(liquid_depth(h, x, y), flow));
                go = Some(Move { to: x + dir * d, from_top: true, releases: true });
            } else if side.drop.is_some() || energy > 0 {
                go = Some(Move { to: x + dir * reach, from_top: false, releases: side.drop.is_some() });
            }
        }
        if let Some(mv) = go {
            if sticky {
                h.keep_awake(x, y);
                return false;
            }
            // A momentum move off a liquid surface may lose momentum. A move that releases energy
            // (a place to fall, pressure) gives at least momentum 1.
            let mut e = energy;
            if e > 0 && !mv.releases && !side.open && !h.rng.chance_u16(keep) {
                e -= 1;
            }
            if mv.releases && keep > 0 {
                e = e.max(1);
            }
            if mv.from_top {
                // Pressure: the top cell of this column goes to the free cell, and the column
                // gets one cell shorter. (The same as this cell moving and the column sinking, but
                // the column does not start to fall.)
                let top = column_top(h, x, y, m);
                h.set_motion(x, top, flow_bits(dir, e.max(1)));
                h.swap(x, top, mv.to, y);
                if top != y {
                    wake_row(h, x, top);
                    opened(h, x, top, true);
                }
            } else {
                h.set_motion(x, y, flow_bits(dir, e));
                h.swap(x, y, mv.to, y);
            }
            return true;
        }
        // Blocked this way: bounce (turn around and lose one level of momentum).
        energy = energy.saturating_sub(1);
        dir = -dir;
    }
    h.set_motion(x, y, flow_bits(dir, 0));
    // A calm top cell near the end of a top row may still have a place to fall farther away than
    // the look-ahead: the level pass looks for it.
    if levels && on_liquid && !pressed && h.mat(x, y - 1).is_air() && (saw_rim || open_start(h, x, y, 1) || open_start(h, x, y, -1)) {
        let p = h.world_pos(x, y);
        h.levels.push(p);
    }
    false
}

/// True if the cell next to (x, y) in direction `dir` is air on top of liquid: the open surface
/// of a liquid, where a top cell can walk.
#[inline(always)]
fn open_start(h: &Hood, x: i32, y: i32, dir: i32) -> bool {
    h.mat(x + dir, y).is_air() && is_liquid(h, x + dir, y + 1) && h.inside(x + dir, y)
}

/// The level pass for one cell (see `schedule`): a calm liquid cell on top of liquid, near the end
/// of a top row, looks along its row on both sides (first through its own row, at most
/// `LOOK_AHEAD` cells, then along the open surface: air on top of liquid) for the nearest place
/// to fall (air on top of air), however far away.
/// - If the place is within the reach of the job, the cell goes there (one row lower if that row
///   is still in this chunk row).
/// - Else, if the cell is at the end of its row, it walks along the open surface as far as the
///   reach allows.
///
/// `far(x, y)` gives the material of a world cell. Returns true if the cell moved.
///
/// Every move goes toward a real place to fall, so the liquid still comes to rest; and it rests
/// flat, because a top row rests only when no place to fall is left on its surface.
/// The cell leaves a place to fall for the row above. The top cells at the ends of that row may
/// be asleep and far away, so their world positions are added to `wake`.
pub fn level(h: &mut Hood, x: i32, y: i32, far: &impl Fn(i32, i32) -> MaterialId, wake: &mut Vec<CellPos>) -> bool {
    let m = h.mat(x, y);
    let mi = m.index();
    if h.mats.phase[mi] != Phase::Liquid || h.mats.viscosity[mi] >= 0.5 {
        return false;
    }
    let motion = h.motion(x, y);
    if motion & (MOTION_SPEED | MOTION_MOMENTUM) != 0 || !h.mat(x, y - 1).is_air() || !is_liquid(h, x, y + 1) {
        return false;
    }
    let p = h.world_pos(x, y);
    // (direction, distance to the place to fall, true if the first cell is free)
    let mut best: Option<(i32, i32, bool)> = None;
    for dir in [1, -1] {
        let open = open_start(h, x, y, dir);
        let mut through = !open;
        for d in 1..=MAX_LEVEL_SCAN {
            if best.is_some_and(|(_, bd, _)| d >= bd) {
                break;
            }
            let (wx, t) = (p.x + dir * d, far(p.x + dir * d, p.y));
            if through {
                if t == m && d < LOOK_AHEAD {
                    continue;
                }
                through = false;
            }
            if !t.is_air() {
                break;
            }
            let b = far(wx, p.y + 1);
            if b.is_air() || matches!(h.mats.phase[b.index()], Phase::Gas | Phase::Fire) {
                best = Some((dir, d, open));
                break;
            }
            if h.mats.phase[b.index()] != Phase::Liquid {
                break;
            }
        }
    }
    let Some((dir, d, open)) = best else { return false };
    // Farthest distance in this direction that stays within the reach of the job.
    let reach = if dir > 0 { 64 + LOOK_AHEAD - x } else { x + LOOK_AHEAD + 1 };
    let (tx, ty) = if d <= reach {
        let tx = x + dir * d;
        let lower = y + 1 < 64 && h.mat(tx, y + 1).is_air();
        (tx, if lower { y + 1 } else { y })
    } else if open {
        (x + dir * reach, y)
    } else {
        return false;
    };
    h.set_motion(x, y, flow_bits(dir, 0));
    h.swap(x, y, tx, ty);
    wake_row(h, x, y);
    wake_row(h, tx, ty);
    wake_row_ends(h.mats, p, far, wake);
    true
}

/// A top liquid cell left `p` (world position): `p` is now air under air, a place to fall for the
/// row above. The top cells at the ends of that row may be far away and asleep, so this looks
/// along the open surface of the row above (air on top of liquid) on both sides and adds the
/// first liquid cell on each side to `wake`.
pub fn wake_row_ends(mats: &MaterialTable, p: CellPos, far: &impl Fn(i32, i32) -> MaterialId, wake: &mut Vec<CellPos>) {
    if !far(p.x, p.y).is_air() || !far(p.x, p.y - 1).is_air() {
        return;
    }
    for side in [1, -1] {
        for d in 1..=MAX_LEVEL_SCAN {
            let wx = p.x + side * d;
            let t = far(wx, p.y - 1);
            if !t.is_air() {
                if mats.phase[t.index()] == Phase::Liquid {
                    wake.push(CellPos::new(wx, p.y - 1));
                }
                break;
            }
            if mats.phase[far(wx, p.y).index()] != Phase::Liquid {
                break;
            }
        }
    }
}


/// A sideways move of a liquid cell.
struct Move {
    /// Target column (the cell stays on its row).
    to: i32,
    /// Move the top cell of this cell's column (pressure), not the cell itself.
    from_top: bool,
    /// The move releases energy (a place to fall, or pressure): the cell gets momentum.
    releases: bool,
}

/// What a liquid cell sees on its row in one direction.
struct Side {
    /// The next cell is the same liquid.
    through: bool,
    /// Through the same liquid: distance to the first free cell.
    rim: Option<i32>,
    /// Not through: the number of free cells next to this cell, at most `flow`.
    free: i32,
    /// Distance to the first free cell with air below it (a place to fall), within the look-ahead.
    drop: Option<i32>,
    /// Every free cell seen rests on liquid (for the walk).
    open: bool,
    /// A heavier liquid (this one) can push under the lighter liquid next to it.
    heavier_under: bool,
}

/// Look along row `y` in direction `dir`, up to `look` cells: first through cells of the same
/// liquid (if the next cell is one), then through free cells, until an obstacle.
#[inline]
fn look_side(h: &mut Hood, x: i32, y: i32, m: MaterialId, dir: i32, look: i32, on_liquid: bool) -> Side {
    let mi = m.index();
    let flow = (h.mats.flow[mi] as i32).clamp(1, MAX_FLOW);
    let mut side = Side { through: false, rim: None, free: 0, drop: None, open: on_liquid, heavier_under: false };
    let first = h.mat(x + dir, y);
    if first == m {
        side.through = true;
    } else if !passable(h, first) || !h.inside(x + dir, y) {
        // A heavier liquid with more of itself on top pushes under a lighter one next to it.
        // (Without the weight on top, the swap gains nothing and the two mix forever.)
        side.heavier_under = h.mats.phase[first.index()] == Phase::Liquid
            && h.mats.density[first.index()] < h.mats.density[mi]
            && h.mat(x, y - 1) == m
            && h.rng.chance(0.3);
        side.open = false;
        return side;
    }
    for d in 1..=look {
        let tx = x + dir * d;
        let t = h.mat(tx, y);
        if t == m && side.rim.is_none() && side.free == 0 {
            continue;
        }
        if !passable(h, t) || !h.inside(tx, y) {
            break;
        }
        if side.through {
            side.rim.get_or_insert(d);
        } else if d <= flow {
            side.free = d;
        }
        let b = h.mat(tx, y + 1);
        if passable(h, b) && h.inside(tx, y + 1) {
            side.drop = Some(d);
            break;
        }
        if h.mats.phase[b.index()] != Phase::Liquid {
            side.open = false;
        }
    }
    if side.through {
        side.open = false;
    }
    side
}

/// Number of liquid cells straight above (x, y), at most `LOOK_AHEAD`. The pressure on the cell.
#[inline]
fn liquid_depth(h: &Hood, x: i32, y: i32) -> i32 {
    (1..=LOOK_AHEAD).find(|&k| !is_liquid(h, x, y - k)).map_or(LOOK_AHEAD, |k| k - 1)
}

/// The top cell of the run of the same liquid from (x, y) up. If the run is taller than
/// `LOOK_AHEAD`, the cell itself (the column then sinks as it falls into the gap).
#[inline]
fn column_top(h: &Hood, x: i32, y: i32, m: MaterialId) -> i32 {
    for k in 1..=LOOK_AHEAD {
        if h.mat(x, y - k) != m {
            return y - k + 1;
        }
    }
    y
}

/// How far pressure pushes a liquid out sideways in one tick: about the square root of the depth
/// (the speed of water that runs out of a hole grows like that), at most `flow`.
#[inline(always)]
fn push_range(depth: i32, flow: i32) -> i32 {
    (1 + (depth as f32).sqrt() as i32 * flow / 3).clamp(1, flow)
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
