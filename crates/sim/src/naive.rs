//! Milestone 0 movement: one thread, whole world, simple rules.
//! It exists so that other crates can be built against a working simulation.
//! Milestone 1 replaces it with the parallel chunk update (see docs/design/03-technical-design.md section 6).

use crate::chunk::FLAG_PARITY;
use crate::world::World;
use foundry_content::{MaterialTable, Phase};
use foundry_core::{CHUNK_SIZE, CellPos, MaterialId, Rng};

pub fn tick(world: &mut World, mats: &MaterialTable, tick: u64, seed: u64, stamp: u64) {
    let parity = (tick & 1) as u8;
    let mut rng = Rng::for_chunk(seed, tick, foundry_core::ChunkPos::new(0, 0), 0x6e61);
    let left_to_right = tick & 1 == 0;
    let w = world.width_chunks;
    for y in (0..world.height_cells()).rev() {
        let cy = y / CHUNK_SIZE;
        for i in 0..w {
            let cx = if left_to_right { i } else { w - 1 - i };
            if world.chunks[(cy * w + cx) as usize].is_none() {
                continue;
            }
            for j in 0..CHUNK_SIZE {
                let lx = if left_to_right { j } else { CHUNK_SIZE - 1 - j };
                update_cell(world, mats, CellPos::new(cx * CHUNK_SIZE + lx, y), parity, &mut rng, stamp);
            }
        }
    }
}

#[inline]
fn slot(world: &World, p: CellPos) -> (usize, usize) {
    (world.chunk_index(p.chunk()), p.local_index())
}

fn update_cell(world: &mut World, mats: &MaterialTable, p: CellPos, parity: u8, rng: &mut Rng, stamp: u64) {
    let (ci, li) = slot(world, p);
    let ch = world.chunks[ci].as_mut().unwrap();
    let m = MaterialId(ch.mat[li]);
    if m.is_air() || ch.flags[li] & FLAG_PARITY == parity {
        return;
    }
    // Fading materials.
    if mats.life[m.index()].is_some() {
        if ch.life[li] <= 1 {
            ch.mat[li] = mats.decay_into[m.index()].0;
            ch.life[li] = 0;
            ch.flags[li] = (ch.flags[li] & !FLAG_PARITY) | parity;
            ch.version = stamp;
            return;
        }
        ch.life[li] -= 1;
    }
    let phase = mats.phase[m.index()];
    let moves: &[(i32, i32)] = match phase {
        Phase::Powder | Phase::Liquid => &[(0, 1)],
        Phase::Gas if mats.density[m.index()] < 1.2 => &[(0, -1)],
        Phase::Gas => &[(0, 1)],
        Phase::Fire => &[(0, -1)],
        _ => return,
    };
    for &(dx, dy) in moves {
        if try_swap(world, mats, p, p.offset(dx, dy), m, parity, stamp) {
            return;
        }
    }
    let first = if rng.coin() { 1 } else { -1 };
    match phase {
        Phase::Powder => {
            if rng.chance(mats.friction[m.index()]) {
                return;
            }
            for dx in [first, -first] {
                if try_swap(world, mats, p, p.offset(dx, 1), m, parity, stamp) {
                    return;
                }
            }
        }
        Phase::Liquid => {
            for dx in [first, -first] {
                if try_swap(world, mats, p, p.offset(dx, 1), m, parity, stamp) {
                    return;
                }
            }
            let flow = mats.flow[m.index()] as i32;
            for dir in [first, -first] {
                let mut best = None;
                for d in 1..=flow {
                    let q = p.offset(dir * d, 0);
                    if !is_free(world, mats, q) {
                        break;
                    }
                    best = Some(q);
                    if is_free(world, mats, q.offset(0, 1)) {
                        break;
                    }
                }
                if let Some(q) = best {
                    if try_swap(world, mats, p, q, m, parity, stamp) {
                        return;
                    }
                }
            }
        }
        Phase::Gas | Phase::Fire => {
            let dy = if phase == Phase::Gas && mats.density[m.index()] >= 1.2 { 1 } else { -1 };
            for dx in [first, -first] {
                if try_swap(world, mats, p, p.offset(dx, dy), m, parity, stamp) {
                    return;
                }
            }
            if phase == Phase::Gas {
                let _ = try_swap(world, mats, p, p.offset(first, 0), m, parity, stamp);
            }
        }
        _ => {}
    }
}

#[inline]
fn is_free(world: &World, mats: &MaterialTable, q: CellPos) -> bool {
    let t = world.mat(q);
    t.is_air() || (world.in_bounds(q) && mats.phase[t.index()] == Phase::Gas)
}

/// Move the cell at `p` into `q` if the material there gives way. Swaps all cell data.
fn try_swap(world: &mut World, mats: &MaterialTable, p: CellPos, q: CellPos, m: MaterialId, parity: u8, stamp: u64) -> bool {
    if !world.in_bounds(q) {
        return false;
    }
    let t = world.mat(q);
    let (mp, tp) = (mats.phase[m.index()], mats.phase[t.index()]);
    let (md, td) = (mats.density[m.index()], mats.density[t.index()]);
    let down = q.y > p.y;
    let gives_way = t.is_air()
        || match (mp, tp) {
            (Phase::Powder | Phase::Liquid | Phase::Fire, Phase::Gas) => true,
            (Phase::Powder | Phase::Liquid, Phase::Liquid) => down && md > td,
            (Phase::Gas, Phase::Gas) => t != m && if q.y < p.y { md < td } else if down { md > td } else { false },
            _ => false,
        };
    if !gives_way {
        return false;
    }
    let (ca, la) = slot(world, p);
    let (cb, lb) = slot(world, q);
    if world.chunks[cb].is_none() {
        world.chunk_mut(q.chunk());
    }
    swap_cells(world, (ca, la), (cb, lb), parity, stamp);
    true
}

fn swap_cells(world: &mut World, (ca, la): (usize, usize), (cb, lb): (usize, usize), parity: u8, stamp: u64) {
    macro_rules! swap_arrays {
        ($a:expr, $b:expr) => {{
            let (a, b) = ($a, $b);
            for (fa, fb) in [(&mut a.flags[la], &mut b.flags[lb])] {
                *fa = (*fa & !FLAG_PARITY) | parity;
                *fb = (*fb & !FLAG_PARITY) | parity;
            }
            std::mem::swap(&mut a.mat[la], &mut b.mat[lb]);
            std::mem::swap(&mut a.temp[la], &mut b.temp[lb]);
            std::mem::swap(&mut a.shade[la], &mut b.shade[lb]);
            std::mem::swap(&mut a.life[la], &mut b.life[lb]);
            std::mem::swap(&mut a.motion[la], &mut b.motion[lb]);
            a.version = stamp;
            b.version = stamp;
        }};
    }
    if ca == cb {
        let c = world.chunks[ca].as_mut().unwrap();
        c.flags[la] = (c.flags[la] & !FLAG_PARITY) | parity;
        c.flags[lb] = (c.flags[lb] & !FLAG_PARITY) | parity;
        c.mat.swap(la, lb);
        c.temp.swap(la, lb);
        c.shade.swap(la, lb);
        c.life.swap(la, lb);
        c.motion.swap(la, lb);
        c.version = stamp;
    } else {
        let (lo, hi) = if ca < cb { (ca, cb) } else { (cb, ca) };
        let (left, right) = world.chunks.split_at_mut(hi);
        let (x, y) = (left[lo].as_mut().unwrap(), right[0].as_mut().unwrap());
        if ca < cb { swap_arrays!(x, y) } else { swap_arrays!(y, x) }
    }
}
