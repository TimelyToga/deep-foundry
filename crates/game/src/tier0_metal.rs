//! Player actions for the Tier 0 metal goals: smelt tin and copper in the crucible, build the
//! bellows, make bronze, gears, a lab, research kits, bricks in the kiln, and repair the Hub.
//!
//! The smelting site stands right of the kiln, on ground that the robot makes flat first (dig and
//! fill, done here with cell writes):
//!
//! ```text
//!              [crucible  ]
//!   [bellows] .  .  [campfire] .  [mold][crate]
//! ```
//!
//! The left half of the crucible sits on the campfire. The tap of its right half pours onto the
//! plate mold, one tile away from the fire, and the mold puts the plates into the crate. The
//! bellows are 3 tiles from the campfire. Everything goes through the windows, as a player does
//! it: open a building, choose a recipe, click a tank or a slot.

use super::tier0_tests::Player;
use crate::factory_host::{FactoryCommand, Placement};
use foundry_content::{ItemRef, Layer};
use foundry_core::{BuildingId, CellPos, MaterialId, TILE_SIZE, TilePos};
use foundry_ui::{BuildingSlots, SlotClick, SlotRef, UiAction};

/// The buildings of the smelting site.
#[derive(Debug, Clone, Copy)]
pub(super) struct Smelter {
    pub campfire: BuildingId,
    pub crucible: BuildingId,
    pub mold: BuildingId,
    pub crate_: BuildingId,
}

/// Ticks between two checks while a machine works.
const CHECK: u32 = 120;

impl Player {
    /// The building of a type on a tile (front layer).
    fn building_on(&self, tile: TilePos) -> Option<BuildingId> {
        self.host.factory.buildings.at_tile(tile, Layer::Front)
    }

    /// Take a building from the inventory into the hand and place it with its top-left tile at
    /// `tile`.
    pub(super) fn place_at(&mut self, part: &str, tile: TilePos) -> Result<BuildingId, String> {
        let p = self.content.factory.part(part).ok_or(format!("no part {part}"))?;
        let kind = self.content.factory.building(part).ok_or(format!("no building {part}"))?;
        self.apply(FactoryCommand::PickToCursor(p));
        self.apply(FactoryCommand::Place(Placement::new(kind, tile, 0)));
        self.apply(FactoryCommand::ClearCursor);
        self.building_on(tile)
            .filter(|id| self.host.factory.buildings.get(*id).is_some_and(|b| b.kind == kind))
            .ok_or_else(|| format!("cannot place {part} at {tile:?}; notices: {:?}", self.host.frame(&self.sim, 0).notices))
    }

    /// Make the ground flat for `w` tiles from tile column `x`: air in the 12 tile rows above
    /// tile row `row` (a hill there would slide into the site), ground (dirt) in the 2 rows from
    /// it down. Returns the tile row where
    /// buildings stand (`row - 1`).
    fn flatten(&mut self, x: i32, w: i32, row: i32) -> i32 {
        let dirt = self.content.expect_material("dirt");
        let tree = ["wood", "leaves"].map(|m| self.content.expect_material(m));
        for cx in x * TILE_SIZE..(x + w) * TILE_SIZE {
            for y in (row - 12) * TILE_SIZE..row * TILE_SIZE {
                self.sim.set_cell(CellPos::new(cx, y), MaterialId::AIR, None);
            }
            // Tree roots (wood) under a campfire catch fire, and the fire spreads under the
            // other buildings: the ground under the site is dirt.
            for y in row * TILE_SIZE..(row + 2) * TILE_SIZE {
                let m = self.sim.cell(CellPos::new(cx, y)).material;
                if !crate::player::blocks(&self.content, m) || tree.contains(&m) {
                    self.sim.set_cell(CellPos::new(cx, y), dirt, None);
                }
            }
        }
        self.ticks(10);
        row - 1
    }

