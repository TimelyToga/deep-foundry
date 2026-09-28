//! Reading and writing world cells for buildings. Only the public `Simulation` API is used:
//! `cell` and `set_cell`.

use crate::geometry::{outside_cell, outside_cells};
use foundry_content::{Content, Phase, Side};
use foundry_core::{CellPos, CellRect, MaterialId, TILE_SIZE, TilePos};
use foundry_sim::Simulation;
use std::collections::{HashSet, VecDeque};

/// The phase of a material.
#[inline]
pub fn phase(content: &Content, m: MaterialId) -> Phase {
    content.materials.phase[m.index()]
}

/// Loose cells move on their own: powder, liquid, gas and fire.
#[inline]
pub fn is_loose(p: Phase) -> bool {
    matches!(p, Phase::Powder | Phase::Liquid | Phase::Gas | Phase::Fire)
}

/// Liquid or gas.
#[inline]
pub fn is_fluid(p: Phase) -> bool {
    matches!(p, Phase::Liquid | Phase::Gas)
}

/// Move a cell with its temperature. The old place becomes air.
pub fn move_cell(sim: &mut Simulation, from: CellPos, to: CellPos) {
    let c = sim.cell(from);
    sim.set_cell(to, c.material, Some(c.temperature));
    sim.set_cell(from, MaterialId::AIR, None);
}

/// All cells of a rectangle, row by row from the top.
pub fn rect_cells(r: CellRect) -> impl Iterator<Item = CellPos> {
    (r.y0..r.y1).flat_map(move |y| (r.x0..r.x1).map(move |x| CellPos::new(x, y)))
}

/// Find up to `need` air cells near some start cells. The search goes out from the start cells,
/// one step at a time, through cells that are not solid. It never enters `exclude`.
/// With `down: false` it moves only up and to the sides, so it finds cells above and beside.
/// It stops after `max_visit` cells. The result is in search order (nearest first).
pub fn find_free_cells(
    sim: &Simulation,
    content: &Content,
    starts: impl IntoIterator<Item = CellPos>,
    exclude: CellRect,
    down: bool,
    need: usize,
    max_visit: usize,
) -> Vec<CellPos> {
    let mut found = vec![];
    if need == 0 {
        return found;
    }
    let mut seen: HashSet<CellPos> = HashSet::new();
    let mut queue: VecDeque<CellPos> = VecDeque::new();
    for p in starts {
        if !exclude.contains(p) && seen.insert(p) {
            queue.push_back(p);
        }
    }
    let steps: &[(i32, i32)] = if down { &[(0, -1), (-1, 0), (1, 0), (0, 1)] } else { &[(0, -1), (-1, 0), (1, 0)] };
    while let Some(p) = queue.pop_front() {
        let m = sim.cell(p).material;
        let ph = phase(content, m);
        if ph == Phase::Solid {
            continue;
        }
        if m.is_air() {
            found.push(p);
            if found.len() >= need {
                break;
            }
        }
        if seen.len() >= max_visit {
            continue;
        }
        for &(dx, dy) in steps {
            let q = p.offset(dx, dy);
            if !exclude.contains(q) && seen.insert(q) {
                queue.push_back(q);
            }
        }
    }
    found
}

/// The cells just outside a rectangle on the left, right and top sides (not below).
/// The search for free cells above and to the sides starts here.
pub fn border_above_and_sides(r: CellRect) -> Vec<CellPos> {
    let mut v = Vec::with_capacity((r.width() + 2 * r.height()) as usize);
    for x in r.x0..r.x1 {
        v.push(CellPos::new(x, r.y0 - 1));
    }
    for y in (r.y0..r.y1).rev() {
        v.push(CellPos::new(r.x0 - 1, y));
        v.push(CellPos::new(r.x1, y));
    }
    v
}

/// Take cells from outside a port side, `depth` rows deep, nearest row first.
/// `accept` is called for each cell of an allowed phase; when it returns true, the cell is taken
/// (it becomes air). At most `max` cells are taken. Returns the number taken.
#[allow(clippy::too_many_arguments)]
pub fn take_from_side(
    sim: &mut Simulation,
    content: &Content,
    tile: TilePos,
    side: Side,
    depth: i32,
    max: u32,
    phases: &[Phase],
    mut accept: impl FnMut(MaterialId) -> bool,
) -> u32 {
    take_from_side_with_temperature(sim, content, tile, side, depth, max, phases, |material, _| accept(material))
}

