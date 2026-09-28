//! Hashes and smooth noise.
//!
//! Every function takes whole-number world positions and a whole-number wavelength. So the result
//! is exact and the same in every chunk, also far from x = 0.

use foundry_core::CHUNK_SIZE;

/// Mix a 64-bit number well (splitmix64).
pub fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A 32-bit seed for one use (`salt`) of the world seed.
pub fn sub_seed(world: u64, salt: u64) -> u32 {
    (splitmix(world ^ salt.wrapping_mul(0xD6E8_FEB8_6659_FD93)) >> 32) as u32
}

/// The last step of murmur3: mix the bits of a 32-bit number.
#[inline(always)]
fn fmix(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^ (h >> 16)
}

/// A hash of a seed and one number.
#[inline(always)]
pub fn hash1(seed: u32, a: i32) -> u32 {
    fmix(seed ^ (a as u32).wrapping_mul(0x9E37_79B1))
}

/// A hash of a seed and two numbers.
#[inline(always)]
pub fn hash2(seed: u32, a: i32, b: i32) -> u32 {
    fmix(fmix(seed ^ (a as u32).wrapping_mul(0x9E37_79B1)) ^ (b as u32).wrapping_mul(0x85EB_CA77))
}

/// 0.0 to 1.0 (1.0 not included) from a hash.
#[inline(always)]
pub fn unit(h: u32) -> f32 {
    (h >> 8) as f32 * (1.0 / 16_777_216.0)
}

/// -1.0 to 1.0 from a hash.
#[inline(always)]
pub fn signed(h: u32) -> f32 {
    unit(h) * 2.0 - 1.0
}

/// 0 at `t` <= 0, 1 at `t` >= 1, smooth between.
#[inline(always)]
pub fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline(always)]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Smooth 1D noise, about -1 to 1. `x` is a cell position, `w` the wavelength in cells.
/// Random values at every `w` cells, joined by a smooth curve (Catmull-Rom).
pub fn noise1(seed: u32, x: i32, w: i32) -> f32 {
    let i = x.div_euclid(w);
    let t = x.rem_euclid(w) as f32 / w as f32;
    let p0 = signed(hash1(seed, i - 1));
    let p1 = signed(hash1(seed, i));
    let p2 = signed(hash1(seed, i + 1));
    let p3 = signed(hash1(seed, i + 2));
    let a = 2.0 * p1;
    let b = p2 - p0;
    let c = 2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3;
    let d = -p0 + 3.0 * p1 - 3.0 * p2 + p3;
    0.5 * (a + t * (b + t * (c + t * d)))
}

/// The gradient at a lattice corner, dotted with the offset (dx, dy). 8 directions.
#[inline(always)]
fn grad(h: u32, dx: f32, dy: f32) -> f32 {
    const D: f32 = std::f32::consts::SQRT_2;
    match h & 7 {
        0 => dx + dy,
        1 => dx - dy,
        2 => -dx + dy,
        3 => -dx - dy,
        4 => D * dx,
        5 => -D * dx,
        6 => D * dy,
        _ => -D * dy,
    }
}

#[inline(always)]
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Smooth 2D gradient noise (Perlin), about -1 to 1. `wx` and `wy` are the wavelengths in cells;
/// different values stretch the pattern.
pub fn noise2(seed: u32, x: i32, y: i32, wx: i32, wy: i32) -> f32 {
    let (ix, iy) = (x.div_euclid(wx), y.div_euclid(wy));
    let fx = x.rem_euclid(wx) as f32 / wx as f32;
    let fy = y.rem_euclid(wy) as f32 / wy as f32;
    let n00 = grad(hash2(seed, ix, iy), fx, fy);
    let n10 = grad(hash2(seed, ix + 1, iy), fx - 1.0, fy);
    let n01 = grad(hash2(seed, ix, iy + 1), fx, fy - 1.0);
    let n11 = grad(hash2(seed, ix + 1, iy + 1), fx - 1.0, fy - 1.0);
    let (u, v) = (fade(fx), fade(fy));
    lerp(lerp(n00, n10, u), lerp(n01, n11, u), v)
}

/// Sum of `octaves` layers of `noise2`, each with half the wavelength and half the strength of
/// the one before. About -1 to 1.
pub fn fbm2(seed: u32, x: i32, y: i32, wx: i32, wy: i32, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut total) = (0.0, 1.0, 0.0);
    let (mut wx, mut wy) = (wx, wy);
    for o in 0..octaves {
        sum += amp * noise2(seed.wrapping_add(o.wrapping_mul(0x632B_E5AB)), x, y, wx.max(2), wy.max(2));
        total += amp;
        amp *= 0.5;
        wx /= 2;
        wy /= 2;
    }
    sum / total
}

