//! Can a player do each Tier 1 and Tier 2 guide goal? The test starts where Tier 0 ends (the Tier 0 goals,
//! technologies and the first Hub repair are set done, and the Tier 0 parts are given), then plays
//! the Tier 1 goals in guide order with the player actions: research in a lab, dig, craft, place,
//! build rooms, open windows and click slots, scan.
//!
//! Shortcuts: machines get steam straight into their tanks (the boiler, pipe and water supply is
//! played in `tier1_lines.rs`), and long grinds of parts that a line already made are given.

use super::tier0_tests::Player;
use crate::factory_host::{FactoryCommand, PlayerInput};
use foundry_core::{BuildingId, CellPos, TILE_SIZE, TechId, TilePos};
use foundry_factory::GUIDE_PERIOD;
use foundry_factory::steam::SteamState;
use foundry_ui::{BuildingSlots, NextGoal, SlotClick, SlotRef, UiAction, next_goal};

/// The Tier 1 test's buildings (they stand from the site column `x`, see `Player::site`).
struct Plant {
    lab: Option<BuildingId>,
    /// Machines that get steam (see the module text).
    steamed: Vec<BuildingId>,
}

impl Player {
    fn give_item(&mut self, id: &str, n: u32) {
        let item = self.content.item(id).unwrap_or_else(|| panic!("no item {id}"));
        if let foundry_content::ItemRef::Material(m) = item {
            self.free_tank_for(m);
        }
        let content = self.content.clone();
        let left = self.host.factory.player.insert(&content, item, n);
        assert_eq!(left, 0, "no room for {n} {id}");
    }

    /// The state at the end of Tier 0.
    fn tier0_done(&mut self) {
        let content = self.content.clone();
        for t in content.factory.techs.iter().enumerate().filter(|(_, t)| t.tier == 0) {
            self.host.factory.progress.debug_complete(&content, TechId(t.0 as u16));
        }
        for g in self.host.factory.guide.goals.clone().iter().filter(|g| g.tier == 0) {
            self.host.factory.progress.debug_finish_goal(&g.id);
        }
        self.host.factory.progress.debug_set_stage(&content, 1);
        for (id, n) in [
            ("bronze_plate", 300),
            ("bronze_gear", 80),
            ("tin_plate", 40),
            ("copper_plate", 80),
            ("clay_brick", 100),
            ("bronze_kit", 500),
            ("wood", 400),
            ("charcoal", 400),
            // Fired in a kiln with bellows in its wall (see the room tests).
            ("firebrick", 120),
        ] {
            self.give_item(id, n);
        }
        self.search_depth = 80;
        // Flat ground for the Tier 1 plant, right of the site.
        let (x, b) = self.site().expect("site");
        self.flatten(x + 8, 24, b + 1);
        self.flatten(x - 34, 26, b + 1);
    }

    /// Fill the steam tanks of the plant machines.
    fn steam_all(&mut self, plant: &Plant) {
        let steam = self.content.expect_material("steam");
        for &id in &plant.steamed {
            if let Some(b) = self.host.factory.buildings.get_mut(id)
                && let SteamState::Machine(t) = &mut b.steam
            {
                t.add(steam, 200.0);
            }
            self.host.factory.buildings.wake(id);
        }
    }

    /// Run `n` ticks and keep the plant supplied with steam.
    fn run_plant(&mut self, plant: &Plant, n: u32) {
        for k in 0..n {
            if k % 300 == 0 {
                self.steam_all(plant);
            }
            self.ticks(1);
        }
    }

