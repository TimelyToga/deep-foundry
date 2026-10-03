//! Shared checks for the early hot-metal buildings.

use foundry_content::{Content, Recipe};
use foundry_core::{CellPos, CellRect, MaterialId, TilePos};
use foundry_sim::Simulation;

/// The part of the difference to the mold body that the metal in a mold loses in one tick.
/// Tin poured at 372 °C into a 20 °C mold is below its freezing point (222 °C) after about 2 s.
pub(crate) const METAL_COOLING: f32 = 0.004;

/// Bellows can feed a nearby fire. The fire and its heat still live in simulation cells.
pub(crate) fn bellows_reaches_fire(bellows: &[TilePos], fire: TilePos) -> bool {
    bellows
        .iter()
        .any(|at| (at.x - fire.x).abs() + (at.y - fire.y).abs() <= 3)
}

/// The temperature below which a filled casting mold can finish its recipe.
pub(crate) fn casting_freeze_point(content: &Content, recipe: &Recipe) -> Option<i16> {
    if !recipe.category.starts_with("casting_") {
        return None;
    }
    recipe.inputs.iter().find_map(|stack| match stack.item {
        foundry_content::ItemRef::Material(mat) => {
            content.materials.freeze[mat.index()].map(|change| change.at)
        }
        foundry_content::ItemRef::Part(_) => None,
    })
}

/// Heat the mold body to the temperature of newly accepted molten metal.
pub(crate) fn prime_mold(sim: &mut Simulation, body: MaterialId, rect: CellRect, temperature: i16) {
    for y in rect.y0..rect.y1 {
        for x in rect.x0..rect.x1 {
            let pos = CellPos::new(x, y);
            let cell = sim.cell(pos);
            if cell.material == body && cell.temperature < temperature {
                sim.set_cell(pos, body, Some(temperature));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::bellows_reaches_fire;
    use foundry_core::TilePos;

    #[test]
    fn bellows_boost_a_nearby_fire_only() {
        assert!(bellows_reaches_fire(
            &[TilePos::new(4, 4)],
            TilePos::new(7, 4)
        ));
        assert!(!bellows_reaches_fire(
            &[TilePos::new(4, 4)],
            TilePos::new(8, 4)
        ));
    }
}
