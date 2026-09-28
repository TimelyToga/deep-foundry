//! `--screenshot`: build the demo world, run some ticks with no thread and no window,
//! render one frame with the real renderer and the UI, and save it as a PNG file.
//!
//! `--ui-state` picks the screen (main menu, pause menu, game, materials window, ...).
//! `--no-ui` draws only the world. With `--mode normal` (or a normal-mode screen such as
//! `building`) the world has the Hub and the robot, and the factory ticks with the cells. Some
//! screens put items into the inventory first, so that the picture shows something.

use crate::args::{Args, UiState};
use crate::debug_panel::{self, PanelState, StatsView};
use crate::demo;
use crate::construct::{BuildView, Mods};
use crate::factory_host::{FactoryCommand, FactoryFrame, FactoryHost, GameCommand, Placement, PlayerInput};
use crate::normal::NormalMode;
use crate::overlay;
use crate::player::MoveInput;
use crate::render_setup;
use crate::robot_sprite::{Anim, Forced, ToolKind, ToolUse};
use crate::tools;
use crate::ui::{self, SandboxUi};
use anyhow::{Context, Result};
use foundry_content::{Content, ItemRef};
use foundry_core::{CellPos, Command, TILE_SIZE, TilePos};
use foundry_factory::Guide;
use foundry_render::headless::{CAPTURE_FORMAT, capture, capture_with, create_device};
use foundry_render::wgpu;
use foundry_render::{Camera, LIGHT_MARGIN, Renderer};
use foundry_ui::{GameMode, GameState, HoverView, MenuPage, PerfView, WindowKind};
use glam::{DVec2, UVec2};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

mod steam_line;

