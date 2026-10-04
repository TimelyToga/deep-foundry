//! GPU setup and rendering with no window: for screenshots, tools and tests.

use crate::camera::Camera;
use crate::renderer::Renderer;

/// The format that `capture` needs. Make the `Renderer` with this format.
pub const CAPTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The device settings the renderer needs. It asks for more texture array layers than the
/// default limit (up to 2048), so that more chunks fit on the GPU.
pub fn device_descriptor(adapter: &wgpu::Adapter) -> wgpu::DeviceDescriptor<'static> {
    let supported = adapter.limits();
    wgpu::DeviceDescriptor {
        label: Some("timtech"),
        required_limits: wgpu::Limits {
            max_texture_array_layers: supported.max_texture_array_layers.min(2048),
            max_texture_dimension_2d: supported.max_texture_dimension_2d.min(16384),
            ..wgpu::Limits::default()
        },
        ..Default::default()
    }
}

/// A GPU device with no window. `None` if there is no usable GPU.
pub fn create_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .ok()?;
    pollster::block_on(adapter.request_device(&device_descriptor(&adapter))).ok()
}

/// Render one frame into a new texture of `camera.viewport` size and read it back.
/// Returns RGBA8 pixels, row by row from the top, with no padding.
/// The renderer must use `CAPTURE_FORMAT`.
pub fn capture(device: &wgpu::Device, queue: &wgpu::Queue, renderer: &mut Renderer, camera: &Camera) -> Vec<u8> {
    capture_with(device, queue, renderer, camera, |_, _| {})
}

/// Like `capture`, but `overlay` can draw more on top of the frame (for example the egui panel)
/// before the image is read back. It gets the command encoder and the target view.
pub fn capture_with(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut Renderer,
    camera: &Camera,
    overlay: impl FnOnce(&mut wgpu::CommandEncoder, &wgpu::TextureView),
) -> Vec<u8> {
    assert_eq!(renderer.target_format(), CAPTURE_FORMAT, "make the renderer with CAPTURE_FORMAT");
    let (w, h) = (camera.viewport.x.max(1), camera.viewport.y.max(1));
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("capture"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: CAPTURE_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let row_bytes = w * 4;
    let padded_row = row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("capture readback"),
        size: padded_row as u64 * h as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("capture") });
    renderer.render(&mut encoder, &view, camera);
    overlay(&mut encoder, &view);
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded_row), rows_per_image: Some(h) },
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    queue.submit([encoder.finish()]);

    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| {
        if let Err(e) = r {
            log::error!("capture: cannot read the texture back: {e}");
        }
    });
    device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU poll failed");
    let data = slice.get_mapped_range().expect("capture buffer is mapped");
    let mut out = Vec::with_capacity((row_bytes * h) as usize);
    for row in data.chunks(padded_row as usize) {
        out.extend_from_slice(&row[..row_bytes as usize]);
    }
    drop(data);
    buffer.unmap();
    out
}
