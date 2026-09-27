//! Scale pass: draws the offscreen world texture on the screen with sharp bilinear filtering.

use super::{fullscreen_pipeline, texture_entry};
use crate::shaders::{ShaderFile, create_pipeline};
use std::path::Path;

pub(crate) struct ScalePass {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// Made again when the world texture changes (see `set_source`).
    bind_group: Option<wgpu::BindGroup>,
}

impl ScalePass {
    pub fn new(device: &wgpu::Device, frame_layout: &wgpu::BindGroupLayout, format: wgpu::TextureFormat, dir: Option<&Path>) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scale"),
            entries: &[
                texture_entry(0, wgpu::TextureSampleType::Float { filterable: true }, wgpu::TextureViewDimension::D2),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scale"),
            bind_group_layouts: &[Some(frame_layout), Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = create_pipeline(device, ShaderFile::Scale, dir, |module| {
            fullscreen_pipeline(
                device,
                "scale",
                &pipeline_layout,
                module,
                ("vs_scale", "fs_scale"),
                wgpu::ColorTargetState {
                    format,
                    // The world texture has premultiplied alpha.
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                },
            )
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("world color"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self { pipeline, layout, sampler, bind_group: None }
    }

    /// Use this world texture from now on.
    pub fn set_source(&mut self, device: &wgpu::Device, world_view: &wgpu::TextureView) {
        self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scale"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(world_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        }));
    }

    /// Draw the world over what is in the target.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, frame: &wgpu::BindGroup) {
        let Some(bind_group) = &self.bind_group else { return };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, frame, &[]);
        pass.set_bind_group(1, bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}