pub fn run(args: &Args, out: &Path, content: Arc<Content>) -> Result<()> {
    let start = Instant::now();
    let state = if args.no_ui { UiState::Playing } else { args.ui_state.unwrap_or(UiState::Playing) };
    let demo = demo::build(content.clone(), args.shape(), args.seed);
    let mut sim = demo.sim;
    let normal = args.start_mode() == GameMode::Normal;
    let mut host = if normal {
        let guide = Arc::new(Guide::load_default().unwrap_or_default());
        let h = FactoryHost::new_game(content.clone(), guide, &mut sim, demo.start_center.0 as i32).map_err(anyhow::Error::msg)?;
        Some(h)
    } else {
        None
    };
    let built = start.elapsed();

    let size = args.image_size();
    let zoom = args.zoom.unwrap_or(if normal { 4.0 } else { 2.0 });
    let robot_center = |h: &FactoryHost| {
        let (x, y) = h.robot.center();
        DVec2::new(x as f64, y as f64 - 8.0)
    };
    let mut center = match (&args.center, &host) {
        (Some(c), _) => DVec2::from(*c),
        (None, Some(h)) => robot_center(h),
        (None, None) => DVec2::from(demo.start_center),
    };
    // The screen plus the light margin, as the game asks for it.
    let view_of = |c: DVec2| Camera::new(c, zoom, UVec2::new(size.0, size.1)).visible_rect().expand(LIGHT_MARGIN);

    // Ask for the chunks on the screen, like the game does. The view is also the anchor: only
    // chunks near it are made and updated.
    sim.apply(Command::SetView { area: view_of(center) });
    // The render views (`--view`). The awake chunks view needs the debug data of the simulation.
    let mut render_settings = foundry_render::RenderSettings::default();
    let mut views = render_setup::DebugViews::default();
    render_setup::apply_view_names(&args.view, &mut render_settings, &mut views).map_err(anyhow::Error::msg)?;
    if views.awake_chunks {
        sim.apply(Command::SetDebug(true));
    }
    let tick_start = Instant::now();
    if let (Some(h), Some(x)) = (host.as_mut(), args.robot_x) {
        let feet = CellPos::new(x, crate::factory_host::ground_top(&sim, &content, x));
        h.robot = crate::player::Robot::standing_at(feet);
        if args.center.is_none() {
            center = robot_center(h);
            sim.apply(Command::SetView { area: view_of(center) });
        }
    }
    if let Some(h) = host.as_mut()
        && args.walk != 0
    {
        let walk = PlayerInput { movement: MoveInput { x: args.walk.signum() as i8, jump: false }, ..Default::default() };
        h.apply(FactoryCommand::Input(walk), &mut sim);
        for _ in 0..args.walk.unsigned_abs() {
            sim.tick();
            h.tick(&mut sim);
            if args.center.is_none() {
                center = robot_center(h);
                sim.apply(Command::SetView { area: view_of(center) });
            }
        }
        h.apply(FactoryCommand::Input(PlayerInput::default()), &mut sim);
    }
    for _ in 0..args.ticks {
        sim.tick();
        if let Some(h) = host.as_mut() {
            h.tick(&mut sim);
        }
    }
    // --pose and --face: the robot acts for the picture.
    let mut pose = Pose::default();
    if let Some(h) = host.as_mut() {
        if let Some(left) = args.face_left {
            h.robot.facing = if left { -1 } else { 1 };
        }
        if let Some((name, frame)) = &args.pose {
            pose = act(h, &mut sim, name, *frame);
            if args.center.is_none() {
                center = robot_center(h);
                sim.apply(Command::SetView { area: view_of(center) });
            }
        }
    }
    let ticks_time = tick_start.elapsed();
    let camera = Camera::new(center, zoom, UVec2::new(size.0, size.1));
    // The normal-mode screens: items, buildings and windows for the picture.
    let mut normal_view = None;
    if let Some(h) = host.as_mut() {
        let (frame, build) = setup_normal_screen(h, &mut sim, state, camera.visible_rect());
        normal_view = Some(NormalView { frame, build });
    }
    let snapshot = sim.take_snapshot();

    let (device, queue) = create_device().context("no GPU adapter found")?;
    let mut renderer = Renderer::new(&device, &queue, CAPTURE_FORMAT, &content);
    renderer.set_time(args.ticks as f64 * foundry_core::TICK_SECONDS);
    renderer.set_surface_level(render_setup::surface_level(snapshot.world_cells));
    *renderer.settings_mut() = render_settings;
    let upload_start = Instant::now();
    if state.has_world() {
        let evicted = renderer.apply_snapshot(&snapshot, &camera);
        if !evicted.is_empty() {
            log::warn!("{} chunks did not fit on the GPU; use a larger zoom", evicted.len());
        }
    }
    let upload_time = upload_start.elapsed();
    // The robot: sprites on the cell grid, and its lights.
    if let Some(h) = &host {
        let look = render_setup::load_robot_look(&mut renderer);
        let mut sprites = vec![];
        let at = h.robot.draw_top_left();
        look.sprites(&h.robot, at, pose.tool, sim.tick_count(), pose.forced, &mut sprites);
        renderer.set_sprites(&sprites);
        render_setup::robot_lights(&mut renderer, Some((&h.robot, at)));
    }

    let pixels = if args.no_ui {
        capture(&device, &queue, &mut renderer, &camera)
    } else {
        let ctx = egui::Context::default();
        let saves_dir = args.saves.clone().unwrap_or_else(crate::saves::default_dir);
        let mut ui = SandboxUi::new(&ctx, content.clone(), saves_dir);
        ui.model.settings.ui_scale = args.ui_scale;
        // The default keys (a picture does not read the settings file).
        let names = crate::keys::KeyNames::default();
        ui.model.settings.key_bindings = crate::keys::rows(&crate::keys::Bindings::default(), &names, state.has_world().then_some(normal));
        // A picture has no frame rate, so the HUD shows "-" for the FPS.
        ui.model.perf = Some(PerfView {
            fps: 0.0,
            tick_ms: snapshot.stats.tick_ms,
            ticks_per_second: 0.0,
            awake_chunks: snapshot.stats.awake_chunks,
            loaded_chunks: snapshot.stats.loaded_chunks,
        });
        let stats = StatsView {
            tick: snapshot.stats.tick,
            tick_ms: snapshot.stats.tick_ms,
            awake_chunks: snapshot.stats.awake_chunks,
            loaded_chunks: snapshot.stats.loaded_chunks,
            packed_chunks: snapshot.stats.packed_chunks,
            gpu_chunks: renderer.stats().resident_chunks,
            gpu_capacity: renderer.stats().chunk_capacity,
            zoom: camera.zoom,
            particles: snapshot.particles.len() as u32,
            sections: snapshot.stats.sections.clone(),
            ..Default::default()
        };
        let show_debug = state == UiState::Debug;
        if let Some(nv) = &normal_view {
            ui.set_mode(GameMode::Normal);
            let mut n = NormalMode::new();
            n.take_frame(nv.frame.clone(), Instant::now());
            n.fill_model(&mut ui.model);
            ui.model.perf = None;
            ui.model.settings.show_fps = false;
            // The HUD shows the building or the cell at the mouse, else at the image center.
            let pos = nv.build.mouse.unwrap_or(CellPos::new(center.x.floor() as i32, center.y.floor() as i32));
            let cell = sim.cell(pos);
            ui.model.hover =
                n.hover(pos).or(Some(HoverView::Cell { pos, material: cell.material, temperature: cell.temperature as f32 }));
            ui.model.hover_detail = n.hover_detail(&content, ui.model.hover.as_ref());
        }
        match state {
            UiState::Menu => {}
            UiState::NewGame => ui.ui.open_menu(MenuPage::NewGame),
            UiState::Load => ui.ui.open_menu(MenuPage::Load),
            UiState::Settings => ui.ui.open_menu(MenuPage::Settings),
            UiState::Pause | UiState::Save => {
                ui.model.state = GameState::Paused;
                if state == UiState::Save {
                    ui.ui.open_menu(MenuPage::Save);
                }
            }
            UiState::Playing
            | UiState::Inventory
            | UiState::Debug
            | UiState::Building
            | UiState::Ghost
            | UiState::Hub
            | UiState::GhostRed
            | UiState::Drag
            | UiState::Alt
            | UiState::Remove
            | UiState::Tanks
            | UiState::Campfire
            | UiState::OreLine
            | UiState::Kiln
            | UiState::KilnHole
            | UiState::SteamLine => {
                ui.model.state = GameState::Playing;
                if state == UiState::Inventory {
                    ui.ui.open_window(WindowKind::Character);
                }
            }
            UiState::Research | UiState::Guide | UiState::GuideWorkbench | UiState::GuideDone => {
                ui.model.state = GameState::Playing;
                ui.ui.open_window(if state == UiState::Research { WindowKind::Research } else { WindowKind::Guide });
            }
        }
        if state.has_world() && normal_view.is_none() {
            // The HUD shows the cell at the image center (there is no mouse).
            let pos = CellPos::new(center.x.floor() as i32, center.y.floor() as i32);
            let cell = sim.cell(pos);
            ui.model.hover = Some(HoverView::Cell { pos, material: cell.material, temperature: cell.temperature as f32 });
        }
        let mut panel = OffscreenUi::new(&device, ctx, ui, size);
        panel.overlay = normal_view.map(|nv| (camera, nv));
        panel.panel.render = render_settings;
        panel.panel.awake_chunks = views.awake_chunks;
        if views.awake_chunks {
            panel.debug_chunks = Some((camera, snapshot.debug_chunks.clone()));
        }
        panel.layout(&device, &queue, &stats, show_debug);
        let draw_world = state.has_world();
        let clear = (!draw_world).then_some(wgpu::Color::BLACK);
        capture_with(&device, &queue, &mut renderer, &camera, |encoder, view| panel.draw(&device, &queue, encoder, view, clear))
    };

    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("cannot make folder {}", dir.display()))?;
    }
    let image = image::RgbaImage::from_raw(size.0, size.1, pixels).context("image size does not match")?;
    image.save(out).with_context(|| format!("cannot write {}", out.display()))?;

    let view = camera.visible_rect();
    println!(
        "saved {} ({}x{}, zoom {}, center {:.1},{:.1}, cells {}..{} x {}..{}, screen {:?}); world built in {:.0} ms, {} ticks in {:.0} ms, {} chunks uploaded in {:.2} ms, {} chunks drawn",
        out.display(),
        size.0,
        size.1,
        camera.zoom,
        center.x,
        center.y,
        view.x0,
        view.x1,
        view.y0,
        view.y1,
        state,
        built.as_secs_f64() * 1000.0,
        args.ticks,
        ticks_time.as_secs_f64() * 1000.0,
        renderer.stats().uploaded_chunks,
        upload_time.as_secs_f64() * 1000.0,
        renderer.stats().drawn_chunks,
    );
    Ok(())
}

