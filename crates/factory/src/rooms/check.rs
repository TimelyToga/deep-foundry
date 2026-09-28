//! Find the room of a controller and check it.
//!
//! 1. The wall: all room parts (wall blocks, hatches, controllers, bellows) that touch the
//!    controller, also at corners, up to `WALL_LIMIT` tiles. The box around them is the most
//!    that the room can be.
//! 2. The start: the tile next to the controller that is inside the box and not a room part.
//! 3. The fill: all tiles that can be reached from the start without crossing a room part, also
//!    at corners (cells move at corners too). A fill tile on the edge of the box is a hole.
//! 4. The wall around the fill must be of the right blocks, whole, with one controller and at
//!    least one hatch.

use super::{Hatch, Shape};
use crate::buildings::Buildings;
use crate::geometry::{neighbor_tile, opposite};
use foundry_content::{Building as BuildingDef, Content, Layer, Side};
use foundry_core::{BuildingId, CellPos, TILE_SIZE, TilePos};
use std::collections::{HashSet, VecDeque};

/// The most wall tiles the search looks at.
pub const WALL_LIMIT: usize = 1024;

const SIDES: [Side; 4] = [Side::Down, Side::Left, Side::Right, Side::Up];
const AROUND: [(i32, i32); 8] = [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)];

/// What is wrong with a room.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// The controller is not in the wall of a room (no inside tile next to it).
    NotInWall,
    /// The inside reaches the outside here.
    Hole { at: TilePos },
    /// More inside tiles than the limit. `tiles` is a lower bound when the search stopped.
    TooBig { tiles: u32, limit: u32 },
    /// A wall block that cannot hold the heat of this room.
    WrongWall { at: TilePos, name: String },
    /// A wall block that lost cells.
    BrokenWall { at: TilePos },
    /// A second controller in the wall.
    TwoControllers { at: TilePos },
    /// No hatch in the wall.
    NoHatch,
}

/// "2 tiles left and 1 tile up from the controller".
fn relative(at: TilePos, from: TilePos) -> String {
    let (dx, dy) = (at.x - from.x, at.y - from.y);
    let part = |n: i32, neg: &str, pos: &str| {
        let unit = if n.abs() == 1 { "tile" } else { "tiles" };
        format!("{} {unit} {}", n.abs(), if n < 0 { neg } else { pos })
    };
    match (dx, dy) {
        (0, 0) => "at the controller".into(),
        (0, _) => format!("{} from the controller", part(dy, "up", "down")),
        (_, 0) => format!("{} from the controller", part(dx, "left", "right")),
        _ => format!("{} and {} from the controller", part(dx, "left", "right"), part(dy, "up", "down")),
    }
}

impl Problem {
    /// A sentence for the player. `controller` is the controller type, `at` its tile.
    pub fn text(&self, content: &Content, controller: &BuildingDef, at: TilePos) -> String {
        match self {
            Problem::NotInWall => "Put the controller in the wall of a closed room: the room on one side, the outside on the other.".into(),
            Problem::Hole { at: h } => format!("The room has a hole {}. Close it with a wall block.", relative(*h, at)),
            Problem::TooBig { tiles, limit } => {
                format!("Too big: the room has {tiles} or more tiles inside. The limit is {limit}.")
            }
            Problem::WrongWall { at: w, name } => format!(
                "Wrong wall block {}: {name}. This room needs {}.",
                relative(*w, at),
                super::wall_names(content, controller)
            ),
            Problem::BrokenWall { at: w } => format!("The wall is broken {}. Remove it and build it again.", relative(*w, at)),
            Problem::TwoControllers { at: w } => format!("A second controller is in the wall {}. A room has one controller.", relative(*w, at)),
            Problem::NoHatch => "No hatch: put a hatch in the wall. Hatches take items in and give products out.".into(),
        }
    }

    /// A few words for the status line of the building window.
    pub fn short(&self) -> &'static str {
        match self {
            Problem::NotInWall => "Not in the wall of a room",
            Problem::Hole { .. } => "The room has a hole",
            Problem::TooBig { .. } => "The room is too big",
            Problem::WrongWall { .. } => "Wrong wall block",
            Problem::BrokenWall { .. } => "A wall is broken",
            Problem::TwoControllers { .. } => "Two controllers",
            Problem::NoHatch => "No hatch",
        }
    }

    /// The tile to mark red.
    pub fn tile(&self) -> Option<TilePos> {
        match self {
            Problem::Hole { at } | Problem::WrongWall { at, .. } | Problem::BrokenWall { at } | Problem::TwoControllers { at } => Some(*at),
            _ => None,
        }
    }
}

/// True for buildings that can be part of a room wall.
pub fn is_room_part(def: &BuildingDef) -> bool {
    matches!(def.kind.as_str(), "room_wall" | "room_port" | "room_controller" | "bellows" | "blower")
}

/// The room part on a tile, if any.
fn part_at<'a>(content: &'a Content, buildings: &Buildings, tile: TilePos) -> Option<(BuildingId, &'a BuildingDef)> {
    let id = buildings.at_tile(tile, Layer::Front)?;
    let def = content.factory.building_def(buildings.get(id)?.kind);
    is_room_part(def).then_some((id, def))
}

