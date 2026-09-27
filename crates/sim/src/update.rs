//! The update of one cell and of one chunk (technical design section 6.2).

use crate::chunk::{LocalRect, MOTION_SPEED};
use crate::hood::Hood;
use crate::{movement, react};
use foundry_content::Phase;
use foundry_core::{CellPos, MaterialId};

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

/// The fall pass for one chunk: cells in `work` that are already falling (fall speed above 0)
/// move straight down. Rows from bottom to top. All other cells wait for the normal passes.
/// Returns true if a cell moved.
pub fn fall_chunk(h: &mut Hood, work: LocalRect, left_to_right: bool) -> bool {
    let mut moved = false;
    for y in (work.y0..work.y1).rev() {
        if !h.center_row_falls(y, work.x0, work.x1) {
            continue;
        }
        for i in 0..work.x1 - work.x0 {
            let x = if left_to_right { work.x0 + i } else { work.x1 - 1 - i };
            if h.motion(x, y) & MOTION_SPEED == 0 || h.is_updated(x, y) {
                continue;
            }
            let m = h.mat(x, y);
            let phase = h.mats.phase[m.index()];
            if matches!(phase, Phase::Powder | Phase::Liquid) {
                moved |= movement::fall_only(h, x, y, phase);
            }
        }
    }
    moved
}

/// The level pass for the collected cells of one chunk (world positions, all in the hood's center
/// chunk). `(p, false)`: a cell that may walk (`movement::level`). `(p, true)`: a place that a
/// top cell left (`movement::wake_row_ends`). `far` reads the material of any world cell in this
/// chunk row and the rows next to it. Far cells to update in the next tick go to `wake`.
/// Returns true if a cell moved.
pub fn level_cells(h: &mut Hood, cells: &[(CellPos, bool)], far: &impl Fn(i32, i32) -> MaterialId, wake: &mut Vec<CellPos>) -> bool {
    let mut moved = false;
    for &(p, opened) in cells {
        if opened {
            movement::wake_row_ends(h.mats, p, far, wake);
        } else {
            moved |= movement::level(h, p.x - h.origin.x, p.y - h.origin.y, far, wake);
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
