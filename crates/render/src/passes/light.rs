//! Light pass: compute steps on the light textures (1 texel per `LIGHT_CELLS` x `LIGHT_CELLS`
//! cells). The shader is `light.wgsl`; its top comment explains the steps.
//!
//! Order in one frame:
//! 1. downsample: world textures -> `src` (emitted light, light that passes) and `aux` (heat).
//! 2. sky: `src` + sky entry per column -> `seed` (start light) and `ping` (light that leaves each
//!    texel at the start).
//! 3. spread `iterations` times: `ping` -> `pong` -> `ping` ... The count is even, so the last
//!    step writes `ping`: the final light map.
//! 4. bloom: `src` -> bloom_down[0] -> [1] -> [2]; then [2] + [1] -> bloom_up[1]; bloom_up[1] +
//!    [0] -> bloom_up[0], which the composite pass reads.

use super::{float_texture, read_buffer, sampler_entry, storage_texture, view_entry};
use crate::frame::LightInstance;
use crate::shaders::{ShaderFile, create_pipeline};
use crate::targets::{AUX_FORMAT, BLOOM_LEVELS, LIGHT_FORMAT, Targets};
use std::path::Path;
use wgpu::ShaderStages;

/// The most point lights in one frame.
pub(crate) const MAX_LIGHTS: usize = 64;

struct Pipelines {
    downsample: wgpu::ComputePipeline,
    sky: wgpu::ComputePipeline,
    spread: wgpu::ComputePipeline,
    spread_last: wgpu::ComputePipeline,
    bloom_down: wgpu::ComputePipeline,
    bloom_up: wgpu::ComputePipeline,
}

struct Layouts {
    downsample: wgpu::BindGroupLayout,
    sky: wgpu::BindGroupLayout,
    spread: wgpu::BindGroupLayout,
    bloom_down: wgpu::BindGroupLayout,
    bloom_up: wgpu::BindGroupLayout,
}

/// Bind groups for the current targets.
struct Groups {
    downsample: wgpu::BindGroup,
    sky: wgpu::BindGroup,
    /// ping -> pong, and pong -> ping.
    spread: [wgpu::BindGroup; 2],
    bloom_down: Vec<wgpu::BindGroup>,
    bloom_up: Vec<wgpu::BindGroup>,
    /// Workgroups for the bloom levels (down[i] size).
    bloom_sizes: Vec<[u32; 2]>,
}

pub(crate) struct LightPass {
    pipelines: Pipelines,
    layouts: Layouts,
    groups: Option<Groups>,
    sampler: wgpu::Sampler,
    lights: wgpu::Buffer,
    sky_entry: wgpu::Buffer,
    sky_capacity: u32,
}

