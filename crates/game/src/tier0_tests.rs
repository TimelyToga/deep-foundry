//! Can a player do each Tier 0 guide goal with the actions that exist now?
//!
//! The test starts a real normal game (the demo world, the Hub, the robot) and plays the goals in
//! guide order with the player actions: walk (and fly over the Hub), dig, scan, hand craft,
//! place, open a building and click slots (through `NormalMode`, as the UI sends them), research.
//! Goals that the game cannot do yet have `waits_for` in the data; the test checks that every
//! other Tier 0 goal has a script here and that the script completes it.
//!
//! It also checks the guide: before each goal, the guide shows that goal as the next one; at
//! the end, the guide says what comes next ("Next: the kiln. It comes in a later update.").

use super::*;
use crate::demo::{self, Shape};
use crate::normal::NormalMode;
use crate::player::MoveInput;
use foundry_factory::progress::GuideState;
use foundry_ui::{NextGoal, SlotClick, SlotRef, UiAction, UiModel, next_goal};
use std::time::Instant;

struct Player {
    host: FactoryHost,
    sim: Simulation,
    content: Arc<Content>,
    /// Where the robot stood at the start (flat ground right of the Hub).
    start: CellPos,
    /// The main-thread side, for opening buildings and clicking slots as the UI does.
    normal: NormalMode,
    tick: u64,
}

impl Player {
    fn new() -> Self {
        let content = Arc::new(Content::load_default().unwrap());
        let guide = Arc::new(Guide::load_default().unwrap());
        let demo = demo::build(content.clone(), Shape::Box { width_chunks: 32, height_chunks: 16 }, 3);
        let mut sim = demo.sim;
        let host = FactoryHost::new_game(content.clone(), guide, &mut sim, demo.start_center.0 as i32).unwrap();
        // Make the chunks around the start, as the game does for the view.
        let r = host.robot.rect();
        sim.apply(Command::SetView { area: CellRect::new(r.x0 - 700, r.y0 - 300, r.x1 + 700, r.y1 + 300) });
        let start = CellPos::new(r.x0, r.y1);
        let mut p = Self { host, sim, content, start, normal: NormalMode::new(), tick: 0 };
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
        let (mut last, mut still, mut jump) = (self.host.robot.left, 0, 0u32);
        for _ in 0..4000 {
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
            if still >= 4 && jump == 0 {
                jump = 60;
            }
            let movement = MoveInput { x: dx.signum() as i8, jump: jump > 0 };
            jump = jump.saturating_sub(1);
            self.apply(FactoryCommand::Input(PlayerInput { movement, aim: r.center_cell(), ..Default::default() }));
            self.ticks(1);
        }
        self.stop();
        Err(format!("the robot could not walk from x {} to x {x}", self.center_x()))
    }

