//! The render passes. Each pass owns its pipeline and its bind groups.
//!
//! Group 0 is the same for all passes: the frame uniforms (`FrameUniforms`).
//! Group 1 belongs to each pass.
//!
//! Later passes (light, sprites, back layer, overlays) go here as new modules.

pub(crate) mod background;
pub(crate) mod scale;
pub(crate) mod world;

pub(crate) fn texture_entry(binding: u32, sample_type: wgpu::TextureSampleType, dim: wgpu::TextureViewDimension) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture { sample_type, view_dimension: dim, multisampled: false },
        count: None,
    }
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
