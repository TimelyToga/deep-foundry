//! World pass: draws the cells of each visible chunk at 1 texel per cell into the offscreen world
//! textures (see `targets.rs`), then the particles over them.

use super::{read_buffer, texture_entry};
use crate::frame::{ChunkInstance, ParticleInstance};
use crate::shaders::{ShaderFile, create_pipeline};
use crate::targets::{COLOR_FORMAT, DATA_FORMAT, EMISSION_FORMAT, Targets};
use std::path::Path;
use wgpu::{BlendComponent, BlendFactor, BlendOperation, BlendState, ColorTargetState, ColorWrites, ShaderStages};

/// The GPU data the world shader reads.
pub(crate) struct WorldInputs<'a> {
    pub cells: &'a wgpu::TextureView,
    pub palette: &'a wgpu::TextureView,
    pub glow_lut: &'a wgpu::TextureView,
    pub materials: &'a wgpu::Buffer,
}

pub(crate) struct WorldPass {
    chunk_pipeline: wgpu::RenderPipeline,
    particle_pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    instances: wgpu::Buffer,
    instance_capacity: u32,
    particles: wgpu::Buffer,
    particle_capacity: u32,
}

/// The four targets of the world pass. Chunks write each texel once: no blending.
fn chunk_targets() -> [Option<ColorTargetState>; 4] {
    let t = |format| Some(ColorTargetState { format, blend: None, write_mask: ColorWrites::ALL });
    [t(COLOR_FORMAT), t(COLOR_FORMAT), t(EMISSION_FORMAT), t(DATA_FORMAT)]
}

/// Particles blend over the chunks: color with premultiplied alpha, emitted light added.
/// They do not change the gas, the light-stopping value or the heat.
fn particle_targets() -> [Option<ColorTargetState>; 4] {
    let add = BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::One, operation: BlendOperation::Add };
    [
        Some(ColorTargetState {
            format: COLOR_FORMAT,
            blend: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            write_mask: ColorWrites::ALL,
        }),
        Some(ColorTargetState { format: COLOR_FORMAT, blend: None, write_mask: ColorWrites::empty() }),
        Some(ColorTargetState {
            format: EMISSION_FORMAT,
            blend: Some(BlendState { color: add, alpha: add }),
            write_mask: ColorWrites::COLOR,
        }),
        Some(ColorTargetState { format: DATA_FORMAT, blend: None, write_mask: ColorWrites::empty() }),
    ]
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
        let f = ShaderStages::FRAGMENT;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("world"),
            entries: &[
                texture_entry(0, f, S::Uint, D::D2Array),
                texture_entry(1, f, S::Float { filterable: false }, D::D2),
                texture_entry(2, f, S::Float { filterable: false }, D::D2),
                read_buffer(3, f),
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
        let chunk_attributes = wgpu::vertex_attr_array![0 => Sint32x2, 1 => Uint32];
        let particle_attributes = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Uint32x2];
        let pipeline = |module: &wgpu::ShaderModule,
                        label: &str,
                        entry: (&str, &str),
                        stride: usize,
                        attributes: &[wgpu::VertexAttribute],
                        targets: &[Option<ColorTargetState>]| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some(entry.0),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: stride as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes,
                    })],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(entry.1),
                    compilation_options: Default::default(),
                    targets,
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let (chunk_pipeline, particle_pipeline) = create_pipeline(device, ShaderFile::World, dir, |module| {
            (
                pipeline(
                    module,
                    "world chunks",
                    ("vs_world", "fs_world"),
                    std::mem::size_of::<ChunkInstance>(),
                    &chunk_attributes,
                    &chunk_targets(),
                ),
                pipeline(
                    module,
                    "world particles",
                    ("vs_particle", "fs_particle"),
                    std::mem::size_of::<ParticleInstance>(),
                    &particle_attributes,
                    &particle_targets(),
                ),
            )
        });
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("chunk instances"),
            size: (instance_capacity.max(1) as u64) * std::mem::size_of::<ChunkInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let particle_capacity = 1024;
        let particles = particle_buffer(device, particle_capacity);
        Self { chunk_pipeline, particle_pipeline, bind_group, instances, instance_capacity, particles, particle_capacity }
    }

    /// Upload the chunks to draw this frame. Returns how many will be drawn.
    pub fn write_instances(&self, queue: &wgpu::Queue, instances: &[ChunkInstance]) -> u32 {
        let n = instances.len().min(self.instance_capacity as usize);
        if n > 0 {
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&instances[..n]));
        }
        n as u32
    }

    /// Upload the particles to draw this frame. The buffer grows when needed.
    pub fn write_particles(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, particles: &[ParticleInstance]) -> u32 {
        if particles.len() > self.particle_capacity as usize {
            self.particle_capacity = (particles.len() as u32).next_power_of_two();
            self.particles = particle_buffer(device, self.particle_capacity);
        }
        if !particles.is_empty() {
            queue.write_buffer(&self.particles, 0, bytemuck::cast_slice(particles));
        }
        particles.len() as u32
    }

    /// Clear the world textures and draw `chunks` chunks and `particles` particles into their
    /// top-left `used` texels.
    pub fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        targets: &Targets,
        frame: &wgpu::BindGroup,
        used: [u32; 2],
        chunks: u32,
        particles: u32,
    ) {
        let attachment = |view| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
            })
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("world"),
            color_attachments: &[
                attachment(&targets.color),
                attachment(&targets.gas),
                attachment(&targets.emission),
                attachment(&targets.data),
            ],
            ..Default::default()
        });
        if chunks == 0 && particles == 0 {
            return;
        }
        pass.set_viewport(0.0, 0.0, used[0] as f32, used[1] as f32, 0.0, 1.0);
        pass.set_bind_group(0, frame, &[]);
        pass.set_bind_group(1, &self.bind_group, &[]);
        if chunks > 0 {
            pass.set_pipeline(&self.chunk_pipeline);
            pass.set_vertex_buffer(0, self.instances.slice(..));
            pass.draw(0..6, 0..chunks);
        }
        if particles > 0 {
            pass.set_pipeline(&self.particle_pipeline);
            pass.set_vertex_buffer(0, self.particles.slice(..));
            pass.draw(0..6, 0..particles);
        }
    }
}

fn particle_buffer(device: &wgpu::Device, capacity: u32) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("particle instances"),
        size: capacity as u64 * std::mem::size_of::<ParticleInstance>() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