impl LightPass {
    pub fn new(device: &wgpu::Device, frame_layout: &wgpu::BindGroupLayout, dir: Option<&Path>) -> Self {
        let c = ShaderStages::COMPUTE;
        let layout = |label: &str, entries: &[wgpu::BindGroupLayoutEntry]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries })
        };
        let layouts = Layouts {
            downsample: layout(
                "light downsample",
                &[
                    float_texture(0, c),
                    float_texture(1, c),
                    read_buffer(2, c),
                    storage_texture(3, LIGHT_FORMAT),
                    storage_texture(4, AUX_FORMAT),
                ],
            ),
            sky: layout(
                "light sky",
                &[float_texture(10, c), read_buffer(11, c), storage_texture(12, LIGHT_FORMAT), storage_texture(13, LIGHT_FORMAT)],
            ),
            spread: layout(
                "light spread",
                &[float_texture(20, c), float_texture(21, c), float_texture(22, c), storage_texture(23, LIGHT_FORMAT)],
            ),
            bloom_down: layout("bloom down", &[float_texture(30, c), sampler_entry(31, c), storage_texture(32, LIGHT_FORMAT)]),
            bloom_up: layout(
                "bloom up",
                &[float_texture(30, c), sampler_entry(31, c), storage_texture(32, LIGHT_FORMAT), float_texture(33, c)],
            ),
        };
        let pipelines = create_pipeline(device, ShaderFile::Light, dir, |module| {
            let make = |entry: &str, group: &wgpu::BindGroupLayout| {
                let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some(entry),
                    bind_group_layouts: &[Some(frame_layout), Some(group)],
                    immediate_size: 0,
                });
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(entry),
                    layout: Some(&layout),
                    module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    cache: None,
                })
            };
            Pipelines {
                downsample: make("cs_downsample", &layouts.downsample),
                sky: make("cs_sky", &layouts.sky),
                spread: make("cs_spread", &layouts.spread),
                spread_last: make("cs_spread_last", &layouts.spread),
                bloom_down: make("cs_bloom_down", &layouts.bloom_down),
                bloom_up: make("cs_bloom_up", &layouts.bloom_up),
            }
        });
        let lights = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point lights"),
            size: (MAX_LIGHTS * std::mem::size_of::<LightInstance>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sky_capacity = 1024;
        let sky_entry = sky_buffer(device, sky_capacity);
        let sampler = super::linear_sampler(device, "bloom");
        Self { pipelines, layouts, groups: None, sampler, lights, sky_entry, sky_capacity }
    }

    /// Make the bind groups for new targets.
    pub fn set_targets(&mut self, device: &wgpu::Device, t: &Targets) {
        let group = |label: &str, layout: &wgpu::BindGroupLayout, entries: &[wgpu::BindGroupEntry]| {
            device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some(label), layout, entries })
        };
        let sampler = wgpu::BindGroupEntry { binding: 31, resource: wgpu::BindingResource::Sampler(&self.sampler) };
        let downsample = group(
            "light downsample",
            &self.layouts.downsample,
            &[
                view_entry(0, &t.emission),
                view_entry(1, &t.data),
                wgpu::BindGroupEntry { binding: 2, resource: self.lights.as_entire_binding() },
                view_entry(3, &t.src),
                view_entry(4, &t.aux),
            ],
        );
        let sky = group(
            "light sky",
            &self.layouts.sky,
            &[
                view_entry(10, &t.src),
                wgpu::BindGroupEntry { binding: 11, resource: self.sky_entry.as_entire_binding() },
                view_entry(12, &t.seed),
                view_entry(13, &t.ping),
            ],
        );
        let spread = |from: &wgpu::TextureView, to: &wgpu::TextureView| {
            group(
                "light spread",
                &self.layouts.spread,
                &[view_entry(20, &t.src), view_entry(21, &t.seed), view_entry(22, from), view_entry(23, to)],
            )
        };
        let bloom_down = (0..BLOOM_LEVELS)
            .map(|i| {
                let input = if i == 0 { &t.src } else { &t.bloom_down[i - 1] };
                group(
                    "bloom down",
                    &self.layouts.bloom_down,
                    &[view_entry(30, input), sampler.clone(), view_entry(32, &t.bloom_down[i])],
                )
            })
            .collect();
        // bloom_up[i] = up(level i + 1) + down[i], from the smallest level up.
        let bloom_up = (0..BLOOM_LEVELS - 1)
            .rev()
            .map(|i| {
                let smaller = if i + 1 == BLOOM_LEVELS - 1 { &t.bloom_down[i + 1] } else { &t.bloom_up[i + 1] };
                group(
                    "bloom up",
                    &self.layouts.bloom_up,
                    &[view_entry(30, smaller), sampler.clone(), view_entry(32, &t.bloom_up[i]), view_entry(33, &t.bloom_down[i])],
                )
            })
            .collect();
        let bloom_sizes = (0..BLOOM_LEVELS).map(|i| (t.light_size >> (i as u32 + 1)).max(glam::UVec2::ONE).to_array()).collect();
        self.groups = Some(Groups {
            downsample,
            sky,
            spread: [spread(&t.ping, &t.pong), spread(&t.pong, &t.ping)],
            bloom_down,
            bloom_up,
            bloom_sizes,
        });
    }

    /// Upload the point lights (at most `MAX_LIGHTS`) and the sky light at the top of each light
    /// column. Returns the number of lights. The sky buffer grows when needed; then the bind
    /// groups are made again from `targets`.
    pub fn write_inputs(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        targets: &Targets,
        lights: &[LightInstance],
        sky: &[f32],
    ) -> u32 {
        let n = lights.len().min(MAX_LIGHTS);
        if n > 0 {
            queue.write_buffer(&self.lights, 0, bytemuck::cast_slice(&lights[..n]));
        }
        if sky.len() > self.sky_capacity as usize {
            self.sky_capacity = (sky.len() as u32).next_power_of_two();
            self.sky_entry = sky_buffer(device, self.sky_capacity);
            self.set_targets(device, targets);
        }
        if !sky.is_empty() {
            queue.write_buffer(&self.sky_entry, 0, bytemuck::cast_slice(sky));
        }
        n as u32
    }

    /// Run all steps. `used` is the part of the light textures to fill. `iterations` is the
    /// number of spread steps (made even, at least 2).
    pub fn run(&self, encoder: &mut wgpu::CommandEncoder, frame: &wgpu::BindGroup, used: [u32; 2], iterations: u32, bloom: bool) {
        let Some(g) = &self.groups else { return };
        let p = &self.pipelines;
        let groups_8 = |size: [u32; 2]| (size[0].div_ceil(8), size[1].div_ceil(8));
        let (wx, wy) = groups_8(used);
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("light"), timestamp_writes: None });
        pass.set_bind_group(0, frame, &[]);

        pass.set_pipeline(&p.downsample);
        pass.set_bind_group(1, &g.downsample, &[]);
        pass.dispatch_workgroups(wx, wy, 1);

        pass.set_pipeline(&p.sky);
        pass.set_bind_group(1, &g.sky, &[]);
        pass.dispatch_workgroups(used[0].div_ceil(64), 1, 1);

        let iterations = iterations.max(2).div_ceil(2) * 2;
        pass.set_pipeline(&p.spread);
        for i in 0..iterations {
            if i == iterations - 1 {
                pass.set_pipeline(&p.spread_last);
            }
            pass.set_bind_group(1, &g.spread[(i % 2) as usize], &[]);
            pass.dispatch_workgroups(wx, wy, 1);
        }

        if bloom {
            pass.set_pipeline(&p.bloom_down);
            for (group, size) in g.bloom_down.iter().zip(&g.bloom_sizes) {
                let (bx, by) = groups_8(*size);
                pass.set_bind_group(1, group, &[]);
                pass.dispatch_workgroups(bx, by, 1);
            }
            pass.set_pipeline(&p.bloom_up);
            // bloom_up groups are in the order: level BLOOM_LEVELS - 2 down to 0.
            for (k, group) in g.bloom_up.iter().enumerate() {
                let level = BLOOM_LEVELS - 2 - k;
                let (bx, by) = groups_8(g.bloom_sizes[level]);
                pass.set_bind_group(1, group, &[]);
                pass.dispatch_workgroups(bx, by, 1);
            }
        }
    }
}

fn sky_buffer(device: &wgpu::Device, capacity: u32) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sky entry"),
        size: capacity as u64 * 4,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