    /// Clear loose powder (dirt the robot threw out while digging) from the site: the tiles
    /// above the buildings, the tap tile and the gap under it. A player digs it away.
    fn clear_site(&mut self, s: Smelter) {
        let Some(c) = self.host.factory.buildings.get(s.crucible).map(|b| b.cell_rect()) else { return };
        self.clear_area(c.x0 - 4 * TILE_SIZE, c.x1 + 3 * TILE_SIZE, c.y0 - 6 * TILE_SIZE, c.y1 + TILE_SIZE);
    }

    /// Clear loose powder and leaves (not building cells) in the cells `x0..x1`, `y0..y1`.
    fn clear_area(&mut self, x0: i32, x1: i32, y0: i32, y1: i32) {
        let leaves = self.content.expect_material("leaves");
        for y in y0..y1 {
            for x in x0..x1 {
                let p = CellPos::new(x, y);
                let m = self.sim.cell(p).material;
                let loose = self.content.materials.phase[m.index()] == foundry_content::Phase::Powder || m == leaves;
                if loose && !self.sim.is_building_cell(p) {
                    self.sim.set_cell(p, MaterialId::AIR, None);
                }
            }
        }
    }

    /// The smelting site: its first tile column and the tile row where its buildings stand. The
    /// first call makes the ground flat there. It is 12 tiles right of the start; the kiln
    /// stands on its left part (tiles x - 5 to x - 3), away from the wood buildings at the start.
    pub(super) fn site(&mut self) -> Result<(i32, i32), String> {
        if let Some(s) = self.site {
            return Ok(s);
        }
        let x = self.start.x.div_euclid(TILE_SIZE) + 12;
        let mid = (x + 3) * TILE_SIZE;
        self.walk_to(mid)?;
        let row = crate::factory_host::ground_top(&self.sim, &self.content, mid).div_euclid(TILE_SIZE);
        // From the kiln (tiles x - 5 to x - 3) to the crate (x + 6).
        let b = self.flatten(x - 6, 14, row);
        self.site = Some((x, b));
        Ok((x, b))
    }

    /// Place the first campfire on the smelting site, away from wood buildings (fire burns them).
    pub(super) fn place_campfire(&mut self) -> Result<(), String> {
        let (x, b) = self.site()?;
        self.walk_to((x + 1) * TILE_SIZE)?;
        self.place_at("campfire", TilePos::new(x + 3, b)).map(|_| ())
    }

    /// The smelting site, built the first time.
    pub(super) fn smelter(&mut self) -> Result<Smelter, String> {
        if let Some(s) = self.smelter {
            return Ok(s);
        }
        let (x, b) = self.site()?;
        // The mold needs 4 clay bricks (from the kiln), the crucible 4 raw clay bricks.
        if self.held("clay_brick") < 4 {
            self.kiln_bricks(4 - self.held("clay_brick"))?;
        }
        self.dig("clay", |p| p.held("clay") >= 64)?;
        self.dig("wood", |p| p.held("wood") >= 60)?;
        self.craft("crucible", 1)?;
        self.craft("plate_mold", 1)?;
        self.craft("crate", 1)?;
        self.walk_to((x + 1) * TILE_SIZE)?;
        let campfire = self.building_on(TilePos::new(x + 3, b)).ok_or("no campfire at the site")?;
        self.clear_area((x - 1) * TILE_SIZE, (x + 8) * TILE_SIZE, (b - 6) * TILE_SIZE, b * TILE_SIZE);
        let crucible = self.place_at("crucible", TilePos::new(x + 3, b - 1))?;
        let mold = self.place_at("plate_mold", TilePos::new(x + 5, b))?;
        let crate_ = self.place_at("crate", TilePos::new(x + 6, b))?;
        let s = Smelter { campfire, crucible, mold, crate_ };
        self.smelter = Some(s);
        Ok(s)
    }