    /// Research a technology in a basic lab next to the site: put kits into the lab and wait.
    fn lab_research(&mut self, plant: &mut Plant, tech: &str) -> Result<(), String> {
        let t = self.content.factory.tech(tech).ok_or(format!("no tech {tech}"))?;
        let lab = match plant.lab {
            Some(l) => l,
            None => {
                self.craft("basic_lab", 1)?;
                let (x, b) = self.site()?;
                self.walk_to((x - 2) * TILE_SIZE)?;
                let l = self.place_at("basic_lab", TilePos::new(x - 6, b - 1))?;
                plant.lab = Some(l);
                l
            }
        };
        self.apply(FactoryCommand::StartResearch(t));
        let kit_ids: Vec<String> = self
            .content
            .factory
            .tech_def(t)
            .kits
            .iter()
            .filter_map(|s| match s.item {
                foundry_content::ItemRef::Part(p) => Some(self.content.factory.part_def(p).id.clone()),
                foundry_content::ItemRef::Material(_) => None,
            })
            .collect();
        for _ in 0..400 {
            if self.host.factory.progress.is_researched(t) {
                return Ok(());
            }
            let view = self.host.factory.building_view(lab);
            let low = |id: &str| {
                let item = self.content.item(id).unwrap();
                view.as_ref().map_or(0, |v| v.inputs.iter().filter(|b| b.item == item).map(|b| b.count).sum::<u32>()) < 4
            };
            let feed: Vec<String> = kit_ids.iter().filter(|k| low(k)).cloned().collect();
            if !feed.is_empty() {
                self.open_id(lab)?;
                for k in &feed {
                    self.click_part_stack(k);
                }
            }
            self.ticks(60);
        }
        Err(format!("research {tech} did not finish: {:?}", self.host.factory.building_view(lab)))
    }

    /// Shift + click a part stack into the open building.
    fn click_part_stack(&mut self, part: &str) {
        self.sync();
        let p = self.content.item(part).unwrap();
        if let Some(i) = self.normal.frame.inventory.slots.iter().position(|s| s.as_ref().is_some_and(|s| s.item == p)) {
            self.ui(&[UiAction::ClickSlot { slot: SlotRef::Inventory(i), click: SlotClick::SHIFT_LEFT }]);
        }
    }

    /// Craft a building and place it at `tile`, standing `stand` tiles from the site column.
    fn build_at(&mut self, part: &str, tile: TilePos, stand: i32) -> Result<BuildingId, String> {
        self.craft(part, 1)?;
        let (x, _) = self.site()?;
        self.walk_to((x + stand) * TILE_SIZE)?;
        let kind = self.content.factory.building(part).ok_or("no building")?;
        let size = self.content.factory.building_def(kind).size;
        self.clear_area(tile.x * TILE_SIZE, (tile.x + size.0 as i32) * TILE_SIZE, (tile.y - 2) * TILE_SIZE, (tile.y + size.1 as i32) * TILE_SIZE);
        self.place_at(part, tile)
    }

    /// Build a room: a ring of 3 × 3 tiles with its top-left tile at (`x0`, `y0`), the
    /// controller in the left wall, a hatch in the roof, walls on the other tiles; the middle tile
    /// is the room. Returns the controller.
    fn build_room(&mut self, controller: &str, hatch: &str, wall: &str, x0: i32, y0: i32) -> Result<BuildingId, String> {
        self.craft(controller, 1)?;
        self.craft(hatch, 1)?;
        self.craft(wall, 6)?;
        self.walk_to((x0 + 5) * TILE_SIZE)?;
        self.clear_area(x0 * TILE_SIZE, (x0 + 3) * TILE_SIZE, (y0 - 3) * TILE_SIZE, (y0 + 3) * TILE_SIZE);
        let mut ctrl = None;
        for dy in 0..3 {
            for dx in 0..3 {
                let part = match (dx, dy) {
                    (1, 1) => continue,
                    (0, 1) => controller,
                    (1, 0) => hatch,
                    _ => wall,
                };
                let id = self.place_at(part, TilePos::new(x0 + dx, y0 + dy))?;
                if part == controller {
                    ctrl = Some(id);
                }
            }
        }
        let id = ctrl.ok_or("no controller")?;
        self.ticks(2);
        match self.host.factory.building_view(id).and_then(|v| v.room) {
            Some(r) if r.valid => Ok(id),
            r => Err(format!("the {controller} room is not valid: {r:?}")),
        }
    }

    /// The building of a type (the first one).
    fn first(&self, kind: &str) -> Option<BuildingId> {
        let k = self.content.factory.building(kind)?;
        self.host.factory.buildings.iter().find(|(_, b)| b.kind == k).map(|(id, _)| id)
    }

    /// Click every output slot of the open machine (its products go to the robot; a tank is
    /// emptied for a new material when all are in use).
    fn take_all_outputs(&mut self, id: BuildingId) {
        let outs = self.host.factory.building_view(id).map(|v| v.outputs).unwrap_or_default();
        for o in &outs {
            if let foundry_content::ItemRef::Material(m) = o.item
                && o.count > 0
            {
                self.free_tank_for(m);
            }
        }
        let n = outs.len();
        for index in 0..n {
            self.ui(&[UiAction::ClickSlot { slot: SlotRef::Building { building: id, group: BuildingSlots::Output, index }, click: SlotClick::LEFT }]);
        }
    }

