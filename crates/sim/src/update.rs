//! The update of one cell and of one chunk (technical design section 6.2).

use crate::chunk::{LocalRect, MOTION_SPEED};
use crate::hood::Hood;
use crate::{movement, react};
use foundry_content::Phase;

/// Update the cells of the center chunk inside `work`: rows from bottom to top.
pub fn update_chunk(h: &mut Hood, work: LocalRect, left_to_right: bool) {
    for y in (work.y0..work.y1).rev() {
        if left_to_right {
            for x in work.x0..work.x1 {
                update_cell(h, x, y);
            }
        } else {
            for x in (work.x0..work.x1).rev() {
                update_cell(h, x, y);
            }
        }
    }
}

/// The fall pass for one chunk: liquid cells in `work` that are already falling (fall speed
/// above 0) move straight down. Rows from bottom to top. All other cells (also falling powder,
/// to keep the cost low) wait for the normal passes.
/// Returns true if a cell moved.
pub fn fall_chunk(h: &mut Hood, work: LocalRect, left_to_right: bool) -> bool {
    let mut moved = false;
    // Only rows that may hold a falling cell (see `Chunk::falling_rows`), from the bottom up.
    let rows_in_work = (u64::MAX >> (64 - (work.y1 - work.y0))) << work.y0;
    let mut rows = h.center_falling_rows() & rows_in_work;
    while rows != 0 {
        let y = 63 - rows.leading_zeros() as i32;
        rows &= !(1u64 << y);
        let mut bits = h.center_row_falling(y, work.x0, work.x1);
        if bits == 0 && work.x0 == 0 && work.x1 == 64 {
            h.clear_center_falling_row(y);
        }
        while bits != 0 {
            let x = if left_to_right { bits.trailing_zeros() } else { 63 - bits.leading_zeros() } as i32;
            bits &= !(1u64 << x);
            // A cell that moved into this row in this pass is already updated.
            if h.is_updated(x, y) || h.motion(x, y) & MOTION_SPEED == 0 {
                continue;
            }
            let m = h.mat(x, y);
            if h.mats.phase[m.index()] == Phase::Liquid {
                moved |= movement::fall_only(h, x, y, Phase::Liquid);
            }
        }
    }
    moved
}

#[inline]
pub fn update_cell(h: &mut Hood, x: i32, y: i32) {
    let m = h.mat(x, y);
    if m.is_air() {
        return;
    }
    if h.is_updated(x, y) {
        // Moved here in this tick, or an old parity bit that matches by chance: check again next tick.
        h.keep_awake(x, y);
        return;
    }
    // Store this tick's bit on every visited cell. Without this, old bits that match by chance
    // keep large areas awake forever.
    h.set_updated(x, y);
    let phase = h.mats.phase[m.index()];

    // Materials that fade (smoke, fire).
    if h.mats.life[m.index()].is_some() {
        let life = h.life(x, y);
        if life <= 1 {
            let into = h.mats.decay_into[m.index()];
            h.replace(x, y, into, None);
            return;
        }
        h.set_life(x, y, life - 1);
        h.keep_awake(x, y);
    }

    match react::try_react(h, x, y, m) {
        react::Outcome::Changed => return,
        react::Outcome::KeepAwake => h.keep_awake(x, y),
        react::Outcome::None => {}
    }

    if phase != Phase::Solid {
        movement::try_move(h, x, y, m, phase);
    }
}
