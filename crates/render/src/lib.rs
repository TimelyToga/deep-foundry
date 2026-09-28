//! The wgpu renderer for Deep Foundry.
//!
//! This crate reads only the snapshot types of `foundry_core` and the material data of
//! `foundry_content`. It never reads the live world.
//!
//! How to use it:
//! 1. Make a `Renderer` with `Renderer::new`.
//! 2. For each snapshot from the simulation, call `apply_snapshot`. Send the chunks it returns to the
//!    simulation in `Command::ForgetChunks`.
//! 3. For each frame, call `render` with a `Camera`.
//!
//! World data on the GPU: a texture array with one 64 × 64 layer for each chunk (format `Rgba16Uint`,
//! each texel is a `CellTexel`). Only chunks that arrive in a snapshot are uploaded. A chunk the renderer
//! has no data for is drawn as air.
//!
//! The passes of one frame, in order:
//! 1. World pass (`passes::world`): each visible chunk at 1 texel per cell into an offscreen texture.
//!    Then the sprites (`sprite.rs`, the robot) go into the same texture, on the cell grid.
//! 2. Background pass (`passes::background`): a dark gradient over the whole target.
//! 3. Scale pass (`passes::scale`): the offscreen texture over the background, with sharp bilinear filtering.
//!
//! The WGSL files are in `assets/shaders/`.

mod camera;
mod frame;
pub mod headless;
mod layers;
pub mod palette;
mod passes;
mod renderer;
mod shaders;
mod sprite;

pub use camera::Camera;
pub use renderer::{RenderStats, Renderer, RendererOptions};
pub use shaders::default_shader_dir;
pub use sprite::{BODY_THROUGH, Sprite, SpriteLayer};

/// Re-exported so that users of this crate use the same wgpu version.
pub use wgpu;
