//! Sprites: pictures from a sprite sheet (the robot and its effects), drawn on the cell grid.
//!
//! - One texel of the sheet is one world cell. A sprite is at a whole world cell, so it lines up
//!   with the cells at every zoom and camera position.
//! - The sprites are drawn into the offscreen world color texture after the world pass. The
//!   composite pass then shows them with the cells, with the same sharp filtering and the same
//!   light: a sprite in a dark cave is dark.
//! - The sheet texels are read with no filtering (no blur).
//!
//! Layers:
//! - `SpriteLayer::Body` (the robot): it is drawn behind the cells, then again over the cells at
//!   `BODY_THROUGH` strength. In air it looks normal. In water or smoke it is partly covered, so
//!   it looks like it is in the liquid, but it is still easy to see. Sand that falls on it covers
//!   it by half.
//! - `SpriteLayer::Front` (dust, flying cells): drawn over the cells, lit like the cells.
//! - `SpriteLayer::Glow` (flame, beam, sparks): drawn over the cells, and they give light. Their
//!   color also goes into the emitted light texture, so they are bright in the dark and the light
//!   pass spreads their light to the cells around them.
//!
//! Glow mask: `Renderer::set_sprite_glow` sets a second picture with the size of the sheet. Its
//! texels give light where the sprite shows over the cells (for example the robot's visor). A
//! sprite that is behind cells gives no light from the mask.
//!
//! Use: `Renderer::set_sprite_sheet` once (then `set_sprite_glow` if there is a mask), then
//! `Renderer::set_sprites` each frame.

use crate::shaders::{ShaderFile, create_pipeline};
use crate::targets::{COLOR_FORMAT, EMISSION_FORMAT, Targets};
use bytemuck::{Pod, Zeroable};
use std::path::Path;

/// How strong the robot shows through the cells in front of it (0 to 1).
pub const BODY_THROUGH: f32 = 0.6;

/// Where a sprite is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpriteLayer {
    /// Behind the cells, and partly through them (the robot).
    #[default]
    Body,
    /// Over the cells, lit like the cells (dust, flying cells).
    Front,
    /// Over the cells, and it gives light (flame, beam, sparks).
    Glow,
}

/// One sprite: a rectangle of the sprite sheet at a world cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sprite {
    /// World cell of the top-left corner.
    pub cell: [i32; 2],
    /// Top-left texel of the rectangle in the sheet.
    pub src: [u16; 2],
    /// Size in texels (= cells).
    pub size: [u16; 2],
    /// The sheet color is multiplied by this (sRGB bytes, RGBA). `[255; 4]` keeps the colors.
    pub tint: [u8; 4],
    /// Mirror the rectangle left-right.
    pub flip_x: bool,
    pub layer: SpriteLayer,
}

/// `SpriteInstance::flags` bits. Keep them the same as in sprite.wgsl.
const FLAG_FLIP_X: u32 = 1;
const FLAG_GLOW: u32 = 2;

/// Instance data for one sprite. Keep it the same as `SpriteInstance` in sprite.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Pod, Zeroable)]
struct SpriteInstance {
    cell: [i32; 2],
    src: [u16; 2],
    size: [u16; 2],
    tint: [u8; 4],
    flags: u32,
}

impl SpriteInstance {
    fn new(s: &Sprite, alpha: f32) -> Self {
        let mut tint = s.tint;
        tint[3] = (tint[3] as f32 * alpha).round() as u8;
        let mut flags = if s.flip_x { FLAG_FLIP_X } else { 0 };
        if s.layer == SpriteLayer::Glow {
            flags |= FLAG_GLOW;
        }
        Self { cell: s.cell, src: s.src, size: s.size, tint, flags }
    }
}

/// The GPU side of the sprites.
pub(crate) struct SpritePass {
    /// Draws behind what is in the color texture ("destination over"). No emitted light.
    under: wgpu::RenderPipeline,
    /// Draws over what is in the color texture, and adds emitted light.
    over: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    /// The sheet, its size, and the glow mask. `None` until a sheet is set.
    sheet: Option<(wgpu::TextureView, [u32; 2])>,
    /// `None` until a sheet is set.
    bind_group: Option<wgpu::BindGroup>,
    instances: wgpu::Buffer,
    capacity: u64,
    /// Instance ranges: body behind the cells, body over the cells, front and glow.
    ranges: [std::ops::Range<u32>; 3],
    /// Reused each frame.
    scratch: Vec<SpriteInstance>,
}