/// What `--pose` did: the tool in use and the fixed animation frame.
#[derive(Default)]
struct Pose {
    tool: Option<ToolUse>,
    forced: Option<Forced>,
}

/// `--pose`: the robot moves or uses a tool for some ticks, so the picture shows the real state
/// (in the air for "jump" and "fly", an empty jetpack for "fall" and "land", dug cells for
/// "dig"). An animation name also fixes the animation (and the frame, if given).
fn act(h: &mut FactoryHost, sim: &mut foundry_sim::Simulation, name: &str, frame: Option<usize>) -> Pose {
    let content = h.factory.content.clone();
    let facing = h.robot.facing;
    let run = |h: &mut FactoryHost, sim: &mut foundry_sim::Simulation, input: PlayerInput, ticks: u32| {
        h.apply(FactoryCommand::Input(input), sim);
        for _ in 0..ticks {
            sim.tick();
            h.tick(sim);
        }
    };
    let go = |x: i8, jump: bool| PlayerInput { movement: MoveInput { x, jump }, ..Default::default() };
    // A place in front of the robot: in the ground for dig and scan, in the air for spray.
    let (cx, cy) = h.robot.center();
    let front = |dx: f32, dy: f32| tools::clamp_aim(&h.robot, CellPos::new((cx + dx * facing as f32) as i32, (cy + dy) as i32));
    let mut tool = None;
    match name {
        "walk" => run(h, sim, go(facing, false), 24),
        "jump" => run(h, sim, go(0, true), 6),
        // Fly until the fuel is empty, then fall (the gauge shows "Jet empty").
        "fall" => run(h, sim, go(0, true), 80),
        // The same, then land (the gauge waits for the refill).
        "land" => {
            run(h, sim, go(0, true), 80);
            h.apply(FactoryCommand::Input(go(0, false)), sim);
            for _ in 0..300 {
                sim.tick();
                h.tick(sim);
                if h.robot.on_ground {
                    break;
                }
            }
            run(h, sim, go(0, false), 2);
        }
        "fly" => run(h, sim, go(0, true), 45),
        "dig" => {
            let aim = front(24.0, 16.0);
            run(h, sim, PlayerInput { aim, dig: true, ..Default::default() }, 12);
            let m = h.frame(sim, 0).dug_material;
            let color = m.and_then(|m| content.materials.colors[m.index()].first().copied());
            tool = Some(ToolUse { kind: ToolKind::Dig, aim, working: m.is_some(), color });
        }
        "spray" => {
            let sand = content.expect_material("sand");
            h.factory.player.insert(&content, ItemRef::Material(sand), 400);
            let aim = front(22.0, -4.0);
            run(h, sim, PlayerInput { aim, spray: Some(sand), ..Default::default() }, 10);
            let color = content.materials.colors[sand.index()].first().copied();
            tool = Some(ToolUse { kind: ToolKind::Spray, aim, working: true, color });
        }
        "scan" => {
            let aim = front(20.0, 12.0);
            run(h, sim, PlayerInput { aim, scan: true, ..Default::default() }, 2);
            tool = Some(ToolUse { kind: ToolKind::Scan, aim, working: true, color: None });
        }
        _ => {}
    }
    h.apply(FactoryCommand::Input(PlayerInput { aim: tool.map_or(CellPos::default(), |t| t.aim), ..Default::default() }), sim);
    Pose { tool, forced: Anim::from_name(name).map(|anim| Forced { anim, frame }) }
}

