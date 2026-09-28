//! What the robot keeps when it digs.
//!
//! Each dug cell becomes one unit of its broken form (`broken_into` in the data: stone becomes
//! gravel, an ore vein becomes raw ore). The robot keeps a useful material in its tanks. It drops
//! the other materials: the game throws a dropped unit out as loose material, so the ground still
//! opens and nothing is lost.
//!
//! The default comes from the data, not from a hand-made list. A material is useful when:
//! - a recipe uses it as an input (clay, sand, wood, raw coal, ...),
//! - a building port takes it (the campfire takes wood and charcoal), or
//! - a reaction turns it into a useful material (raw malachite and charcoal make molten copper,
//!   which the copper plate recipe uses).
//!
//! The player can change the setting of each material. Only the changes are saved.

use crate::Factory;
use foundry_content::{Content, ItemRef, Matcher, Phase};
use foundry_core::MaterialId;
use std::collections::BTreeMap;

/// What happened to one dug cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dug {
    /// One unit went into the tanks.
    Kept,
    /// The robot does not keep this material: the game throws out one unit of it (the broken
    /// form) as loose material.
    Dropped(MaterialId),
    /// The robot keeps this material, but the tanks have no room. The cell stays.
    NoRoom(MaterialId),
}

/// The keep or drop setting of every material.
#[derive(Debug, Clone, Default)]
pub struct DigRules {
    /// The default from the data, by material index.
    useful: Vec<bool>,
    /// The player's changes: material -> keep.
    pub changed: BTreeMap<MaterialId, bool>,
}

impl DigRules {
    pub fn new(content: &Content) -> Self {
        Self { useful: useful_materials(content), changed: BTreeMap::new() }
    }

    /// The default for a material (from the data).
    pub fn default_keep(&self, m: MaterialId) -> bool {
        self.useful.get(m.index()).copied().unwrap_or(true)
    }

    /// True if the robot keeps this material when it digs.
    pub fn keeps(&self, m: MaterialId) -> bool {
        self.changed.get(&m).copied().unwrap_or_else(|| self.default_keep(m))
    }

    /// Set keep or drop for a material. A setting equal to the default is not stored.
    pub fn set(&mut self, m: MaterialId, keep: bool) {
        if keep == self.default_keep(m) {
            self.changed.remove(&m);
        } else {
            self.changed.insert(m, keep);
        }
    }
}