impl SpritePass {
    pub fn new(device: &wgpu::Device, frame_layout: &wgpu::BindGroupLayout, dir: Option<&Path>) -> Self {
        let entry = |binding| {
            super::passes::texture_entry(
                binding,
                wgpu::ShaderStages::FRAGMENT,
                wgpu::TextureSampleType::Float { filterable: false },
                wgpu::TextureViewDimension::D2,
            )
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sprites"),
            entries: &[entry(0), entry(1)],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sprites"),
            bind_group_layouts: &[Some(frame_layout), Some(&layout)],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![0 => Sint32x2, 1 => Uint16x2, 2 => Uint16x2, 3 => Unorm8x4, 4 => Uint32];
        let behind = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::OneMinusDstAlpha,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        // Emitted light is added. Its alpha (how much a cell stops light) does not change.
        let add = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
        };
        let pipeline = |label: &str, blend: wgpu::BlendState, emission: wgpu::ColorWrites| {
            create_pipeline(device, ShaderFile::Sprite, dir, |module| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module,
                        entry_point: Some("vs_sprite"),
                        compilation_options: Default::default(),
                        buffers: &[Some(wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<SpriteInstance>() as u64,
                            step_mode: wgpu::VertexStepMode::Instance,
                            attributes: &attributes,
                        })],
                    },
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module,
                        entry_point: Some("fs_sprite"),
                        compilation_options: Default::default(),
                        targets: &[
                            Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL }),
                            Some(wgpu::ColorTargetState { format: EMISSION_FORMAT, blend: Some(add), write_mask: emission }),
                        ],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            })
        };
        let under = pipeline("sprites behind", wgpu::BlendState { color: behind, alpha: behind }, wgpu::ColorWrites::empty());
        let over = pipeline("sprites over", wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING, wgpu::ColorWrites::ALL);
        let capacity = 64;
        Self {
            under,
            over,
            layout,
            sheet: None,
            bind_group: None,
            instances: Self::buffer(device, capacity),
            capacity,
            ranges: [0..0, 0..0, 0..0],
            scratch: Vec::new(),
        }
    }

    fn buffer(device: &wgpu::Device, capacity: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sprite instances"),
            size: capacity * std::mem::size_of::<SpriteInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// A texture with sprite sheet texels: RGBA8, row by row from the top.
    fn texture(device: &wgpu::Device, queue: &wgpu::Queue, label: &str, width: u32, height: u32, rgba: &[u8]) -> wgpu::TextureView {
        use wgpu::util::DeviceExt;
        assert_eq!(rgba.len(), (width * height * 4) as usize, "sheet size does not match the data");
        device
            .create_texture_with_data(
                queue,
                &wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                rgba,
            )
            .create_view(&Default::default())
    }

    /// Upload the sheet: RGBA8 texels (sRGB colors, not premultiplied), row by row from the top.
    /// The glow mask is empty until `set_glow`.
    pub fn set_sheet(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, width: u32, height: u32, rgba: &[u8]) {
        let view = Self::texture(device, queue, "sprite sheet", width, height, rgba);
        self.sheet = Some((view, [width, height]));
        self.set_glow(device, queue, &vec![0; rgba.len()]);
    }

    /// Upload the glow mask: the same size and layout as the sheet. Its texels (sRGB colors, not
    /// premultiplied) give light where the sprite shows over the cells. Does nothing before a
    /// sheet is set.
    pub fn set_glow(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, rgba: &[u8]) {
        let Some((sheet, [width, height])) = &self.sheet else { return };
        let glow = Self::texture(device, queue, "sprite glow", *width, *height, rgba);
        self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sprites"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(sheet) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&glow) },
            ],
        }));
    }

    /// Upload the sprites to draw from now on.
    pub fn set_sprites(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, sprites: &[Sprite]) {
        let body = || sprites.iter().filter(|s| s.layer == SpriteLayer::Body);
        self.scratch.clear();
        // "Behind" puts each new sprite behind the ones before it, so this range is reversed:
        // the last sprite of the list is still in front.
        self.scratch.extend(body().rev().map(|s| SpriteInstance::new(s, 1.0)));
        let a = self.scratch.len() as u32;
        self.scratch.extend(body().map(|s| SpriteInstance::new(s, BODY_THROUGH)));
        let b = self.scratch.len() as u32;
        self.scratch.extend(sprites.iter().filter(|s| s.layer != SpriteLayer::Body).map(|s| SpriteInstance::new(s, 1.0)));
        let c = self.scratch.len() as u32;
        self.ranges = [0..a, a..b, b..c];
        if self.scratch.is_empty() {
            return;
        }
        if self.scratch.len() as u64 > self.capacity {
            self.capacity = (self.scratch.len() as u64).next_power_of_two();
            self.instances = Self::buffer(device, self.capacity);
        }
        queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&self.scratch));
    }

    /// Draw the sprites into the world color and emitted light textures (after the world pass).
    /// `used` is the part of the textures that the world pass drew.
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, targets: &Targets, frame: &wgpu::BindGroup, used: [u32; 2]) {
        let Some(bind_group) = &self.bind_group else { return };
        if self.ranges.iter().all(|r| r.is_empty()) {
            return;
        }
        let attachment = |view| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sprites"),
            color_attachments: &[attachment(&targets.color), attachment(&targets.emission)],
            ..Default::default()
        });
        pass.set_viewport(0.0, 0.0, used[0] as f32, used[1] as f32, 0.0, 1.0);
        pass.set_bind_group(0, frame, &[]);
        pass.set_bind_group(1, bind_group, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        for (range, pipeline) in self.ranges.iter().zip([&self.under, &self.over, &self.over]) {
            if !range.is_empty() {
                pass.set_pipeline(pipeline);
                pass.draw(0..6, range.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_size_matches_the_shader() {
        assert_eq!(std::mem::size_of::<SpriteInstance>(), 24);
    }

    #[test]
    fn body_alpha_is_scaled() {
        let s = Sprite { cell: [1, 2], src: [3, 4], size: [5, 6], tint: [255, 255, 255, 200], flip_x: true, layer: SpriteLayer::Body };
        let i = SpriteInstance::new(&s, 0.5);
        assert_eq!(i.tint[3], 100);
        assert_eq!(i.flags, FLAG_FLIP_X);
    }

    #[test]
    fn glow_sprites_have_the_glow_flag() {
        let s = Sprite { cell: [0, 0], src: [0, 0], size: [1, 1], tint: [255; 4], flip_x: false, layer: SpriteLayer::Glow };
        assert_eq!(SpriteInstance::new(&s, 1.0).flags, FLAG_GLOW);
        let front = Sprite { layer: SpriteLayer::Front, ..s };
        assert_eq!(SpriteInstance::new(&front, 1.0).flags, 0);
    }
}
