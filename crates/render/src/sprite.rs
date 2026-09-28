//! Sprites: pictures from a sprite sheet (the robot and its effects), drawn on the cell grid.
//!
//! - One texel of the sheet is one world cell. A sprite is at a whole world cell, so it lines up
//!   with the cells at every zoom and camera position.
//! - The sprites are drawn into the offscreen world texture after the world pass. The scale pass
//!   then shows them with the cells, with the same sharp filtering.
//! - The sheet texels are read with no filtering (no blur).
//!
//! Layers:
//! - `SpriteLayer::Body` (the robot): it is drawn behind the cells, then again over the cells at
//!   `BODY_THROUGH` strength. In air it looks normal. In water or smoke it is partly covered, so
//!   it looks like it is in the liquid, but it is still easy to see. Sand that falls on it covers
//!   it by half.
//! - `SpriteLayer::Front` (flame, beam, sparks): drawn over everything.
//!
//! Use: `Renderer::set_sprite_sheet` once, then `Renderer::set_sprites` each frame.

use crate::shaders::{ShaderFile, create_pipeline};
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
    /// Over the cells (effects).
    Front,
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
        Self { cell: s.cell, src: s.src, size: s.size, tint, flags: s.flip_x as u32 }
    }
}

/// The GPU side of the sprites.
pub(crate) struct SpritePass {
    /// Draws behind what is in the target ("destination over").
    under: wgpu::RenderPipeline,
    /// Draws over what is in the target.
    over: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    /// `None` until a sheet is set.
    bind_group: Option<wgpu::BindGroup>,
    instances: wgpu::Buffer,
    capacity: u64,
    /// Instance ranges: body behind the cells, body over the cells, front.
    ranges: [std::ops::Range<u32>; 3],
    /// Reused each frame.
    scratch: Vec<SpriteInstance>,
}

impl SpritePass {
    pub fn new(device: &wgpu::Device, frame_layout: &wgpu::BindGroupLayout, format: wgpu::TextureFormat, dir: Option<&Path>) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sprites"),
            entries: &[super::passes::texture_entry(
                0,
                wgpu::TextureSampleType::Float { filterable: false },
                wgpu::TextureViewDimension::D2,
            )],
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
        let pipeline = |label: &str, blend: wgpu::BlendState| {
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
                        targets: &[Some(wgpu::ColorTargetState { format, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            })
        };
        let under = pipeline("sprites behind", wgpu::BlendState { color: behind, alpha: behind });
        let over = pipeline("sprites over", wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
        let capacity = 64;
        Self {
            under,
            over,
            layout,
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

    /// Upload the sheet: RGBA8 texels (sRGB colors, not premultiplied), row by row from the top.
    pub fn set_sheet(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, width: u32, height: u32, rgba: &[u8]) {
        use wgpu::util::DeviceExt;
        assert_eq!(rgba.len(), (width * height * 4) as usize, "sheet size does not match the data");
        let texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("sprite sheet"),
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
        );
        let view = texture.create_view(&Default::default());
        self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sprites"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }],
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
        self.scratch.extend(sprites.iter().filter(|s| s.layer == SpriteLayer::Front).map(|s| SpriteInstance::new(s, 1.0)));
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

    /// Draw the sprites into the world texture (after the world pass). `used` is the part of the
    /// texture that the world pass drew.
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, frame: &wgpu::BindGroup, used: [u32; 2]) {
        let Some(bind_group) = &self.bind_group else { return };
        if self.ranges.iter().all(|r| r.is_empty()) {
            return;
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sprites"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
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
        assert_eq!(i.flags, 1);
    }
}
