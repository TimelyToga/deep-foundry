//! The render passes. Each pass owns its pipelines and its bind groups.
//!
//! Group 0 is the same for all passes: the frame uniforms (`FrameUniforms`).
//! Group 1 belongs to each pass (or to each step of the light pass).
//!
//! Later passes (back layer, buildings) go here as new modules.

pub(crate) mod composite;
pub(crate) mod light;
pub(crate) mod world;

use wgpu::{BindGroupLayoutEntry, BindingType, ShaderStages, TextureSampleType, TextureViewDimension};

pub(crate) fn texture_entry(
    binding: u32,
    visibility: ShaderStages,
    sample_type: TextureSampleType,
    dim: TextureViewDimension,
) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility,
        ty: BindingType::Texture { sample_type, view_dimension: dim, multisampled: false },
        count: None,
    }
}

/// A filterable 2D float texture.
pub(crate) fn float_texture(binding: u32, visibility: ShaderStages) -> BindGroupLayoutEntry {
    texture_entry(binding, visibility, TextureSampleType::Float { filterable: true }, TextureViewDimension::D2)
}

/// A 2D texture that a compute shader writes.
pub(crate) fn storage_texture(binding: u32, format: wgpu::TextureFormat) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::COMPUTE,
        ty: BindingType::StorageTexture {
            access: wgpu::StorageTextureAccess::WriteOnly,
            format,
            view_dimension: TextureViewDimension::D2,
        },
        count: None,
    }
}

/// A buffer that shaders only read.
pub(crate) fn read_buffer(binding: u32, visibility: ShaderStages) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility,
        ty: BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

pub(crate) fn sampler_entry(binding: u32, visibility: ShaderStages) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility,
        ty: BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

/// A linear sampler that clamps at the edges.
pub(crate) fn linear_sampler(device: &wgpu::Device, label: &str) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some(label),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    })
}

pub(crate) fn view_entry(binding: u32, view: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource: wgpu::BindingResource::TextureView(view) }
}

/// A pipeline that draws one triangle over the whole target, with no vertex buffers.
pub(crate) fn fullscreen_pipeline(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    entry: (&str, &str),
    target: wgpu::ColorTargetState,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(entry.0),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(entry.1),
            compilation_options: Default::default(),
            targets: &[Some(target)],
        }),
        multiview_mask: None,
        cache: None,
    })
}
