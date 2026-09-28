//! Render settings of the game: where the surface is (for the sky light), the robot's lamp, and
//! the debug views (keys F4 to F6, the Debug window, and `--view` for screenshots).

use crate::player::{ROBOT_H, ROBOT_W, Robot};
use crate::robot_sprite::RobotLook;
use foundry_core::CHUNK_SIZE;
use foundry_render::{PointLight, RenderSettings, Renderer};
use glam::DVec2;

/// The row of the ground surface (about), for the sky light and the sky color.
/// A world with no side limit has `DEFAULT_SKY_CHUNKS` chunks of sky above the surface level.
/// The demo box world (`--world`) has its ground at 55% of its height.
pub fn surface_level(world_cells: (i32, i32)) -> i32 {
    if world_cells.0 == 0 { foundry_sim::DEFAULT_SKY_CHUNKS * CHUNK_SIZE } else { world_cells.1 * 55 / 100 }
}

/// Give the renderer the robot's sprite sheet and its glow mask (`assets/sprites/robot.png`).
pub fn load_robot_look(renderer: &mut Renderer) -> RobotLook {
    let look = RobotLook::load();
    renderer.set_sprite_sheet(look.size.0, look.size.1, &look.rgba);
    renderer.set_sprite_glow(&look.glow);
    look
}

/// The robot's lights this frame: its lamp, and the jetpack flame while it flies. `at` is the
/// drawn top-left corner of its body (between two ticks). `None`: no robot (the sandbox).
/// The robot's sprites are set by the caller (`RobotLook::sprites`).
pub fn robot_lights(renderer: &mut Renderer, robot: Option<(&Robot, (f32, f32))>) {
    let Some((r, at)) = robot else {
        renderer.set_lights(&[]);
        return;
    };
    let center = (at.0 + ROBOT_W as f32 * 0.5, at.1 + ROBOT_H as f32 * 0.5);
    let lamp = robot_lamp(center, r.facing);
    if r.jetting {
        // The jetpack flame lights the ground below the robot.
        let flame = PointLight {
            pos: DVec2::new(center.0 as f64 - r.facing as f64 * 5.0, at.1 as f64 + ROBOT_H as f64),
            radius: 5.0,
            color: [0.9, 0.45, 0.12],
        };
        renderer.set_lights(&[lamp, flame]);
    } else {
        renderer.set_lights(&[lamp]);
    }
}

/// The robot's lamp: a warm light a little in front of its head.
pub fn robot_lamp(center: (f32, f32), facing: i8) -> PointLight {
    PointLight {
        pos: DVec2::new(center.0 as f64 + facing as f64 * 3.0, center.1 as f64 - 4.0),
        radius: 7.0,
        color: [0.6, 0.55, 0.42],
    }
}

/// The debug views that the game shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DebugViews {
    /// Awake chunks and their update rectangles (F4). The simulation sends them only when this is on.
    pub awake_chunks: bool,
}

/// The names that `--view` knows, with what they do.
pub const VIEW_NAMES: &[(&str, &str)] = &[
    ("nolight", "no light pass (every cell at full color)"),
    ("nobloom", "no bloom"),
    ("noshimmer", "no heat shimmer"),
    ("heat", "heat map: cells colored by temperature"),
    ("grid", "chunk grid"),
    ("light", "only the light map"),
    ("chunks", "awake chunks and their update rectangles"),
];

/// Apply `--view` names to the render settings and the debug views.
pub fn apply_view_names(names: &[String], s: &mut RenderSettings, views: &mut DebugViews) -> Result<(), String> {
    for name in names {
        match name.as_str() {
            "nolight" => s.lighting = false,
            "nobloom" => s.bloom = false,
            "noshimmer" => s.heat_shimmer = false,
            "heat" => s.heat_map = true,
            "grid" => s.chunk_grid = true,
            "light" => s.light_only = true,
            "chunks" => views.awake_chunks = true,
            other => {
                let known: Vec<&str> = VIEW_NAMES.iter().map(|v| v.0).collect();
                return Err(format!("--view: unknown view `{other}` (known: {})", known.join(", ")));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_names_change_the_settings() {
        let mut s = RenderSettings::default();
        let mut v = DebugViews::default();
        let names: Vec<String> = ["heat", "grid", "chunks", "nobloom"].iter().map(|s| s.to_string()).collect();
        apply_view_names(&names, &mut s, &mut v).unwrap();
        assert!(s.heat_map && s.chunk_grid && !s.bloom && s.lighting);
        assert!(v.awake_chunks);
        assert!(apply_view_names(&["x".to_string()], &mut s, &mut v).is_err());
    }

    #[test]
    fn surface_of_the_worlds() {
        assert_eq!(surface_level((0, 9216)), 1024);
        assert_eq!(surface_level((1024, 1000)), 550);
    }
}