/// The materials that the robot keeps by default, by material index (see the module text).
pub fn useful_materials(content: &Content) -> Vec<bool> {
    let n = content.materials.names.len();
    let mut useful = vec![false; n];
    let mark = |item: ItemRef, useful: &mut Vec<bool>| {
        if let ItemRef::Material(m) = item
            && !m.is_air()
        {
            useful[m.index()] = true;
        }
    };
    let f = &content.factory;
    for r in &f.recipes {
        for s in &r.inputs {
            mark(s.item, &mut useful);
        }
    }
    for b in &f.buildings {
        for p in &b.ports {
            for item in &p.filter {
                mark(*item, &mut useful);
            }
        }
    }
    // Reactions that make a useful material: their inputs are useful too. Repeat until nothing
    // changes (a chain such as raw chalcopyrite -> roasted chalcopyrite -> molten copper).
    loop {
        let mut changed = false;
        for r in &content.reactions {
            let makes_useful = [r.into_a, r.into_b].into_iter().flatten().any(|m| !m.is_air() && useful[m.index()]);
            if !makes_useful {
                continue;
            }
            for side in [r.a, r.b] {
                for m in matching(content, side) {
                    if !useful[m.index()] {
                        useful[m.index()] = true;
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
    // Air, gases and fire never go into a tank.
    for (i, u) in useful.iter_mut().enumerate() {
        if matches!(content.materials.phase[i], Phase::Empty | Phase::Gas | Phase::Fire) {
            *u = false;
        }
    }
    useful
}

/// The materials that a reaction input matches (none for "any").
fn matching(content: &Content, m: Matcher) -> Vec<MaterialId> {
    let n = content.materials.names.len();
    match m {
        Matcher::Material(x) => vec![x],
        Matcher::Tag(_) => (0..n).map(|i| MaterialId(i as u16)).filter(|x| m.matches(*x, &content.materials)).collect(),
        Matcher::Any => vec![],
    }
}

/// True if a material can be in a robot tank: not air, gas or fire, and it is its own broken
/// form (an ore vein is not; its raw ore is).
pub fn tank_material(content: &Content, m: MaterialId) -> bool {
    !m.is_air()
        && !matches!(content.materials.phase[m.index()], Phase::Empty | Phase::Gas | Phase::Fire)
        && content.materials.broken_into.get(m.index()) == Some(&m)
}

impl Factory {
    /// True if the robot keeps this material when it digs.
    pub fn keeps(&self, m: MaterialId) -> bool {
        self.dig.keeps(m)
    }

    /// Set keep (true) or drop (false) for a material that the robot digs.
    pub fn set_keep(&mut self, m: MaterialId, keep: bool) {
        self.dig.set(m, keep);
    }

    /// The robot dug one cell of `material`. One unit of its broken form goes into the tanks if
    /// the robot keeps it; else the caller throws it out (`Dug::Dropped`). The first dig of a
    /// material discovers it, the same as a scan. With no room in the tanks nothing happens.
    pub fn take_dug_cell(&mut self, material: MaterialId) -> Dug {
        let broken = self.content.materials.broken_into.get(material.index()).copied().unwrap_or(material);
        let item = ItemRef::Material(broken);
        let result = if !self.keeps(broken) {
            Dug::Dropped(broken)
        } else if self.player.room_for(&self.content, item, 1) == 0 {
            return Dug::NoRoom(broken);
        } else {
            self.player.insert(&self.content, item, 1);
            Dug::Kept
        };
        if !self.progress.is_material_discovered(material) {
            self.scan(material);
        }
        result
    }

    /// The materials for the keep or drop list, sorted by name, with their setting: the
    /// discovered materials that can be in a tank, and the materials in the tanks now.
    pub fn dig_list(&self) -> Vec<(MaterialId, bool)> {
        let c = &self.content;
        let mut list: Vec<MaterialId> = self.progress.discovered_materials().filter(|m| tank_material(c, *m)).collect();
        for t in &self.player.tanks {
            if let Some(m) = t.material
                && !list.contains(&m)
            {
                list.push(m);
            }
        }
        list.sort_by(|a, b| c.materials.names[a.index()].cmp(&c.materials.names[b.index()]));
        list.into_iter().map(|m| (m, self.keeps(m))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn content() -> Arc<Content> {
        Arc::new(Content::load_default().expect("content loads"))
    }

    /// The default keep list, by name.
    fn kept(c: &Content) -> Vec<&str> {
        let useful = useful_materials(c);
        (0..useful.len()).filter(|&i| useful[i] && tank_material(c, MaterialId(i as u16))).map(|i| c.materials.ids[i].as_str()).collect()
    }

    #[test]
    fn the_default_keeps_ores_and_recipe_materials_and_drops_dirt_and_stone() {
        let c = content();
        let keep = kept(&c);
        println!("Kept by default ({}): {}", keep.len(), keep.join(", "));
        for id in ["clay", "sand", "wood", "ash", "raw_malachite", "raw_cassiterite", "raw_coal", "raw_magnetite", "raw_limestone", "raw_chalcopyrite", "charcoal"] {
            assert!(keep.contains(&id), "{id} should be kept: {keep:?}");
        }
        for id in ["dirt", "gravel", "stone", "snow", "leaves", "water"] {
            assert!(!keep.contains(&id), "{id} should be dropped: {keep:?}");
        }
    }

    #[test]
    fn digging_drops_what_the_robot_does_not_keep() {
        let c = content();
        let mut f = Factory::new(c.clone());
        let (dirt, stone, gravel, malachite) = (c.expect_material("dirt"), c.expect_material("stone"), c.expect_material("gravel"), c.expect_material("malachite"));
        // Stone breaks into gravel: the robot drops it, and the dig discovers it.
        assert_eq!(f.take_dug_cell(stone), Dug::Dropped(gravel));
        assert!(f.progress.is_material_discovered(stone) && f.progress.is_material_discovered(gravel));
        assert_eq!(f.take_dug_cell(dirt), Dug::Dropped(dirt));
        assert_eq!(f.take_dug_cell(malachite), Dug::Kept);
        assert_eq!(f.player.count(ItemRef::Material(dirt)), 0);
        // The player keeps dirt now.
        f.set_keep(dirt, true);
        assert_eq!(f.take_dug_cell(dirt), Dug::Kept);
        assert_eq!(f.player.count(ItemRef::Material(dirt)), 1);
        assert_eq!(f.dig.changed.len(), 1);
        // The list has the discovered tank materials (not the stone vein itself), by name.
        let names: Vec<&str> = f.dig_list().iter().map(|(m, _)| c.materials.ids[m.index()].as_str()).collect();
        assert_eq!(names, vec!["dirt", "gravel", "raw_malachite"]);
        // Back to the default: nothing is stored.
        f.set_keep(dirt, false);
        assert!(f.dig.changed.is_empty());
    }
}
