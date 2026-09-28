//! Tests of the construction rules of the main thread: drag lines, rotation and flip.

use super::*;
use std::sync::{Arc, OnceLock};

fn content() -> Arc<Content> {
    static C: OnceLock<Arc<Content>> = OnceLock::new();
    C.get_or_init(|| Arc::new(Content::load_default().unwrap())).clone()
}

fn line(id: &str, start: TilePos, rotation: u8) -> DragLine {
    let c = content();
    let kind = c.factory.building(id).unwrap();
    DragLine::new(7, kind, c.factory.building_def(kind), start, rotation, false)
}

#[test]
fn a_drag_line_follows_the_first_direction_and_never_repeats_a_tile() {
    let mut d = line("wood_wall", TilePos::new(10, 5), 0);
    assert_eq!(d.advance(TilePos::new(10, 5)), DragStep::default(), "no move: nothing new");
    // The first move goes right (a little down too): the line is sideways, to the right.
    let s = d.advance(TilePos::new(13, 6));
    assert_eq!(d.dir, Some(Dir::Right));
    assert_eq!(s.place, vec![TilePos::new(11, 5), TilePos::new(12, 5), TilePos::new(13, 5)]);
    // Back toward the start and past it: nothing new, and nothing twice.
    assert!(d.advance(TilePos::new(12, 9)).place.is_empty());
    assert!(d.advance(TilePos::new(4, 5)).place.is_empty());
    assert_eq!(d.advance(TilePos::new(15, 2)).place, vec![TilePos::new(14, 5), TilePos::new(15, 5)]);
    let mut all = d.sent.clone();
    all.dedup();
    assert_eq!(all.len(), d.sent.len());
    assert_eq!(d.sent.len(), 6);
}

#[test]
fn a_drag_line_goes_up_and_steps_by_the_footprint() {
    // A 2 × 2 workbench: each step is two tiles.
    let mut d = line("workbench", TilePos::new(0, 0), 0);
    assert_eq!(d.size, (2, 2));
    assert!(d.advance(TilePos::new(0, -1)).place.is_empty(), "one tile is less than a footprint");
    assert_eq!(d.dir, Some(Dir::Up));
    assert_eq!(d.advance(TilePos::new(1, -5)).place, vec![TilePos::new(0, -2), TilePos::new(0, -4)]);
}

#[test]
fn belts_face_the_drag_direction() {
    // A belt in the hand faces right; the drag goes left.
    let mut d = line("wood_belt", TilePos::new(0, 0), 0);
    let s = d.advance(TilePos::new(-2, 0));
    assert_eq!(s.turn_start, Some((2, false)), "the first belt turns to face left");
    assert_eq!(s.place.len(), 2);
    assert_eq!((d.rotation, d.flip), (2, false));
    // A belt that already faces the drag direction does not turn.
    let mut d = line("wood_belt", TilePos::new(0, 0), 0);
    assert_eq!(d.advance(TilePos::new(3, 0)).turn_start, None);
    // An up or down drag keeps the rotation (belts face only sideways).
    let mut d = line("wood_belt", TilePos::new(0, 0), 2);
    let s = d.advance(TilePos::new(0, 2));
    assert_eq!((s.turn_start, d.rotation), (None, 2));
}

#[test]
fn a_stopped_line_places_nothing_more() {
    let mut d = line("wood_wall", TilePos::new(0, 0), 0);
    d.advance(TilePos::new(2, 0));
    d.stop = Some((TilePos::new(2, 0), "Blocked by stone: dig first".into()));
    assert!(d.advance(TilePos::new(6, 0)).place.is_empty());
}

#[test]
fn rotation_rules() {
    let c = content();
    let def = |id: &str| c.factory.building_def(c.factory.building(id).unwrap());
    // Belts take half turns; other buildings quarter turns.
    assert_eq!(next_rotation(def("wood_belt"), 0, false, false), 2);
    assert_eq!(next_rotation(def("wood_belt"), 2, false, false), 0);
    assert_eq!(next_rotation(def("workbench"), 3, false, false), 0);
    assert_eq!(next_rotation(def("workbench"), 0, true, false), 3);
    // A placed building that is not square turns by half turns (its footprint stays).
    let stamp = def("stamp_mill");
    assert_ne!(stamp.size.0, stamp.size.1);
    assert_eq!(next_rotation(stamp, 0, false, true), 2);
    assert_eq!(next_rotation(stamp, 0, false, false), 1);
    assert_eq!(turned_size(stamp, 1), (stamp.size.1, stamp.size.0));
}

#[test]
fn only_buildings_with_a_mirror_image_flip() {
    let c = content();
    let def = |id: &str| c.factory.building_def(c.factory.building(id).unwrap());
    // The crusher has a pipe on the left: its mirror image differs.
    assert!(can_flip(def("steam_crusher")));
    // A crate's bulk input is on the left, so its mirror image moves the port to the right.
    assert!(can_flip(def("crate")));
    assert!(!can_flip(def("wood_wall")));
    assert!(!can_flip(def("wood_belt")));
}

#[test]
fn each_building_kind_keeps_its_rotation() {
    let c = content();
    let (crusher, workbench) = (c.factory.building("steam_crusher").unwrap(), c.factory.building("workbench").unwrap());
    let mut b = Construct::default();
    b.rotate(&c, crusher, false);
    b.rotate(&c, crusher, false);
    assert!(b.flip(&c, crusher));
    assert_eq!(b.transform(crusher), (2, true));
    assert_eq!(b.transform(workbench), (0, false), "another kind has its own rotation");
    assert!(b.flip(&c, c.factory.building("crate").unwrap()));
    // The pipette recipe is only for its own kind.
    let r = foundry_core::RecipeId(0);
    b.hand_recipe = Some((crusher, r));
    assert_eq!(b.recipe_for(crusher), Some(r));
    assert_eq!(b.recipe_for(workbench), None);
    assert!(b.next_action() < b.next_action());
}

#[test]
fn the_footprint_is_centered_on_the_mouse() {
    assert_eq!(footprint_at(CellPos::new(100, 100), (1, 1)), TilePos::new(12, 12));
    // A 2 × 2 footprint: the mouse is near its middle.
    assert_eq!(footprint_at(CellPos::new(100, 100), (2, 2)), TilePos::new(12, 12));
    assert_eq!(footprint_at(CellPos::new(95, 95), (2, 2)), TilePos::new(11, 11));
}
