//! The `Renderer`: GPU resources for the world and the order of the passes.

use crate::camera::Camera;
use crate::frame::{ChunkInstance, FrameUniforms};
use crate::layers::LayerMap;
use crate::palette::{self, GLOW_LUT_SIZE, SHADES};
use crate::passes::background::BackgroundPass;
use crate::passes::scale::ScalePass;
use crate::passes::world::{WORLD_COLOR_FORMAT, WorldInputs, WorldPass};
use crate::shaders;
use crate::sprite::{Sprite, SpritePass};
use foundry_content::Content;
use foundry_core::{CHUNK_AREA, CHUNK_SIZE, CellRect, CellTexel, ChunkPos, Snapshot};
use glam::{IVec2, UVec2};
use std::num::NonZeroU64;
use std::path::PathBuf;
use wgpu::util::DeviceExt;

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
}

/// The offscreen texture that the world pass draws into (1 texel per cell).
struct WorldTarget {
    view: wgpu::TextureView,
    size: UVec2,
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

    world_target: Option<WorldTarget>,
    world_pass: WorldPass,
    background_pass: BackgroundPass,
    scale_pass: ScalePass,
    /// Sprites (the robot) drawn into the world texture after the cells (`sprite.rs`).
    sprite_pass: SpritePass,

    /// Reused staging memory for chunk uploads.
    belt: wgpu::util::StagingBelt,
    /// Reused each frame.
    instances: Vec<ChunkInstance>,
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
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
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
        let background_pass = BackgroundPass::new(device, &frame_layout, target_format, dir);
        let scale_pass = ScalePass::new(device, &frame_layout, target_format, dir);
        let sprite_pass = SpritePass::new(device, &frame_layout, WORLD_COLOR_FORMAT, dir);

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
            world_target: None,
            world_pass,
            background_pass,
            scale_pass,
            sprite_pass,
            // 4 MiB holds 128 chunks.
            belt: wgpu::util::StagingBelt::new(device.clone(), 4 << 20),
            instances: Vec::with_capacity(capacity as usize),
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

    /// Set the sprite sheet: RGBA8 texels (sRGB colors, not premultiplied), row by row from the top.
    pub fn set_sprite_sheet(&mut self, width: u32, height: u32, rgba: &[u8]) {
        self.sprite_pass.set_sheet(&self.device, &self.queue, width, height, rgba);
    }

    /// The sprites to draw from now on (until the next call). See `sprite.rs`.
    pub fn set_sprites(&mut self, sprites: &[Sprite]) {
        self.sprite_pass.set_sprites(&self.device, &self.queue, sprites);
    }

    /// Forget all chunk data, for example for a new world.
    pub fn clear_chunks(&mut self) {
        self.layers.clear();
        self.stats.resident_chunks = 0;
    }

    /// Upload the chunks of a snapshot. Uploads only the chunks in the snapshot.
    ///
    /// When the GPU has no free layer, the chunks farthest from `camera` are dropped.
    /// Returns the dropped chunks. Send them to the simulation in `Command::ForgetChunks`,
    /// so that it sends them again when they are needed.
    pub fn apply_snapshot(&mut self, snapshot: &Snapshot, camera: &Camera) -> Vec<ChunkPos> {
        self.world_cells = snapshot.world_cells;
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
            uploaded += 1;
        }
        self.belt.finish_and_recall_on_submit(&encoder);
        self.queue.submit([encoder.finish()]);
        // A chunk that was dropped and then stored again in the same snapshot is not dropped.
        evicted.retain(|p| self.layers.get(*p).is_none());
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
        // The world texture must fit in the device limit.
        let max_side = self.max_texture_side as f32 - 4.0;
        let min_zoom = (camera.viewport.max_element() as f32 / max_side).max(1.0 / 64.0);
        let camera = Camera { zoom: camera.zoom.max(min_zoom), ..*camera };

        // The part of the world the world texture holds: the screen plus one cell on each side.
        let top_left = camera.top_left();
        let bottom_right = top_left + camera.viewport.as_dvec2() / camera.zoom as f64;
        let origin = IVec2::new(top_left.x.floor() as i32 - 1, top_left.y.floor() as i32 - 1);
        let end = IVec2::new(bottom_right.x.ceil() as i32 + 1, bottom_right.y.ceil() as i32 + 1);
        let used = (end - origin).as_uvec2();
        let tex_size = self.ensure_world_target(used);

        // Chunks to draw: those on the screen and inside the world. A world width of 0 means no
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
        let count = self.world_pass.write_instances(&self.queue, &self.instances);
        self.stats.drawn_chunks = count;

        let view_offset = top_left - origin.as_dvec2();
        let uniforms = FrameUniforms {
            screen_size: camera.viewport.as_vec2().to_array(),
            world_tex_size: tex_size.as_vec2().to_array(),
            world_used_size: used.as_vec2().to_array(),
            view_offset: view_offset.as_vec2().to_array(),
            target_origin: origin.to_array(),
            world_cells: [self.world_cells.0 as f32, self.world_cells.1 as f32],
            view_top_left: top_left.as_vec2().to_array(),
            zoom: camera.zoom,
            time: self.time,
            material_count: self.material_count,
            output_srgb: self.target_format.is_srgb() as u32,
            _pad: [0; 2],
        };
        self.queue.write_buffer(&self.frame_buffer, 0, bytemuck::bytes_of(&uniforms));

        // 1. World pass: cells into the offscreen texture.
        let world_view = &self.world_target.as_ref().expect("made by ensure_world_target").view;
        self.world_pass.draw(encoder, world_view, &self.frame_bind_group, used.to_array(), count);
        self.sprite_pass.draw(encoder, world_view, &self.frame_bind_group, used.to_array());

        // 2. Background and 3. scale pass, into the target.
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("screen"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            ..Default::default()
        });
        self.background_pass.draw(&mut pass, &self.frame_bind_group);
        self.scale_pass.draw(&mut pass, &self.frame_bind_group);
    }

    /// Make the world texture larger if `needed` does not fit. Returns its size.
    fn ensure_world_target(&mut self, needed: UVec2) -> UVec2 {
        if let Some(t) = &self.world_target
            && t.size.x >= needed.x
            && t.size.y >= needed.y
        {
            return t.size;
        }
        // Round up, so that small zoom changes do not make a new texture each frame.
        let old = self.world_target.as_ref().map_or(UVec2::ZERO, |t| t.size);
        let size = needed.max(old).map(|v| (v.div_ceil(256) * 256).min(self.max_texture_side));
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("world color"),
            size: wgpu::Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: WORLD_COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        self.scale_pass.set_source(&self.device, &view);
        self.world_target = Some(WorldTarget { view, size });
        size
    }
}