/// Distance in cells between the lattice points of a `Coarse` field.
pub const STEP: i32 = 8;
/// Grid columns of a chunk with a border of one cell: world x = x0 - 1 to x0 + 64.
pub const GW: usize = CHUNK_SIZE as usize + 2;
/// Grid rows of a chunk with one more row below: world y = y0 to y0 + 64.
pub const GH: usize = CHUNK_SIZE as usize + 1;
/// Lattice columns: x0 - 8 to x0 + 72.
const LX: usize = (CHUNK_SIZE / STEP) as usize + 3;
/// Lattice rows: y0 to y0 + 72.
const LY: usize = (CHUNK_SIZE / STEP) as usize + 2;

/// A field sampled every `STEP` cells around one chunk, and read with linear steps between the
/// samples. The lattice points are at world positions that are multiples of `STEP`, so two
/// chunks read the same value at the same cell.
pub struct Coarse {
    v: [[f32; LX]; LY],
}

impl Coarse {
    /// Sample `f(x, y)` at the lattice points around the chunk with top-left cell (x0, y0).
    /// Only the lattice rows for grid rows `first_row` and below are sampled.
    pub fn new(x0: i32, y0: i32, first_row: usize, f: impl Fn(i32, i32) -> f32) -> Self {
        let mut v = [[0.0; LX]; LY];
        let j0 = first_row / STEP as usize;
        for (j, row) in v.iter_mut().enumerate().skip(j0) {
            let y = y0 + j as i32 * STEP;
            for (i, out) in row.iter_mut().enumerate() {
                *out = f(x0 - STEP + i as i32 * STEP, y);
            }
        }
        Self { v }
    }

    /// The values of grid row `r` (world y = y0 + r) at the `GW` grid columns.
    pub fn row(&self, r: usize, out: &mut [f32; GW]) {
        let j = r / STEP as usize;
        let t = (r % STEP as usize) as f32 / STEP as f32;
        let mut line = [0.0f32; LX];
        for (i, l) in line.iter_mut().enumerate() {
            *l = lerp(self.v[j][i], self.v[j + 1][i], t);
        }
        // Grid column c is world x = x0 - 1 + c, which is STEP - 1 + c cells right of the first
        // lattice column.
        for (c, o) in out.iter_mut().enumerate() {
            let ox = STEP as usize - 1 + c;
            let i = ox / STEP as usize;
            let u = (ox % STEP as usize) as f32 / STEP as f32;
            *o = lerp(line[i], line[i + 1], u);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_ranges() {
        let mut lo1 = f32::MAX;
        let mut hi1 = f32::MIN;
        let mut lo2 = f32::MAX;
        let mut hi2 = f32::MIN;
        for x in -5000..5000 {
            let v = noise1(7, x, 97);
            lo1 = lo1.min(v);
            hi1 = hi1.max(v);
            let w = noise2(7, x, x * 3 + 11, 64, 40);
            lo2 = lo2.min(w);
            hi2 = hi2.max(w);
        }
        assert!(lo1 > -1.5 && hi1 < 1.5 && hi1 - lo1 > 1.0, "{lo1} {hi1}");
        assert!(lo2 > -1.1 && hi2 < 1.1 && hi2 - lo2 > 0.8, "{lo2} {hi2}");
    }

    #[test]
    fn noise_is_continuous() {
        for x in -300..300 {
            assert!((noise1(3, x, 50) - noise1(3, x + 1, 50)).abs() < 0.1);
            assert!((noise2(3, x, 7, 50, 50) - noise2(3, x + 1, 7, 50, 50)).abs() < 0.1);
        }
    }

    #[test]
    fn coarse_matches_samples_at_lattice_points() {
        let f = |x: i32, y: i32| noise2(5, x, y, 64, 64);
        let (x0, y0) = (-128, 640);
        let c = Coarse::new(x0, y0, 0, f);
        let mut row = [0.0; GW];
        for r in [0usize, 8, 64] {
            c.row(r, &mut row);
            // Column 1 is world x0, a lattice point.
            assert!((row[1] - f(x0, y0 + r as i32)).abs() < 1e-5);
            assert!((row[GW - 1] - f(x0 + 64, y0 + r as i32)).abs() < 1e-5);
        }
    }
}
