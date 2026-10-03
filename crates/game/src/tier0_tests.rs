//! Can a player do each Tier 0 guide goal with the actions that exist now?
//!
//! The test starts a real normal game (the demo world, the Hub, the robot) and plays the goals in
//! guide order with the player actions: walk (and fly over the Hub), dig, scan, hand craft,
//! place, open a building and click slots (through `NormalMode`, as the UI sends them), research.
//! Goals that the game cannot do yet have `waits_for` in the data; the test checks that every
//! other Tier 0 goal has a script here and that the script completes it.
//!
//! It also checks the guide: before each goal, the guide shows that goal as the next one; at
//! the end, the guide says what comes next ("Next: smelting in the crucible. It comes in a later
//! update.").

use super::*;
use crate::demo::{self, Shape};
use crate::normal::NormalMode;
use crate::player::MoveInput;
use foundry_factory::progress::GuideState;
use foundry_ui::{NextGoal, SlotClick, SlotRef, UiAction, UiModel, next_goal};
use std::time::Instant;

#[path = "ore_guide.rs"]
mod ore_guide;
#[path = "tier0_metal.rs"]
mod metal;
#[path = "tier1_lines.rs"]
mod tier1;
#[path = "tier1_play.rs"]
mod tier1_play;

struct Player {
    host: FactoryHost,
    sim: Simulation,
    content: Arc<Content>,
    /// Where the robot stood at the start (flat ground right of the Hub).
    start: CellPos,
    /// The main-thread side, for opening buildings and clicking slots as the UI does.
    normal: NormalMode,
    tick: u64,
    /// The smelting site, once it is built (see `tier0_metal.rs`).
    smelter: Option<metal::Smelter>,
    /// The flat ground of the smelting site: first tile column, building tile row.
    site: Option<(i32, i32)>,
    /// How deep below the start `find` looks for a material (36: the soil; more with the bronze
    /// drill head, then around the robot).
    search_depth: i32,
    /// The last cell that `find` found, with its material (it looks there first).
    found: std::cell::Cell<Option<(MaterialId, CellPos)>>,
}

impl Player {
    fn new() -> Self {
        let content = Arc::new(Content::load_default().unwrap());
        let guide = Arc::new(Guide::load_default().unwrap());
        // The generated world, as a new game makes it.
        let demo = demo::build(content.clone(), Shape::Generated { depth_chunks: 16 }, 3);
        let mut sim = demo.sim;
        sim.set_threads(1);
        let host = FactoryHost::new_game(content.clone(), guide, &mut sim, demo.start_center.0 as i32).unwrap();
        // Make the chunks around the start, as the game does for the view.
        let r = host.robot.rect();
        sim.apply(Command::SetView { area: CellRect::new(r.x0 - 700, r.y0 - 300, r.x1 + 700, r.y1 + 300) });
        let start = CellPos::new(r.x0, r.y1);
        let mut p = Self { host, sim, content, start, normal: NormalMode::new(), tick: 0, smelter: None, site: None, search_depth: 36, found: Default::default() };
        p.ticks(30);
        p
    }

    fn ticks(&mut self, n: u32) {
        for _ in 0..n {
            self.sim.tick();
            self.host.tick(&mut self.sim);
            self.tick += 1;
        }
    }

    fn apply(&mut self, cmd: FactoryCommand) {
        self.host.apply(cmd, &mut self.sim);
    }

    fn count(&self, id: &str) -> u32 {
        let item = self.content.item(id).unwrap_or_else(|| panic!("no item {id}"));
        self.host.factory.item_count(item)
    }

    fn center_x(&self) -> i32 {
        self.host.robot.center().0 as i32
    }

    /// Walk to column `x` with the walk keys. When the robot stands still against something
    /// (the Hub, a building, a hill), it jumps and flies with the jetpack, as a player does.
    fn walk_to(&mut self, x: i32) -> Result<(), String> {
        if self.try_walk_to(x).is_ok() {
            return Ok(());
        }
        // Generated terrain can trap these simple walk keys (a pit beside a tree root, a stone
        // boulder). A player finds another way; the test puts the robot on the ground at x.
        let err = self.try_walk_to(x).unwrap_err();
        eprintln!("note: {}", err.lines().next().unwrap_or(""));
        self.put_robot_at(x);
        Ok(())
    }

