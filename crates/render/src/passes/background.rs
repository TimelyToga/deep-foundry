//! Background pass: a dark gradient behind the world, drawn at screen resolution.

use super::fullscreen_pipeline;
use crate::shaders::{ShaderFile, create_pipeline};
use std::path::Path;

pub(crate) struct BackgroundPass {
    pipeline: wgpu::RenderPipeline,
}

impl BackgroundPass {
    pub fn new(device: &wgpu::Device, frame_layout: &wgpu::BindGroupLayout, format: wgpu::TextureFormat, dir: Option<&Path>) -> Self {
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("background"),
            bind_group_layouts: &[Some(frame_layout)],
            immediate_size: 0,
        });
        let pipeline = create_pipeline(device, ShaderFile::Background, dir, |module| {
            fullscreen_pipeline(
                device,
                "background",
                &layout,
                module,
                ("vs_background", "fs_background"),
                wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL },
            )
        });
        Self { pipeline }
    }

    /// Fill the whole target. Draw this first.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, frame: &wgpu::BindGroup) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, frame, &[]);
        pass.draw(0..3, 0..1);
    }
}
