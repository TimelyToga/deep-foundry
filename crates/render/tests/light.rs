//! Tests of the light pass on the GPU: sky light, dark caves, light from lava, point lights,
//! particles. Also the frame time benchmark and images to check the look by eye.
//! The tests do nothing if there is no GPU.

use foundry_content::Content;
use foundry_core::{CHUNK_SIZE, ChunkImage, ChunkPos, ParticleView, Snapshot, local_index, pack_texel};
use foundry_render::headless::{CAPTURE_FORMAT, capture, create_device};
use foundry_render::{Camera, PointLight, Renderer, RendererOptions, wgpu};
use glam::{DVec2, UVec2};

struct Scene {
    device: wgpu::Device,
    queue: wgpu::Queue,
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

impl Scene {
    fn id(&self, name: &str) -> u16 {
        self.content.expect_material(name).0
    }

    /// Upload the snapshot and render one image.
    fn shot(&mut self, snap: &Snapshot, camera: &Camera) -> Vec<u8> {
        self.renderer.apply_snapshot(snap, camera);
        capture(&self.device, &self.queue, &mut self.renderer, camera)
    }
}

/// A chunk where `f(world x, world y)` gives (material, temperature) for each cell.
fn chunk(pos: ChunkPos, f: impl Fn(i32, i32) -> (u16, i16)) -> ChunkImage {
    let mut c = ChunkImage::new_air(pos);
    let o = pos.origin();
    for y in 0..CHUNK_SIZE {
        for x in 0..CHUNK_SIZE {
            let (m, t) = f(o.x + x, o.y + y);
            c.texels[local_index(x, y)] = pack_texel(m, t, ((x * 7 + y * 13) % 8) as u8, 0, 0);
        }
    }
    c
}

/// A snapshot of `w` x `h` chunks from (0, 0), made by `f(world x, world y)`.
fn world(w: i32, h: i32, f: impl Fn(i32, i32) -> (u16, i16)) -> Snapshot {
    let mut snap = Snapshot { world_cells: (0, h * CHUNK_SIZE), ..Default::default() };
    for cy in 0..h {
        for cx in 0..w {
            snap.chunks.push(chunk(ChunkPos::new(cx, cy), &f));
        }
    }
    snap
}

/// Brightness (0 to 255 x 3) of the pixel that shows world cell (x, y).
fn brightness(img: &[u8], camera: &Camera, x: f64, y: f64) -> u32 {
    let p = camera.cell_to_screen(DVec2::new(x + 0.5, y + 0.5));
    let i = ((p.y as u32 * camera.viewport.x + p.x as u32) * 4) as usize;
    img[i] as u32 + img[i + 1] as u32 + img[i + 2] as u32
}

#[test]
fn sky_lights_the_surface_but_not_a_closed_cave() {
    let Some(mut s) = scene(64) else { return };
    let stone = s.id("stone");
    // Air above y = 100. Stone below, with a closed cave around (128, 180).
    let snap = world(4, 4, |x, y| {
        let (dx, dy) = (x - 128, y - 180);
        if y < 100 || dx * dx + dy * dy < 30 * 30 { (0, 20) } else { (stone, 20) }
    });
    s.renderer.set_surface_level(100);
    let camera = Camera::new(DVec2::new(128.0, 128.0), 2.0, UVec2::new(512, 512));
    let img = s.shot(&snap, &camera);
    let surface = brightness(&img, &camera, 60.0, 101.0);
    let cave_floor = brightness(&img, &camera, 128.0, 211.0);
    let deep = brightness(&img, &camera, 30.0, 240.0);
    assert!(surface > 150, "the sunlit stone surface is bright: {surface}");
    assert!(cave_floor * 3 < surface, "the closed cave is dark: {cave_floor} (surface {surface})");
    assert!(deep * 3 < surface, "deep stone is dark: {deep} (surface {surface})");

    // With no light pass, the cave floor has its full color.
    s.renderer.settings_mut().lighting = false;
    let img = s.shot(&Snapshot { world_cells: snap.world_cells, ..Default::default() }, &camera);
    assert!(brightness(&img, &camera, 128.0, 211.0) > 150);
}

#[test]
fn lava_lights_its_cave_and_the_rock_blocks_it() {
    let Some(mut s) = scene(64) else { return };
    let (stone, lava) = (s.id("stone"), s.id("lava"));
    // All underground (no sky). A cave from x = 40 to 200, y = 100 to 160, lava at its left end.
    // A second cave behind 40 cells of rock, x = 240 to 250.
    let snap = world(5, 4, |x, y| {
        let in_cave = (40..200).contains(&x) && (100..160).contains(&y);
        let in_second = (240..250).contains(&x) && (100..160).contains(&y);
        if in_cave && x < 60 && y >= 130 {
            (lava, 1200)
        } else if in_cave || in_second {
            (0, 20)
        } else {
            (stone, 20)
        }
    });
    s.renderer.set_surface_level(-1000);
    let camera = Camera::new(DVec2::new(150.0, 130.0), 2.0, UVec2::new(640, 400));
    let img = s.shot(&snap, &camera);
    let near_floor = brightness(&img, &camera, 70.0, 160.0);
    let far_floor = brightness(&img, &camera, 195.0, 160.0);
    let behind_rock = brightness(&img, &camera, 245.0, 160.0);
    assert!(near_floor > 120, "the floor next to the lava is lit: {near_floor}");
    assert!(near_floor > far_floor + 40, "light gets weaker with distance: {near_floor} vs {far_floor}");
    assert!(behind_rock < 45, "40 cells of rock stop the light: {behind_rock}");
}

#[test]
fn a_point_light_lights_a_dark_cave() {
    let Some(mut s) = scene(64) else { return };
    let stone = s.id("stone");
    let snap = world(4, 4, |x, y| if (40..200).contains(&x) && (100..160).contains(&y) { (0, 20) } else { (stone, 20) });
    s.renderer.set_surface_level(-1000);
    let camera = Camera::new(DVec2::new(120.0, 130.0), 2.0, UVec2::new(512, 300));
    let dark = brightness(&s.shot(&snap, &camera), &camera, 100.0, 160.0);
    s.renderer.set_lights(&[PointLight { pos: DVec2::new(100.0, 140.0), radius: 6.0, color: [1.0, 1.0, 1.0] }]);
    let lit = brightness(&s.shot(&Snapshot { world_cells: snap.world_cells, ..Default::default() }, &camera), &camera, 100.0, 160.0);
    assert!(dark < 45 && lit > dark + 60, "the lamp lights the floor: {dark} -> {lit}");
}

#[test]
fn particles_are_drawn_and_hot_ones_glow() {
    let Some(mut s) = scene(64) else { return };
    let (stone, sand) = (s.id("stone"), s.id("sand"));
    // A dark closed room. One cold sand particle and one hot stone particle in it.
    let mut snap = world(3, 3, |x, y| if (40..150).contains(&x) && (40..150).contains(&y) { (0, 20) } else { (stone, 20) });
    s.renderer.set_surface_level(-1000);
    snap.particles = vec![
        ParticleView { x: 60.5, y: 80.5, vx: 0.0, vy: 0.0, material: sand, temperature: 20, shade: 0 },
        ParticleView { x: 120.5, y: 80.5, vx: 2.0, vy: 0.0, material: stone, temperature: 1400, shade: 0 },
    ];
    let camera = Camera::new(DVec2::new(96.0, 96.0), 4.0, UVec2::new(512, 512));
    s.renderer.settings_mut().lighting = false;
    let img = s.shot(&snap, &camera);
    assert!(brightness(&img, &camera, 60.0, 80.0) > 200, "the sand particle is drawn");
    assert_eq!(s.renderer.stats().drawn_particles, 2);
    // The streak goes back along the velocity: the cell behind the hot particle has color too.
    assert!(brightness(&img, &camera, 119.0, 80.0) > 100, "the streak behind a moving particle");
    // With light: the hot particle glows and lights the room around it.
    s.renderer.settings_mut().lighting = true;
    let img = s.shot(&Snapshot { particles: snap.particles.clone(), world_cells: snap.world_cells, ..Default::default() }, &camera);
    assert!(brightness(&img, &camera, 120.0, 80.0) > 250, "the hot particle glows");
    assert!(brightness(&img, &camera, 120.0, 149.0) > brightness(&img, &camera, 45.0, 149.0) + 20, "and lights the floor below it");
}

#[test]
fn heat_shimmer_moves_only_the_cells_above_hot_places() {
    let Some(mut s) = scene(64) else { return };
    let (stone, lava) = (s.id("stone"), s.id("lava"));
    // A cave with lava on the left half of its floor, and stone pillars with fine detail above it
    // (the shimmer moves them sideways).
    let snap = world(4, 3, |x, y| {
        if !(20..236).contains(&x) || !(20..170).contains(&y) {
            return (stone, 20);
        }
        if y >= 150 {
            return if x < 120 { (lava, 1200) } else { (stone, 20) };
        }
        if y < 120 && x % 5 == 0 { (stone, 20) } else { (0, 20) }
    });
    s.renderer.set_surface_level(-1000);
    s.renderer.set_time(0.7);
    s.renderer.settings_mut().bloom = false;
    let camera = Camera::new(DVec2::new(128.0, 96.0), 4.0, UVec2::new(1024, 768));
    s.renderer.settings_mut().heat_shimmer = false;
    let still = s.shot(&snap, &camera);
    s.renderer.settings_mut().heat_shimmer = true;
    let moved = capture(&s.device, &s.queue, &mut s.renderer, &camera);
    // Count the pixels that changed in a band just above the lava, and in the same band on the
    // cold side.
    let changed = |x0: f64, x1: f64| {
        let a = camera.cell_to_screen(DVec2::new(x0, 118.0));
        let b = camera.cell_to_screen(DVec2::new(x1, 148.0));
        let mut n = 0;
        for py in a.y as u32..b.y as u32 {
            for px in a.x as u32..b.x as u32 {
                let i = ((py * 1024 + px) * 4) as usize;
                n += (still[i..i + 3] != moved[i..i + 3]) as u32;
            }
        }
        n
    };
    let hot = changed(30.0, 110.0);
    let cold = changed(140.0, 220.0);
    assert!(hot > 200, "the shimmer moves pixels above the lava: {hot}");
    assert_eq!(cold, 0, "nothing moves above cold stone");
}

/// A test world: air above row `surface`, stone below with round caves; lava or water in some caves.
fn cave_world(s: &Scene, chunks: (i32, i32), surface: i32) -> Snapshot {
    let (stone, lava, water) = (s.id("stone"), s.id("lava"), s.id("water"));
    world(chunks.0, chunks.1, |x, y| {
        if y < surface {
            return (0, 20);
        }
        // One round cave in every block of 160 x 120 cells.
        let (bx, by) = (x.rem_euclid(160) - 80, (y - surface).rem_euclid(120) - 60);
        if bx * bx * 4 + by * by * 9 < 60 * 60 * 4 && y > surface + 40 {
            if by > 18 {
                return match (x.div_euclid(160) + (y - surface).div_euclid(120)) % 3 {
                    0 => (lava, 1200),
                    1 => (water, 20),
                    _ => (0, 20),
                };
            }
            return (0, 20);
        }
        (stone, 20)
    })
}

/// Time of one frame at 2560 x 1440 (mostly GPU time), with and without the light pass.
/// Run with `cargo test --release -p foundry_render --test light -- --ignored --nocapture frame_cost`.
#[test]
#[ignore]
fn frame_cost() {
    let Some(mut s) = scene(2048) else { return };
    let snap = cave_world(&s, (48, 30), 400);
    let target = s.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: 2560, height: 1440, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: CAPTURE_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    s.renderer.set_surface_level(400);
    for zoom in [1.0f32, 2.0, 3.0, 4.0] {
        let camera = Camera::new(DVec2::new(1536.0, 400.0 + 1440.0 / zoom as f64 * 0.3), zoom, UVec2::new(2560, 1440));
        s.renderer.apply_snapshot(&snap, &camera);
        for lighting in [false, true] {
            s.renderer.settings_mut().lighting = lighting;
            // Many frames in a row, then one wait: the time per frame is the GPU time (the GPU
            // works on one frame while the CPU makes the next).
            let frames = 60;
            let mut best = f64::MAX;
            for _ in 0..3 {
                let t = std::time::Instant::now();
                for _ in 0..frames {
                    let mut encoder = s.device.create_command_encoder(&Default::default());
                    s.renderer.render(&mut encoder, &view, &camera);
                    s.queue.submit([encoder.finish()]);
                }
                s.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                best = best.min(t.elapsed().as_secs_f64() * 1000.0 / frames as f64);
            }
            println!(
                "zoom {zoom}: lighting {lighting:5}: {best:.2} ms per frame, light map {:?}, chunks {}",
                s.renderer.stats().light_size,
                s.renderer.stats().drawn_chunks
            );
        }
    }
}

/// Writes out/light_caves.png: the test cave world with the light pass, to check the look by eye.
/// Run with `cargo test -p foundry_render --test light -- --ignored light_caves_image`.
#[test]
#[ignore]
fn light_caves_image() {
    let Some(mut s) = scene(2048) else { return };
    let snap = cave_world(&s, (16, 12), 200);
    s.renderer.set_surface_level(200);
    let camera = Camera::new(DVec2::new(512.0, 380.0), 2.0, UVec2::new(1600, 900));
    let img = s.shot(&snap, &camera);
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/light_caves.png");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    image::RgbaImage::from_raw(1600, 900, img).unwrap().save(&out).unwrap();
    println!("saved {}", out.display());
}

/// One cell of a look scene: material, temperature, life, flags.
type Cell = (u16, i16, u8, u8);

/// A snapshot of `w` x `h` chunks from (0, 0), made by `f(world x, world y)`.
fn world_full(w: i32, h: i32, f: impl Fn(i32, i32) -> Cell) -> Snapshot {
    let mut snap = Snapshot { world_cells: (0, h * CHUNK_SIZE), ..Default::default() };
    for cy in 0..h {
        for cx in 0..w {
            let pos = ChunkPos::new(cx, cy);
            let mut c = ChunkImage::new_air(pos);
            for y in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    let (wx, wy) = (cx * CHUNK_SIZE + x, cy * CHUNK_SIZE + y);
                    let (m, t, life, flags) = f(wx, wy);
                    let shade = (hash(wx, wy) % 8) as u8;
                    c.texels[local_index(x, y)] = pack_texel(m, t, shade, life, flags);
                }
            }
            snap.chunks.push(c);
        }
    }
    snap
}