    /// Make coke in the coke oven until the robot has `n`: choose the recipe, put coal in (the
    /// input and the fuel), and take the products out now and then.
    fn make_coke(&mut self, n: u32) -> Result<(), String> {
        self.make_coke_and(n, "coke")
    }

    /// Run the coke oven until the robot has `n` of `product` (coke or creosote).
    fn make_coke_and(&mut self, n: u32, product: &str) -> Result<(), String> {
        let id = self.first("coke_oven_controller").ok_or("no coke oven")?;
        self.open_id(id)?;
        self.choose(id, "coke")?;
        for _ in 0..400 {
            if self.held(product) >= n {
                return Ok(());
            }
            if self.held("raw_coal") < 64 {
                self.walk_to(930)?;
                self.dig("coal", |p| p.held("raw_coal") >= 300)?;
            }
            self.open_id(id)?;
            self.click_tank("raw_coal");
            self.take_all_outputs(id);
            self.ticks(300);
        }
        Err(format!("the coke oven made {} {product}: {:?}", self.held(product), self.host.factory.building_view(id)))
    }

    /// Run a steam machine on the robot's material until the robot has `n` of the product: choose
    /// the recipe, Ctrl + click the input tank, give steam, and take the products out of the
    /// window (an output port with no room keeps them in the machine).
    fn run_steam_machine(&mut self, plant: &Plant, id: BuildingId, recipe: &str, input: &str, product: &str, n: u32) -> Result<(), String> {
        self.open_id(id)?;
        self.choose(id, recipe)?;
        for _ in 0..300 {
            if self.held(product) >= n {
                return Ok(());
            }
            self.open_id(id)?;
            self.click_tank(input);
            self.run_plant(plant, 120);
            self.take_all_outputs(id);
        }
        Err(format!("{recipe}: {} of {n} {product}: {:?}", self.held(product), self.host.factory.building_view(id)))
    }

    /// Build a blast furnace: a ring of 4 × 4 tiles (2 × 2 inside) from its top-left tile, the
    /// controller and a steam blower in the left wall, a hatch in the roof (in), and two hatches in
    /// the right wall: the low one for the iron, the one above it for the slag. Returns the
    /// controller and the blower.
    fn build_blast_furnace(&mut self, plant: &mut Plant, x0: i32, y0: i32) -> Result<BuildingId, String> {
        self.craft("blast_furnace_controller", 1)?;
        self.craft("blast_furnace_hatch", 3)?;
        self.craft("steam_blower", 1)?;
        self.craft("firebrick_wall", 7)?;
        self.walk_to((x0 + 7) * TILE_SIZE)?;
        self.clear_area(x0 * TILE_SIZE, (x0 + 5) * TILE_SIZE, (y0 - 3) * TILE_SIZE, (y0 + 4) * TILE_SIZE);
        let mut ctrl = None;
        for dy in 0..4 {
            for dx in 0..4 {
                let inside = (1..3).contains(&dx) && (1..3).contains(&dy);
                if inside {
                    continue;
                }
                let part = match (dx, dy) {
                    (0, 1) => "blast_furnace_controller",
                    (0, 2) => "steam_blower",
                    (1, 0) | (3, 1) | (3, 2) => "blast_furnace_hatch",
                    _ => "firebrick_wall",
                };
                let id = self.place_at(part, TilePos::new(x0 + dx, y0 + dy))?;
                match part {
                    "blast_furnace_controller" => ctrl = Some(id),
                    "steam_blower" => plant.steamed.push(id),
                    _ => {}
                }
            }
        }
        let id = ctrl.ok_or("no controller")?;
        self.ticks(2);
        match self.host.factory.building_view(id).and_then(|v| v.room) {
            Some(r) if r.valid => Ok(id),
            r => Err(format!("the blast furnace room is not valid: {r:?}")),
        }
    }

