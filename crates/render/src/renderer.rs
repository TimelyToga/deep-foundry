//! The `Renderer`: GPU resources for the world and the order of the passes.

use crate::camera::Camera;
use crate::frame::{ChunkInstance, FrameUniforms, LightInstance, ParticleInstance, flags};
use crate::layers::LayerMap;
use crate::palette::{self, GLOW_LUT_SIZE, SHADES};
use crate::passes::composite::CompositePass;
use crate::passes::light::LightPass;
use crate::passes::world::{WorldInputs, WorldPass};
use crate::shaders;
use crate::sky::SkyColumns;
use crate::targets::{ALIGN_CELLS, LIGHT_CELLS, Targets};
use foundry_content::Content;
use foundry_core::{CHUNK_AREA, CHUNK_SIZE, CellRect, CellTexel, ChunkPos, ParticleView, Snapshot};
use glam::{DVec2, IVec2, UVec2};
use std::num::NonZeroU64;
use std::path::PathBuf;
use wgpu::util::DeviceExt;

/// Cells around the screen that the light pass also works on, so that light from just outside
/// the screen reaches it. The game asks the simulation for at least this margin of chunks.
pub const LIGHT_MARGIN: i32 = 64;

/// Settings for a new `Renderer`.
#[derive(Debug, Clone)]
pub struct RendererOptions {
    /// Folder with the WGSL files. `None` uses only the shaders built into the program.
    pub shader_dir: Option<PathBuf>,
    /// The most chunks the GPU keeps. The device limit `max_texture_array_layers` can make it smaller.
    pub max_chunks: u32,
}

impl Default for RendererOptions {
    fn default() -> Self {
        Self { shader_dir: Some(shaders::default_shader_dir()), max_chunks: 2048 }
    }
}

/// How the world looks. Change it with `Renderer::settings_mut` at any time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderSettings {
    /// The light pass: dark caves, light from lava, fire and hot cells, sky light on the surface.
    /// Off: every cell has its full color (the look with no light).
    pub lighting: bool,
    /// A soft glow around bright light.
    pub bloom: bool,
    /// The air above very hot places moves a little.
    pub heat_shimmer: bool,
    /// Debug view: each cell has the color of its temperature. No light.
    pub heat_map: bool,
    /// Debug view: lines on the chunk borders (and on the tile borders when zoomed in).
    pub chunk_grid: bool,
    /// Debug view: only the light map.
    pub light_only: bool,
    /// Light that is everywhere, also deep underground (linear, 0 to 1).
    pub ambient: f32,
    /// Brightness of the sky light (0 to about 1.5).
    pub sky_light: f32,
    /// Light kept for each step of 4 cells away from a light (0.5 to 0.97). More: light goes farther.
    pub light_keep: f32,
    /// Spread steps of the light pass. Light goes at most 4 cells for each step. More steps cost
    /// more GPU time.
    pub light_steps: u32,
    pub bloom_strength: f32,
    /// Largest sideways move of the heat shimmer, in cells.
    pub shimmer_strength: f32,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            lighting: true,
            bloom: true,
            heat_shimmer: true,
            heat_map: false,
            chunk_grid: false,
            light_only: false,
            ambient: 0.025,
            sky_light: 1.0,
            light_keep: 0.9,
            light_steps: 24,
            bloom_strength: 0.22,
            shimmer_strength: 0.45,
        }
    }
}

/// A light that the game adds, for example the robot's lamp.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointLight {
    /// World position in cells.
    pub pos: DVec2,
    /// Radius of the bright middle in cells. The light spreads farther than this.
    pub radius: f32,
    /// Linear RGB. 1.0 is about as bright as lava.
    pub color: [f32; 3],
}

