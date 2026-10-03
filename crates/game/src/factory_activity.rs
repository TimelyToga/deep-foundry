//! Keep cell physics around every factory running when the robot and camera leave.
//!
//! One anchor per occupied chunk avoids an anchor per wall or belt. The simulation's
//! normal margin covers neighboring fuel, water, room interiors and port drops.
//! The cell save retains anchors. Reattach by exact chunk bounds after loading so
//! repeated save/load does not leak anchors or leave removed factories running.
use foundry_core::ChunkPos;
use foundry_factory::Factory;
use foundry_sim::{AnchorId, Simulation};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct FactoryActivity {
    anchors: BTreeMap<ChunkPos, AnchorId>,
    /// `Buildings::layout` at the last sync, and the building count: nothing to do while they
    /// stay the same (the buildings did not change).
    synced: Option<(u64, usize)>,
}

impl FactoryActivity {
    pub(super) fn sync(&mut self, factory: &Factory, sim: &mut Simulation) {
        let key = (factory.buildings.layout(), factory.buildings.len());
        if self.synced == Some(key) {
            return;
        }
        self.synced = Some(key);
        let needed: BTreeSet<_> = factory.buildings.iter().flat_map(|(_, b)| b.cell_rect().chunks()).collect();
        self.anchors.retain(|chunk, id| {
            if needed.contains(chunk) {
                true
            } else {
                sim.remove_anchor(*id);
                false
            }
        });
        for chunk in needed {
            self.anchors.entry(chunk).or_insert_with(|| {
                let area = chunk.cell_rect();
                let restored = sim.anchors().iter().find(|(_, bounds)| *bounds == area).map(|(id, _)| *id);
                restored.unwrap_or_else(|| sim.add_anchor(area))
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundry_content::Content;
    use foundry_core::{CellPos, CellRect, Command, TilePos};
    use foundry_sim::SimConfig;
    use std::sync::Arc;

    #[test]
    fn distant_factory_cells_keep_moving_and_removed_buildings_release_anchors() {
        let content = Arc::new(Content::load_default().unwrap());
        let mut sim = Simulation::new(content.clone(), SimConfig::finite(40, 4, 7));
        sim.set_threads(1);
        let mut factory = Factory::new(content.clone());
        let kind = content.factory.building("wood_belt").unwrap();
        let a = factory.place(kind, TilePos::new(240, 12), 0, false, &mut sim).unwrap();
        let b = factory.place(kind, TilePos::new(241, 12), 0, false, &mut sim).unwrap();
        let mut activity = FactoryActivity::default();
        sim.apply(Command::SetView {
            area: CellRect::new(0, 0, 128, 128),
        });
        let sand = content.expect_material("sand");
        let start = CellPos::new(1924, 65);
        sim.set_cell(start, sand, None);
        activity.sync(&factory, &mut sim);
        assert_eq!(activity.anchors.len(), 1, "neighboring buildings share a chunk anchor");
        let ids = sim.anchors().to_vec();
        // A loaded host reconstructs the map while the simulation retains saved anchors.
        activity = FactoryActivity::default();
        activity.sync(&factory, &mut sim);
        assert_eq!(sim.anchors(), ids, "unchanged factories retain stable anchors");
        for _ in 0..90 {
            factory.tick(&mut sim);
            sim.tick();
        }
        assert_ne!(sim.cell(start).material, sand, "falling sand runs far outside the camera");
        assert!(sim.count_material(CellRect::new(1900, 80, 1990, 200), sand) > 0);
        factory.remove(a, &mut sim).unwrap();
        activity.sync(&factory, &mut sim);
        assert_eq!(activity.anchors.len(), 1);
        factory.remove(b, &mut sim).unwrap();
        activity.sync(&factory, &mut sim);
        assert!(activity.anchors.is_empty());
        assert!(sim.anchors().is_empty());
    }
}