    /// Run the blast furnace until molten pig iron pours out of its low tap, then point at it and
    /// hold the scan key.
    fn tap_iron(&mut self, plant: &Plant) -> Result<(), String> {
        let id = self.first("blast_furnace_controller").ok_or("no blast furnace")?;
        // Crushed iron ore: dig hematite and crush it.
        if self.held("crushed_hematite") < 64 {
            self.walk_to(720)?;
            self.dig("hematite", |p| p.held("raw_hematite") >= 80)?;
            let crusher = self.first("steam_crusher").ok_or("no steam crusher")?;
            self.run_steam_machine(plant, crusher, "crushed_hematite", "raw_hematite", "crushed_hematite", 64)?;
        }
        self.open_id(id)?;
        self.choose(id, "pig_iron_from_hematite")?;
        for m in ["crushed_hematite", "coke", "crushed_limestone"] {
            self.click_tank(m);
        }
        let iron = self.content.expect_material("molten_pig_iron");
        let r = self.host.factory.buildings.get(id).ok_or("no controller")?.cell_rect();
        let area = foundry_core::CellRect::new(r.x0 + 3 * TILE_SIZE, r.y0 - TILE_SIZE, r.x0 + 8 * TILE_SIZE, r.y0 + 4 * TILE_SIZE);
        // Stand where the taps can be seen and scanned.
        self.walk_to(area.x0 + 6 * TILE_SIZE)?;
        for k in 0..1500 {
            if k % 120 == 0 {
                self.open_id(id)?;
                for m in ["crushed_hematite", "coke", "crushed_limestone"] {
                    self.click_tank(m);
                }
            }
            self.run_plant(plant, 5);
            let found = (area.y0..area.y1).flat_map(|y| (area.x0..area.x1).map(move |x| CellPos::new(x, y))).find(|p| self.sim.cell(*p).material == iron);
            if let Some(at) = found {
                for _ in 0..10 {
                    self.apply(FactoryCommand::Input(PlayerInput { aim: at, scan: true, ..Default::default() }));
                    self.ticks(1);
                }
                self.apply(FactoryCommand::Input(PlayerInput::default()));
                if self.discovered("molten_pig_iron") {
                    return Ok(());
                }
            }
        }
        let inside: Vec<String> = (r.y0 - 8..r.y0 + 16)
            .map(|y| (r.x0..r.x0 + 4 * TILE_SIZE).map(|x| {
                let m = self.sim.cell(CellPos::new(x, y)).material;
                if m.is_air() { '.' } else { self.content.materials.ids[m.index()].chars().next().unwrap() }
            }).collect())
            .collect();
        Err(format!("no molten pig iron: {:?}\ncells:\n{}", self.host.factory.building_view(id), inside.join("\n")))
    }

    /// Steel: barrels at the taps of the blast furnace, an arm from the iron barrel into a steel
    /// converter, a plate mold under its tap and a crate. When the furnace has filled the barrel
    /// a little, the rest of the iron is given (the same line, run longer).
    fn make_steel(&mut self, plant: &mut Plant, x: i32, b: i32, plates: u32) -> Result<(), String> {
        self.lab_research(plant, "steel_making")?;
        for (part, n) in [("steel_converter", 1), ("arm", 1), ("plate_mold", 1), ("crate", 1), ("barrel", 2)] {
            self.craft(part, n)?;
        }
        self.walk_to((x + 22) * TILE_SIZE)?;
        self.clear_area((x + 26) * TILE_SIZE, (x + 32) * TILE_SIZE, (b - 6) * TILE_SIZE, (b + 1) * TILE_SIZE);
        // Iron and slag that poured out before froze there: the robot digs them away.
        for m in ["pig_iron_block", "slag_block", "molten_pig_iron", "molten_slag"] {
            let mat = self.content.expect_material(m);
            for cy in (b - 6) * TILE_SIZE..(b + 1) * TILE_SIZE {
                for cx in (x + 26) * TILE_SIZE..(x + 32) * TILE_SIZE {
                    if self.sim.cell(CellPos::new(cx, cy)).material == mat {
                        self.sim.set_cell(CellPos::new(cx, cy), foundry_core::MaterialId::AIR, None);
                    }
                }
            }
        }
        let iron_barrel = self.place_at("barrel", TilePos::new(x + 26, b - 1))?;
        self.place_at("barrel", TilePos::new(x + 26, b - 2))?;
        self.place_at("arm", TilePos::new(x + 27, b - 1))?;
        let converter = self.place_at("steel_converter", TilePos::new(x + 28, b - 3))?;
        let mold = self.place_at("plate_mold", TilePos::new(x + 29, b))?;
        let crate_ = self.place_at("crate", TilePos::new(x + 30, b))?;
        plant.steamed.push(converter);
        self.open_id(mold)?;
        self.choose(mold, "steel_plate")?;
        self.open_id(converter)?;
        self.choose(converter, "steel_converting")?;
        let iron = self.content.item("molten_pig_iron").unwrap();
        let furnace = self.first("blast_furnace_controller").ok_or("no blast furnace")?;
        // The furnace fills the barrel (the arm also takes from it into the converter).
        for _ in 0..60 {
            let in_barrel = self.host.factory.buildings.inventory(iron_barrel).map_or(0, |i| i.count(iron));
            let in_converter = self.host.factory.building_view(converter).map_or(0, |v| v.inputs.first().map_or(0, |b| b.count));
            if in_barrel + in_converter >= 14 {
                break;
            }
            self.open_id(furnace)?;
            for m in ["crushed_hematite", "coke", "crushed_limestone"] {
                self.click_tank(m);
            }
            self.run_plant(plant, 120);
        }
        let content = self.content.clone();
        self.host.factory.buildings.insert(&content, iron_barrel, iron, 400);
        for _ in 0..100 {
            if self.stored(crate_, "steel_plate") + self.held("steel_plate") >= plates {
                break;
            }
            if self.host.factory.buildings.inventory(iron_barrel).map_or(0, |i| i.count(iron)) < 64 {
                self.host.factory.buildings.insert(&content, iron_barrel, iron, 400);
            }
            self.run_plant(plant, 300);
        }
        if self.stored(crate_, "steel_plate") + self.held("steel_plate") >= plates {
            self.open_id(crate_)?;
            for index in 0..8 {
                self.ui(&[UiAction::ClickSlot { slot: SlotRef::Building { building: crate_, group: BuildingSlots::Input, index }, click: SlotClick::SHIFT_LEFT }]);
            }
            Ok(())
        } else {
            Err(format!("steel: {} plates; converter {:?}; mold {:?}", self.stored(crate_, "steel_plate"), self.host.factory.building_view(converter), self.host.factory.building_view(mold)))
        }
    }

