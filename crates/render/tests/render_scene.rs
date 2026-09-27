//! Render small scenes on the GPU and check pixels. The tests do nothing if there is no GPU.

use foundry_content::Content;
use foundry_core::{CHUNK_SIZE, ChunkImage, ChunkPos, Snapshot, local_index, pack_texel};
use foundry_render::headless::{CAPTURE_FORMAT, capture, create_device};
use foundry_render::{Camera, Renderer, RendererOptions};
use glam::{DVec2, UVec2};

struct Scene {
    device: foundry_render::wgpu::Device,
    queue: foundry_render::wgpu::Queue,
    renderer: Renderer,
    content: Content,
}

fn scene(max_chunks: u32) -> Option<Scene> {
    let Some((device, queue)) = create_device() else {
        eprintln!("no GPU: test skipped");
        return None;
    };
    let content = Content::load_default().unwrap();
    let options = RendererOptions { max_chunks, ..Default::default() };
    let renderer = Renderer::with_options(&device, &queue, CAPTURE_FORMAT, &content, options);
    Some(Scene { device, queue, renderer, content })
}

/// A chunk where `f(local x, local y)` gives (material, temperature) for each cell.
fn chunk(pos: ChunkPos, f: impl Fn(i32, i32) -> (u16, i16)) -> ChunkImage {
    let mut c = ChunkImage::new_air(pos);
    for y in 0..CHUNK_SIZE {
        for x in 0..CHUNK_SIZE {
            let (m, t) = f(x, y);
            c.texels[local_index(x, y)] = pack_texel(m, t, (x + y) as u8, 0, 0);
        }
    }
    c
}

fn pixel(img: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * width + x) * 4) as usize;
    [img[i], img[i + 1], img[i + 2], img[i + 3]]
}

#[test]
fn draws_cells_in_the_right_place_with_the_right_colors() {
    let Some(mut s) = scene(64) else { return };
    let stone = s.content.expect_material("stone").0;
    let water = s.content.expect_material("water").0;
    let mut snap = Snapshot { world_cells: (128, 128), ..Default::default() };
    // Chunk (0,0): stone in the lower half. Chunk (1,0): water. Chunk (0,1): stone at 1500 °C.
    // Chunk (1,1) is not sent, so it must look like air.
    snap.chunks.push(chunk(ChunkPos::new(0, 0), |_, y| if y >= 32 { (stone, 20) } else { (0, 20) }));
    snap.chunks.push(chunk(ChunkPos::new(1, 0), |_, _| (water, 20)));
    snap.chunks.push(chunk(ChunkPos::new(0, 1), |_, _| (stone, 1500)));
    let camera = Camera::new(DVec2::new(64.0, 64.0), 2.0, UVec2::new(256, 256));
    let evicted = s.renderer.apply_snapshot(&snap, &camera);
    assert!(evicted.is_empty());
    let img = capture(&s.device, &s.queue, &mut s.renderer, &camera);
    assert_eq!(img.len(), 256 * 256 * 4);

    // Cell (10, 40) is stone. At zoom 2 it covers pixels (20..22, 80..82).
    let stone_colors: Vec<[u8; 4]> = s.content.materials.colors[stone as usize].clone();
    let p = pixel(&img, 256, 20, 80);
    assert!(stone_colors.contains(&p), "stone pixel {p:?} not in {stone_colors:?}");
    // Whole zoom: both pixels of one cell are the same (crisp).
    assert_eq!(pixel(&img, 256, 21, 81), p);

    // Cell (10, 10) is air: the dark background shows.
    let air = pixel(&img, 256, 20, 20);
    assert!(air[0] < 60 && air[1] < 60 && air[2] < 80, "air shows the background, got {air:?}");
    // The chunk that was never sent also shows the background.
    let missing = pixel(&img, 256, 200, 200);
    assert!(missing[0] < 60 && missing[2] < 80, "missing chunk is air, got {missing:?}");

    // Water is blue.
    let w = pixel(&img, 256, 180, 40);
    assert!(w[2] > w[0] + 60, "water pixel {w:?}");

    // Hot stone glows: bright, with more red than blue.
    let hot = pixel(&img, 256, 40, 180);
    assert!(hot[0] > 200 && hot[0] > hot[2], "hot stone pixel {hot:?}");
}

#[test]
fn full_gpu_drops_far_chunks() {
    let Some(mut s) = scene(2) else { return };
    let stone = s.content.expect_material("stone").0;
    let mut snap = Snapshot { world_cells: (64 * 8, 64), ..Default::default() };
    for x in [0, 1, 7] {
        snap.chunks.push(chunk(ChunkPos::new(x, 0), |_, _| (stone, 20)));
    }
    let camera = Camera::new(DVec2::new(32.0, 32.0), 1.0, UVec2::new(64, 64));
    let evicted = s.renderer.apply_snapshot(&snap, &camera);
    assert_eq!(evicted.len(), 1);
    assert_eq!(s.renderer.stats().resident_chunks, 2);
    // The visible chunk (0,0) is kept.
    assert_ne!(evicted[0], ChunkPos::new(0, 0));
}

#[test]
fn fractional_camera_moves_smoothly() {
    let Some(mut s) = scene(16) else { return };
    let stone = s.content.expect_material("stone").0;
    let mut snap = Snapshot { world_cells: (64, 64), ..Default::default() };
    // A vertical stone edge at x = 32.
    snap.chunks.push(chunk(ChunkPos::new(0, 0), |x, _| if x >= 32 { (stone, 20) } else { (0, 20) }));
    let mut edges = vec![];
    for step in 0..5 {
        let camera = Camera::new(DVec2::new(32.0 + step as f64 * 0.25, 32.0), 4.0, UVec2::new(64, 64));
        s.renderer.apply_snapshot(&snap, &camera);
        let img = capture(&s.device, &s.queue, &mut s.renderer, &camera);
        // First pixel of row 32 that is bright (stone).
        let edge = (0..64).find(|&x| pixel(&img, 64, x, 32)[0] > 80).unwrap();
        edges.push(edge);
        snap.chunks.clear();
    }
    // Moving the camera right by 1/4 cell moves the edge left by 1 pixel at zoom 4.
    assert_eq!(edges, vec![32, 31, 30, 29, 28]);
}
