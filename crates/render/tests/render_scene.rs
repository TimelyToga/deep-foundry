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
    // With no light pass every cell has its palette color.
    s.renderer.settings_mut().lighting = false;
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
    s.renderer.settings_mut().lighting = false;
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

#[test]
fn sprites_are_on_the_cell_grid_and_show_through_water() {
    use foundry_render::{Sprite, SpriteLayer};
    let Some(mut s) = scene(16) else { return };
    // No light pass: the test checks exact colors. (tests/light.rs checks sprites with light.)
    s.renderer.settings_mut().lighting = false;
    let water = s.content.expect_material("water").0;
    let mut snap = Snapshot { world_cells: (64, 64), ..Default::default() };
    // Water in the right half of the chunk.
    snap.chunks.push(chunk(ChunkPos::new(0, 0), |x, _| if x >= 32 { (water, 20) } else { (0, 20) }));
    // A 2 x 1 sheet: a red texel and a green texel.
    s.renderer.set_sprite_sheet(2, 1, &[255, 0, 0, 255, 0, 255, 0, 255]);
    let sprite = |cell: [i32; 2], flip_x: bool, layer| Sprite { cell, src: [0, 0], size: [2, 1], tint: [255; 4], flip_x, layer };
    s.renderer.set_sprites(&[
        sprite([10, 10], false, SpriteLayer::Body),
        sprite([10, 20], true, SpriteLayer::Body),
        sprite([40, 10], false, SpriteLayer::Body),
        sprite([40, 20], false, SpriteLayer::Front),
    ]);
    // A camera a quarter cell off the grid, at zoom 4.
    let camera = Camera::new(DVec2::new(32.25, 32.0), 4.0, UVec2::new(256, 256));
    s.renderer.apply_snapshot(&snap, &camera);
    let img = capture(&s.device, &s.queue, &mut s.renderer, &camera);
    let at = |cx: f64, cy: f64| {
        let p = camera.cell_to_screen(DVec2::new(cx + 0.5, cy + 0.5));
        pixel(&img, 256, p.x as u32, p.y as u32)
    };
    // In air: exact colors, in the right cells, flipped when asked.
    assert_eq!(at(10.0, 10.0), [255, 0, 0, 255]);
    assert_eq!(at(11.0, 10.0), [0, 255, 0, 255]);
    assert_eq!(at(10.0, 20.0), [0, 255, 0, 255]);
    assert_eq!(at(11.0, 20.0), [255, 0, 0, 255]);
    // The sprite ends at the cell edge: the cell after it is air.
    let air = at(12.0, 10.0);
    assert!(air[0] < 60 && air[1] < 60, "{air:?}");
    // In water, the body sprite is partly covered: red, but mixed with the water blue.
    let wet = at(40.0, 10.0);
    assert!(wet[0] > 90 && wet[0] < 230 && wet[2] > 60, "body in water {wet:?}");
    // A front sprite is over the water.
    assert_eq!(at(40.0, 20.0), [255, 0, 0, 255]);
}

/// Upload speed. Run with `cargo test --release -p foundry_render -- --ignored --nocapture`.
#[test]
#[ignore]
fn upload_speed() {
    let Some(mut s) = scene(2048) else { return };
    let stone = s.content.expect_material("stone").0;
    let mut snap = Snapshot { world_cells: (64 * 32, 64 * 16), ..Default::default() };
    for y in 0..16 {
        for x in 0..32 {
            snap.chunks.push(chunk(ChunkPos::new(x, y), |_, _| (stone, 20)));
        }
    }
    let camera = Camera::new(DVec2::new(1024.0, 512.0), 1.0, UVec2::new(2048, 1024));
    let mut times = vec![];
    for _ in 0..30 {
        let t = std::time::Instant::now();
        s.renderer.apply_snapshot(&snap, &camera);
        times.push(t.elapsed().as_secs_f64() * 1000.0);
        s.device.poll(foundry_render::wgpu::PollType::wait_indefinitely()).unwrap();
    }
    let steady = &times[5..];
    let avg = steady.iter().sum::<f64>() / steady.len() as f64;
    println!(
        "upload of {} chunks: first {:.2} ms, then average {:.2} ms ({:.1} us per chunk)",
        snap.chunks.len(),
        times[0],
        avg,
        avg * 1000.0 / snap.chunks.len() as f64
    );
}

/// Writes out/glow_ramp.png: stone, lava, water and smoke from 300 °C (left) to 1800 °C (right),
/// to check the glow colors by eye. Run with `cargo test -p foundry_render -- --ignored`.
#[test]
#[ignore]
fn glow_ramp_image() {
    let Some(mut s) = scene(16) else { return };
    let rows = ["stone", "lava", "water", "smoke"].map(|id| s.content.expect_material(id).0);
    let mut snap = Snapshot { world_cells: (64 * 4, 64), ..Default::default() };
    for cx in 0..4 {
        snap.chunks.push(chunk(ChunkPos::new(cx, 0), |x, y| {
            let wx = cx * CHUNK_SIZE + x;
            let t = 300 + wx * 1500 / (4 * CHUNK_SIZE);
            (rows[(y / 16) as usize], t as i16)
        }));
    }
    let camera = Camera::new(DVec2::new(128.0, 32.0), 4.0, UVec2::new(1024, 256));
    s.renderer.apply_snapshot(&snap, &camera);
    let img = capture(&s.device, &s.queue, &mut s.renderer, &camera);
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/glow_ramp.png");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    image::RgbaImage::from_raw(1024, 256, img).unwrap().save(&out).unwrap();
    println!("saved {}", out.display());
}
