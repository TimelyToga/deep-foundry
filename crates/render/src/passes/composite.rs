//! Composite pass: the screen image from the world textures, the light map and the bloom.
//! It draws the background, the cells with sharp bilinear filtering, the light, the gas, the
//! bloom, the heat shimmer and the chunk grid (debug). See composite.wgsl.

use super::{float_texture, fullscreen_pipeline, sampler_entry, view_entry};
use crate::shaders::{ShaderFile, create_pipeline};
use crate::targets::Targets;
use std::path::Path;
use wgpu::ShaderStages;

pub(crate) struct CompositePass {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// Made again when the targets change (see `set_targets`).
    bind_group: Option<wgpu::BindGroup>,
}

impl CompositePass {
    pub fn new(
        device: &wgpu::Device,
        frame_layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
        dir: Option<&Path>,
    ) -> Self {
        let f = ShaderStages::FRAGMENT;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composite"),
            entries: &[
                float_texture(0, f),
                float_texture(1, f),
                float_texture(2, f),
                float_texture(3, f),
                float_texture(4, f),
                float_texture(5, f),
                sampler_entry(6, f),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("composite"),
            bind_group_layouts: &[Some(frame_layout), Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = create_pipeline(device, ShaderFile::Composite, dir, |module| {
            fullscreen_pipeline(
                device,
                "composite",
                &pipeline_layout,
                module,
                ("vs_composite", "fs_composite"),
                wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL },
            )
        });
        let sampler = super::linear_sampler(device, "composite");
        Self { pipeline, layout, sampler, bind_group: None }
    }

    /// Use these targets from now on.
    pub fn set_targets(&mut self, device: &wgpu::Device, t: &Targets) {
        self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("composite"),
            layout: &self.layout,
            entries: &[
                view_entry(0, &t.color),
                view_entry(1, &t.gas),
                view_entry(2, &t.emission),
                view_entry(3, &t.ping),
                view_entry(4, &t.aux),
                view_entry(5, &t.bloom_up[0]),
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        }));
    }

    /// Draw the whole target. It clears the target first.
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, frame: &wgpu::BindGroup) {
        let Some(bind_group) = &self.bind_group else { return };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, frame, &[]);
        pass.set_bind_group(1, bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}
