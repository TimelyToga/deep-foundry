//! Headless tools for TimTech: scene tests, benchmarks, and PNG pictures of the cell world.
//! No window and no GPU. See `README.md` in this crate for how to make a scene and run the commands.
//!
//! - `scene`: load a scene (PNG + RON) and build a `Simulation` from it.
//! - `check`: the check types and how they are tested.
//! - `runner`: run scenes and checks, print the result table.
//! - `bench`: benchmark worlds made in code, timing, baselines.
//! - `image`: read and write PNG files, draw the world into an image.
//! - `paths`: where scenes, pictures and baselines are.
//! - `worldgen`: pictures of the world generator.

pub mod bench;
pub mod check;
pub mod image;
pub mod paths;
pub mod runner;
pub mod scene;
pub mod worldgen;

pub use check::Check;
pub use image::Image;
pub use runner::{CheckResult, Outcome, SceneReport, run_scene, run_tests};
pub use scene::Scene;
