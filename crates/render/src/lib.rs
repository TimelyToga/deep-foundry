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
//! 4. Optional: `settings_mut` (light, bloom, debug views), `set_surface_level` (where the sky
//!    light starts), `set_lights` (point lights such as the robot's lamp), `set_sprite_sheet` and
//!    `set_sprites` (pictures over the world).
//!
//! World data on the GPU: a texture array with one 64 × 64 layer for each chunk (format `Rgba16Uint`,
//! each texel is a `CellTexel`). Only chunks that arrive in a snapshot are uploaded. A chunk the renderer
//! has no data for is drawn as air.
//!
//! The passes of one frame, in order:
//! 1. World pass (`passes::world`): each visible chunk and each particle at 1 texel per cell into
//!    offscreen textures (colors, gas, emitted light, heat; see `targets.rs`). The area is the
//!    screen plus `LIGHT_MARGIN` cells on each side.
//! 2. Light pass (`passes::light`, compute): a light map at 1/4 of the cell resolution. Light from
//!    emitting and hot cells, point lights and the sky spreads out; cells that stop light block it.
//!    Then blurred copies of the emitted light for the bloom.
//! 3. Composite pass (`passes::composite`): the screen image. Background (sky or rock wall), cells
//!    with sharp bilinear filtering, color = base x (ambient + light) + emitted light, gas, bloom,
//!    heat shimmer, and the debug views (heat map, chunk grid, light only).
//! 4. Sprite pass (`passes::sprite`): pictures from the sprite sheet (the robot) at screen
//!    resolution, lit by the light map.
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
mod sky;
mod targets;

pub use camera::Camera;
pub use renderer::{LIGHT_MARGIN, PointLight, RenderSettings, RenderStats, Renderer, RendererOptions, Sprite};
pub use shaders::default_shader_dir;

/// Re-exported so that users of this crate use the same wgpu version.
pub use wgpu;