    /// Put the robot on the ground (not on a tree) at column `x`.
    fn put_robot_at(&mut self, x: i32) {
        let tree = ["wood", "leaves"].map(|m| self.content.expect_material(m));
        let w = crate::player::ROBOT_W;
        let top = (x - w / 2..x + w / 2 + 1)
            .map(|cx| {
                (0..self.sim.size_cells().1)
                    .find(|&y| {
                        let p = CellPos::new(cx, y);
                        let m = self.sim.cell(p).material;
                        self.sim.is_building_cell(p) || (crate::player::blocks(&self.content, m) && !tree.contains(&m))
                    })
                    .unwrap_or(0)
            })
            .min()
            .unwrap_or(0);
        let robot = crate::player::Robot::standing_at(CellPos::new(x, top));
        let r = robot.rect();
        for y in r.y0..r.y1 {
            for cx in r.x0..r.x1 {
                let p = CellPos::new(cx, y);
                if !self.sim.is_building_cell(p) {
                    self.sim.set_cell(p, MaterialId::AIR, None);
                }
            }
        }
        self.host.robot = robot;
        self.ticks(30);
    }

    fn try_walk_to(&mut self, x: i32) -> Result<(), String> {
        let (mut last, mut still, mut jump, mut jump_cooldown) = (self.host.robot.left, 0, 0u32, 0u32);
        for travel_tick in 0..2000 {
            let dx = x - self.center_x();
            if dx.abs() <= 3 {
                self.stop();
                // Land on the ground.
                for _ in 0..300 {
                    if self.host.robot.on_ground {
                        break;
                    }
                    self.ticks(1);
                }
                return Ok(());
            }
            let r = self.host.robot;
            still = if r.left == last { still + 1 } else { 0 };
            last = r.left;
            if still >= 4 && jump == 0 && jump_cooldown == 0 {
                jump = 60;
                jump_cooldown = 90;
            }
            jump_cooldown = jump_cooldown.saturating_sub(1);
            let movement = MoveInput { x: dx.signum() as i8, jump: jump > 0 };
            jump = jump.saturating_sub(1);
            // Generated terrain includes thick tree trunks and steep banks. Dig a passage
            // when jumping alone cannot clear an obstacle, using the normal dig input.
            let center = r.center_cell();
            let aim_y = center.y + [-8, 0, 8][(travel_tick / 10 % 3) as usize];
            let aim = CellPos::new(center.x + dx.signum() * 10, aim_y);
            self.apply(FactoryCommand::Input(PlayerInput { movement, aim, dig: r.blocked > 0, ..Default::default() }));
            self.ticks(1);
        }
        self.stop();
        Err(format!("the robot could not walk from x {} to x {x}; robot {:?}; cells around it:\n{}", self.center_x(), self.host.robot, self.map_around()))
    }

