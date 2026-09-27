//! World pass: draws the cells of each visible chunk at 1 texel per cell into the offscreen world texture.

use super::texture_entry;
use crate::frame::ChunkInstance;
use crate::shaders::{ShaderFile, create_pipeline};
use std::path::Path;

/// Format of the offscreen world texture. The colors in it are sRGB values with premultiplied alpha.
pub(crate) const WORLD_COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The GPU data the world shader reads.
pub(crate) struct WorldInputs<'a> {
    pub cells: &'a wgpu::TextureView,
    pub palette: &'a wgpu::TextureView,
    pub glow_lut: &'a wgpu::TextureView,
    pub materials: &'a wgpu::Buffer,
}

pub(crate) struct WorldPass {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    instances: wgpu::Buffer,
    instance_capacity: u32,
}

impl WorldPass {
    pub fn new(
        device: &wgpu::Device,
        frame_layout: &wgpu::BindGroupLayout,
        inputs: WorldInputs<'_>,
        instance_capacity: u32,
        dir: Option<&Path>,
    ) -> Self {
        use wgpu::{TextureSampleType as S, TextureViewDimension as D};
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("world"),
            entries: &[
                texture_entry(0, S::Uint, D::D2Array),
                texture_entry(1, S::Float { filterable: false }, D::D2),
                texture_entry(2, S::Float { filterable: false }, D::D2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("world"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(inputs.cells) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(inputs.palette) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(inputs.glow_lut) },
                wgpu::BindGroupEntry { binding: 3, resource: inputs.materials.as_entire_binding() },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("world"),
            bind_group_layouts: &[Some(frame_layout), Some(&layout)],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![0 => Sint32x2, 1 => Uint32];
        let pipeline = create_pipeline(device, ShaderFile::World, dir, |module| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("world"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some("vs_world"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<ChunkInstance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &attributes,
                    })],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some("fs_world"),
                    compilation_options: Default::default(),
                    // Chunks never overlap, so each texel is written once: no blending.
                    targets: &[Some(wgpu::ColorTargetState {
                        format: WORLD_COLOR_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        });
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("chunk instances"),
            size: (instance_capacity.max(1) as u64) * std::mem::size_of::<ChunkInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { pipeline, bind_group, instances, instance_capacity }
    }

    /// Upload the chunks to draw this frame. Returns how many will be drawn.
    pub fn write_instances(&self, queue: &wgpu::Queue, instances: &[ChunkInstance]) -> u32 {
        let n = instances.len().min(self.instance_capacity as usize);
        if n > 0 {
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&instances[..n]));
        }
        n as u32
    }

    /// Clear the world texture to transparent and draw `count` chunks into its top-left `used` texels.
    pub fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        frame: &wgpu::BindGroup,
        used: [u32; 2],
        count: u32,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("world"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        if count == 0 {
            return;
        }
        pass.set_viewport(0.0, 0.0, used[0] as f32, used[1] as f32, 0.0, 1.0);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, frame, &[]);
        pass.set_bind_group(1, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass.draw(0..6, 0..count);
    }
}
