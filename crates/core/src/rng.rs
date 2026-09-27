//! A small, fast, deterministic random number generator (wyrand).
//!
//! The simulation makes one generator per chunk per tick with `Rng::for_chunk`.
//! So results do not depend on thread timing or on the number of threads.

use crate::pos::ChunkPos;

#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self { state: splitmix64(seed) }
    }

    /// The generator for one chunk in one tick. `salt` separates different passes (movement, heat, ...).
    pub fn for_chunk(world_seed: u64, tick: u64, chunk: ChunkPos, salt: u64) -> Self {
        let mut h = splitmix64(world_seed ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        h = splitmix64(h ^ tick);
        h = splitmix64(h ^ (chunk.x as u32 as u64) ^ ((chunk.y as u32 as u64) << 32));
        Self { state: h }
    }

    #[inline(always)]
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0xa076_1d64_78bd_642f);
        let t = (self.state as u128).wrapping_mul((self.state ^ 0xe703_7ed1_a0b4_28db) as u128);
        ((t >> 64) as u64) ^ (t as u64)
    }

    #[inline(always)]
    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// A number in `0..n`. `n` must be greater than 0.
    #[inline(always)]
    pub fn below(&mut self, n: u32) -> u32 {
        ((self.next_u32() as u64 * n as u64) >> 32) as u32
    }

    /// True or false with equal chance.
    #[inline(always)]
    pub fn coin(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }

    /// True with chance `p` (0.0 to 1.0).
    #[inline(always)]
    pub fn chance(&mut self, p: f32) -> bool {
        (self.next_u32() as f32) < p * 4_294_967_296.0
    }

    /// True with chance `p / 65536`. Faster than `chance` when `p` is stored as a `u16`.
    #[inline(always)]
    pub fn chance_u16(&mut self, p: u16) -> bool {
        ((self.next_u32() >> 16) as u16) < p
    }

    /// A float in `0.0..1.0`.
    #[inline(always)]
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }
}

#[inline(always)]
fn splitmix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_numbers() {
        let mut a = Rng::for_chunk(7, 100, ChunkPos::new(3, -2), 1);
        let mut b = Rng::for_chunk(7, 100, ChunkPos::new(3, -2), 1);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn below_stays_in_range_and_chance_is_fair() {
        let mut r = Rng::new(1);
        let mut hits = 0;
        for _ in 0..100_000 {
            assert!(r.below(5) < 5);
            if r.chance(0.25) {
                hits += 1;
            }
        }
        assert!((23_000..27_000).contains(&hits), "{hits}");
    }
}