    /// Open a building's window: walk near it, point at it and click.
    pub(super) fn open_id(&mut self, id: BuildingId) -> Result<(), String> {
        let Some(r) = self.host.factory.buildings.get(id).map(|b| b.cell_rect()) else {
            return Err(format!("building {id:?} is gone; smelter {:?}; notices {:?}", self.smelter, self.host.frame(&self.sim, 0).notices));
        };
        let center = CellPos::new((r.x0 + r.x1) / 2, (r.y0 + r.y1) / 2);
        if (center.x - self.center_x()).abs() > 56 {
            self.walk_to(center.x - 24)?;
        }
        let opened = self.open(center)?;
        if opened != id {
            return Err(format!("opened {opened:?}, not {id:?}"));
        }
        Ok(())
    }

    /// The index of the robot's tank that holds a material.
    fn tank_of(&self, material: &str) -> Option<usize> {
        let m = ItemRef::Material(self.content.expect_material(material));
        self.normal.frame.inventory.tanks.iter().position(|t| t.item == Some(m))
    }

    /// The index of the robot's inventory slot that holds a part.
    fn slot_of(&self, part: &str) -> Option<usize> {
        let p = self.content.item(part)?;
        self.normal.frame.inventory.slots.iter().position(|s| s.as_ref().is_some_and(|s| s.item == p))
    }

    /// Click the tank of a material with a building window open: the material goes in.
    fn click_tank(&mut self, material: &str) {
        self.sync();
        if let Some(t) = self.tank_of(material) {
            self.ui(&[UiAction::ClickSlot { slot: SlotRef::Tank(t), click: SlotClick::CTRL_LEFT }]);
        }
    }

    /// Shift + click the inventory slot of a part with a building window open: the stack goes in.
    fn click_part(&mut self, part: &str) {
        self.sync();
        if let Some(i) = self.slot_of(part) {
            self.ui(&[UiAction::ClickSlot { slot: SlotRef::Inventory(i), click: SlotClick::SHIFT_LEFT }]);
        }
    }

    /// Choose the recipe of the open building.
    fn choose(&mut self, id: BuildingId, recipe: &str) -> Result<(), String> {
        let r = self.content.factory.recipe(recipe).ok_or(format!("no recipe {recipe}"))?;
        if self.host.factory.building_view(id).is_some_and(|v| v.recipe == Some(r)) {
            return Ok(());
        }
        self.ui(&[UiAction::SetRecipe { building: id, recipe: Some(r) }]);
        match self.host.factory.building_view(id) {
            Some(v) if v.recipe == Some(r) => Ok(()),
            v => Err(format!("cannot choose {recipe}: {v:?}")),
        }
    }

    /// Shift + click every slot of the open crate: its stacks go to the robot.
    fn empty_crate(&mut self, id: BuildingId) {
        for index in 0..8 {
            self.ui(&[UiAction::ClickSlot { slot: SlotRef::Building { building: id, group: BuildingSlots::Input, index }, click: SlotClick::SHIFT_LEFT }]);
        }
    }

    /// How many of an item the robot holds (not counting the crates).
    pub(super) fn held(&self, id: &str) -> u32 {
        self.host.factory.player.count(self.content.item(id).unwrap())
    }

    /// Have at least `n` charcoal: make more in the kiln when needed.
    fn charcoal(&mut self, n: u32) -> Result<(), String> {
        while self.held("charcoal") < n {
            self.make_charcoal(24)?;
        }
        Ok(())
    }

    /// Put wood into the campfire's fuel slot when it runs low.
    fn fuel_campfire(&mut self, s: Smelter) -> Result<(), String> {
        let units = self.host.factory.building_view(s.campfire).and_then(|v| v.fuel).map_or(0, |f| f.units);
        if units >= 40 {
            return Ok(());
        }
        if self.held("wood") < 60 {
            self.dig("wood", |p| p.held("wood") >= 120)?;
        }
        self.open_id(s.campfire)?;
        self.click_tank("wood");
        Ok(())
    }