fn step(t: TilePos, (dx, dy): (i32, i32)) -> TilePos {
    TilePos::new(t.x + dx, t.y + dy)
}

/// Find and check the room of a controller.
pub fn check(content: &Content, buildings: &Buildings, controller: BuildingId) -> Result<Shape, Problem> {
    let ctrl = buildings.get(controller).ok_or(Problem::NotInWall)?;
    let def = content.factory.building_def(ctrl.kind);
    let limit = def.param("max_tiles", super::DEFAULT_MAX_TILES).max(1.0) as usize;
    let own: Vec<TilePos> = ctrl.tiles().collect();

    // 1. The wall and its box.
    let mut wall: HashSet<TilePos> = own.iter().copied().collect();
    let mut queue: VecDeque<TilePos> = own.iter().copied().collect();
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    while let Some(t) = queue.pop_front() {
        (x0, y0, x1, y1) = (x0.min(t.x), y0.min(t.y), x1.max(t.x), y1.max(t.y));
        if wall.len() >= WALL_LIMIT {
            continue;
        }
        for d in AROUND {
            let n = step(t, d);
            if !wall.contains(&n) && part_at(content, buildings, n).is_some() {
                wall.insert(n);
                queue.push_back(n);
            }
        }
    }
    let inside_box = |t: TilePos| t.x > x0 && t.x < x1 && t.y > y0 && t.y < y1;

    // 2. The start tile.
    let start = own
        .iter()
        .flat_map(|&t| SIDES.iter().map(move |&s| neighbor_tile(t, s)))
        .find(|&n| inside_box(n) && part_at(content, buildings, n).is_none())
        .ok_or(Problem::NotInWall)?;

    // 3. The fill. A closed room stays inside the box, so the box limits the search.
    let search_limit = limit.max(64) * 4;
    let mut seen: HashSet<TilePos> = HashSet::from([start]);
    let mut inside: Vec<TilePos> = vec![start];
    let mut k = 0;
    while k < inside.len() {
        let t = inside[k];
        k += 1;
        for d in AROUND {
            let n = step(t, d);
            if seen.contains(&n) || part_at(content, buildings, n).is_some() {
                continue;
            }
            if !inside_box(n) {
                return Err(Problem::Hole { at: n });
            }
            seen.insert(n);
            inside.push(n);
            if inside.len() > search_limit {
                return Err(Problem::TooBig { tiles: inside.len() as u32, limit: limit as u32 });
            }
        }
    }
    if inside.len() > limit {
        return Err(Problem::TooBig { tiles: inside.len() as u32, limit: limit as u32 });
    }
    inside.sort_unstable_by_key(|t| (t.y, t.x));

    // 4. The wall around the fill.
    let mut walls: Vec<TilePos> = vec![];
    for &t in &inside {
        for d in AROUND {
            let n = step(t, d);
            if !seen.contains(&n) && !walls.contains(&n) {
                walls.push(n);
            }
        }
    }
    walls.sort_unstable_by_key(|t| (t.y, t.x));
    let mut hatches = vec![];
    let mut done: Vec<BuildingId> = vec![];
    for &w in &walls {
        let Some((id, wdef)) = part_at(content, buildings, w) else { continue };
        if done.contains(&id) {
            continue;
        }
        done.push(id);
        let b = buildings.get(id).expect("a building on the tile");
        if b.lost_cells > 0 {
            return Err(Problem::BrokenWall { at: w });
        }
        match wdef.kind.as_str() {
            "room_wall" if wdef.max_temp < def.max_temp => return Err(Problem::WrongWall { at: w, name: wdef.name.clone() }),
            "room_controller" if id != controller => return Err(Problem::TwoControllers { at: w }),
            "room_port" => hatches.push(Hatch { id, tile: w, outer: outer_side(content, buildings, w, &seen) }),
            _ => {}
        }
    }
    if hatches.is_empty() {
        return Err(Problem::NoHatch);
    }

    // The open tiles and the fire bed.
    let open: Vec<TilePos> = inside.iter().copied().filter(|&t| buildings.at_tile(t, Layer::Front).is_none()).collect();
    let mut bed = vec![];
    for &t in &open {
        if !seen.contains(&neighbor_tile(t, Side::Down)) {
            let o = t.origin();
            bed.extend((0..TILE_SIZE).map(|x| CellPos::new(o.x + x, o.y + TILE_SIZE - 1)));
        }
    }
    Ok(Shape { inside, open, bed, hatches, walls })
}

/// The side of a hatch that faces out of the room: the side opposite the room, or else any side
/// whose tile is not inside and not a room part.
fn outer_side(content: &Content, buildings: &Buildings, tile: TilePos, inside: &HashSet<TilePos>) -> Option<Side> {
    let free = |s: Side| {
        let n = neighbor_tile(tile, s);
        !inside.contains(&n) && part_at(content, buildings, n).is_none()
    };
    let room_side = SIDES.iter().copied().find(|&s| inside.contains(&neighbor_tile(tile, s)))?;
    let away = opposite(room_side);
    if free(away) {
        return Some(away);
    }
    SIDES.iter().copied().find(|&s| free(s))
}