    /// Glass vials: the steam furnace melts sand and ash, its tap pours into a vial mold, the mold
    /// fills a crate.
    fn make_glass(&mut self, plant: &mut Plant, x: i32, b: i32, vials: u32) -> Result<(), String> {
        self.lab_research(plant, "glass")?;
        // 16 sand and 4 ash make 16 molten glass: 2 vials.
        let sand = 16 * vials.div_ceil(2) + 16;
        if self.held("sand") < sand {
            self.walk_to(-78)?;
            self.dig("sand", move |p| p.held("sand") >= sand)?;
        }
        // Ash comes from burned wood (fires and rooms); the test gives it.
        self.give_item("ash", 4 * vials.div_ceil(2) + 8);
        self.craft("vial_mold", 1)?;
        self.craft("crate", 1)?;
        self.walk_to((x + 10) * TILE_SIZE)?;
        self.clear_area((x + 13) * TILE_SIZE, (x + 15) * TILE_SIZE, (b - 3) * TILE_SIZE, (b + 1) * TILE_SIZE);
        let mold = self.place_at("vial_mold", TilePos::new(x + 13, b))?;
        let crate_ = self.place_at("crate", TilePos::new(x + 14, b))?;
        let furnace = self.first("steam_furnace").ok_or("no steam furnace")?;
        self.open_id(mold)?;
        self.choose(mold, "glass_vial")?;
        self.open_id(furnace)?;
        self.choose(furnace, "molten_glass")?;
        for _ in 0..100 {
            if self.stored(crate_, "glass_vial") >= vials {
                self.open_id(crate_)?;
                for index in 0..8 {
                    self.ui(&[UiAction::ClickSlot { slot: SlotRef::Building { building: crate_, group: BuildingSlots::Input, index }, click: SlotClick::SHIFT_LEFT }]);
                }
                return Ok(());
            }
            self.open_id(furnace)?;
            self.click_tank("sand");
            self.click_tank("ash");
            self.run_plant(plant, 300);
        }
        Err(format!("glass: {} vials; furnace {:?}; mold {:?}", self.stored(crate_, "glass_vial"), self.host.factory.building_view(furnace), self.host.factory.building_view(mold)))
    }

