//! Progression on the generated terrain, using the same inputs as the player.
use super::*;

fn generated_player(seed: u64) -> Player {
    let content = Arc::new(Content::load_default().unwrap());
    let guide = Arc::new(Guide::load_default().unwrap());
    let demo = demo::build(content.clone(), Shape::Generated { depth_chunks: 16 }, seed);
    let mut sim = demo.sim;
    sim.set_threads(1);
    let host = FactoryHost::new_game(content.clone(), guide, &mut sim, demo.start_center.0 as i32).unwrap();
    let r = host.robot.rect();
    sim.apply(Command::SetView {
        area: CellRect::new(r.x0 - 700, r.y0 - 300, r.x1 + 700, r.y1 + 300),
    });
    let start = CellPos::new(r.x0, r.y1);
    let mut player = Player {
        host,
        sim,
        content,
        start,
        normal: NormalMode::new(),
        tick: 0,
        smelter: None,
        site: None,
        search_depth: 36,
        found: Default::default(),
    };
    player.ticks(30);
    player
}

#[test]
fn generated_start_supports_digging_crafting_bricks_and_research() {
    let mut player = generated_player(3);
    for goal in [
        "t0_dig",
        "t0_clay",
        "t0_wood",
        "t0_workbench",
        "t0_raw_bricks",
        "t0_campfire",
        "t0_fire_bricks",
        "t0_sluice",
        "t0_copper_ore",
        "t0_tin_ore",
        "t0_research_bronze",
        "t0_research_labs",
    ] {
        eprintln!("Generated world: {goal}");
        player
            .play(goal)
            .unwrap_or_else(|| panic!("no player script for {goal}"))
            .unwrap_or_else(|error| panic!("generated world goal {goal}: {error}"));
        player.ticks(GUIDE_PERIOD as u32 + 1);
        assert!(player.host.factory.progress.is_goal_done(goal), "{goal}");
    }
}
