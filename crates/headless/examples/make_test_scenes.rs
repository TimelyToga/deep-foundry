//! Draws the PNG files of the scene tests in `assets/scenes/tests/`.
//!
//! Run: `cargo run -p foundry_headless --example make_test_scenes`
//!
//! The RON files are written by hand. If you change a picture here, check the rectangles
//! in the RON file of the same name. You can also draw scene PNGs with any paint program.

use foundry_headless::image::{Image, color};
use foundry_headless::paths;

const SAND: &str = "#d9c38c";
const WATER: &str = "#2f6fd6";
const STONE: &str = "#6b6a70";
const OIL: &str = "#2a2118";
const LAVA: &str = "#ff5a1a";
const STEAM: &str = "#e8eef5";
const SMOKE: &str = "#3b3a3e";

/// A function that draws one scene picture.
type Draw = fn() -> Image;

fn main() {
    let dir = paths::test_scenes_dir();
    let scenes: [(&str, Draw); 7] = [
        ("sand_pile", sand_pile),
        ("water_level", water_level),
        ("sand_sinks_in_water", sand_sinks_in_water),
        ("oil_floats_on_water", oil_floats_on_water),
        ("gas_rises", gas_rises),
        ("lava_meets_water", lava_meets_water),
        ("determinism_mix", determinism_mix),
    ];
    for (name, draw) in scenes {
        let path = dir.join(format!("{name}.png"));
        draw().save_png(&path).expect("write png");
        println!("wrote {}", path.display());
    }
}

/// A stone box: floor rows `h-4..h`, walls 4 cells thick from `top` down. The top is open.
fn open_box(img: &mut Image, top: i32) {
    let (w, h) = (img.width as i32, img.height as i32);
    let stone = color(STONE);
    img.fill_rect(0, h - 4, w, 4, stone);
    img.fill_rect(0, top, 4, h - top, stone);
    img.fill_rect(w - 4, top, 4, h - top, stone);
}

/// 96 x 64. A stone floor (rows 60..64) and a 16 x 15 sand block in the air (x 40..56, y 4..19).
fn sand_pile() -> Image {
    let mut img = Image::new(96, 64);
    img.fill_rect(0, 60, 96, 4, color(STONE));
    img.fill_rect(40, 4, 16, 15, color(SAND));
    img
}

/// 100 x 60. Open stone box, inside x 4..96 (92 wide), floor at y 56.
/// A 23 x 40 water block (920 cells) at the left side. Flat, it is exactly 10 rows: y 46..56.
fn water_level() -> Image {
    let mut img = Image::new(100, 60);
    open_box(&mut img, 10);
    img.fill_rect(4, 16, 23, 40, color(WATER));
    img
}

/// 80 x 70. Open stone box, inside x 4..76 (72 wide), floor at y 66.
/// Water fills y 30..66. A 20 x 18 sand block (360 cells) is above the water at x 30..50, y 10..28.
fn sand_sinks_in_water() -> Image {
    let mut img = Image::new(80, 70);
    open_box(&mut img, 10);
    img.fill_rect(4, 30, 72, 36, color(WATER));
    img.fill_rect(30, 10, 20, 18, color(SAND));
    img
}

/// 80 x 70. Same box as sand_sinks_in_water. Water fills y 30..66.
/// A 40 x 10 oil block (400 cells) is at the bottom of the water: x 20..60, y 56..66.
fn oil_floats_on_water() -> Image {
    let mut img = Image::new(80, 70);
    open_box(&mut img, 10);
    img.fill_rect(4, 30, 72, 36, color(WATER));
    img.fill_rect(20, 56, 40, 10, color(OIL));
    img
}

/// 100 x 80. Two closed stone chambers, walls 4 cells thick.
/// Left chamber inside x 4..44, y 4..76: 640 cells of hot steam at the bottom (y 60..76).
/// Right chamber inside x 56..96, y 4..76: 640 cells of smoke at the bottom (y 60..76).
fn gas_rises() -> Image {
    let mut img = Image::new(100, 80);
    let stone = color(STONE);
    for x0 in [0, 52] {
        img.fill_rect(x0, 0, 48, 80, stone);
        img.fill_rect(x0 + 4, 4, 40, 72, [0, 0, 0, 0]);
    }
    img.fill_rect(4, 60, 40, 16, color(STEAM));
    img.fill_rect(56, 60, 40, 16, color(SMOKE));
    img
}

/// 100 x 60. Open stone box, inside x 4..96, floor at y 56, walls from y 20.
/// Lava x 4..40 and water x 60..96, both y 36..56 (720 cells each). Air between them.
fn lava_meets_water() -> Image {
    let mut img = Image::new(100, 60);
    open_box(&mut img, 20);
    img.fill_rect(4, 36, 36, 20, color(LAVA));
    img.fill_rect(60, 36, 36, 20, color(WATER));
    img
}

/// 96 x 64. Open stone box with water at the bottom, and blocks of sand, oil, smoke, steam and lava above it.
fn determinism_mix() -> Image {
    let mut img = Image::new(96, 64);
    open_box(&mut img, 8);
    img.fill_rect(4, 44, 88, 16, color(WATER));
    img.fill_rect(10, 10, 20, 20, color(SAND));
    img.fill_rect(60, 20, 20, 10, color(OIL));
    img.fill_rect(40, 30, 10, 10, color(SMOKE));
    img.fill_rect(36, 10, 20, 6, color(STEAM));
    img.fill_rect(70, 36, 10, 6, color(LAVA));
    img
}
