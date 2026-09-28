//! Shared checks for the early hot-metal buildings.

use foundry_core::TilePos;

/// Bellows can feed a nearby fire. The fire and its heat still live in simulation cells.
pub(crate) fn bellows_reaches_fire(bellows: &[TilePos], fire: TilePos) -> bool {
    bellows.iter().any(|at| {
        (at.x - fire.x).abs() + (at.y - fire.y).abs() <= 3
    })
}

#[cfg(test)]
mod tests {
    use super::bellows_reaches_fire;
    use foundry_core::TilePos;

    #[test]
    fn bellows_boost_a_nearby_fire_only() {
        assert!(bellows_reaches_fire(&[TilePos::new(4, 4)], TilePos::new(7, 4)));
        assert!(!bellows_reaches_fire(&[TilePos::new(4, 4)], TilePos::new(8, 4)));
    }
}