/// Like take_from_side, but the accept function also receives the cell temperature.
#[allow(clippy::too_many_arguments)]
pub fn take_from_side_with_temperature(
    sim: &mut Simulation,
    content: &Content,
    tile: TilePos,
    side: Side,
    depth: i32,
    max: u32,
    phases: &[Phase],
    mut accept: impl FnMut(MaterialId, i16) -> bool,
) -> u32 {
    let mut taken = 0;
    for p in outside_cells(tile, side, depth) {
        if taken >= max {
            break;
        }
        let m = sim.cell(p).material;
        if m.is_air() || !phases.contains(&phase(content, m)) {
            continue;
        }
        if accept(m, sim.cell(p).temperature) {
            sim.set_cell(p, MaterialId::AIR, None);
            taken += 1;
        }
    }
    taken
}

/// Put up to `n` cells of `m` into air cells in the row that touches a port side, from the middle
/// out. `temperature: None` uses the material's own temperature. Returns the number placed.
pub fn put_to_side(sim: &mut Simulation, tile: TilePos, side: Side, m: MaterialId, n: u32, temperature: Option<i16>) -> u32 {
    let mut placed = 0;
    for along in crate::geometry::MIDDLE_OUT {
        if placed >= n {
            break;
        }
        let p = outside_cell(tile, side, 0, along);
        if sim.cell(p).material.is_air() {
            sim.set_cell(p, m, temperature);
            placed += 1;
        }
    }
    placed
}

/// True if at least one cell in the row that touches a port side is air.
pub fn side_has_room(sim: &Simulation, tile: TilePos, side: Side) -> bool {
    (0..TILE_SIZE).any(|a| sim.cell(outside_cell(tile, side, 0, a)).material.is_air())
}

/// Average temperature of the cells outside a port side, `depth` rows deep.
pub fn side_temperature(sim: &Simulation, tile: TilePos, side: Side, depth: i32) -> i16 {
    let mut sum: i64 = 0;
    let mut n: i64 = 0;
    for p in outside_cells(tile, side, depth) {
        sum += sim.cell(p).temperature as i64;
        n += 1;
    }
    (sum / n.max(1)) as i16
}

/// Raise the temperature of the cells on both sides of a port side (the touching row outside
/// and the edge row of the building) by `step`, up to `target`.
pub fn heat_side(sim: &mut Simulation, tile: TilePos, side: Side, target: i16, step: i16) {
    for along in 0..TILE_SIZE {
        let outside = outside_cell(tile, side, 0, along);
        // The edge cell of this tile on that side is one row back from the outside row.
        let inside = outside_cell(tile, side, -1, along);
        for p in [outside, inside] {
            let c = sim.cell(p);
            if c.temperature < target {
                let t = (c.temperature as i32 + step as i32).min(target as i32) as i16;
                sim.set_cell(p, c.material, Some(t));
            }
        }
    }
}

/// Put cells of the given materials into the world near `area`: first air cells inside the area
/// (from the bottom row up), then air cells around it. Returns the materials and counts that did
/// not fit (only if the search found no more room).
pub fn release_cells(
    sim: &mut Simulation,
    content: &Content,
    area: CellRect,
    cells: &[(MaterialId, u32)],
) -> Vec<(MaterialId, u32)> {
    let total: u32 = cells.iter().map(|c| c.1).sum();
    if total == 0 {
        return vec![];
    }
    let starts: Vec<CellPos> = (area.y0..area.y1).rev().flat_map(|y| (area.x0..area.x1).map(move |x| CellPos::new(x, y))).collect();
    let free = find_free_cells(sim, content, starts, CellRect::EMPTY, true, total as usize, 64 * 1024);
    let mut spots = free.into_iter();
    let mut left = vec![];
    for &(m, n) in cells {
        let mut placed = 0;
        for _ in 0..n {
            match spots.next() {
                Some(p) => {
                    sim.set_cell(p, m, None);
                    placed += 1;
                }
                None => break,
            }
        }
        if placed < n {
            left.push((m, n - placed));
        }
    }
    left
}
