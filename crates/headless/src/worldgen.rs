//! Pictures of the world generator: make the chunks of a cell area with `foundry_worldgen` and
//! draw them into a PNG. No simulation runs.

use crate::image::Image;
use foundry_content::{Content, Phase};
use foundry_core::{CHUNK_AREA, CHUNK_SIZE, ChunkPos, local_index};
use foundry_sim::{ChunkCells, ChunkSource};
use foundry_worldgen::WorldGen;
use rayon::prelude::*;
use std::time::Instant;

/// The cells of an area of the generated world.
pub struct Area {
    pub x0: i32,
    pub y0: i32,
    pub width: i32,
    pub height: i32,
    /// Material of each cell, row by row from the top.
    pub mat: Vec<u16>,
}

/// Timing of `generate_area`.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub chunks: usize,
    /// Wall time of the whole area on all threads, in seconds.
    pub total_s: f64,
    /// Mean time of one chunk on one thread (up to the first 1000 chunks), in microseconds.
    pub per_chunk_us: f64,
}

fn make_chunk(source: &WorldGen, seed: u64, pos: ChunkPos, mat: &mut [u16; CHUNK_AREA], temp: &mut [i16; CHUNK_AREA]) {
    mat.fill(0);
    temp.fill(ChunkCells::MATERIAL_DEFAULT);
    let mut cells = ChunkCells { pos, seed, mat, temp, awake: false };
    source.generate(&mut cells);
}

/// Make the chunks that cover the area (in parallel) and copy their cells.
pub fn generate_area(source: &WorldGen, seed: u64, x0: i32, y0: i32, width: i32, height: i32) -> (Area, Timing) {
    let (cx0, cy0) = (x0.div_euclid(CHUNK_SIZE), y0.div_euclid(CHUNK_SIZE));
    let (cx1, cy1) = ((x0 + width - 1).div_euclid(CHUNK_SIZE), (y0 + height - 1).div_euclid(CHUNK_SIZE));
    let positions: Vec<ChunkPos> = (cy0..=cy1).flat_map(|cy| (cx0..=cx1).map(move |cx| ChunkPos::new(cx, cy))).collect();

    // One thread, for the time per chunk.
    let mut mat = Box::new([0u16; CHUNK_AREA]);
    let mut temp = Box::new([0i16; CHUNK_AREA]);
    let sample = &positions[..positions.len().min(1000)];
    let start = Instant::now();
    for &p in sample {
        make_chunk(source, seed, p, &mut mat, &mut temp);
    }
    let per_chunk_us = start.elapsed().as_secs_f64() * 1e6 / sample.len().max(1) as f64;

    let start = Instant::now();
    let chunks: Vec<Box<[u16; CHUNK_AREA]>> = positions
        .par_iter()
        .map(|&p| {
            let mut mat = Box::new([0u16; CHUNK_AREA]);
            let mut temp = Box::new([0i16; CHUNK_AREA]);
            make_chunk(source, seed, p, &mut mat, &mut temp);
            mat
        })
        .collect();
    let total_s = start.elapsed().as_secs_f64();

    let mut out = vec![0u16; (width.max(0) * height.max(0)) as usize];
    let cw = (cx1 - cx0 + 1) as usize;
    for (i, ch) in chunks.iter().enumerate() {
        let (ccx, ccy) = (cx0 + (i % cw) as i32, cy0 + (i / cw) as i32);
        for ly in 0..CHUNK_SIZE {
            let y = ccy * CHUNK_SIZE + ly - y0;
            if !(0..height).contains(&y) {
                continue;
            }
            for lx in 0..CHUNK_SIZE {
                let x = ccx * CHUNK_SIZE + lx - x0;
                if (0..width).contains(&x) {
                    out[(y * width + x) as usize] = ch[local_index(lx, ly)];
                }
            }
        }
    }
    let area = Area { x0, y0, width, height, mat: out };
    (area, Timing { chunks: positions.len(), total_s, per_chunk_us })
}

/// A small hash of a cell position, to pick one of the colors of a material.
fn cell_hash(x: i32, y: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^ (h >> 13)
}

/// Draw the area. Each output pixel is the average of `k` × `k` cells. Air above the ground is a
/// sky color; air below the ground (caves) is dark.
pub fn draw(area: &Area, content: &Content, surface_y: i32, k: i32) -> Image {
    let mats = &content.materials;
    let k = k.max(1);
    let (w, h) = ((area.width + k - 1) / k, (area.height + k - 1) / k);
    let mut sums = vec![[0u32; 4]; (w * h) as usize];
    let wood = content.material("wood").map(|m| m.0);
    let leaves = content.material("leaves").map(|m| m.0);
    for x in 0..area.width {
        // A column that starts deep is under the ground from the top.
        let mut underground = area.y0 > surface_y + 300;
        for y in 0..area.height {
            let (wx, wy) = (area.x0 + x, area.y0 + y);
            let m = area.mat[(y * area.width + x) as usize];
            let phase = mats.phase[m as usize];
            if matches!(phase, Phase::Solid | Phase::Powder) && Some(m) != wood && Some(m) != leaves {
                underground = true;
            }
            let back = if underground {
                [26, 22, 24]
            } else {
                // Sky: lighter near the ground.
                let t = ((wy - (surface_y - 1024)) as f32 / 1100.0).clamp(0.0, 1.0);
                [(70.0 + 90.0 * t) as u32, (120.0 + 80.0 * t) as u32, (190.0 + 45.0 * t) as u32]
            };
            let rgb = if m == 0 {
                back
            } else {
                let colors = &mats.colors[m as usize];
                let c = colors[cell_hash(wx, wy) as usize % colors.len().max(1)];
                let a = c[3] as u32;
                [
                    (c[0] as u32 * a + back[0] * (255 - a)) / 255,
                    (c[1] as u32 * a + back[1] * (255 - a)) / 255,
                    (c[2] as u32 * a + back[2] * (255 - a)) / 255,
                ]
            };
            let s = &mut sums[((y / k) * w + x / k) as usize];
            s[0] += rgb[0];
            s[1] += rgb[1];
            s[2] += rgb[2];
            s[3] += 1;
        }
    }
    let mut img = Image::new(w as u32, h as u32);
    for (p, s) in img.pixels.iter_mut().zip(&sums) {
        let n = s[3].max(1);
        *p = [(s[0] / n) as u8, (s[1] / n) as u8, (s[2] / n) as u8, 255];
    }
    img
}
