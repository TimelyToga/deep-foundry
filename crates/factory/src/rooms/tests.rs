//! Unit tests of the room check, the hatch roles and the texts.

use super::*;
use crate::Factory;
use foundry_content::Content;
use foundry_core::TilePos;
use foundry_sim::{SimConfig, Simulation};
use std::sync::Arc;

fn setup() -> (Factory, Simulation) {
    let content = Arc::new(Content::load_default().expect("content loads"));
    let sim = Simulation::new(content.clone(), SimConfig::finite(2, 2, 1));
    (Factory::new(content), sim)
}

fn place(f: &mut Factory, sim: &mut Simulation, id: &str, x: i32, y: i32) -> BuildingId {
    let kind = f.content.factory.building(id).unwrap();
    f.place(kind, TilePos::new(x, y), 0, false, sim).unwrap()
}

/// Draw a room from text: `#` wall, `C` controller, `H` hatch, `.` inside or outside (air).
/// The top-left character is tile (1, 1). Returns the controller.
fn draw(f: &mut Factory, sim: &mut Simulation, rows: &[&str]) -> BuildingId {
    let mut ctrl = None;
    for (y, row) in rows.iter().enumerate() {
        for (x, ch) in row.chars().enumerate() {
            let (x, y) = (x as i32 + 1, y as i32 + 1);
            match ch {
                '#' => {
                    place(f, sim, "clay_brick_wall", x, y);
                }
                'C' => ctrl = Some(place(f, sim, "kiln_controller", x, y)),
                'H' => {
                    place(f, sim, "kiln_hatch", x, y);
                }
                _ => {}
            }
        }
    }
    ctrl.expect("a controller")
}

fn check_room(f: &Factory, ctrl: BuildingId) -> Result<Shape, Problem> {
    check::check(&f.content, &f.buildings, ctrl)
}

#[test]
fn a_closed_room_is_found_with_its_hatch_roles() {
    let (mut f, mut sim) = setup();
    let ctrl = draw(
        &mut f,
        &mut sim,
        &[
            "##H##", //
            "C...H", //
            "#...#", //
            "##H##",
        ],
    );
    let shape = check_room(&f, ctrl).unwrap();
    assert_eq!(shape.inside.len(), 6);
    assert_eq!(shape.open.len(), 6);
    // The bed is the bottom row of the 3 lowest inside tiles.
    assert_eq!(shape.bed.len(), 24);
    assert!(shape.bed.iter().all(|p| p.y == 4 * 8 - 1));
    let sides: Vec<Option<Side>> = shape.hatches.iter().map(|h| h.outer).collect();
    assert_eq!(sides, vec![Some(Side::Up), Some(Side::Right), Some(Side::Down)]);
    let roles: Vec<(bool, bool)> = shape.hatches.iter().map(|h| (h.takes_in(), h.gives_out())).collect();
    assert_eq!(roles, vec![(true, false), (true, true), (false, true)]);
}

#[test]
fn a_missing_corner_is_a_hole() {
    // Cells move at corners too, so the corner tile must be a wall.
    let (mut f, mut sim) = setup();
    let ctrl = draw(
        &mut f,
        &mut sim,
        &[
            "##H#.", //
            "C...#", //
            "#####",
        ],
    );
    assert_eq!(check_room(&f, ctrl), Err(Problem::Hole { at: TilePos::new(5, 1) }));
}

#[test]
fn an_l_shaped_room_is_valid() {
    let (mut f, mut sim) = setup();
    let ctrl = draw(
        &mut f,
        &mut sim,
        &[
            "##H..", //
            "#.#..", //
            "C.####", //
            "#....#", //
            "######",
        ],
    );
    let shape = check_room(&f, ctrl).unwrap();
    assert_eq!(shape.inside.len(), 6);
    // Only tiles that stand on the wall get a bed: the 4 tiles of the bottom row.
    assert_eq!(shape.bed.len(), 4 * 8);
}

#[test]
fn problem_texts_say_where_from_the_controller() {
    let (f, _) = setup();
    let def = f.content.factory.building_def(f.content.factory.building("kiln_controller").unwrap());
    let at = TilePos::new(10, 10);
    let hole = Problem::Hole { at: TilePos::new(8, 9) };
    assert_eq!(hole.text(&f.content, def, at), "The room has a hole 2 tiles left and 1 tile up from the controller. Close it with a wall block.");
    let wall = Problem::WrongWall { at: TilePos::new(10, 13), name: "Wood block".into() };
    assert_eq!(
        wall.text(&f.content, def, at),
        "Wrong wall block 3 tiles down from the controller: Wood block. This room needs Clay brick wall or Firebrick wall."
    );
    assert_eq!(hole.tile(), Some(TilePos::new(8, 9)));
    assert_eq!(Problem::NoHatch.tile(), None);
}

#[test]
fn a_wrong_building_in_the_wall_is_a_hole_there() {
    // A wood block is not a room part: the room leaks through it.
    let (mut f, mut sim) = setup();
    let ctrl = draw(
        &mut f,
        &mut sim,
        &[
            "##H#", //
            "C..#", //
            "####",
        ],
    );
    let wall = f.buildings.at_tile(TilePos::new(4, 2), foundry_content::Layer::Front).unwrap();
    f.remove(wall, &mut sim).unwrap();
    place(&mut f, &mut sim, "wood_wall", 4, 2);
    assert_eq!(check_room(&f, ctrl), Err(Problem::Hole { at: TilePos::new(4, 2) }));
}

#[test]
fn the_room_size_sets_the_speed() {
    let (mut f, mut sim) = setup();
    let ctrl = draw(
        &mut f,
        &mut sim,
        &[
            "##H#", //
            "C..#", //
            "####",
        ],
    );
    let content = f.content.clone();
    tick(&content, &mut f.buildings, &mut sim);
    let def = content.factory.building_def(f.buildings.get(ctrl).unwrap().kind);
    let room = f.buildings.room(ctrl).unwrap();
    assert!(room.is_valid());
    // The kiln data has speed_per_tile 8: 2 tiles give speed 16.
    assert_eq!(room.speed(def), 2.0 * def.param("speed_per_tile", 1.0) as f64);
    assert_eq!(f.buildings.room_at(TilePos::new(2, 2)), Some(ctrl));
    assert_eq!(f.buildings.room_at(TilePos::new(1, 1)), Some(ctrl), "a wall tile");
    assert_eq!(f.buildings.room_at(TilePos::new(9, 9)), None);
    assert!(f.buildings.set_blast(ctrl, 300));
    assert_eq!(f.buildings.room(ctrl).unwrap().blast_at(f.buildings.now()), 300);
}