    /// Smelt `plates` plates of a metal ("tin" or "copper") at the smelting site, from raw ore,
    /// and take them out of the crate.
    pub(super) fn smelt(&mut self, metal: &str, plates: u32) -> Result<(), String> {
        let (ore, recipe, vein) = match metal {
            "tin" => ("raw_cassiterite", "tin_smelting", "cassiterite"),
            _ => ("raw_malachite", "copper_smelting", "malachite"),
        };
        let plate = format!("{metal}_plate");
        let s = self.smelter()?;
        let need = plates * 16;
        if self.held(ore) < need {
            let more = need - self.held(ore);
            let goal = self.held(ore) + more;
            self.dig(vein, move |p| p.held(ore) >= goal)?;
        }
        self.charcoal(plates * 4)?;
        self.fuel_campfire(s)?;
        self.open_id(s.mold)?;
        self.choose(s.mold, &plate)?;
        self.open_id(s.crucible)?;
        self.choose(s.crucible, recipe)?;
        let start = self.held(&plate) + self.stored(s.crate_, &plate);
        for _ in 0..plates * 20 + 50 {
            if self.held(&plate) + self.stored(s.crate_, &plate) >= start + plates {
                break;
            }
            // Keep two batches loaded and the fire burning.
            let v = self.host.factory.building_view(s.crucible).ok_or("no crucible")?;
            if v.inputs.first().is_some_and(|b| b.count < 16) || v.inputs.get(1).is_some_and(|b| b.count < 4) {
                self.clear_site(s);
                self.fuel_campfire(s)?;
                self.open_id(s.crucible)?;
                self.click_tank(ore);
                self.click_tank("charcoal");
            }
            self.ticks(CHECK);
        }
        self.open_id(s.crate_)?;
        self.empty_crate(s.crate_);
        if self.held(&plate) >= start + plates {
            Ok(())
        } else {
            let r = self.host.factory.buildings.get(s.crucible).map(|b| b.cell_rect()).unwrap();
            let map: Vec<String> = (r.y0 - 4..r.y1 + 12)
                .map(|y| (r.x0 - 4..r.x1 + 24).map(|x| {
                    let m = self.sim.cell(CellPos::new(x, y)).material;
                    if m.is_air() { '.' } else { self.content.materials.names[m.index()].chars().next().unwrap_or('?') }
                }).collect())
                .collect();
            Err(format!(
                "smelting {metal}: {} of {plates} plates; cells at the site:\n{}\ncrucible {:?}; mold {:?}",
                self.held(&plate).saturating_sub(start),
                map.join("\n"),
                self.host.factory.building_view(s.crucible),
                self.host.factory.building_view(s.mold)
            ))
        }
    }

    /// How many of an item a storage building holds.
    fn stored(&self, id: BuildingId, item: &str) -> u32 {
        let it = self.content.item(item).unwrap();
        self.host.factory.buildings.inventory(id).map_or(0, |inv| inv.count(it))
    }

    /// Melt copper and tin plates into bronze in the crucible and cast `plates` bronze plates
    /// (a multiple of 4).
    pub(super) fn bronze(&mut self, plates: u32) -> Result<(), String> {
        let batches = plates.div_ceil(4);
        let copper = (batches * 3).saturating_sub(self.held("copper_plate"));
        let tin = batches.saturating_sub(self.held("tin_plate"));
        if copper > 0 {
            self.smelt("copper", copper)?;
        }
        if tin > 0 {
            self.smelt("tin", tin)?;
        }
        let s = self.smelter()?;
        self.fuel_campfire(s)?;
        self.open_id(s.mold)?;
        self.choose(s.mold, "bronze_plate")?;
        self.open_id(s.crucible)?;
        self.choose(s.crucible, "bronze_alloy")?;
        let start = self.held("bronze_plate") + self.stored(s.crate_, "bronze_plate");
        for _ in 0..batches * 20 + 50 {
            if self.held("bronze_plate") + self.stored(s.crate_, "bronze_plate") >= start + batches * 4 {
                break;
            }
            let v = self.host.factory.building_view(s.crucible).ok_or("no crucible")?;
            if v.inputs.first().is_some_and(|b| b.count < 3) || v.inputs.get(1).is_some_and(|b| b.count < 1) {
                self.clear_site(s);
                self.fuel_campfire(s)?;
                self.open_id(s.crucible)?;
                self.click_part("copper_plate");
                self.click_part("tin_plate");
            }
            self.ticks(CHECK);
        }
        self.open_id(s.crate_)?;
        self.empty_crate(s.crate_);
        let made = self.held("bronze_plate").saturating_sub(start);
        if made >= plates {
            Ok(())
        } else {
            Err(format!("bronze: {made} of {plates} plates; crucible {:?}; mold {:?}", self.host.factory.building_view(s.crucible), self.host.factory.building_view(s.mold)))
        }
    }

