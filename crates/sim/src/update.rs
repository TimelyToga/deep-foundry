//! The update of one cell and of one chunk (technical design section 6.2).

use crate::chunk::LocalRect;
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