    /// The cell of a material nearest to the robot, near the start.
    fn find(&self, material: MaterialId) -> Option<CellPos> {
        let (cx, cy) = self.host.robot.center();
        let (cx, cy) = (cx as i32, cy as i32);
        let mut best: Option<(i64, CellPos)> = None;
        for y in self.start.y - 120..self.start.y + 120 {
            for x in self.start.x - 650..self.start.x + 650 {
                let p = CellPos::new(x, y);
                if self.sim.cell(p).material != material {
                    continue;
                }
                let d = ((x - cx) as i64).pow(2) + ((y - cy) as i64).pow(2);
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, p));
                }
            }
        }
        best.map(|(_, p)| p)
    }

    /// Dig cells of `material` until `done` is true.
    fn dig(&mut self, material: &str, done: impl Fn(&Player) -> bool) -> Result<(), String> {
        let m = self.content.expect_material(material);
        for _ in 0..800 {
            if done(self) {
                self.stop();
                return Ok(());
            }
            let Some(at) = self.find(m) else { return Err(format!("no {material} left near the start")) };
            if (at.x - self.center_x()).abs() > 40 {
                // Stand beside it, on the side of the robot.
                let side = if self.center_x() > at.x { 12 } else { -12 };
                self.walk_to(at.x + side)?;
            }
            self.apply(FactoryCommand::Input(PlayerInput { aim: at, dig: true, ..Default::default() }));
            self.ticks(3);
        }
        self.stop();
        if done(self) { Ok(()) } else { Err(format!("digging {material} did not finish the goal")) }
    }

    fn stop(&mut self) {
        self.apply(FactoryCommand::Input(PlayerInput::default()));
    }

    /// Hand craft and wait for the result.
    fn craft(&mut self, recipe: &str, n: u32) -> Result<(), String> {
        let r = self.content.factory.recipe(recipe).ok_or(format!("no recipe {recipe}"))?;
        let out = self.content.factory.recipe_def(r).outputs[0].item;
        let before = self.host.factory.item_count(out);
        self.apply(FactoryCommand::Craft { recipe: r, count: n });
        for _ in 0..3000 {
            if self.host.factory.item_count(out) >= before + n {
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
        self.walk_to(self.start.x)?;
        let kind = self.content.factory.building("campfire").ok_or("no campfire")?;
        let rect = self.host.factory.buildings.iter().find(|(_, b)| b.kind == kind).map(|(_, b)| b.cell_rect()).ok_or("no campfire placed")?;
        let id = self.open(CellPos::new(rect.x0 + 3, rect.y0 + 3))?;
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

    /// Play one goal with the player actions. `None`: there is no script for this goal.
    fn process_ore(&mut self, wash: bool) -> Result<(), String> {
        for i in 0..self.content.factory.techs.len() {
            self.host
                .factory
                .progress
                .debug_complete(&self.content, foundry_core::TechId(i as u16));
        }
        let x = self.center_x().div_euclid(foundry_core::TILE_SIZE) + 4 + if wash { 20 } else { 0 };
        let base = self.host.robot.center().1 as i32 / foundry_core::TILE_SIZE - 5;
        for y in (base - 12) * foundry_core::TILE_SIZE..(base + 2) * foundry_core::TILE_SIZE {
            for x_cell in (x - 1) * foundry_core::TILE_SIZE..(x + 15) * foundry_core::TILE_SIZE {
                self.sim
                    .set_cell(CellPos::new(x_cell, y), foundry_core::MaterialId::AIR, None);
            }
        }
        let stone = self.content.expect_material("stone");
        for y in (base + 1) * foundry_core::TILE_SIZE..(base + 2) * foundry_core::TILE_SIZE {
            for x_cell in (x + 2) * foundry_core::TILE_SIZE..(x + 15) * foundry_core::TILE_SIZE {
                self.sim.set_cell(CellPos::new(x_cell, y), stone, None);
            }
        }
        let place =
            |p: &mut Player, name: &str, at: TilePos| -> Result<foundry_core::BuildingId, String> {
                let kind = p
                    .content
                    .factory
                    .building(name)
                    .ok_or_else(|| format!("no building {name}"))?;
                p.host
                    .factory
                    .place(kind, at, 0, false, &mut p.sim)
                    .map_err(|e| format!("cannot place {name}: {e}"))
            };
        let _hopper = place(self, "hopper", TilePos::new(x, base - 7))?;
        let stamp = place(self, "stamp_mill", TilePos::new(x, base - 5))?;
        for belt_x in x + 2..=x + 6 {
            place(self, "wood_belt", TilePos::new(belt_x, base - 1))?;
        }
        let output = if wash {
            let sluice = place(self, "sluice", TilePos::new(x + 7, base - 2))?;
            self.host
                .factory
                .set_recipe(
                    sluice,
                    Some(
                        self.content
                            .factory
                            .recipe("washed_malachite")
                            .ok_or("no wash recipe")?,
                    ),
                )
                .map_err(|e| e.to_string())?;
            place(self, "crate", TilePos::new(x + 10, base - 2))?
        } else {
            place(self, "crate", TilePos::new(x + 2, base - 3))?
        };
        self.host
            .factory
            .set_recipe(
                stamp,
                Some(
                    self.content
                        .factory
                        .recipe("crushed_malachite")
                        .ok_or("no crush recipe")?,
                ),
            )
            .map_err(|e| e.to_string())?;

        let raw = self.content.expect_material("raw_malachite");
        for y in (base - 10) * foundry_core::TILE_SIZE..(base - 9) * foundry_core::TILE_SIZE {
            for x_cell in x * foundry_core::TILE_SIZE..(x + 1) * foundry_core::TILE_SIZE {
                self.sim.set_cell(CellPos::new(x_cell, y), raw, None);
            }
        }
        if wash {
            let water = self.content.expect_material("water");
            for y in (base - 6) * foundry_core::TILE_SIZE..(base - 4) * foundry_core::TILE_SIZE {
                for x_cell in (x + 7) * foundry_core::TILE_SIZE..(x + 8) * foundry_core::TILE_SIZE {
                    self.sim.set_cell(CellPos::new(x_cell, y), water, None);
                }
            }
        }
        let item = ItemRef::Material(self.content.expect_material(if wash {
            "washed_malachite"
        } else {
            "crushed_malachite"
        }));
        let needed = if wash { 3 } else { 16 };
        for _ in 0..4800 {
            self.ticks(1);
            if self
                .host
                .factory
                .buildings
                .inventory(output)
                .is_some_and(|inv| inv.count(item) >= needed)
            {
                return Ok(());
            }
        }
        Err(format!(
            "ore line produced {} of {item:?}; hopper={:?}, stamp={:?}, output={:?}",
            self.host.factory.item_count(item),
            self.host.factory.building_view(_hopper),
            self.host.factory.building_view(stamp),
            self.host.factory.building_view(output)
        ))
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
            "t0_campfire" => self.dig("wood", has("wood", 10)).and_then(|_| self.craft("campfire", 1)).and_then(|_| self.place("campfire")),
            "t0_fire_bricks" => self.fire_bricks(8),
            "t0_sluice" => self.dig("wood", has("wood", 20)).and_then(|_| self.craft("sluice", 1)).and_then(|_| self.place("sluice")),
            "t0_stamp_mill" => self.process_ore(false),
            "t0_wash_ore" => self.process_ore(true),
            "t0_research_labs" => self.research("research"),
            _ => return None,
        })
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
    let NextGoal::Waiting(next) = next_goal(&model.guide) else { panic!("the guide should say what comes next") };
    assert_eq!(next.next_text().unwrap(), "Next: the kiln. It comes in a later update.");
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
    // Dirt next to water becomes mud (a reaction), so count both.
    let mud = p.content.expect_material("mud");
    let area = CellRect::new(p.start.x - 700, p.start.y - 300, p.start.x + 700, p.start.y + 300);
    let before = p.sim.count_material(area, dirt) + p.sim.count_material(area, mud);
    p.dig("malachite", |p| p.count("raw_malachite") >= 30).unwrap();
    assert_eq!(p.count("dirt"), 0, "no dirt in the tanks");
    p.ticks(240);
    assert_eq!(p.sim.particles().count_material(dirt), 0, "the thrown dirt landed");
    let after = p.sim.count_material(area, dirt) + p.sim.count_material(area, mud);
    assert_eq!(after, before, "no dirt was lost");
}