    /// Rubber sheets: rubber tree wood → steam extractor → resin in a barrel under it → steam
    /// furnace (Rubber) → steam press, moved through the windows.
    fn make_rubber(&mut self, plant: &mut Plant, x: i32, b: i32, sheets: u32) -> Result<(), String> {
        // Rubber trees grow from 600 cells away from the Hub on: look along the surface for the
        // nearest one (a player sees them) and walk there.
        let rubber = self.content.expect_material("rubber_wood");
        let tree = [-1, 1]
            .into_iter()
            .flat_map(|dir| (600..1500).step_by(8).map(move |d| dir * d))
            .filter(|&tx| (900..1080).step_by(4).any(|y| self.sim.cell(CellPos::new(tx, y)).material == rubber))
            .min_by_key(|tx| tx.abs())
            .ok_or("no rubber tree within 1500 cells")?;
        self.walk_to(tree - 20 * tree.signum())?;
        self.dig("rubber_wood", |p| p.held("rubber_wood") >= 1)?;
        self.lab_research(plant, "rubber")?;
        while self.held("rubber_wood") < 16 * 2 * sheets {
            let more = 16 * 2 * sheets;
            self.dig("rubber_wood", move |p| p.held("rubber_wood") >= more)?;
        }
        for (part, n) in [("steam_extractor", 1), ("steam_press", 1), ("barrel", 1)] {
            self.craft(part, n)?;
        }
        self.walk_to((x - 26) * TILE_SIZE)?;
        self.clear_area((x - 32) * TILE_SIZE, (x - 27) * TILE_SIZE, (b - 4) * TILE_SIZE, (b + 1) * TILE_SIZE);
        let extractor = self.place_at("steam_extractor", TilePos::new(x - 32, b - 2))?;
        let barrel = self.place_at("barrel", TilePos::new(x - 31, b))?;
        let press = self.place_at("steam_press", TilePos::new(x - 29, b - 1))?;
        plant.steamed.push(extractor);
        plant.steamed.push(press);
        let furnace = self.first("steam_furnace").ok_or("no steam furnace")?;
        self.open_id(extractor)?;
        self.choose(extractor, "resin")?;
        self.open_id(furnace)?;
        self.choose(furnace, "rubber")?;
        self.open_id(press)?;
        self.choose(press, "rubber_sheet")?;
        for _ in 0..200 {
            if self.held("rubber_sheet") >= sheets {
                return Ok(());
            }
            self.open_id(extractor)?;
            self.click_tank("rubber_wood");
            self.run_plant(plant, 120);
            // Resin from the barrel into the furnace, rubber from the furnace into the press.
            self.open_id(barrel)?;
            self.ui(&[UiAction::ClickSlot { slot: SlotRef::Building { building: barrel, group: BuildingSlots::Input, index: 0 }, click: SlotClick::SHIFT_LEFT }]);
            self.open_id(furnace)?;
            self.click_tank("resin");
            self.run_plant(plant, 120);
            self.take_all_outputs(furnace);
            self.open_id(press)?;
            self.click_tank("rubber");
            self.run_plant(plant, 120);
            self.take_all_outputs(press);
        }
        Err(format!(
            "rubber: {} sheets; extractor {:?}; furnace {:?}; press {:?}",
            self.held("rubber_sheet"),
            self.host.factory.building_view(extractor),
            self.host.factory.building_view(furnace),
            self.host.factory.building_view(press)
        ))
    }

    /// Run a machine on parts in its window until the robot has `n` of the product.
    fn assemble(&mut self, plant: &Plant, id: BuildingId, recipe: &str, parts: &[&str], product: &str, n: u32) -> Result<(), String> {
        self.open_id(id)?;
        self.choose(id, recipe)?;
        for _ in 0..200 {
            if self.held(product) >= n {
                return Ok(());
            }
            self.open_id(id)?;
            for part in parts {
                self.click_part_stack(part);
            }
            self.run_plant(plant, 240);
            self.take_all_outputs(id);
        }
        Err(format!("{recipe}: {} of {n}: {:?}", self.held(product), self.host.factory.building_view(id)))
    }

    /// Deliver what the next Hub repair needs: open the Hub and shift + click the stacks.
    fn deliver_hub(&mut self, parts: &[&str]) -> Result<(), String> {
        let hub = self.host.factory.buildings.iter().find(|(_, b)| matches!(b.logic, foundry_factory::Logic::Hub(_))).map(|(id, _)| id).ok_or("no Hub")?;
        let stage = self.host.factory.progress.stage();
        self.open_id(hub)?;
        for _ in 0..4 {
            for part in parts {
                self.click_part_stack(part);
            }
            self.ticks(30);
        }
        if self.host.factory.progress.stage() > stage { Ok(()) } else { Err(format!("the Hub is not repaired: {:?}", self.host.factory.building_view(hub))) }
    }