fn hash(x: i32, y: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9e37_79b1) ^ (y as u32).wrapping_mul(0x85eb_ca77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^ (h >> 12)
}

/// Writes out/look_fire.png: a dark cave with burning wood, fire, smoke, lava with steam above
/// it, molten steel, hot stone from 600 to 1500 °C, and hot sparks. To check the look by eye.
/// Run with `cargo test -p foundry_render --test light -- --ignored look_image`.
#[test]
#[ignore]
fn look_image() {
    let Some(mut s) = scene(256) else { return };
    let [stone, wood, fire, smoke, lava, steam, steel, water] =
        ["stone", "wood", "fire", "smoke", "lava", "steam", "molten_steel", "water"].map(|n| s.id(n));
    const BURNING: u8 = 1 << 2;
    let snap = world_full(6, 3, |x, y| {
        let h = hash(x, y);
        let cave = (16..368).contains(&x) && (24..172).contains(&y);
        if !cave {
            return (stone, 20, 0, 0);
        }
        // Burning wood pile, fire above it, smoke above the fire.
        if (30..100).contains(&x) {
            let top = 150 - ((x - 65).abs() / 3);
            if y >= top {
                return (wood, 400, 0, BURNING);
            }
            if y >= top - 14 && !h.is_multiple_of(3) {
                return (fire, 800, (10 + h % 30) as u8, 0);
            }
            if (40..92).contains(&y) && h % 5 < 3 {
                return (smoke, 150, (40 + h % 200) as u8, 0);
            }
        }
        // Lava with steam above, then water on the right side of the pool.
        if (120..220).contains(&x) {
            if y >= 150 {
                return if x < 190 { (lava, 1200, 0, 0) } else { (water, 60, 0, 0) };
            }
            if (95..140).contains(&y) && h % 4 < 2 && x > 170 {
                return (steam, 110, 0, 0);
            }
        }
        // Molten steel.
        if (240..290).contains(&x) && y >= 155 {
            return (steel, 1550, 0, 0);
        }
        // Stone blocks from 600 °C (left) to 1500 °C (right).
        if (300..360).contains(&x) && y >= 140 {
            let t = 600 + (x - 300) * 15;
            return (stone, t as i16, 0, 0);
        }
        (0, 20, 0, 0)
    });
    let mut snap = snap;
    // Hot sparks from the fire and the lava, and water drops.
    for i in 0..40 {
        let f = i as f32;
        snap.particles.push(ParticleView {
            x: 50.0 + (f * 7.3) % 40.0,
            y: 110.0 - (f * 3.7) % 30.0,
            vx: ((f * 1.7) % 3.0) - 1.5,
            vy: -1.0 - (f % 4.0) * 0.5,
            material: stone,
            temperature: 1300,
            shade: (i % 8) as u8,
        });
        snap.particles.push(ParticleView {
            x: 195.0 + (f * 5.1) % 20.0,
            y: 140.0 - (f * 2.3) % 25.0,
            vx: ((f * 1.3) % 2.0) - 1.0,
            vy: 1.2,
            material: water,
            temperature: 20,
            shade: (i % 8) as u8,
        });
    }
    s.renderer.set_surface_level(-1000);
    s.renderer.set_time(1.3);
    let camera = Camera::new(DVec2::new(192.0, 100.0), 4.0, UVec2::new(1536, 640));
    let img = s.shot(&snap, &camera);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out");
    std::fs::create_dir_all(&dir).unwrap();
    image::RgbaImage::from_raw(1536, 640, img).unwrap().save(dir.join("look_fire.png")).unwrap();
    // The same with only the light map.
    s.renderer.settings_mut().light_only = true;
    let img = capture(&s.device, &s.queue, &mut s.renderer, &camera);
    image::RgbaImage::from_raw(1536, 640, img).unwrap().save(dir.join("look_fire_light.png")).unwrap();
    println!("saved {}", dir.join("look_fire.png").display());
}