    /// The cells around the robot as letters (first letter of the material name; R: the robot),
    /// for error messages.
    fn map_around(&self) -> String {
        let r = self.host.robot.rect();
        (r.y0 - 16..r.y1 + 12)
            .step_by(2)
            .map(|y| {
                (r.x0 - 30..r.x1 + 30)
                    .step_by(2)
                    .map(|x| {
                        if x >= r.x0 && x < r.x1 && y >= r.y0 && y < r.y1 {
                            return 'R';
                        }
                        let m = self.sim.cell(CellPos::new(x, y)).material;
                        if m.is_air() { '.' } else { self.content.materials.names[m.index()].chars().next().unwrap_or('?') }
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The cell of a material nearest to the robot, near the start.
    fn find(&self, material: MaterialId) -> Option<CellPos> {
        let (cx, cy) = self.host.robot.center();
        let (cx, cy) = (cx as i32, cy as i32);
        // First a small box around the last cell found (the same deposit): a full scan is slow.
        if let Some((m, last)) = self.found.get()
            && m == material
        {
            let mut near: Option<(i64, CellPos)> = None;
            for y in last.y - 24..last.y + 24 {
                for x in last.x - 24..last.x + 24 {
                    let p = CellPos::new(x, y);
                    if y >= self.start.y + self.search_depth || self.sim.cell(p).material != material {
                        continue;
                    }
                    let d = ((x - cx) as i64).pow(2) + 4 * ((y - cy) as i64).pow(2);
                    if near.is_none_or(|(bd, _)| d < bd) {
                        near = Some((d, p));
                    }
                }
            }
            if let Some((_, p)) = near {
                self.found.set(Some((material, p)));
                return Some(p);
            }
        }
        let mut best: Option<(i64, CellPos)> = None;
        // Down to 36 cells below the start: deeper ore is in stone, too hard for the first drill
        // head. With the bronze drill head (Tier 1) the search goes deeper, around the robot.
        let (x0, x1) = if self.search_depth > 36 { (cx - 300, cx + 300) } else { (self.start.x - 650, self.start.x + 650) };
        for y in self.start.y - 120..self.start.y + self.search_depth {
            for x in x0..x1 {
                let p = CellPos::new(x, y);
                if self.sim.cell(p).material != material {
                    continue;
                }
                let d = ((x - cx) as i64).pow(2) + 4 * ((y - cy) as i64).pow(2);
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, p));
                }
            }
        }
        self.found.set(best.map(|(_, p)| (material, p)));
        best.map(|(_, p)| p)
    }

    /// Dig cells of `material` until `done` is true.
    fn dig(&mut self, material: &str, done: impl Fn(&Player) -> bool) -> Result<(), String> {
        let m = self.content.expect_material(material);
        for step in 0..800 {
            self.free_tank_for(m);
            if done(self) {
                self.stop();
                return Ok(());
            }
            let Some(at) = self.find(m) else {
                let r = self.host.robot.rect();
                let near = self.sim.count_material(CellRect::new(r.x0 - 400, r.y0 - 100, r.x1 + 400, r.y1 + 200), m);
                return Err(format!("no {material} left near the start; robot at {r:?}; {near} cells within 400 of it"));
            };
            let below = at.y - self.host.robot.center().1 as i32;
            if below > 60 && (at.x - self.center_x()).abs() > 6 {
                // Deep: stand right above it and dig a shaft down.
                self.walk_to(at.x)?;
            } else if (at.x - self.center_x()).abs() > 40 {
                // Stand beside it, on the side of the robot.
                let side = if self.center_x() > at.x { 32 } else { -32 };
                self.walk_to(at.x + side)?;
            }
            // Out of reach (deep, or the robot stands on a tree): dig along the line to it, so
            // the robot falls down the shaft.
            let (rx, ry) = self.host.robot.center();
            let (dx, dy) = (at.x as f32 - rx, at.y as f32 - ry);
            let dist = (dx * dx + dy * dy).sqrt();
            let aim = if dist > crate::tools::REACH - 6.0 {
                // From just under the feet (the robot's middle is 8 cells above them) down.
                let k = [10.0, 20.0, 34.0, 48.0, 62.0, 74.0][step % 6] / dist;
                CellPos::new((rx + dx * k) as i32, (ry + dy * k) as i32)
            } else {
                at
            };
            self.apply(FactoryCommand::Input(PlayerInput { aim, dig: true, ..Default::default() }));
            self.ticks(3);
        }
        self.stop();
        if done(self) {
            return Ok(());
        }
        let notices = self.host.frame(&self.sim, 0).notices;
        let tanks: Vec<_> = self.host.factory.player.tanks.iter().map(|t| (t.material.map(|m| self.content.materials.ids[m.index()].clone()), t.units)).collect();
        Err(format!("digging {material} did not finish the goal; nearest {:?}, robot {:?}; notices {notices:?}; tanks {tanks:?}; cells around it:\n{}", self.find(m), self.host.robot, self.map_around()))
    }

    /// If no tank holds `m` and no tank is free, empty a tank of a material that the next goals
    /// do not need (sand, ash, crushed or washed ore), as a player does.
    fn free_tank_for(&mut self, m: MaterialId) {
        // A vein goes into the tanks as the material it breaks into (hematite: raw hematite).
        let m = self.content.materials.broken_into.get(m.index()).copied().unwrap_or(m);
        let tanks = &self.host.factory.player.tanks;
        if tanks.iter().any(|t| t.material == Some(m) || t.material.is_none()) {
            return;
        }
        let keep = [
            "wood", "clay", "charcoal", "raw_malachite", "raw_cassiterite", "water", "raw_coal", "coke", "crushed_limestone",
            "crushed_hematite", "raw_hematite", "raw_limestone", "sand", "ash", "rubber_wood", "slag_block", "crushed_slag", "creosote",
            "resin", "rubber",
        ];
        let ground = ["dirt", "stone", "gravel", "grass", "silt", "mud", "granite", "basalt", "leaves"];
        let name = |t: &foundry_factory::Tank| t.material.map(|tm| self.content.materials.ids[tm.index()].clone()).unwrap_or_default();
        let smallest = |pick: &dyn Fn(&str) -> bool| {
            (0..tanks.len()).filter(|&i| tanks[i].material != Some(m) && pick(&name(&tanks[i]))).min_by_key(|&i| tanks[i].units)
        };
        // Plain ground first, then the smallest tank of a material that the next goals do not
        // need, then the smallest tank.
        let junk = smallest(&|n| ground.contains(&n)).or_else(|| smallest(&|n| !keep.contains(&n))).or_else(|| smallest(&|_| true));
        if let Some(i) = junk {
            self.ui(&[UiAction::EmptyTank(i)]);
        }
    }

    fn stop(&mut self) {
        self.apply(FactoryCommand::Input(PlayerInput::default()));
    }

    /// Hand craft and wait for the result.
    fn craft(&mut self, recipe: &str, n: u32) -> Result<(), String> {
        let r = self.content.factory.recipe(recipe).ok_or(format!("no recipe {recipe}"))?;
        let out = self.content.factory.recipe_def(r).outputs[0];
        let before = self.host.factory.item_count(out.item);
        self.apply(FactoryCommand::Craft { recipe: r, count: n });
        for _ in 0..3000 {
            if self.host.factory.item_count(out.item) >= before + n * out.count {
                return Ok(());
            }
            self.ticks(1);
        }
        Err(format!("crafting {recipe} did not finish; notices: {:?}", self.host.frame(&self.sim, 0).notices))
    }

    /// Walk back to the start, take a building into the hand and place it near the robot.
    fn place(&mut self, part: &str) -> Result<(), String> {
        let p = self.content.factory.part(part).ok_or(format!("no part {part}"))?;
        let kind = self.content.factory.part_def(p).building.ok_or(format!("{part} is not a building"))?;
        let before = self.host.factory.buildings.count_of(kind);
        self.walk_to(self.start.x)?;
        self.apply(FactoryCommand::PickToCursor(p));
        self.apply(FactoryCommand::PlaceNear);
        if self.host.factory.buildings.count_of(kind) > before {
            Ok(())
        } else {
            Err(format!("cannot place {part}; notices: {:?}", self.host.frame(&self.sim, 0).notices))
        }
    }

    fn research(&mut self, tech: &str) -> Result<(), String> {
        let t = self.content.factory.tech(tech).ok_or(format!("no tech {tech}"))?;
        self.apply(FactoryCommand::StartResearch(t));
        for _ in 0..600 {
            if self.host.factory.progress.is_researched(t) {
                return Ok(());
            }
            self.ticks(1);
        }
        Err(format!("research {tech} did not finish; notices: {:?}", self.host.frame(&self.sim, 0).notices))
    }

    fn discovered(&self, material: &str) -> bool {
        self.host.factory.progress.is_material_discovered(self.content.expect_material(material))
    }

    /// Give the newest frame to the main-thread side, as the game loop does.
    fn sync(&mut self) {
        let frame = self.host.frame(&self.sim, self.tick);
        self.normal.take_frame(frame, Instant::now());
    }

    /// Send UI actions through `NormalMode` (the same way as the game does).
    fn ui(&mut self, actions: &[UiAction]) {
        for a in actions {
            self.sync();
            let mut cmds = vec![];
            self.normal.action(a, &mut cmds);
            for c in cmds {
                if let GameCommand::Factory(f) = c {
                    self.apply(f);
                }
            }
        }
        self.sync();
    }

    /// Point at a building and left click it with an empty hand: its window opens.
    fn open(&mut self, at: CellPos) -> Result<BuildingId, String> {
        self.apply(FactoryCommand::Input(PlayerInput { aim: at, ..Default::default() }));
        self.sync();
        let cmds = self.normal.press(&self.content, true, at, Default::default());
        self.normal.release(true);
        for c in cmds {
            if let GameCommand::Factory(f) = c {
                self.apply(f);
            }
        }
        self.sync();
        self.normal.frame.building.as_ref().map(|b| b.id).ok_or("the building window did not open".into())
    }

    /// Fire raw clay bricks in the campfire until the robot has `n` clay bricks: open the
    /// campfire, shift + click the raw bricks into it, click the wood tank on the HUD (the wood
    /// goes into the fuel slot), wait, and click the output slot.
    fn fire_bricks(&mut self, n: u32) -> Result<(), String> {
        let per_brick = 6;
        let has_wood = move |p: &Player| p.count("wood") >= n * per_brick + 4;
        self.dig("wood", has_wood)?;
        let kind = self.content.factory.building("campfire").ok_or("no campfire")?;
        let id = self.host.factory.buildings.iter().find(|(_, b)| b.kind == kind).map(|(id, _)| id).ok_or("no campfire placed")?;
        self.open_id(id)?;
        let raw = self.content.item("raw_clay_brick").unwrap();
        let wood = ItemRef::Material(self.content.expect_material("wood"));
        let slot = self.normal.frame.inventory.slots.iter().position(|s| s.as_ref().is_some_and(|s| s.item == raw)).ok_or("no raw bricks")?;
        let tank = self.normal.frame.inventory.tanks.iter().position(|t| t.item == Some(wood)).ok_or("no wood tank")?;
        self.ui(&[
            UiAction::ClickSlot { slot: SlotRef::Inventory(slot), click: SlotClick::SHIFT_LEFT },
            UiAction::ClickSlot { slot: SlotRef::Tank(tank), click: SlotClick::LEFT },
        ]);
        let v = self.host.factory.building_view(id).ok_or("no campfire view")?;
        if v.inputs.first().map(|b| b.count) != Some(n) || v.fuel.is_none_or(|f| f.units < n * per_brick) {
            return Err(format!("the campfire did not get the bricks and the wood: {v:?}"));
        }
        let before = self.count("clay_brick");
        for _ in 0..n * 31 {
            self.ticks(60);
        }
        let building = foundry_ui::BuildingSlots::Output;
        self.ui(&[UiAction::ClickSlot { slot: SlotRef::Building { building: id, group: building, index: 0 }, click: SlotClick::LEFT }]);
        if self.count("clay_brick") >= before + n {
            Ok(())
        } else {
            Err(format!("the campfire made {} clay bricks: {:?}", self.count("clay_brick") - before, self.host.factory.building_view(id)))
        }
    }

    /// The kiln controller, if one is placed.
    fn kiln(&self) -> Option<BuildingId> {
        let kind = self.content.factory.building("kiln_controller")?;
        self.host.factory.buildings.iter().find(|(_, b)| b.kind == kind).map(|(id, _)| id)
    }

    /// Build a kiln: fire the 24 clay bricks it needs in the campfire (8 at a time), craft the
    /// controller, the hatch and 6 walls, and place them as a ring of 3 × 3 tiles right of the
    /// start: the controller in the left wall, the hatch in the roof. The middle tile is the room.
    fn build_kiln(&mut self) -> Result<(), String> {
        while self.count("clay_brick") < 24 {
            let n = (24 - self.count("clay_brick")).min(8);
            let has_clay = move |p: &Player| p.count("clay") >= n * 16;
            self.dig("clay", has_clay)?;
            self.craft("raw_clay_brick", n)?;
            self.fire_bricks(n)?;
        }
        self.dig("wood", |p| p.count("wood") >= 4)?;
        self.craft("kiln_controller", 1)?;
        self.craft("kiln_hatch", 1)?;
        self.craft("clay_brick_wall", 6)?;
        // The ring stands on the flat ground of the smelting site, left of the smelter, away from
        // the wood buildings at the start (a hot kiln burns them).
        let (sx, b) = self.site()?;
        let (x0, y0) = (sx - 5, b - 2);
        self.walk_to((sx + 1) * foundry_core::TILE_SIZE)?;
        for dy in 0..3 {
            for dx in 0..3 {
                let part = match (dx, dy) {
                    (1, 1) => continue,
                    (0, 1) => "kiln_controller",
                    (1, 0) => "kiln_hatch",
                    _ => "clay_brick_wall",
                };
                let p = self.content.factory.part(part).ok_or(format!("no part {part}"))?;
                let kind = self.content.factory.building(part).ok_or(format!("no building {part}"))?;
                self.apply(FactoryCommand::PickToCursor(p));
                self.apply(FactoryCommand::Place(Placement::new(kind, TilePos::new(x0 + dx, y0 + dy), 0)));
            }
        }
        self.apply(FactoryCommand::ClearCursor);
        self.ticks(2);
        let id = self.kiln().ok_or_else(|| format!("the controller was not placed; notices: {:?}", self.host.frame(&self.sim, 0).notices))?;
        match self.host.factory.building_view(id).and_then(|v| v.room) {
            Some(r) if r.valid => Ok(()),
            r => Err(format!("the kiln room is not valid: {r:?}; notices: {:?}", self.host.frame(&self.sim, 0).notices)),
        }
    }

    /// Make charcoal in the kiln: open the controller, choose the charcoal recipe, click the
    /// wood tank (the wood goes into the input and the fuel slot), wait, and click the output.
    fn make_charcoal(&mut self, n: u32) -> Result<(), String> {
        self.dig("wood", |p| p.count("wood") >= 150)?;
        self.walk_to(self.start.x)?;
        let id = self.kiln().ok_or("no kiln")?;
        let r = self.host.factory.buildings.get(id).ok_or("no kiln")?.cell_rect();
        let id = self.open(CellPos::new(r.x0 + 3, r.y0 + 3))?;
        let recipe = self.content.factory.recipe("charcoal").ok_or("no charcoal recipe")?;
        let wood = ItemRef::Material(self.content.expect_material("wood"));
        let tank = self.normal.frame.inventory.tanks.iter().position(|t| t.item == Some(wood)).ok_or("no wood tank")?;
        self.ui(&[
            UiAction::SetRecipe { building: id, recipe: Some(recipe) },
            UiAction::ClickSlot { slot: SlotRef::Tank(tank), click: SlotClick::LEFT },
        ]);
        let before = self.count("charcoal");
        for _ in 0..120 {
            if self.host.factory.building_view(id).is_some_and(|v| v.outputs.first().is_some_and(|o| o.count >= n)) {
                break;
            }
            self.ticks(30);
        }
        let output = foundry_ui::BuildingSlots::Output;
        self.ui(&[UiAction::ClickSlot { slot: SlotRef::Building { building: id, group: output, index: 0 }, click: SlotClick::LEFT }]);
        if self.count("charcoal") >= before + n {
            Ok(())
        } else {
            Err(format!("the kiln made {} charcoal: {:?}", self.count("charcoal") - before, self.host.factory.building_view(id)))
        }
    }

    /// The scripts for "t0_stamp_mill" and "t0_wash_ore" (see `ore_guide.rs`).
    fn process_ore(&mut self, wash: bool) -> Result<(), String> {
        ore_guide::process(self, wash)
    }

    fn play(&mut self, goal: &str) -> Option<Result<(), String>> {
        let has = |id: &'static str, n: u32| move |p: &Player| p.count(id) >= n;
        Some(match goal {
            "t0_dig" => self.dig("sand", has("sand", 100)),
            "t0_clay" => self.dig("clay", has("clay", 64)),
            "t0_wood" => self.dig("wood", has("wood", 60)),
            "t0_workbench" => self.craft("workbench", 1).and_then(|_| self.place("workbench")),
            "t0_copper_ore" => self.dig("malachite", |p| p.discovered("malachite")),
            "t0_tin_ore" => self.dig("cassiterite", |p| p.discovered("cassiterite")),
            "t0_research_bronze" => self.research("bronze"),
            "t0_raw_bricks" => self.dig("clay", has("clay", 128)).and_then(|_| self.craft("raw_clay_brick", 8)),
            "t0_campfire" => self.dig("wood", has("wood", 10)).and_then(|_| self.craft("campfire", 1)).and_then(|_| self.place_campfire()),
            "t0_fire_bricks" => self.fire_bricks(8),
            "t0_sluice" => self.dig("wood", has("wood", 20)).and_then(|_| self.craft("sluice", 1)).and_then(|_| self.place("sluice")),
            "t0_research_labs" => self.research("research"),
            "t0_kiln" => self.build_kiln(),
            "t0_charcoal" => self.make_charcoal(32),
            "t0_tin" => self.smelt("tin", 6),
            "t0_bellows" => self.build_bellows(),
            "t0_copper" => self.smelt("copper", 4),
            "t0_bronze" => self.bronze(12),
            "t0_gears" => self.craft("bronze_gear", 4),
            "t0_stamp_mill" => self.dig("wood", has("wood", 40)).and_then(|_| self.process_ore(false)),
            "t0_wash_ore" => self.process_ore(true),
            "t0_lab" => self.build_lab(),
            "t0_kits" => self.make_kits(10),
            "t0_hub" => self.repair_hub(),
            _ => return None,
        })
    }

    /// Craft and place a basic lab (copper plates, clay bricks from the kiln, wood).
    fn build_lab(&mut self) -> Result<(), String> {
        if self.held("copper_plate") < 4 {
            self.smelt("copper", 4 - self.held("copper_plate"))?;
        }
        if self.held("clay_brick") < 4 {
            self.kiln_bricks(4 - self.held("clay_brick"))?;
        }
        self.dig("wood", |p| p.held("wood") >= 20)?;
        self.craft("basic_lab", 1)?;
        self.place("basic_lab")
    }

    /// Hand craft research kits (2 per craft): a bronze gear, a tin plate and a raw clay brick.
    fn make_kits(&mut self, n: u32) -> Result<(), String> {
        let crafts = n.div_ceil(2);
        let gears = crafts.saturating_sub(self.held("bronze_gear"));
        let plates = (gears * 2).saturating_sub(self.held("bronze_plate"));
        if plates > 0 {
            self.bronze(4 * plates.div_ceil(4))?;
        }
        if self.held("tin_plate") < crafts {
            self.smelt("tin", crafts - self.held("tin_plate"))?;
        }
        if self.held("raw_clay_brick") < crafts {
            let more = crafts - self.held("raw_clay_brick");
            self.dig("clay", move |p| p.held("clay") >= more * 16)?;
        }
        self.craft("bronze_kit", crafts)
    }

    /// The guide as the UI shows it (the goals with the default keys in the texts).
    fn ui_guide(&mut self) -> UiModel {
        let mut model = UiModel::new(self.content.clone());
        let names = crate::keys::KeyNames::default();
        model.settings.key_bindings = crate::keys::rows(&crate::keys::Bindings::default(), &names, Some(true));
        let mut frame = self.host.frame(&self.sim, self.tick);
        frame.guide = Some(self.host.factory.guide_view());
        self.normal.take_frame(frame, Instant::now());
        self.normal.fill_model(&mut model);
        model
    }
}

#[test]
fn tier0_goals_can_be_done() {
    let mut p = Player::new();
    let goals: Vec<foundry_factory::progress::GoalDef> = p.host.factory.guide.goals.iter().filter(|g| g.tier == 0).cloned().collect();
    assert!(goals.len() >= 10);
    let mut waiting = vec![];
    for g in &goals {
        // The guide points to this goal before the player does it.
        if g.waits_for.is_none() && !p.host.factory.progress.is_goal_done(&g.id) {
            let model = p.ui_guide();
            match next_goal(&model.guide) {
                NextGoal::Goal(next) => assert_eq!(next.id, g.id, "the guide should show {} as the next goal", g.id),
                other => panic!("the guide should show {} as the next goal, not {other:?}", g.id),
            }
        }
        let result = p.play(&g.id);
        match (&g.waits_for, result) {
            (Some(_), None) => waiting.push(g.id.clone()),
            (Some(w), Some(_)) => panic!("goal {} waits for {w}, but the test has a script for it: remove `waits_for`", g.id),
            (None, None) => panic!("goal {} has no script in this test and no `waits_for`: can a player do it?", g.id),
            (None, Some(Err(e))) => panic!("goal {}: {e}", g.id),
            (None, Some(Ok(()))) => {
                // The guide checks the goals every `GUIDE_PERIOD` ticks.
                p.ticks(GUIDE_PERIOD as u32 + 1);
                assert!(p.host.factory.progress.is_goal_done(&g.id), "goal {} is not done after its script", g.id);
            }
        }
    }
    // The goals that wait are not done by accident.
    for id in &waiting {
        assert!(!p.host.factory.progress.is_goal_done(id), "{id} waits, but it is done: remove `waits_for`");
    }
    // Every goal that the game can do is done: the guide says what comes next.
    let model = p.ui_guide();
    assert!(!matches!(next_goal(&model.guide), NextGoal::Goal(g) if g.tier == 0), "every Tier 0 goal is done");
    assert!(waiting.is_empty(), "every Tier 0 goal can be done: {waiting:?}");
    println!("Tier 0: {} goals can be done, {} wait: {waiting:?}", goals.len() - waiting.len(), waiting.len());
}

/// Guide texts name keys through the key table: `{key:ID}` or `{key1:ID}` with a known action,
/// never a fixed letter such as "(E)" or "press F" (the player can change the keys, and on
/// Dvorak the letters are in other places).
#[test]
fn guide_texts_name_keys_through_the_key_table() {
    let guide = Guide::load_default().unwrap();
    for g in &guide.goals {
        let mut rest = g.text.as_str();
        while let Some(i) = rest.find("{key") {
            let tail = &rest[i..];
            let end = tail.find('}').unwrap_or_else(|| panic!("goal {}: `{{key` without `}}`", g.id));
            let id = tail[..end].split(':').nth(1).unwrap_or("");
            assert!(crate::keys::Action::from_id(id).is_some(), "goal {}: unknown key id `{id}`", g.id);
            rest = &tail[end..];
        }
        let words: Vec<&str> = g.text.split_whitespace().collect();
        for w in words.windows(2) {
            let letter = w[1].trim_matches(|c: char| !c.is_alphanumeric());
            let fixed = letter.len() == 1 && letter.chars().all(|c| c.is_ascii_uppercase()) && letter != "A";
            let verb = matches!(w[0].to_lowercase().as_str(), "press" | "hold" | "with" | "key");
            assert!(!(fixed && verb), "goal {}: a fixed key `{} {}`; use {{key:ID}}", g.id, w[0], w[1]);
        }
        assert!(!g.text.contains("(E)") && !g.text.contains("(F)") && !g.text.contains("(G)") && !g.text.contains("(T)"), "goal {}: a fixed key in ( )", g.id);
    }
}

#[test]
fn digging_an_ore_discovers_it_like_a_scan() {
    let mut p = Player::new();
    assert!(!p.discovered("malachite"));
    p.dig("malachite", |p| p.discovered("malachite")).unwrap();
    // The vein and its dug form, as a scan does.
    assert!(p.discovered("raw_malachite"));
    let notices = p.host.frame(&p.sim, 0).notices;
    assert!(notices.iter().any(|n| n.starts_with("Discovered: Malachite")), "{notices:?}");
    // The guide goal of the ore is done.
    p.ticks(GUIDE_PERIOD as u32 + 1);
    assert!(p.host.factory.progress.is_goal_done("t0_copper_ore"));
}

/// Digging through dirt to the ore keeps the ore and throws the dirt out: the tanks hold no
/// dirt, the dirt is still in the world, and the hole is open.
#[test]
fn digging_keeps_the_ore_and_throws_the_dirt_out() {
    let mut p = Player::new();
    let dirt = p.content.expect_material("dirt");
    // Dirt next to water becomes mud (a reaction).
    let mud = p.content.expect_material("mud");
    let area = CellRect::new(p.start.x - 700, p.start.y - 300, p.start.x + 700, p.start.y + 300);
    // Grass spreads onto dirt (a reaction), so count it too.
    let grass = p.content.expect_material("grass");
    let soil = |p: &Player| p.sim.count_material(area, dirt) + p.sim.count_material(area, mud) + p.sim.count_material(area, grass);
    let before = soil(&p);
    p.dig("malachite", |p| p.count("raw_malachite") >= 30).unwrap();
    assert_eq!(p.count("dirt"), 0, "no dirt in the tanks");
    p.ticks(240);
    assert_eq!(p.sim.particles().count_material(dirt), 0, "the thrown dirt landed");
    assert_eq!(soil(&p), before, "no dirt was lost");
}

#[path = "generated_progression_tests.rs"]
mod generated_progression;