    fn play_t1(&mut self, plant: &mut Plant, goal: &str) -> Option<Result<(), String>> {
        let (x, b) = match self.site() {
            Ok(s) => s,
            Err(e) => return Some(Err(e)),
        };
        Some(match goal {
            "t1_drill_head" => self.lab_research(plant, "bronze_drill_head"),
            "t1_boiler" => self.lab_research(plant, "steam_power").and_then(|_| self.build_at("small_boiler", TilePos::new(x, b - 1), -2).map(|_| ())),
            "t1_steam_crusher" => self.lab_research(plant, "steam_machines_1").and_then(|_| {
                let id = self.build_at("steam_crusher", TilePos::new(x + 3, b - 1), 1)?;
                plant.steamed.push(id);
                Ok(())
            }),
            "t1_arm" => self.lab_research(plant, "automation").and_then(|_| {
                self.build_at("arm", TilePos::new(x + 6, b), 1)?;
                self.build_at("arm", TilePos::new(x + 7, b), 1).map(|_| ())
            }),
            "t1_assembler" => self.build_at("steam_assembler", TilePos::new(x + 8, b - 1), 4).map(|id| plant.steamed.push(id)),
            "t1_steam_furnace" => self.lab_research(plant, "steam_smelting").and_then(|_| {
                let id = self.build_at("steam_furnace", TilePos::new(x + 11, b - 1), 8)?;
                plant.steamed.push(id);
                Ok(())
            }),
            "t1_iron_ore" => self.walk_to(720).and_then(|_| self.dig("hematite", |p| p.discovered("raw_hematite"))),
            "t1_steam_drill" => self.lab_research(plant, "steam_mining").and_then(|_| self.build_at("steam_drill", TilePos::new(x + 16, b - 1), 13).map(|_| ())),
            "t1_coal" => self.walk_to(930).and_then(|_| self.dig("coal", |p| p.held("raw_coal") >= 200)),
            "t1_coke_oven" => self
                .lab_research(plant, "ore_processing")
                .and_then(|_| self.lab_research(plant, "coke_and_iron"))
                .and_then(|_| self.build_room("coke_oven_controller", "coke_oven_hatch", "firebrick_wall", x + 18, b - 2).map(|_| ())),
            "t1_coke" => self.make_coke(100),
            "t1_limestone" => self
                .walk_to(-390)
                .and_then(|_| self.dig("limestone", |p| p.held("raw_limestone") >= 112))
                .and_then(|_| {
                    let crusher = self.first("steam_crusher").ok_or("no steam crusher")?;
                    self.run_steam_machine(plant, crusher, "crushed_limestone", "raw_limestone", "crushed_limestone", 100)
                }),
            "t1_blast_furnace" => self.build_blast_furnace(plant, x + 22, b - 3).map(|_| ()),
            "t1_pig_iron" => self.tap_iron(plant),
            "t1_steel" => self.make_steel(plant, x, b, 20),
            "t1_slag" => {
                // The converter's slag, and blast furnace slag (frozen, given) crushed.
                self.give_item("slag_block", 112);
                match self.first("steam_crusher") {
                    Some(crusher) => self.run_steam_machine(plant, crusher, "crushed_slag", "slag_block", "crushed_slag", 100),
                    None => Err("no steam crusher".into()),
                }
            }
            "t1_creosote" => self.make_coke_and(100, "creosote"),
            // 10 for the goal and 10 for the steam kits.
            "t1_glass" => self.make_glass(plant, x, b, 20),
            "t1_rubber" => self.make_rubber(plant, x, b, 10),
            "t1_iron_belts" => self.lab_research(plant, "iron_logistics").and_then(|_| {
                self.craft("iron_belt", 10)?;
                // A line of 20 belts left of the site, 3 tiles up (a belt bridge over the robot).
                self.walk_to((x - 20) * TILE_SIZE)?;
                self.clear_area((x - 30) * TILE_SIZE, (x - 10) * TILE_SIZE, (b - 5) * TILE_SIZE, (b + 1) * TILE_SIZE);
                for k in 0..20 {
                    self.place_at("iron_belt", TilePos::new(x - 30 + k, b - 3))?;
                }
                Ok(())
            }),
            "t1_steam_kits" => self.lab_research(plant, "steam_kit_tech").and_then(|_| {
                let assembler = self.first("steam_assembler").ok_or("no assembler")?;
                if self.held("bronze_pipe_section") < 10 {
                    self.craft("bronze_pipe_section", 10)?;
                }
                self.assemble(plant, assembler, "steam_kit", &["steel_plate", "bronze_pipe_section", "glass_vial"], "steam_kit", 10)
            }),
            "t1_steam_lab" => {
                // Glass panes from a pane mold, as the vials (given here).
                self.give_item("glass_pane", 54);
                self.build_at("steam_lab", TilePos::new(x + 24, b - 5), 20).map(|_| ())
            }
            "t2_electricity" => {
                // Steam kits from the steam kit line, and steel and bronze from the lines of Tier 1
                // (given).
                for (id, n) in [("steam_kit", 120), ("steel_plate", 60), ("bronze_plate", 40), ("bronze_pipe_section", 10), ("bronze_gear", 20)] {
                    self.give_item(id, n);
                }
                self.lab_research(plant, "electricity")
            }
            "t2_turbine" => self.build_at("steam_turbine", TilePos::new(x + 10, b - 6), 12).map(|id| plant.steamed.push(id)),
            "t2_cables" => {
                self.give_item("rubber_sheet", 10);
                self.give_item("copper_wire", 140);
                self.craft("copper_cable", 3).and_then(|_| {
                    // From the power port of the turbine (tile x + 11, b - 5) to the right.
                    self.walk_to((x + 14) * TILE_SIZE)?;
                    for k in 0..10 {
                        self.place_at("copper_cable", TilePos::new(x + 11 + k, b - 5))?;
                    }
                    Ok(())
                })
            }
            "t2_motors" => self.craft("electric_motor", 10),
            "t2_macerator" => self.lab_research(plant, "lv_machines").and_then(|_| self.build_at("macerator", TilePos::new(x + 13, b - 6), 14).map(|_| ())),
            "t2_electric_furnace" => self.build_at("electric_furnace", TilePos::new(x + 16, b - 6), 14).map(|_| ()),
            "t2_electric_assembler" => self
                .craft("electric_motor", 2)
                .and_then(|_| self.build_at("electric_assembler", TilePos::new(x + 18, b - 5), 14).map(|_| ())),
            "t2_electric_drill" => self.lab_research(plant, "electric_mining").and_then(|_| {
                self.craft("electric_motor", 3)?;
                self.build_at("electric_drill", TilePos::new(x - 20, b - 1), -16).map(|_| ())
            }),
            "t1_hub" => {
                // The grind of the second repair (more of what the lines above make) is given.
                for (id, n) in [("steel_plate", 100), ("copper_wire", 200), ("rubber_sheet", 20)] {
                    self.give_item(id, n);
                }
                self.walk_to(40).and_then(|_| self.deliver_hub(&["steel_plate", "copper_wire", "glass_pane", "rubber_sheet"]))
            }
            _ => return None,
        })
    }
}