/// The factory views of a normal-mode picture.
struct NormalView {
    frame: FactoryFrame,
    build: BuildView,
}

/// Make the normal-mode screen: put the items for the picture into the inventory, place and open
/// buildings, and drive the main-thread side (`NormalMode`) like the mouse would. Returns the
/// factory views and the construction shapes.
fn setup_normal_screen(
    h: &mut FactoryHost,
    sim: &mut foundry_sim::Simulation,
    state: UiState,
    view: foundry_core::CellRect,
) -> (FactoryFrame, BuildView) {
    let content = h.factory.content.clone();
    let part = |id: &str| content.factory.part(id).expect("part in the data");
    let kind = |id: &str| content.factory.building(id).expect("building in the data");
    let mat = |id: &str| ItemRef::Material(content.expect_material(id));
    let give = |h: &mut FactoryHost, item: ItemRef, n: u32| {
        h.factory.player.insert(&content, item, n);
    };
    let run = |h: &mut FactoryHost, sim: &mut foundry_sim::Simulation, cmds: Vec<GameCommand>| {
        for c in cmds {
            if let GameCommand::Factory(c) = c {
                h.apply(c, sim);
            }
        }
    };
    // The middle cell of a footprint.
    let middle = |at: foundry_core::TilePos, k: foundry_core::BuildingKindId| {
        let size = content.factory.building_def(k).size;
        CellPos::new(at.x * TILE_SIZE + size.0 as i32 * 4, at.y * TILE_SIZE + size.1 as i32 * 4)
    };
    let mut n = NormalMode::new();
    let mut mouse = None;
    match state {
        UiState::Inventory => {
            // The robot dug some clay and sand, and makes clay bricks. It found dirt, stone
            // (gravel) and copper ore: the keep or drop list shows them.
            give(h, mat("clay"), 180);
            give(h, mat("sand"), 120);
            for id in ["dirt", "stone", "malachite"] {
                h.factory.scan(content.expect_material(id));
            }
            h.apply(FactoryCommand::Craft { recipe: content.factory.recipe("raw_clay_brick").expect("recipe"), count: 5 }, sim);
            for _ in 0..40 {
                h.tick(sim);
            }
        }
        UiState::Building => {
            // A crate right of the robot, with some items in it.
            // The start inventory has one crate.
            h.apply(FactoryCommand::PickToCursor(part("crate")), sim);
            if let Some(at) = h.free_place(kind("crate"), sim) {
                h.apply(FactoryCommand::Place(Placement::new(kind("crate"), at, 0)), sim);
                h.apply(FactoryCommand::ClearCursor, sim);
                let cell = at.origin();
                h.apply(FactoryCommand::OpenAt(cell), sim);
                if let Some(id) = h.building_at(cell)
                    && let Some(inv) = h.factory.buildings.inventory_mut(id)
                {
                    // A crate slot holds a part stack or bulk material.
                    inv.insert(&content, mat("clay"), 8300);
                    inv.insert(&content, mat("sand"), 4100);
                    inv.insert(&content, mat("raw_malachite"), 950);
                    inv.insert(&content, ItemRef::Part(part("raw_clay_brick")), 12);
                    inv.insert(&content, ItemRef::Part(part("wood_belt")), 20);
                }
                // The robot has material in its tanks too.
                give(h, mat("dirt"), 5200);
                give(h, mat("raw_cassiterite"), 640);
            }
        }
        UiState::Tanks => {
            // Every tank is full; the robot digs and has no room.
            for (id, units) in [("clay", 6000u32), ("sand", 6000), ("dirt", 6000), ("gravel", 6000), ("raw_malachite", 6000), ("raw_cassiterite", 6000), ("wood", 5970), ("ash", 6000)] {
                let have = h.factory.player.count(mat(id));
                give(h, mat(id), units.saturating_sub(have));
            }
            let r = h.robot.rect();
            let aim = CellPos::new(r.x1 + 4, r.y1 + 3);
            h.apply(FactoryCommand::Input(PlayerInput { aim, dig: true, view, ..Default::default() }), sim);
            for _ in 0..5 {
                h.tick(sim);
            }
            h.apply(FactoryCommand::Input(PlayerInput { view, ..Default::default() }), sim);
        }
        UiState::Hub => {
            // Some deliveries are done already.
            let brick = ItemRef::Part(part("clay_brick"));
            h.factory.progress.deliver(&content, foundry_content::Stack { item: brick, count: 36 });
            give(h, brick, 20);
            // Copper wire is for stage 2: the Hub holds it until then.
            let hub_id = h.factory.buildings.iter().find(|(_, b)| content.factory.building_def(b.kind).kind == "hub").map(|(id, _)| id);
            if let Some(hub) = hub_id {
                h.factory.buildings.insert(&content, hub, ItemRef::Part(part("copper_wire")), 40);
            }
            let hub = h.factory.buildings.iter().find(|(_, b)| content.factory.building_def(b.kind).kind == "hub").map(|(_, b)| b.at.origin());
            if let Some(cell) = hub {
                h.apply(FactoryCommand::OpenAt(cell), sim);
            }
        }
        UiState::Ghost | UiState::GhostRed => {
            // A steam crusher (three ports) in the hand, at a free place or down in the ground.
            give(h, ItemRef::Part(part("steam_crusher")), 2);
            h.apply(FactoryCommand::PickToCursor(part("steam_crusher")), sim);
            let k = kind("steam_crusher");
            if let Some(at) = h.free_place(k, sim) {
                let at = if state == UiState::GhostRed { foundry_core::TilePos::new(at.x + 1, at.y + 2) } else { at };
                mouse = Some(middle(at, k));
            }
        }
        UiState::Drag => {
            // A line of belts dragged to the right, starting next to the robot.
            give(h, ItemRef::Part(part("wood_belt")), 40);
            h.apply(FactoryCommand::PickToCursor(part("wood_belt")), sim);
            n.take_frame(h.frame(sim, 0), Instant::now());
            if let Some(at) = h.free_place(kind("wood_belt"), sim) {
                let start = middle(at, kind("wood_belt"));
                let cmds = n.press(&content, true, start, Mods::default());
                run(h, sim, cmds);
                for step in 1..=6 {
                    let m = start.offset(step * TILE_SIZE, 0);
                    let cmds = n.update(m);
                    run(h, sim, cmds);
                    n.take_frame(h.frame(sim, 0), Instant::now());
                    mouse = Some(m);
                }
            }
        }
        UiState::Alt => {
            // Two machines side by side with recipes, and belts on the first, seen in the alt mode.
            let mut x = None;
            give(h, ItemRef::Part(part("steam_press")), 1);
            give(h, ItemRef::Part(part("steam_crusher")), 1);
            h.apply(FactoryCommand::PickToCursor(part("steam_press")), sim);
            let first = h.free_place(kind("steam_press"), sim);
            if let Some(at) = first {
                h.apply(FactoryCommand::Place(Placement::new(kind("steam_press"), at, 0)), sim);
                h.apply(FactoryCommand::PickToCursor(part("steam_crusher")), sim);
                // The first free place to the right, as low as possible.
                let k = kind("steam_crusher");
                let spot = (3..12).flat_map(|dx| (-4..=3).rev().map(move |dy| foundry_core::TilePos::new(at.x + dx, at.y + dy)));
                if let Some(c) = spot.into_iter().find(|t| h.check_place(k, *t, 0, false, sim).is_ok()) {
                    h.apply(FactoryCommand::Place(Placement::new(k, c, 0)), sim);
                    x = Some(c);
                }
            }
            // Recipes: the first recipe each machine can run (the alt icon is its product).
            let ids: Vec<_> = h.factory.buildings.iter().map(|(id, b)| (id, b.kind)).collect();
            for (id, k) in ids {
                if let Some(r) = foundry_factory::Buildings::recipes_for(&content, k).first() {
                    let _ = h.factory.buildings.set_recipe(&content, id, Some(*r));
                }
            }
            give(h, ItemRef::Part(part("wood_belt")), 8);
            h.apply(FactoryCommand::PickToCursor(part("wood_belt")), sim);
            if let Some(at) = first {
                for i in 0..2 {
                    h.apply(FactoryCommand::Place(Placement::new(kind("wood_belt"), foundry_core::TilePos::new(at.x + i, at.y - 1), 2)), sim);
                }
            }
            h.apply(FactoryCommand::ClearCursor, sim);
            for _ in 0..30 {
                h.tick(sim);
            }
            n.build.alt = true;
            mouse = x.map(|at| middle(at, kind("steam_crusher")));
        }
        UiState::Remove => {
            // Four belts; the remove button went down on the first and moved over the others.
            give(h, ItemRef::Part(part("wood_belt")), 8);
            h.apply(FactoryCommand::PickToCursor(part("wood_belt")), sim);
            if let Some(at) = h.free_place(kind("wood_belt"), sim) {
                for i in 0..4 {
                    h.apply(FactoryCommand::Place(Placement::new(kind("wood_belt"), foundry_core::TilePos::new(at.x + i, at.y), 0)), sim);
                }
                h.apply(FactoryCommand::ClearCursor, sim);
                let first = middle(at, kind("wood_belt"));
                h.apply(FactoryCommand::Input(PlayerInput { aim: first, ..Default::default() }), sim);
                n.take_frame(h.frame(sim, 0), Instant::now());
                n.press(&content, false, first, Mods::default());
                for i in 0..4 {
                    let m = first.offset(i * TILE_SIZE, 0);
                    if let Some(c) = n.input(&content, m, view) {
                        run(h, sim, vec![c]);
                    }
                    mouse = Some(m);
                }
                for _ in 0..6 {
                    h.tick(sim);
                }
            }
        }
        UiState::Research => {
            h.apply(FactoryCommand::Windows { research: true, guide: false }, sim);
        }
        UiState::Guide => {
            give(h, mat("clay"), 40);
            h.apply(FactoryCommand::Windows { research: false, guide: true }, sim);
        }
        UiState::Campfire => {
            // A campfire right of the robot: raw clay bricks in it, wood in the fuel slot, and
            // the first bricks are done.
            give(h, ItemRef::Part(part("campfire")), 1);
            h.apply(FactoryCommand::PickToCursor(part("campfire")), sim);
            h.apply(FactoryCommand::PlaceNear, sim);
            h.apply(FactoryCommand::ClearCursor, sim);
            let fire = h.factory.buildings.iter().find(|(_, b)| b.kind == kind("campfire")).map(|(id, b)| (id, b.at.origin()));
            if let Some((id, cell)) = fire {
                h.factory.buildings.insert(&content, id, ItemRef::Part(part("raw_clay_brick")), 8);
                h.factory.buildings.insert(&content, id, mat("wood"), 60);
                for _ in 0..(2 * 30 + 12) * 60 {
                    h.tick(sim);
                }
                h.apply(FactoryCommand::OpenAt(cell), sim);
            }
            give(h, mat("wood"), 40);
        }
        UiState::OreLine => {
            // Live Tier 0 line at the right of the spawn: hopper -> stamp mill -> belts ->
            // water-fed sluice -> crate. Feed it real cells and run the simulation to completion.
            for i in 0..content.factory.techs.len() {
                h.factory
                    .progress
                    .debug_complete(&content, foundry_core::TechId(i as u16));
            }
            // Put the output crate within the robot's interaction range so its inventory is
            // visible in the final capture.
            let x = h.robot.center().0 as i32 / TILE_SIZE - 12;
            let ground = h.robot.center().1 as i32 / TILE_SIZE - 5;
            for cy in (ground - 12) * TILE_SIZE..(ground + 2) * TILE_SIZE {
                for cx in (x - 1) * TILE_SIZE..(x + 15) * TILE_SIZE {
                    sim.set_cell(CellPos::new(cx, cy), foundry_core::MaterialId::AIR, None);
                }
            }
            let hopper_at = TilePos::new(x, ground - 7);
            let stamp_at = TilePos::new(x, ground - 5);
            let hopper = h
                .factory
                .place(kind("hopper"), hopper_at, 0, false, sim)
                .expect("hopper fits");
            let stamp = h
                .factory
                .place(kind("stamp_mill"), stamp_at, 0, false, sim)
                .expect("stamp mill fits");
            for belt_x in x + 2..=x + 6 {
                h.factory
                    .place(
                        kind("wood_belt"),
                        TilePos::new(belt_x, ground - 1),
                        0,
                        false,
                        sim,
                    )
                    .expect("belt fits");
            }
            let sluice_at = TilePos::new(x + 7, ground - 2);
            let sluice = h
                .factory
                .place(kind("sluice"), sluice_at, 0, false, sim)
                .expect("sluice fits");
            let crate_at = TilePos::new(x + 10, ground - 2);
            let crate_id = h
                .factory
                .place(kind("crate"), crate_at, 0, false, sim)
                .expect("crate fits");
            h.factory
                .set_recipe(
                    stamp,
                    Some(
                        content
                            .factory
                            .recipe("crushed_malachite")
                            .expect("crush recipe"),
                    ),
                )
                .expect("known crush recipe");
            h.factory
                .set_recipe(
                    sluice,
                    Some(
                        content
                            .factory
                            .recipe("washed_malachite")
                            .expect("wash recipe"),
                    ),
                )
                .expect("known wash recipe");

            let raw = content.expect_material("raw_malachite");
            for cy in (ground - 10) * TILE_SIZE..(ground - 9) * TILE_SIZE {
                for cx in x * TILE_SIZE..(x + 1) * TILE_SIZE {
                    sim.set_cell(CellPos::new(cx, cy), raw, None);
                }
            }
            let water = content.expect_material("water");
            for cy in (ground - 6) * TILE_SIZE..(ground - 4) * TILE_SIZE {
                for cx in (x + 7) * TILE_SIZE..(x + 8) * TILE_SIZE {
                    sim.set_cell(CellPos::new(cx, cy), water, None);
                }
            }
            let stone = content.expect_material("stone");
            for cy in (ground + 1) * TILE_SIZE..(ground + 2) * TILE_SIZE {
                for cx in (x + 2) * TILE_SIZE..(x + 18) * TILE_SIZE {
                    sim.set_cell(CellPos::new(cx, cy), stone, None);
                }
            }
            let washed = mat("washed_malachite");
            for _ in 0..4800 {
                sim.tick();
                h.tick(sim);
                if h.factory
                    .buildings
                    .inventory(crate_id)
                    .is_some_and(|inv| inv.count(washed) >= 6)
                {
                    break;
                }
            }
            // The long autonomous run lets nearby generated terrain settle. Clear the final
            // view around the line while preserving the machines' material cells.
            for ty in ground - 12..=ground + 1 {
                for tx in x - 1..x + 15 {
                    let machine = (tx == x && ty == ground - 7)
                        || ((x..x + 2).contains(&tx) && (ground - 5..ground - 2).contains(&ty))
                        || ((x + 2..x + 7).contains(&tx) && ty == ground - 1)
                        || ((x + 7..x + 10).contains(&tx) && ty == ground - 2)
                        || ((x + 10..x + 12).contains(&tx) && (ground - 2..ground).contains(&ty));
                    if !machine {
                        for cy in ty * TILE_SIZE..(ty + 1) * TILE_SIZE {
                            for cx in tx * TILE_SIZE..(tx + 1) * TILE_SIZE {
                                sim.set_cell(
                                    CellPos::new(cx, cy),
                                    foundry_core::MaterialId::AIR,
                                    None,
                                );
                            }
                        }
                    }
                }
            }
            for cy in (ground + 1) * TILE_SIZE..(ground + 2) * TILE_SIZE {
                for cx in (x + 2) * TILE_SIZE..(x + 18) * TILE_SIZE {
                    sim.set_cell(CellPos::new(cx, cy), content.expect_material("stone"), None);
                }
            }
            h.apply(FactoryCommand::OpenAt(crate_at.origin()), sim);
            let _ = hopper;
        }
        UiState::Kiln | UiState::KilnHole => {
            // A kiln right of the robot: a room of 3 x 2 tiles with clay brick walls, the
            // controller in the left wall and a hatch in the roof. Charcoal burns in it and it
            // fires raw clay bricks.
            if let Some(at) = h.free_place(kind("clay_brick_wall"), sim) {
                let (x0, y0) = (at.x + 1, at.y - 3);
                let rect = foundry_core::CellRect::new(x0 * TILE_SIZE, y0 * TILE_SIZE, (x0 + 5) * TILE_SIZE, (y0 + 4) * TILE_SIZE);
                for y in rect.y0..rect.y1 {
                    for x in rect.x0..rect.x1 {
                        sim.set_cell(foundry_core::CellPos::new(x, y), foundry_core::MaterialId::AIR, None);
                    }
                }
                let t = |x: i32, y: i32| foundry_core::TilePos::new(x0 + x, y0 + y);
                let ctrl = h.factory.place(kind("kiln_controller"), t(0, 2), 0, false, sim).ok();
                let _ = h.factory.place(kind("kiln_hatch"), t(2, 0), 0, false, sim);
                for y in 0..4 {
                    for x in 0..5 {
                        let ring = x == 0 || x == 4 || y == 0 || y == 3;
                        let hole = state == UiState::KilnHole && (x, y) == (4, 1);
                        if ring && !hole {
                            let _ = h.factory.place(kind("clay_brick_wall"), t(x, y), 0, false, sim);
                        }
                    }
                }
                if let Some(id) = ctrl {
                    let _ = h.factory.set_recipe(id, content.factory.recipe("clay_brick"));
                    h.factory.buildings.insert(&content, id, ItemRef::Part(part("raw_clay_brick")), 16);
                    h.factory.buildings.insert(&content, id, mat("charcoal"), 400);
                    for _ in 0..8 * 60 {
                        sim.tick();
                        h.tick(sim);
                    }
                    h.apply(FactoryCommand::OpenAt(t(0, 2).origin().offset(3, 3)), sim);
                }
            }
            give(h, mat("charcoal"), 400);
        }
        UiState::SteamLine => {
            mouse = Some(steam_line::setup(h, sim, &content));
        }
        UiState::GuideWorkbench | UiState::GuideDone => {
            // The goals up to the workbench and the two ores are done.
            give(h, mat("sand"), 100);
            give(h, mat("clay"), 64);
            give(h, mat("wood"), 60);
            for id in ["malachite", "cassiterite"] {
                h.factory.scan(content.expect_material(id));
            }
            let place = |h: &mut FactoryHost, sim: &mut foundry_sim::Simulation, id: &str| {
                give(h, ItemRef::Part(part(id)), 1);
                h.apply(FactoryCommand::PickToCursor(part(id)), sim);
                h.apply(FactoryCommand::PlaceNear, sim);
                h.apply(FactoryCommand::ClearCursor, sim);
            };
            place(h, sim, "workbench");
            if state == UiState::GuideDone {
                // Every goal that the game can do is done: research, bricks, campfire, sluice,
                // kiln, charcoal.
                for tech in ["bronze", "research"] {
                    h.apply(FactoryCommand::StartResearch(content.factory.tech(tech).expect("tech")), sim);
                    for _ in 0..60 {
                        h.tick(sim);
                    }
                }
                give(h, ItemRef::Part(part("raw_clay_brick")), 8);
                give(h, ItemRef::Part(part("clay_brick")), 8);
                place(h, sim, "campfire");
                place(h, sim, "sluice");
                place(h, sim, "kiln_controller");
                give(h, mat("charcoal"), 32);
            }
            h.factory.update_guide();
            h.apply(FactoryCommand::Windows { research: false, guide: true }, sim);
        }
        _ => {}
    }
    // The input of the mouse (the ghost at the mouse, the remove button, the alt mode).
    n.take_frame(h.frame(sim, 0), Instant::now());
    let aim = mouse.unwrap_or_default();
    if let Some(c) = n.input(&content, aim, view) {
        run(h, sim, vec![c]);
    }
    // Tick 0 is a multiple of every view period, so all views are in this frame.
    let mut frame = h.frame(sim, 0);
    if frame.guide.is_none() {
        frame.guide = Some(h.factory.guide_view());
    }
    n.take_frame(frame.clone(), Instant::now());
    let build = n.build_view(&content, mouse);
    (frame, build)
}