    /// Build bellows 3 tiles left of the campfire.
    pub(super) fn build_bellows(&mut self) -> Result<(), String> {
        let s = self.smelter()?;
        if self.held("tin_plate") < 2 {
            self.smelt("tin", 2 - self.held("tin_plate"))?;
        }
        self.dig("wood", |p| p.held("wood") >= 10)?;
        self.craft("bellows", 1)?;
        let at = self.host.factory.buildings.get(s.campfire).ok_or("no campfire")?.at;
        // Stand left of the bellows, between them and the kiln.
        self.walk_to((at.x - 5) * TILE_SIZE + 4)?;
        self.place_at("bellows", TilePos::new(at.x - 3, at.y)).map(|_| ())
    }

    /// Fire `n` clay bricks in the kiln (raw bricks in, charcoal as fuel).
    pub(super) fn kiln_bricks(&mut self, n: u32) -> Result<(), String> {
        let raw = n.saturating_sub(self.held("raw_clay_brick"));
        if raw > 0 {
            self.dig("clay", move |p| p.held("clay") >= raw * 16)?;
            self.craft("raw_clay_brick", raw)?;
        }
        self.charcoal(40)?;
        let id = self.kiln().ok_or("no kiln")?;
        self.open_id(id)?;
        self.choose(id, "clay_brick")?;
        let start = self.held("clay_brick");
        for _ in 0..200 {
            self.open_id(id)?;
            self.click_part("raw_clay_brick");
            let fuel = self.host.factory.building_view(id).and_then(|v| v.fuel).map_or(0, |f| f.units);
            if fuel < 20 {
                self.charcoal(20)?;
                self.open_id(id)?;
                self.click_tank("charcoal");
            }
            self.ui(&[UiAction::ClickSlot { slot: SlotRef::Building { building: id, group: BuildingSlots::Output, index: 0 }, click: SlotClick::LEFT }]);
            if self.held("clay_brick") >= start + n {
                return Ok(());
            }
            self.ticks(CHECK);
        }
        Err(format!("the kiln made {} of {n} clay bricks: {:?}", self.held("clay_brick") - start, self.host.factory.building_view(id)))
    }

    /// Deliver what the Hub needs: open the Hub and shift + click the stacks.
    pub(super) fn repair_hub(&mut self) -> Result<(), String> {
        let bronze = 16 + 8 * 2;
        if self.held("bronze_plate") + self.held("bronze_gear") * 2 < bronze {
            self.bronze(4 * (bronze - self.held("bronze_plate") - self.held("bronze_gear") * 2).div_ceil(4))?;
        }
        if self.held("bronze_gear") < 8 {
            self.craft("bronze_gear", 8 - self.held("bronze_gear"))?;
        }
        if self.held("clay_brick") < 32 {
            self.kiln_bricks(32 - self.held("clay_brick"))?;
        }
        let hub = self.host.factory.buildings.iter().find(|(_, b)| matches!(b.logic, foundry_factory::Logic::Hub(_))).map(|(id, _)| id).ok_or("no Hub")?;
        self.open_id(hub)?;
        for part in ["bronze_plate", "bronze_gear", "clay_brick"] {
            self.click_part(part);
        }
        for _ in 0..10 {
            if self.host.factory.progress.stage() >= 1 {
                return Ok(());
            }
            self.ticks(30);
        }
        Err(format!("the Hub is not repaired: {:?}", self.host.factory.building_view(hub)))
    }
}