/// Numbers about the renderer, for the stats panel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderStats {
    /// Chunks that have a layer on the GPU.
    pub resident_chunks: u32,
    /// The most chunks the GPU can keep.
    pub chunk_capacity: u32,
    /// Chunks uploaded by the last `apply_snapshot`.
    pub uploaded_chunks: u32,
    /// Chunks drawn in the last frame.
    pub drawn_chunks: u32,
    /// Particles drawn in the last frame.
    pub drawn_particles: u32,
    /// Size of the light map in the last frame (texels).
    pub light_size: (u32, u32),
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    target_format: wgpu::TextureFormat,
    /// The device limit `max_texture_dimension_2d`.
    max_texture_side: u32,

    frame_buffer: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,

    /// One 64 x 64 layer per chunk. Format `Rgba16Uint`; each texel is a `CellTexel`.
    cells: wgpu::Texture,
    layers: LayerMap,
    material_count: u32,
    /// Where sky light stops above the view, from the chunks on the GPU.
    sky: SkyColumns,

    targets: Option<Targets>,
    world_pass: WorldPass,
    light_pass: LightPass,
    composite_pass: CompositePass,

    /// Reused staging memory for chunk uploads.
    belt: wgpu::util::StagingBelt,
    /// Reused each frame.
    instances: Vec<ChunkInstance>,
    particle_instances: Vec<ParticleInstance>,
    light_instances: Vec<LightInstance>,
    sky_entry: Vec<f32>,
    /// The particles of the last snapshot.
    particles: Vec<ParticleView>,
    lights: Vec<PointLight>,
    settings: RenderSettings,
    surface_y: i32,
    world_cells: (i32, i32),
    time: f32,
    stats: RenderStats,
}