/// The game UI, drawn with no window.
struct OffscreenUi {
    renderer: egui_wgpu::Renderer,
    ui: SandboxUi,
    /// The camera and the factory views, for the shapes over the world (normal mode).
    overlay: Option<(Camera, NormalView)>,
    /// The awake chunks view (`--view chunks`).
    debug_chunks: Option<(Camera, Vec<foundry_core::DebugChunk>)>,
    /// What the debug panel shows.
    panel: PanelState,
    ctx: egui::Context,
    size: (u32, u32),
    pixels_per_point: f32,
    jobs: Vec<egui::ClippedPrimitive>,
}

impl OffscreenUi {
    fn new(device: &wgpu::Device, ctx: egui::Context, ui: SandboxUi, size: (u32, u32)) -> Self {
        Self {
            renderer: egui_wgpu::Renderer::new(device, CAPTURE_FORMAT, egui_wgpu::RendererOptions::default()),
            ui,
            overlay: None,
            debug_chunks: None,
            panel: PanelState {
                view_keys: ["F4", "F5", "F6"].map(str::to_string),
                ..Default::default()
            },
            ctx,
            size,
            pixels_per_point: 1.0,
            jobs: Vec::new(),
        }
    }

    /// Run the UI several times: the fonts become active in the second frame, and windows
    /// measure their size in their first frame.
    fn layout(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, stats: &StatsView, show_debug: bool) {
        for _ in 0..6 {
            // The UI scale is the egui zoom factor, so the screen in points depends on it.
            let ppp = self.ctx.zoom_factor();
            let points = egui::vec2(self.size.0 as f32, self.size.1 as f32) / ppp;
            let mut raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, points)),
                ..Default::default()
            };
            raw.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(1.0);
            let ui = &mut self.ui;
            let overlay_data = &self.overlay;
            let (debug_chunks, panel) = (&self.debug_chunks, &self.panel);
            let mut out = self.ctx.run_ui(raw, |root| {
                if show_debug {
                    debug_panel::draw(root, stats, panel, &mut Vec::new());
                }
                if let Some((camera, chunks)) = debug_chunks {
                    let painter = root.ctx().layer_painter(egui::LayerId::background());
                    debug_panel::draw_overlay(&painter, chunks, camera, root.ctx().pixels_per_point());
                }
                let _ = ui.ui.show(root.ctx(), &ui.model);
                if let Some((camera, nv)) = overlay_data {
                    let painter = root.ctx().layer_painter(egui::LayerId::background());
                    let content = ui.model.content.clone();
                    let scene = overlay::Scene {
                        camera,
                        ppp: root.ctx().pixels_per_point(),
                        robot_at: None,
                        build: &nv.build,
                        content: &content,
                        atlas: ui.ui.atlas(),
                    };
                    overlay::draw(&painter, &nv.frame, &scene);
                }
            });
            for (id, deltas) in out.textures_delta.set.drain() {
                for delta in &deltas {
                    self.renderer.update_texture(device, queue, id, delta);
                }
            }
            for id in out.textures_delta.free.drain() {
                self.renderer.free_texture(&id);
            }
            self.pixels_per_point = out.pixels_per_point;
            self.jobs = self.ctx.tessellate(std::mem::take(&mut out.shapes), out.pixels_per_point);
        }
    }

    fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clear: Option<wgpu::Color>,
    ) {
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [self.size.0, self.size.1], pixels_per_point: self.pixels_per_point };
        // No paint callbacks are used, so there are no extra command buffers to send.
        let _ = ui::render_egui(&mut self.renderer, device, queue, encoder, target, &self.jobs, &screen, clear);
    }
}
