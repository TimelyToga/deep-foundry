//! Sprite pass: pictures from the sprite sheet over the world, after the composite pass. They get
//! the light of the light map at their place. See sprite.wgsl.

use super::{float_texture, sampler_entry, view_entry};
use crate::frame::SpriteInstance;
use crate::shaders::{ShaderFile, create_pipeline};
use crate::targets::Targets;
use std::path::Path;
use wgpu::ShaderStages;
use wgpu::util::DeviceExt;

pub(crate) struct SpritePass {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The sheet: colors and emitted light, both premultiplied sRGB.
    sheet: (wgpu::TextureView, wgpu::TextureView),
    has_sheet: bool,
    bind_group: Option<wgpu::BindGroup>,
    instances: wgpu::Buffer,
    capacity: u32,
}

impl SpritePass {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame_layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
        dir: Option<&Path>,
    ) -> Self {
        let f = ShaderStages::FRAGMENT;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sprite"),
            entries: &[float_texture(0, f), float_texture(1, f), float_texture(2, f), sampler_entry(3, f)],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sprite"),
            bind_group_layouts: &[Some(frame_layout), Some(&layout)],
            immediate_size: 0,
        });
        let attributes =
            wgpu::vertex_attr_array![0 => Float32x2, 1 => Uint32x4, 2 => Float32x2, 3 => Float32x2, 4 => Uint32];
        let pipeline = create_pipeline(device, ShaderFile::Sprite, dir, |module| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("sprite"),
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
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        });
        let empty = [0u8; 4];
        let sheet = (sheet_texture(device, queue, &empty, 1, 1), sheet_texture(device, queue, &empty, 1, 1));
        let capacity = 16;
        Self {
            pipeline,
            layout,
            sampler: super::linear_sampler(device, "sprite"),
            sheet,
            has_sheet: false,
            bind_group: None,
            instances: instance_buffer(device, capacity),
            capacity,
        }
    }

    /// Use a new sprite sheet. `color` and `emission` are RGBA8 pixels (sRGB, straight alpha),
    /// row by row, `width` x `height` each. Call `set_targets` after this.
    pub fn set_sheet(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, color: &[u8], emission: &[u8], width: u32, height: u32) {
        let n = (width * height * 4) as usize;
        if width == 0 || height == 0 || color.len() != n || emission.len() != n {
            log::error!("sprite sheet: expected {n} bytes for {width} x {height}");
            return;
        }
        self.sheet = (
            sheet_texture(device, queue, &premultiply(color), width, height),
            sheet_texture(device, queue, &premultiply(emission), width, height),
        );
        self.has_sheet = true;
    }

    /// Make the bind group for these targets (the light map) and the current sheet.
    pub fn set_targets(&mut self, device: &wgpu::Device, t: &Targets) {
        self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sprite"),
            layout: &self.layout,
            entries: &[
                view_entry(0, &self.sheet.0),
                view_entry(1, &self.sheet.1),
                view_entry(2, &t.ping),
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        }));
    }

    /// Upload the sprites of this frame. Returns how many will be drawn.
    pub fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, sprites: &[SpriteInstance]) -> u32 {
        if !self.has_sheet {
            return 0;
        }
        if sprites.len() > self.capacity as usize {
            self.capacity = (sprites.len() as u32).next_power_of_two();
            self.instances = instance_buffer(device, self.capacity);
        }
        if !sprites.is_empty() {
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(sprites));
        }
        sprites.len() as u32
    }

    /// Draw `count` sprites over what is in the target.
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, frame: &wgpu::BindGroup, count: u32) {
        let Some(bind_group) = &self.bind_group else { return };
        if count == 0 {
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
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, frame, &[]);
        pass.set_bind_group(1, bind_group, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass.draw(0..6, 0..count);
    }
}

/// Premultiply the colors by alpha, so that filtering at the edges does not make dark fringes.
fn premultiply(rgba: &[u8]) -> Vec<u8> {
    rgba.as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            let a = p[3] as u32;
            [(p[0] as u32 * a / 255) as u8, (p[1] as u32 * a / 255) as u8, (p[2] as u32 * a / 255) as u8, p[3]]
        })
        .collect()
}

fn sheet_texture(device: &wgpu::Device, queue: &wgpu::Queue, rgba: &[u8], width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture_with_data(
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
        )
        .create_view(&Default::default())
}

fn instance_buffer(device: &wgpu::Device, capacity: u32) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sprite instances"),
        size: capacity as u64 * std::mem::size_of::<SpriteInstance>() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
