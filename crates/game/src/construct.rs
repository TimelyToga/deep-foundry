//! Construction on the main thread (game design section 18): the rotation of the building in the
//! hand, drag lines, the remove button, the pipette and the alt mode. No window code here, so the
//! rules have plain unit tests.
//!
//! The factory checks and does the real work on the simulation thread (`host_build.rs`). This
//! module decides which commands to send.
//!
//! Rules:
//! - Each building kind keeps its own rotation and flip. The next building of that kind in the
//!   hand has them again.
//! - A drag places a line along the first direction that the mouse moves (left, right, up or
//!   down). Each step is one footprint long, so no tile gets two buildings. Belts in a sideways
//!   drag face the drag direction. The line stops at the first place that fails.
//! - Every press of a mouse button (and every key that changes buildings) is one "action". The
//!   undo stack keeps one entry for each action, so one Ctrl + Z takes back a whole drag line.

use foundry_content::{Building as BuildingDef, Content};
use foundry_core::{BuildingKindId, CellPos, RecipeId, TILE_SIZE, TilePos};
use foundry_factory::Transform;
use std::collections::HashMap;

/// The next rotation for R (or Shift + R when `back`). Belts face only left or right, so they
/// take half turns. `in_place`: a placed building turns where it stands, so a building that is
/// not square also takes half turns (its footprint must not change).
pub fn next_rotation(def: &BuildingDef, rotation: u8, back: bool, in_place: bool) -> u8 {
    let half = def.kind == "belt" || (in_place && def.size.0 != def.size.1);
    let step = if half { 2 } else { 1 };
    if back { (rotation + 4 - step) % 4 } else { (rotation + step) % 4 }
}

/// True if F changes this building: its mirror image has other ports. Belts are turned with R,
/// not flipped.
pub fn can_flip(def: &BuildingDef) -> bool {
    if def.kind == "belt" {
        return false;
    }
    let at = TilePos::new(0, 0);
    let ports = |flip: bool| {
        let mut v: Vec<_> = foundry_factory::buildings::placed_ports(def, at, Transform::new(0, flip))
            .into_iter()
            .map(|p| (p.kind, p.tile, p.side))
            .collect();
        v.sort_by_key(|(k, t, s)| (*k as u8, t.x, t.y, *s as u8));
        v
    };
    ports(false) != ports(true)
}

/// The size in tiles after a rotation.
pub fn turned_size(def: &BuildingDef, rotation: u8) -> (u8, u8) {
    Transform::new(rotation, false).size(def.size)
}

/// The top-left tile of a footprint of `size` tiles centered on the mouse cell.
pub fn footprint_at(mouse: CellPos, size: (u8, u8)) -> TilePos {
    let t = TILE_SIZE as f32;
    let x = (mouse.x as f32 / t - size.0 as f32 * 0.5 + 0.5).floor() as i32;
    let y = (mouse.y as f32 / t - size.1 as f32 * 0.5 + 0.5).floor() as i32;
    TilePos::new(x, y)
}

/// The building in the hand at the mouse, as the main thread sees it now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalGhost {
    pub request: crate::factory_host::GhostRequest,
    /// Size in tiles after the rotation.
    pub size: (u8, u8),
}

/// What the overlay draws for construction in one frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BuildView {
    /// The building in the hand at the mouse (not while a line is dragged).
    pub ghost: Option<LocalGhost>,
    /// The line that is dragged now.
    pub drag: Option<DragLine>,
    /// A building is in the hand: show the tile grid.
    pub grid: bool,
    /// Alt mode: recipe icons and belt directions.
    pub alt: bool,
    /// The mouse cell (`None` over the UI).
    pub mouse: Option<CellPos>,
    /// The remove button is down.
    pub removing: bool,
}

/// Keys and mouse buttons that change what a click does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    /// Ctrl, or Cmd on macOS.
    pub ctrl: bool,
}

/// The direction of a drag line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

impl Dir {
    /// One step in tiles, as (dx, dy).
    pub fn step(self) -> (i32, i32) {
        match self {
            Dir::Left => (-1, 0),
            Dir::Right => (1, 0),
            Dir::Up => (0, -1),
            Dir::Down => (0, 1),
        }
    }

    pub fn is_sideways(self) -> bool {
        matches!(self, Dir::Left | Dir::Right)
    }
}

/// A line of buildings that the player drags with the left button down.
#[derive(Debug, Clone, PartialEq)]
pub struct DragLine {
    /// The press that started it (the undo entry).
    pub action: u32,
    pub kind: BuildingKindId,
    /// Top-left tile of the first footprint (placed when the button went down).
    pub start: TilePos,
    /// Footprint size in tiles (after the rotation).
    pub size: (u8, u8),
    pub rotation: u8,
    pub flip: bool,
    pub recipe: Option<RecipeId>,
    /// Belts face the drag direction.
    pub is_belt: bool,
    /// Set by the first mouse move away from the start.
    pub dir: Option<Dir>,
    /// Footprints sent so far, in order. The first one is `start`.
    pub sent: Vec<TilePos>,
    /// The line stopped here, for this reason (a placement failed).
    pub stop: Option<(TilePos, String)>,
}

