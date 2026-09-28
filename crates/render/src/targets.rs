//! The offscreen textures of one frame.
//!
//! Cell resolution (1 texel per cell), written by the world pass:
//! - `color`: the cells that are not gas. sRGB values with premultiplied alpha. Air is transparent.
//! - `gas`: the gas cells, the same way. The composite pass draws them a little soft.
//! - `emission`: light that each cell gives (linear RGB), and in `a` how much the cell stops light.
//! - `data`: `r` = how hot the cell is for the heat shimmer (0 to 1).
//!
//! Light resolution (1 texel per `LIGHT_CELLS` x `LIGHT_CELLS` cells), written by the light pass:
//! - `src`: the emitted light of the texel (rgb) and how much light passes through it (a).
//! - `aux`: `r` = heat for the shimmer.
//! - `seed`: the light that starts in the texel: emitted light plus sky light (rgb); the direct
//!   sky light (a).
//! - `ping`, `pong`: the light while it spreads. The final light is in `ping`.
//! - `bloom`: smaller and smaller copies of the emitted light, blurred, for the bloom.
//!
//! All textures are made larger than needed, so that small changes of the zoom or the window
//! size do not make new textures each frame. The top-left part is used.

use glam::UVec2;

/// Cells per light texel, on each side. Keep it the same as LIGHT_CELLS in common.wgsl.
pub(crate) const LIGHT_CELLS: u32 = 4;
/// The world textures start on a multiple of this many cells, so that the light and bloom
/// texels stay on the same cells when the camera moves.
pub(crate) const ALIGN_CELLS: i32 = 32;
/// Number of bloom levels (each half the size of the one before).
pub(crate) const BLOOM_LEVELS: usize = 3;

pub(crate) const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
pub(crate) const EMISSION_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub(crate) const DATA_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
pub(crate) const LIGHT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub(crate) const AUX_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

pub(crate) struct Targets {
    /// Size of the cell-resolution textures.
    pub size: UVec2,
    /// Size of the light-resolution textures.
    pub light_size: UVec2,
    pub color: wgpu::TextureView,
    pub gas: wgpu::TextureView,
    pub emission: wgpu::TextureView,
    pub data: wgpu::TextureView,
    pub src: wgpu::TextureView,
    pub aux: wgpu::TextureView,
    pub seed: wgpu::TextureView,
    pub ping: wgpu::TextureView,
    pub pong: wgpu::TextureView,
    /// Blurred emitted light at 1/2, 1/4 and 1/8 of the light resolution.
    pub bloom_down: [wgpu::TextureView; BLOOM_LEVELS],
    /// The bloom levels added up from the smallest: `bloom_up[i]` has the size of `bloom_down[i]`.
    /// `bloom_up[BLOOM_LEVELS - 1]` is not used (the smallest level is `bloom_down`).
    pub bloom_up: [wgpu::TextureView; BLOOM_LEVELS],
}

impl Targets {
    /// Textures for at least `needed` cells. The size is rounded up.
    pub fn new(device: &wgpu::Device, needed: UVec2, max_side: u32) -> Self {
        // A multiple of 64 cells, so that every light and bloom level has whole texels.
        let size = needed.map(|v| (v.div_ceil(256) * 256).min(max_side / 64 * 64).max(64));
        let light_size = size / LIGHT_CELLS;
        let tex = |label: &str, size: UVec2, format: wgpu::TextureFormat, usage: wgpu::TextureUsages| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width: size.x.max(1), height: size.y.max(1), depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        use wgpu::TextureUsages as U;
        let drawn = U::RENDER_ATTACHMENT | U::TEXTURE_BINDING;
        let computed = U::STORAGE_BINDING | U::TEXTURE_BINDING;
        let level = |i: usize| (light_size >> (i as u32 + 1)).max(UVec2::ONE);
        Self {
            size,
            light_size,
            color: tex("world color", size, COLOR_FORMAT, drawn),
            gas: tex("world gas", size, COLOR_FORMAT, drawn),
            emission: tex("world emission", size, EMISSION_FORMAT, drawn),
            data: tex("world data", size, DATA_FORMAT, drawn),
            src: tex("light source", light_size, LIGHT_FORMAT, computed),
            aux: tex("light aux", light_size, AUX_FORMAT, computed),
            seed: tex("light seed", light_size, LIGHT_FORMAT, computed),
            ping: tex("light ping", light_size, LIGHT_FORMAT, computed),
            pong: tex("light pong", light_size, LIGHT_FORMAT, computed),
            bloom_down: std::array::from_fn(|i| tex("bloom down", level(i), LIGHT_FORMAT, computed)),
            bloom_up: std::array::from_fn(|i| tex("bloom up", level(i), LIGHT_FORMAT, computed)),
        }
    }

    /// True if the textures hold `needed` cells.
    pub fn fits(&self, needed: UVec2) -> bool {
        self.size.x >= needed.x && self.size.y >= needed.y
    }
}