impl Renderer {
    /// A renderer that draws into targets of `target_format`, with the default options.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
        content: &Content,
    ) -> Self {
        Self::with_options(device, queue, target_format, content, RendererOptions::default())
    }

    pub fn with_options(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
        content: &Content,
        options: RendererOptions,
    ) -> Self {
        let dir = options.shader_dir.as_deref().filter(|d| d.is_dir());
        if options.shader_dir.is_some() && dir.is_none() {
            log::warn!("shader folder {:?} not found; using the built-in shaders", options.shader_dir);
        }

        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(std::mem::size_of::<FrameUniforms>() as u64),
                },
                count: None,
            }],
        });
        let frame_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame"),
            size: std::mem::size_of::<FrameUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame"),
            layout: &frame_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: frame_buffer.as_entire_binding() }],
        });

        // Cell texture array.
        let capacity = options.max_chunks.min(device.limits().max_texture_array_layers).max(1);
        let cells = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("world cells"),
            size: wgpu::Extent3d {
                width: CHUNK_SIZE as u32,
                height: CHUNK_SIZE as u32,
                depth_or_array_layers: capacity,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let cells_view = cells.create_view(&wgpu::TextureViewDescriptor {
            label: Some("world cells"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });

        // Palette: SHADES columns, one row per material.
        let material_count = content.materials.len() as u32;
        let palette_data = palette::build_palette(content);
        let palette_tex = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("palette"),
                size: wgpu::Extent3d { width: SHADES as u32, height: material_count, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            bytemuck::cast_slice(&palette_data),
        );
        let glow_data = palette::build_glow_lut();
        let glow_tex = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("glow lookup"),
                size: wgpu::Extent3d { width: GLOW_LUT_SIZE as u32, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            bytemuck::cast_slice(&glow_data),
        );
        let materials = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("material info"),
            contents: bytemuck::cast_slice(&palette::build_material_info(content)),
            usage: wgpu::BufferUsages::STORAGE,
        });

        let world_pass = WorldPass::new(
            device,
            &frame_layout,
            WorldInputs {
                cells: &cells_view,
                palette: &palette_tex.create_view(&Default::default()),
                glow_lut: &glow_tex.create_view(&Default::default()),
                materials: &materials,
            },
            capacity,
            dir,
        );
        let light_pass = LightPass::new(device, &frame_layout, dir);
        let composite_pass = CompositePass::new(device, &frame_layout, target_format, dir);

        Self {
            device: device.clone(),
            queue: queue.clone(),
            target_format,
            max_texture_side: device.limits().max_texture_dimension_2d,
            frame_buffer,
            frame_bind_group,
            cells,
            layers: LayerMap::new(capacity),
            material_count,
            sky: SkyColumns::new(palette::build_sky_blockers(content)),
            targets: None,
            world_pass,
            light_pass,
            composite_pass,
            // 4 MiB holds 128 chunks.
            belt: wgpu::util::StagingBelt::new(device.clone(), 4 << 20),
            instances: Vec::with_capacity(capacity as usize),
            particle_instances: Vec::new(),
            light_instances: Vec::new(),
            sky_entry: Vec::new(),
            particles: Vec::new(),
            lights: Vec::new(),
            settings: RenderSettings::default(),
            surface_y: 0,
            world_cells: (0, 0),
            time: 0.0,
            stats: RenderStats { chunk_capacity: capacity, ..Default::default() },
        }
    }

    pub fn target_format(&self) -> wgpu::TextureFormat {
        self.target_format
    }

    pub fn stats(&self) -> RenderStats {
        self.stats
    }

    pub fn settings(&self) -> &RenderSettings {
        &self.settings
    }

    pub fn settings_mut(&mut self) -> &mut RenderSettings {
        &mut self.settings
    }

    /// The row of the ground surface in this world (about). The sky color gets lighter toward it.
    /// Chunks the renderer has no data for count as open sky (air) when they are above this row,
    /// and as rock below it. Default: 0.
    pub fn set_surface_level(&mut self, y: i32) {
        self.surface_y = y;
    }

    /// The point lights of the next frames, for example the robot's lamp. At most 64 are used.
    pub fn set_lights(&mut self, lights: &[PointLight]) {
        self.lights.clear();
        self.lights.extend_from_slice(lights);
    }

    /// The most chunks the GPU can keep.
    pub fn chunk_capacity(&self) -> u32 {
        self.layers.capacity()
    }

    /// The smallest zoom at which the chunks that the simulation sends for this screen size fit on the GPU.
    /// `margin` is the extra border (in cells) of the view area that the game asks for.
    /// Below this zoom, chunks would be dropped and sent again all the time.
    pub fn min_zoom(&self, viewport: UVec2, margin: i32) -> f32 {
        let budget = (self.layers.capacity() as f64 * 0.9).max(1.0);
        let chunks_for = |zoom: f64| {
            let span = |px: u32| ((px as f64 / zoom + 2.0 * margin as f64) / CHUNK_SIZE as f64).ceil() + 1.0;
            span(viewport.x) * span(viewport.y)
        };
        let mut zoom = 0.25f64;
        while zoom < 64.0 && chunks_for(zoom) > budget {
            zoom *= 1.01;
        }
        zoom as f32
    }

    /// Set the animation time in seconds (for example the liquid color shift).
    pub fn set_time(&mut self, seconds: f64) {
        // Wrap so that f32 keeps enough precision.
        self.time = (seconds % 3600.0) as f32;
    }

    /// Forget all chunk data, for example for a new world.
    pub fn clear_chunks(&mut self) {
        self.layers.clear();
        self.sky.clear();
        self.particles.clear();
        self.stats.resident_chunks = 0;
    }

    /// Upload the chunks of a snapshot, and keep its particles. Uploads only the chunks in the
    /// snapshot.
    ///
    /// When the GPU has no free layer, the chunks farthest from `camera` are dropped.
    /// Returns the dropped chunks. Send them to the simulation in `Command::ForgetChunks`,
    /// so that it sends them again when they are needed.
    pub fn apply_snapshot(&mut self, snapshot: &Snapshot, camera: &Camera) -> Vec<ChunkPos> {
        self.world_cells = snapshot.world_cells;
        self.particles.clear();
        self.particles.extend_from_slice(&snapshot.particles);
        let mut evicted = Vec::new();
        self.stats.uploaded_chunks = 0;
        if snapshot.chunks.is_empty() {
            return evicted;
        }
        let view = camera.visible_rect();
        let mut encoder =
            self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("chunk upload") });
        let mut uploaded = 0;
        for image in &snapshot.chunks {
            if image.texels.len() != CHUNK_AREA {
                log::error!("chunk {:?} has {} texels; expected {CHUNK_AREA}", image.pos, image.texels.len());
                continue;
            }
            let Some(layer) = self.layers.assign(image.pos, view, &mut evicted) else { continue };
            self.upload_layer(&mut encoder, layer, &image.texels);
            self.sky.update(image.pos, &image.texels);
            uploaded += 1;
        }
        self.belt.finish_and_recall_on_submit(&encoder);
        self.queue.submit([encoder.finish()]);
        // A chunk that was dropped and then stored again in the same snapshot is not dropped.
        evicted.retain(|p| self.layers.get(*p).is_none());
        for p in &evicted {
            self.sky.remove(*p);
        }
        self.stats.uploaded_chunks = uploaded;
        self.stats.resident_chunks = self.layers.len() as u32;
        evicted
    }

    /// Copy one chunk into reused staging memory and add a copy command to its layer.
    fn upload_layer(&mut self, encoder: &mut wgpu::CommandEncoder, layer: u32, texels: &[CellTexel]) {
        let size = CHUNK_SIZE as u32;
        let bytes: &[u8] = bytemuck::cast_slice(texels);
        let slice = self.belt.allocate(
            wgpu::BufferSize::new(bytes.len() as u64).expect("a chunk has data"),
            wgpu::BufferSize::new(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64).expect("not zero"),
        );
        slice.get_mapped_range_mut().expect("staging memory is mapped").copy_from_slice(bytes);
        let (buffer, offset) = (slice.buffer().clone(), slice.offset());
        encoder.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset,
                    bytes_per_row: Some(size * std::mem::size_of::<CellTexel>() as u32),
                    rows_per_image: Some(size),
                },
            },
            wgpu::TexelCopyTextureInfo {
                texture: &self.cells,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: 0, z: layer },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        );
    }

    /// Draw one frame into `target`. The target size must be `camera.viewport`, and its format must be
    /// the `target_format` given to `new`. The passes clear the target first.
    pub fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, camera: &Camera) {
        if camera.viewport.x == 0 || camera.viewport.y == 0 {
            return;
        }
        // The world textures (the screen plus the margins) must fit in the device limit.
        let border = (2 * (LIGHT_MARGIN + ALIGN_CELLS) + 256) as f32;
        let max_side = (self.max_texture_side as f32 - border).max(64.0);
        let min_zoom = (camera.viewport.max_element() as f32 / max_side).max(1.0 / 64.0);
        let camera = Camera { zoom: camera.zoom.max(min_zoom), ..*camera };
        let s = self.settings;
        let lighting = s.lighting && !s.heat_map;

        // The part of the world the world textures hold: the screen plus the light margin, on the
        // grid of ALIGN_CELLS, so that light texels stay on the same cells when the camera moves.
        let top_left = camera.top_left();
        let bottom_right = top_left + camera.viewport.as_dvec2() / camera.zoom as f64;
        let align = ALIGN_CELLS as f64;
        let margin = LIGHT_MARGIN as f64;
        let down = |v: f64| ((v - margin) / align).floor() as i32 * ALIGN_CELLS;
        let up = |v: f64| ((v + margin) / align).ceil() as i32 * ALIGN_CELLS;
        let origin = IVec2::new(down(top_left.x), down(top_left.y));
        let end = IVec2::new(up(bottom_right.x), up(bottom_right.y));
        let used = (end - origin).as_uvec2();
        let light_used = used / LIGHT_CELLS;
        self.ensure_targets(used);
        let targets = self.targets.as_ref().expect("made by ensure_targets");

        // Chunks to draw: those in the area and inside the world. A world width of 0 means no
        // limit to the left and right.
        let mut area = CellRect::new(origin.x, origin.y, end.x, end.y);
        let (w, h) = self.world_cells;
        if h > 0 {
            let (x0, x1) = if w > 0 { (0, w) } else { (area.x0, area.x1) };
            area = area.intersect(&CellRect::new(x0, 0, x1, h));
        }
        self.instances.clear();
        for pos in area.chunks() {
            if let Some(layer) = self.layers.get(pos) {
                let o = pos.origin();
                self.instances.push(ChunkInstance { origin: [o.x, o.y], layer, _pad: 0 });
            }
        }
        let chunk_count = self.world_pass.write_instances(&self.queue, &self.instances);

        // Particles and lights, relative to the texture origin (so that f32 keeps whole cells).
        let rel = |x: f64, y: f64| [(x - origin.x as f64) as f32, (y - origin.y as f64) as f32];
        let (fw, fh) = (used.x as f32, used.y as f32);
        self.particle_instances.clear();
        self.particle_instances.extend(self.particles.iter().filter_map(|p| {
            let pos = rel(p.x as f64, p.y as f64);
            let inside = pos[0] >= -8.0 && pos[1] >= -8.0 && pos[0] < fw + 8.0 && pos[1] < fh + 8.0;
            inside.then_some(ParticleInstance {
                pos,
                vel: [p.vx, p.vy],
                material_temp: p.material as u32 | ((p.temperature as u16 as u32) << 16),
                shade: p.shade as u32,
            })
        }));
        let particle_count = self.world_pass.write_particles(&self.device, &self.queue, &self.particle_instances);

        let mut light_count = 0;
        if lighting {
            self.light_instances.clear();
            self.light_instances.extend(self.lights.iter().map(|l| LightInstance {
                pos: rel(l.pos.x, l.pos.y),
                radius: l.radius.max(1.0),
                _pad: 0.0,
                color: [l.color[0], l.color[1], l.color[2], 0.0],
            }));
            let open_above = self.surface_y;
            self.sky.entry(origin.x, origin.y, LIGHT_CELLS as i32, light_used.x as usize, open_above, &mut self.sky_entry);
            light_count = self.light_pass.write_inputs(
                &self.device,
                &self.queue,
                targets,
                &self.light_instances,
                &self.sky_entry,
            );
        }

        let mut bits = 0;
        for (on, bit) in [
            (lighting, flags::LIGHTING),
            (s.heat_map, flags::HEAT_MAP),
            (s.chunk_grid, flags::CHUNK_GRID),
            (s.bloom && lighting, flags::BLOOM),
            (s.heat_shimmer && lighting, flags::SHIMMER),
            (s.light_only && lighting, flags::LIGHT_ONLY),
        ] {
            if on {
                bits |= bit;
            }
        }
        let view_offset = top_left - origin.as_dvec2();
        let sky = s.sky_light.max(0.0);
        let uniforms = FrameUniforms {
            screen_size: camera.viewport.as_vec2().to_array(),
            world_tex_size: targets.size.as_vec2().to_array(),
            world_used_size: used.as_vec2().to_array(),
            view_offset: view_offset.as_vec2().to_array(),
            target_origin: origin.to_array(),
            origin_wrapped: [origin.x.rem_euclid(65536) as f32, origin.y as f32],
            world_cells: [self.world_cells.0 as f32, self.world_cells.1 as f32],
            light_tex_size: targets.light_size.as_vec2().to_array(),
            light_used: light_used.to_array(),
            zoom: camera.zoom,
            time: self.time,
            material_count: self.material_count,
            output_srgb: self.target_format.is_srgb() as u32,
            flags: bits,
            light_count,
            particle_count,
            surface_y: self.surface_y as f32,
            _pad: [0; 2],
            // A little warm, like daylight.
            sky_color: [sky, sky * 0.97, sky * 0.92, 0.0],
            ambient: [s.ambient, s.ambient, s.ambient * 1.15, 0.0],
            params: [s.light_keep.clamp(0.3, 0.99), s.bloom_strength, s.shimmer_strength, 1.0],
        };
        self.queue.write_buffer(&self.frame_buffer, 0, bytemuck::bytes_of(&uniforms));

        // 1. World pass: cells and particles into the world textures.
        self.world_pass.draw(encoder, targets, &self.frame_bind_group, used.to_array(), chunk_count, particle_count);
        // 2. Light pass.
        if lighting {
            self.light_pass.run(encoder, &self.frame_bind_group, light_used.to_array(), s.light_steps, s.bloom);
        }
        // 3. Composite pass into the target.
        self.composite_pass.draw(encoder, target, &self.frame_bind_group);

        self.stats.drawn_chunks = chunk_count;
        self.stats.drawn_particles = particle_count;
        self.stats.light_size = if lighting { (light_used.x, light_used.y) } else { (0, 0) };
    }

    /// Make the world and light textures larger if `needed` cells do not fit.
    fn ensure_targets(&mut self, needed: UVec2) {
        if self.targets.as_ref().is_some_and(|t| t.fits(needed)) {
            return;
        }
        // Keep the old size where it is larger, so that the textures do not change back and forth.
        let old = self.targets.as_ref().map_or(UVec2::ZERO, |t| t.size);
        let targets = Targets::new(&self.device, needed.max(old), self.max_texture_side);
        self.light_pass.set_targets(&self.device, &targets);
        self.composite_pass.set_targets(&self.device, &targets);
        self.targets = Some(targets);
    }
}
