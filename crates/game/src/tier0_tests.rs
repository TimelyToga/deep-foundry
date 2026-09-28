//! Can a player do each Tier 0 guide goal with the actions that exist now?
//!
//! The test starts a real normal game (the demo world, the Hub, the robot) and plays each goal
//! with the player actions: dig, scan, hand craft, place, research. It moves the robot next to
//! the material it needs, as a player walks there. Goals that the game cannot do yet have
//! `waits_for` in the data; the test checks that every other Tier 0 goal has a script here and
//! that the script completes it.

use super::*;
use crate::demo::{self, Shape};
use foundry_factory::progress::GuideState;

struct Player {
    host: FactoryHost,
    sim: Simulation,
    content: Arc<Content>,
    /// Where the robot stood at the start (flat ground right of the Hub).
    start: CellPos,
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
        let mut p = Self { host, sim, content, start };
        p.ticks(30);
        p
    }

    fn ticks(&mut self, n: u32) {
        for _ in 0..n {
            self.sim.tick();
            self.host.tick(&mut self.sim);
        }
    }

    fn apply(&mut self, cmd: FactoryCommand) {
        self.host.apply(cmd, &mut self.sim);
    }

    fn count(&self, id: &str) -> u32 {
        let item = self.content.item(id).unwrap_or_else(|| panic!("no item {id}"));
        self.host.factory.item_count(item)
    }

    /// Stand on the ground at column `x` (the player walks there).
    fn go_to(&mut self, x: i32) {
        let y = ground_top(&self.sim, &self.content, x);
        self.host.robot = Robot::standing_at(CellPos::new(x, y));
        self.ticks(2);
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
        for _ in 0..600 {
            if done(self) {
                self.stop();
                return Ok(());
            }
            let Some(at) = self.find(m) else { return Err(format!("no {material} left near the start")) };
            let (cx, _) = self.host.robot.center();
            if (at.x - cx as i32).abs() > 40 {
                self.go_to(at.x + 6);
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

    /// Take a building into the hand and place it near the start.
    fn place(&mut self, part: &str) -> Result<(), String> {
        let p = self.content.factory.part(part).ok_or(format!("no part {part}"))?;
        let kind = self.content.factory.part_def(p).building.ok_or(format!("{part} is not a building"))?;
        let before = self.host.factory.buildings.count_of(kind);
        self.go_to(self.start.x);
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

    /// Play one goal with the player actions. `None`: there is no script for this goal.
    fn play(&mut self, goal: &str) -> Option<Result<(), String>> {
        let has = |id: &'static str, n: u32| move |p: &Player| p.count(id) >= n;
        Some(match goal {
            "t0_dig" => self.dig("sand", has("sand", 100)),
            "t0_clay" => self.dig("clay", has("clay", 64)),
            "t0_wood" => self.dig("wood", has("wood", 60)),
            "t0_workbench" => self.craft("workbench", 1).and_then(|_| self.place("workbench")),
            "t0_copper_ore" => self.dig("malachite", |p| p.discovered("malachite")),
            "t0_tin_ore" => self.dig("cassiterite", |p| p.discovered("cassiterite")),
            "t0_raw_bricks" => self.dig("clay", has("clay", 128)).and_then(|_| self.craft("raw_clay_brick", 8)),
            "t0_campfire" => self.dig("wood", has("wood", 10)).and_then(|_| self.craft("campfire", 1)).and_then(|_| self.place("campfire")),
            "t0_research_bronze" => self.research("bronze"),
            "t0_sluice" => self.dig("wood", has("wood", 20)).and_then(|_| self.craft("sluice", 1)).and_then(|_| self.place("sluice")),
            "t0_research_labs" => self.research("research"),
            _ => return None,
        })
    }
}

#[test]
fn tier0_goals_can_be_done() {
    let mut p = Player::new();
    let goals: Vec<foundry_factory::progress::GoalDef> = p.host.factory.guide.goals.iter().filter(|g| g.tier == 0).cloned().collect();
    assert!(goals.len() >= 10);
    let mut waiting = vec![];
    for g in &goals {
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
    println!("Tier 0: {} goals can be done, {} wait: {waiting:?}", goals.len() - waiting.len(), waiting.len());
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