#[test]
fn tier1_goals_can_be_done() {
    let mut p = Player::new();
    p.tier0_done();
    let mut plant = Plant { lab: None, steamed: vec![] };
    let goals: Vec<foundry_factory::progress::GoalDef> = p.host.factory.guide.goals.iter().filter(|g| g.tier == 1 || g.tier == 2).cloned().collect();
    let mut missing = vec![];
    for g in &goals {
        if !p.host.factory.progress.is_goal_done(&g.id) {
            let model = p.ui_guide();
            match next_goal(&model.guide) {
                NextGoal::Goal(next) => assert_eq!(next.id, g.id, "the guide should show {} as the next goal", g.id),
                other => panic!("the guide should show {} as the next goal, not {other:?}", g.id),
            }
        }
        let r = p.play_t1(&mut plant, &g.id);
        match r {
            None => {
                missing.push(g.id.clone());
                break;
            }
            Some(Err(e)) => panic!("goal {}: {e}", g.id),
            Some(Ok(())) => {
                p.ticks(GUIDE_PERIOD as u32 + 1);
                assert!(p.host.factory.progress.is_goal_done(&g.id), "goal {} is not done after its script", g.id);
            }
        }
    }
    assert!(missing.is_empty(), "goals with no script yet: {missing:?}");
}