/// What a drag step asks for.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DragStep {
    /// New footprints to place, in order.
    pub place: Vec<TilePos>,
    /// The first belt must turn to face the drag direction: (rotation, flip).
    pub turn_start: Option<(u8, bool)>,
}

impl DragLine {
    pub fn new(action: u32, kind: BuildingKindId, def: &BuildingDef, start: TilePos, rotation: u8, flip: bool) -> Self {
        Self {
            action,
            kind,
            start,
            size: turned_size(def, rotation),
            rotation,
            flip,
            recipe: None,
            is_belt: def.kind == "belt",
            dir: None,
            sent: vec![start],
            stop: None,
        }
    }

    /// The mouse footprint is now at `at` (its top-left tile). Returns the new footprints on the
    /// line up to the mouse, in order. The line only grows: going back places nothing, and a
    /// footprint is never sent twice.
    pub fn advance(&mut self, at: TilePos) -> DragStep {
        let mut out = DragStep::default();
        if self.stop.is_some() {
            return out;
        }
        let (dx, dy) = (at.x - self.start.x, at.y - self.start.y);
        let dir = match self.dir {
            Some(d) => d,
            None => {
                if dx == 0 && dy == 0 {
                    return out;
                }
                let d = if dx.abs() >= dy.abs() {
                    if dx > 0 { Dir::Right } else { Dir::Left }
                } else if dy > 0 {
                    Dir::Down
                } else {
                    Dir::Up
                };
                self.dir = Some(d);
                if self.is_belt && d.is_sideways() {
                    let rotation = if d == Dir::Right { 0 } else { 2 };
                    if (rotation, false) != (self.rotation, self.flip) {
                        out.turn_start = Some((rotation, false));
                    }
                    self.rotation = rotation;
                    self.flip = false;
                }
                d
            }
        };
        let (sx, sy) = dir.step();
        // Distance along the line in tiles, and the footprint length along it.
        let (dist, len) = if dir.is_sideways() { (dx * sx, self.size.0 as i32) } else { (dy * sy, self.size.1 as i32) };
        let want = (dist.max(0) / len.max(1)) as usize;
        for k in self.sent.len()..=want {
            let k = k as i32;
            let p = TilePos::new(self.start.x + sx * len * k, self.start.y + sy * len * k);
            self.sent.push(p);
            out.place.push(p);
        }
        out
    }
}

/// The construction state of the main thread.
#[derive(Debug, Default)]
pub struct Construct {
    /// Rotation and flip of each building kind, kept for the next placement of that kind.
    transforms: HashMap<BuildingKindId, (u8, bool)>,
    /// New buildings of this kind get this recipe (set by the pipette).
    pub hand_recipe: Option<(BuildingKindId, RecipeId)>,
    /// The drag line while the left button is down with a building in the hand.
    pub drag: Option<DragLine>,
    /// The right button went down on a building and removes the buildings under the mouse path.
    /// The number of that press.
    pub removing: Option<u32>,
    /// Alt mode: recipe icons and belt arrows over the buildings.
    pub alt: bool,
    last_action: u32,
}

impl Construct {
    /// A new action number (for undo).
    pub fn next_action(&mut self) -> u32 {
        self.last_action += 1;
        self.last_action
    }

    /// The rotation and flip for the next building of this kind.
    pub fn transform(&self, kind: BuildingKindId) -> (u8, bool) {
        self.transforms.get(&kind).copied().unwrap_or((0, false))
    }

    pub fn set_transform(&mut self, kind: BuildingKindId, rotation: u8, flip: bool) {
        self.transforms.insert(kind, (rotation & 3, flip));
    }

    /// R (or Shift + R) with a building in the hand.
    pub fn rotate(&mut self, content: &Content, kind: BuildingKindId, back: bool) {
        let (r, f) = self.transform(kind);
        let r = next_rotation(content.factory.building_def(kind), r, back, false);
        self.set_transform(kind, r, f);
    }

    /// F with a building in the hand. Returns false if this building has no mirror image.
    pub fn flip(&mut self, content: &Content, kind: BuildingKindId) -> bool {
        if !can_flip(content.factory.building_def(kind)) {
            return false;
        }
        let (r, f) = self.transform(kind);
        self.set_transform(kind, r, !f);
        true
    }

    /// The recipe for a new building of this kind.
    pub fn recipe_for(&self, kind: BuildingKindId) -> Option<RecipeId> {
        self.hand_recipe.filter(|(k, _)| *k == kind).map(|(_, r)| r)
    }
}

#[cfg(test)]
#[path = "construct_tests.rs"]
mod tests;
