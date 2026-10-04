//! Command line options.

use foundry_ui::GameMode;
use std::path::PathBuf;

pub const USAGE: &str = "\
TimTech

USAGE:
    timtech [OPTIONS]

OPTIONS:
    --seed N                 World seed (default 1)
    --depth N                Depth of the world below the surface, in chunks of 64 x 64 cells
                             (default 128). The world has no limit to the left and right.
    --world WxH              A finite world of W x H chunks with bedrock walls, in place of the
                             world with no side limit (for tests)
    --world gen              A world made by the world generator: surface biomes, trees, lakes,
                             caves and ores (the default)
    --world demo             The small demo world (a test world) in place of the generated world
    --exit-after SECONDS     Quit after this time and print the average FPS and frame time
    --no-vsync               Do not wait for the display refresh (to measure the highest FPS)
    --size WxH               Window size in screen pixels (default 1600x900 points)
    --saves DIR              Folder for saved games (default: the app data folder of the system)
    --ui-state STATE         Start in this screen: menu, newgame, load, settings, pause, save,
                             playing, inventory or debug (default: menu; playing with --exit-after).
                             Normal mode only: building (a workbench window), ghost (a building in
                             the hand), research, guide, hub (the Hub window),
                             ghost-red, drag (a belt line), alt (alt mode), remove,
                             tanks (full tanks: the tank HUD says what to do),
                             guide-workbench (the guide after the workbench and the ores),
                             guide-done (the guide when the rest waits for new machines),
                             campfire (a campfire window: raw bricks, wood fuel, clay bricks),
                             steam-line (a functioning boiler, bronze pipes and steam crusher),
                             smelter (the Tier 0 crucible site), automation (a Tier 1 line with
                             a steam drill, arms, a steam furnace, a mold and an assembler),
                             cave (the robot and lamps in the cave under the start area; give
                             --center 300,1354 with the generated world)
    --mode MODE              The mode of a world that --ui-state starts: sandbox (default) or
                             normal (the robot, the factory and the Hub)
    --ui-scale S             Size of the UI, 0.75 to 2 (default: the settings file, else 1)
    --settings FILE          The settings file (default: settings.ron in the app data folder)
    --smoke-test             Play a fixed list of UI actions in the window (new game, paint, pause,
                             save, load, quit to menu, continue, delete) and quit; exit code 1 on a
                             failure. Use it with --saves and an empty folder. It uses the demo world
                             unless --world is given.
    --screenshot OUT.png     Render one image with no window, save it, and quit
      --ticks N              Ticks to run before the screenshot (default 0)
      --size WxH             Image size in pixels (default 1600x900)
      --zoom Z               Screen pixels per cell (default 2)
      --center X,Y           World cell at the image center (default: the middle of the demo area)
      --no-ui                Draw only the world, with no UI (default screen: playing)
      --walk N               Normal mode: the robot walks N ticks to the right first (N < 0: left)
      --pose NAME[:FRAME]    Normal mode: the robot does this for the picture: idle, walk, jump,
                             fall, land, fly, wade (an animation; FRAME picks one frame), or
                             dig, spray, scan (the tool points at a place in front of the robot)
      --face left|right      Normal mode: the direction the robot looks in the picture
      --robot X              Normal mode: put the robot on the ground (or in the water) at column X
      --record OUT.mp4       Record a video in place of one picture (needs ffmpeg). It runs one tick
                             per frame at 60 frames per second, after the setup of --screenshot.
        --seconds S          Length of the video (default 4)
        --speed N            Ticks per frame (default 1)
        --pan DX,DY          The camera moves this many cells per second
        --zoom-to Z          The zoom at the end of the video (it changes smoothly from --zoom)
        --follow             The camera follows the robot (normal mode)
        --no-hud             Draw the world and the shapes on the buildings, but no windows or HUD
        --pour MAT,DX,DY[,R[,FROM[,TO]]]
                             Pour this material (a circle of radius R, default 1) every tick into the
                             air cells at DX,DY from the start center, from FROM to TO seconds.
                             Give it again for more streams.
        --boom DX,DY,T[,STRENGTH[,HEAT]]
                             An explosion at DX,DY from the start center at T seconds (strength
                             default 120, heat default 0 °C). Give it again for more.
        --drive LIST         Normal mode: the robot input, as steps KEYS:SECONDS, comma separated.
                             KEYS is idle, or keys joined with +: left, right, jump (also flies with
                             the jetpack), dig/DX/DY (dig at DX,DY from the robot center).
                             Example: right:1,right+jump:0.5,dig/12/6:2
      --view LIST            Render views, comma separated: nolight, nobloom, noshimmer, heat
                             (heat map), grid (chunk grid), light (only the light map), chunks
                             (awake chunks)
    -h, --help               Show this text
";

#[derive(Debug, Clone, PartialEq)]
pub struct Args {
    pub seed: u64,
    /// Chunks below the surface level (for the world with no side limit).
    pub depth: i32,
    /// A finite world of this many chunks (width, height). `None`: no limit to the left and right.
    pub world: Option<(i32, i32)>,
    /// New worlds come from the world generator (the default, `--world gen`), else from the demo
    /// source (`--world demo`).
    pub generated: bool,
    pub exit_after: Option<f64>,
    pub no_vsync: bool,
    pub screenshot: Option<PathBuf>,
    pub ticks: u32,
    /// Image size for screenshots, or window size in pixels. `None`: the default.
    pub size: Option<(u32, u32)>,
    pub zoom: Option<f32>,
    pub center: Option<(f64, f64)>,
    /// Screenshots: draw only the world.
    pub no_ui: bool,
    /// Screenshots: render views (`--view`), see `render_setup::VIEW_NAMES`.
    pub view: Vec<String>,
    /// Folder for saves. `None`: the default folder.
    pub saves: Option<PathBuf>,
    /// The screen to start in. `None`: the default.
    pub ui_state: Option<UiState>,
    pub ui_scale: f32,
    /// `--ui-scale` was given (it wins over the settings file).
    pub ui_scale_set: bool,
    /// The settings file. `None`: `settings.ron` in the app data folder.
    pub settings: Option<PathBuf>,
    /// Run the scripted check of the UI flow in the window.
    pub smoke_test: bool,
    /// The mode of the world that `--ui-state` starts.
    pub mode: GameMode,
    /// Screenshots of the normal mode: ticks the robot walks first (negative: to the left).
    pub walk: i32,
    /// Screenshots of the normal mode: what the robot does (`POSES`), and a fixed frame.
    pub pose: Option<(String, Option<usize>)>,
    /// Screenshots of the normal mode: the robot looks left (`Some(true)`) or right.
    pub face_left: Option<bool>,
    /// Screenshots of the normal mode: put the robot at this column first.
    pub robot_x: Option<i32>,
    /// `--record`: write a video to this file in place of one picture.
    pub record: Option<PathBuf>,
    /// The options of `--record`.
    pub rec: Record,
}

/// The options of `--record`.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub seconds: f64,
    /// Ticks per frame.
    pub speed: u32,
    /// Camera movement in cells per second.
    pub pan: (f64, f64),
    /// The zoom at the end. `None`: the zoom does not change.
    pub zoom_to: Option<f32>,
    /// The camera follows the robot.
    pub follow: bool,
    /// Draw no windows and no HUD (only the shapes on the buildings).
    pub no_hud: bool,
    pub pours: Vec<Pour>,
    pub booms: Vec<Boom>,
    pub drive: Vec<DriveStep>,
}

impl Default for Record {
    fn default() -> Self {
        Self {
            seconds: 4.0,
            speed: 1,
            pan: (0.0, 0.0),
            zoom_to: None,
            follow: false,
            no_hud: false,
            pours: Vec::new(),
            booms: Vec::new(),
            drive: Vec::new(),
        }
    }
}

/// `--pour`: a stream of material, painted into the air every tick.
#[derive(Debug, Clone, PartialEq)]
pub struct Pour {
    /// The material id (as in the data files).
    pub material: String,
    /// Cells from the start center.
    pub at: (i32, i32),
    pub radius: u16,
    /// Start and end time in seconds.
    pub from: f64,
    pub to: f64,
}

/// `--boom`: an explosion.
#[derive(Debug, Clone, PartialEq)]
pub struct Boom {
    /// Cells from the start center.
    pub at: (i32, i32),
    /// Time in seconds.
    pub time: f64,
    pub strength: f32,
    pub heat: i16,
}

/// One step of `--drive`: the keys that are down for some seconds.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DriveStep {
    /// -1: left, 1: right.
    pub x: i8,
    pub jump: bool,
    /// Dig at this place, in cells from the robot center.
    pub dig: Option<(i32, i32)>,
    pub seconds: f64,
}

/// The names that `--pose` knows: the robot animations, then the tools.
pub const POSES: [&str; 10] = ["idle", "walk", "jump", "fall", "land", "fly", "wade", "dig", "spray", "scan"];

/// The screens that `--ui-state` can start in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiState {
    Menu,
    NewGame,
    Load,
    Settings,
    Pause,
    Save,
    Playing,
    Inventory,
    Debug,
    /// Normal mode: a workbench is placed and its window is open.
    Building,
    /// Normal mode: a building is in the hand and its ghost is at the mouse.
    Ghost,
    /// Normal mode: the research window.
    Research,
    /// Normal mode: the guide window.
    Guide,
    /// Normal mode: the Hub window.
    Hub,
    /// Normal mode: a ghost that cannot be placed, with the reason.
    GhostRed,
    /// Normal mode: a line of belts is dragged.
    Drag,
    /// Normal mode: the alt mode with recipe icons over machines.
    Alt,
    /// Normal mode: the remove button takes a building.
    Remove,
    /// Normal mode: the tanks are full and the dig tool has no room.
    Tanks,
    /// Normal mode: the guide window after the first goals (up to the workbench and the ores).
    GuideWorkbench,
    /// Normal mode: the guide window when every goal that the game can do is done.
    GuideDone,
    /// Normal mode: a campfire that fires raw clay bricks, with its window open.
    Campfire,
    /// Normal mode: a live hopper, stamp mill, belt, sluice and crate line.
    OreLine,
    /// Normal mode: a kiln room that fires clay bricks with charcoal, with its window open.
    Kiln,
    /// Normal mode: the same kiln with a hole in its right wall (red marks).
    KilnHole,
    /// Normal mode: a boiler makes steam through bronze pipes and runs a crusher.
    SteamLine,
    /// Normal mode: the Tier 0 smelting site (campfire, crucible, bellows, mold, crate).
    Smelter,
    /// Normal mode: a Tier 1 automated line (steam drill, arms, steam furnace, mold, assembler).
    Automation,
    /// Normal mode: the robot in the cave under the start area, with lamps (use --center).
    Cave,
}

impl UiState {
    fn parse(s: &str) -> Option<UiState> {
        Some(match s {
            "menu" => UiState::Menu,
            "newgame" => UiState::NewGame,
            "load" => UiState::Load,
            "settings" => UiState::Settings,
            "pause" => UiState::Pause,
            "save" => UiState::Save,
            "playing" => UiState::Playing,
            "inventory" => UiState::Inventory,
            "debug" => UiState::Debug,
            "building" => UiState::Building,
            "ghost" => UiState::Ghost,
            "research" => UiState::Research,
            "guide" => UiState::Guide,
            "hub" => UiState::Hub,
            "ghost-red" => UiState::GhostRed,
            "drag" => UiState::Drag,
            "alt" => UiState::Alt,
            "remove" => UiState::Remove,
            "tanks" => UiState::Tanks,
            "guide-workbench" => UiState::GuideWorkbench,
            "guide-done" => UiState::GuideDone,
            "campfire" => UiState::Campfire,
            "ore-line" => UiState::OreLine,
            "kiln" => UiState::Kiln,
            "kiln-hole" => UiState::KilnHole,
            "steam-line" => UiState::SteamLine,
            "smelter" => UiState::Smelter,
            "automation" => UiState::Automation,
            "cave" => UiState::Cave,
            _ => return None,
        })
    }

    /// True for the screens that show a world.
    pub fn has_world(self) -> bool {
        !matches!(self, UiState::Menu | UiState::NewGame | UiState::Load | UiState::Settings)
    }

    /// True for the screens that exist only in the normal mode.
    pub fn needs_normal(self) -> bool {
        matches!(
            self,
            UiState::Building
                | UiState::Ghost
                | UiState::Research
                | UiState::Guide
                | UiState::Hub
                | UiState::GhostRed
                | UiState::Drag
                | UiState::Alt
                | UiState::Remove
                | UiState::Tanks
                | UiState::GuideWorkbench
                | UiState::GuideDone
                | UiState::Campfire
                | UiState::OreLine
                | UiState::Kiln
                | UiState::KilnHole
                | UiState::SteamLine
                | UiState::Smelter
                | UiState::Automation
                | UiState::Cave
        )
    }
}

impl Default for Args {
    fn default() -> Self {
        Self {
            seed: 1,
            depth: foundry_sim::DEFAULT_DEPTH_CHUNKS,
            world: None,
            generated: true,
            exit_after: None,
            no_vsync: false,
            screenshot: None,
            ticks: 0,
            size: None,
            zoom: None,
            center: None,
            no_ui: false,
            view: Vec::new(),
            saves: None,
            ui_state: None,
            ui_scale: 1.0,
            ui_scale_set: false,
            settings: None,
            smoke_test: false,
            mode: GameMode::Sandbox,
            walk: 0,
            pose: None,
            face_left: None,
            robot_x: None,
            record: None,
            rec: Record::default(),
        }
    }
}

impl Args {
    /// Size of a screenshot image.
    pub fn image_size(&self) -> (u32, u32) {
        self.size.unwrap_or((1600, 900))
    }

    /// The screen to start in: `--ui-state`, else the game for `--exit-after` (to measure the
    /// frame rate of the world), else the main menu.
    pub fn start_state(&self) -> UiState {
        self.ui_state.unwrap_or(if self.exit_after.is_some() { UiState::Playing } else { UiState::Menu })
    }

    /// The mode of a world that `--ui-state` starts. The normal-mode screens need the normal mode.
    pub fn start_mode(&self) -> GameMode {
        if self.ui_state.is_some_and(UiState::needs_normal) { GameMode::Normal } else { self.mode }
    }

    /// The shape of the world to make.
    pub fn shape(&self) -> crate::demo::Shape {
        match self.world {
            Some((w, h)) => crate::demo::Shape::Box { width_chunks: w, height_chunks: h },
            None if self.generated => crate::demo::Shape::Generated { depth_chunks: self.depth },
            None => crate::demo::Shape::Infinite { depth_chunks: self.depth },
        }
    }
}

/// What the program should do.
#[derive(Debug, Clone, PartialEq)]
pub enum Parsed {
    Run(Box<Args>),
    Help,
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Parsed, String> {
    let mut out = Args::default();
    let mut world_given = false;
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "-h" | "--help" => return Ok(Parsed::Help),
            "--seed" => out.seed = number(&value("--seed")?, "--seed")?,
            "--depth" => {
                let d: i32 = number(&value("--depth")?, "--depth")?;
                if !(1..=1024).contains(&d) {
                    return Err("--depth must be 1 to 1024 chunks".into());
                }
                out.depth = d;
            }
            "--world" => {
                world_given = true;
                let v = value("--world")?;
                if v == "gen" || v == "demo" {
                    out.generated = v == "gen";
                    out.world = None;
                    continue;
                }
                let (w, h) = pair::<i32>(&v, 'x', "--world")?;
                if !(1..=256).contains(&w) || !(1..=256).contains(&h) {
                    return Err("--world: each size must be 1 to 256 chunks".into());
                }
                out.world = Some((w, h));
                out.generated = false;
            }
            "--exit-after" => {
                let s: f64 = number(&value("--exit-after")?, "--exit-after")?;
                if s <= 0.0 {
                    return Err("--exit-after must be above 0".into());
                }
                out.exit_after = Some(s);
            }
            "--no-vsync" => out.no_vsync = true,
            "--screenshot" => out.screenshot = Some(PathBuf::from(value("--screenshot")?)),
            "--ticks" => out.ticks = number(&value("--ticks")?, "--ticks")?,
            "--size" => {
                let (w, h) = pair::<u32>(&value("--size")?, 'x', "--size")?;
                if !(1..=16384).contains(&w) || !(1..=16384).contains(&h) {
                    return Err("--size: each size must be 1 to 16384 pixels".into());
                }
                out.size = Some((w, h));
            }
            "--zoom" => {
                let z: f32 = number(&value("--zoom")?, "--zoom")?;
                if !(0.25..=64.0).contains(&z) {
                    return Err("--zoom must be 0.25 to 64".into());
                }
                out.zoom = Some(z);
            }
            "--no-ui" => out.no_ui = true,
            "--smoke-test" => out.smoke_test = true,
            "--mode" => {
                out.mode = match value("--mode")?.as_str() {
                    "normal" => GameMode::Normal,
                    "sandbox" => GameMode::Sandbox,
                    other => return Err(format!("--mode: unknown mode `{other}` (normal or sandbox)")),
                }
            }
            "--walk" => out.walk = number(&value("--walk")?, "--walk")?,
            "--pose" => {
                let v = value("--pose")?;
                let (name, frame) = match v.split_once(':') {
                    Some((n, f)) => (n.to_string(), Some(number::<usize>(f, "--pose")?)),
                    None => (v.clone(), None),
                };
                if !POSES.contains(&name.as_str()) {
                    return Err(format!("--pose: unknown pose `{name}` ({})", POSES.join(", ")));
                }
                out.pose = Some((name, frame));
                out.mode = GameMode::Normal;
            }
            "--robot" => {
                out.robot_x = Some(number(&value("--robot")?, "--robot")?);
                out.mode = GameMode::Normal;
            }
            "--face" => {
                out.face_left = Some(match value("--face")?.as_str() {
                    "left" => true,
                    "right" => false,
                    other => return Err(format!("--face: `{other}` is not left or right")),
                })
            }
            "--view" => {
                let names: Vec<String> = value("--view")?.split(',').map(|v| v.trim().to_string()).collect();
                let (mut s, mut v) = Default::default();
                crate::render_setup::apply_view_names(&names, &mut s, &mut v)?;
                out.view = names;
            }
            "--saves" => out.saves = Some(PathBuf::from(value("--saves")?)),
            "--settings" => out.settings = Some(PathBuf::from(value("--settings")?)),
            "--ui-state" => {
                let v = value("--ui-state")?;
                out.ui_state = Some(UiState::parse(&v).ok_or_else(|| format!("--ui-state: unknown screen `{v}`"))?);
            }
            "--ui-scale" => {
                let s: f32 = number(&value("--ui-scale")?, "--ui-scale")?;
                if !(0.75..=2.0).contains(&s) {
                    return Err("--ui-scale must be 0.75 to 2".into());
                }
                out.ui_scale = s;
                out.ui_scale_set = true;
            }
            "--center" => out.center = Some(pair::<f64>(&value("--center")?, ',', "--center")?),
            "--record" => out.record = Some(PathBuf::from(value("--record")?)),
            "--seconds" => {
                let s: f64 = number(&value("--seconds")?, "--seconds")?;
                if !(s > 0.0 && s <= 600.0) {
                    return Err("--seconds must be above 0 and at most 600".into());
                }
                out.rec.seconds = s;
            }
            "--speed" => {
                let n: u32 = number(&value("--speed")?, "--speed")?;
                if !(1..=100).contains(&n) {
                    return Err("--speed must be 1 to 100 ticks".into());
                }
                out.rec.speed = n;
            }
            "--pan" => out.rec.pan = pair::<f64>(&value("--pan")?, ',', "--pan")?,
            "--zoom-to" => {
                let z: f32 = number(&value("--zoom-to")?, "--zoom-to")?;
                if !(0.25..=64.0).contains(&z) {
                    return Err("--zoom-to must be 0.25 to 64".into());
                }
                out.rec.zoom_to = Some(z);
            }
            "--follow" => {
                out.rec.follow = true;
                out.mode = GameMode::Normal;
            }
            "--no-hud" => out.rec.no_hud = true,
            "--pour" => out.rec.pours.push(pour(&value("--pour")?)?),
            "--boom" => out.rec.booms.push(boom(&value("--boom")?)?),
            "--drive" => {
                out.rec.drive = drive(&value("--drive")?)?;
                out.mode = GameMode::Normal;
            }
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    // The smoke test plays a fixed script in the demo world (clay next to the start).
    if out.smoke_test && !world_given {
        out.generated = false;
    }
    Ok(Parsed::Run(Box::new(out)))
}

fn number<T: std::str::FromStr>(s: &str, name: &str) -> Result<T, String> {
    s.trim().parse().map_err(|_| format!("{name}: `{s}` is not a valid number"))
}

/// `--pour MAT,DX,DY[,R[,FROM[,TO]]]`.
fn pour(v: &str) -> Result<Pour, String> {
    let parts: Vec<&str> = v.split(',').map(str::trim).collect();
    if !(3..=6).contains(&parts.len()) || parts[0].is_empty() {
        return Err("--pour: expected MAT,DX,DY[,R[,FROM[,TO]]]".into());
    }
    let n = |i: usize| number::<f64>(parts[i], "--pour");
    Ok(Pour {
        material: parts[0].to_string(),
        at: (number(parts[1], "--pour")?, number(parts[2], "--pour")?),
        radius: if parts.len() > 3 { number(parts[3], "--pour")? } else { 1 },
        from: if parts.len() > 4 { n(4)? } else { 0.0 },
        to: if parts.len() > 5 { n(5)? } else { f64::INFINITY },
    })
}

/// `--boom DX,DY,T[,STRENGTH[,HEAT]]`.
fn boom(v: &str) -> Result<Boom, String> {
    let parts: Vec<&str> = v.split(',').map(str::trim).collect();
    if !(3..=5).contains(&parts.len()) {
        return Err("--boom: expected DX,DY,T[,STRENGTH[,HEAT]]".into());
    }
    Ok(Boom {
        at: (number(parts[0], "--boom")?, number(parts[1], "--boom")?),
        time: number(parts[2], "--boom")?,
        strength: if parts.len() > 3 { number(parts[3], "--boom")? } else { 120.0 },
        heat: if parts.len() > 4 { number(parts[4], "--boom")? } else { 0 },
    })
}

/// `--drive KEYS:SECONDS,...`.
fn drive(v: &str) -> Result<Vec<DriveStep>, String> {
    let mut steps = Vec::new();
    for step in v.split(',').map(str::trim) {
        let (keys, secs) = step.split_once(':').ok_or_else(|| format!("--drive: `{step}` needs :SECONDS"))?;
        let mut s = DriveStep { seconds: number(secs, "--drive")?, ..Default::default() };
        for key in keys.split('+') {
            match key {
                "idle" => {}
                "left" => s.x = -1,
                "right" => s.x = 1,
                "jump" => s.jump = true,
                k if k.starts_with("dig/") => {
                    let (dx, dy) = pair::<i32>(&k[4..], '/', "--drive dig")?;
                    s.dig = Some((dx, dy));
                }
                other => return Err(format!("--drive: unknown key `{other}` (idle, left, right, jump, dig/DX/DY)")),
            }
        }
        steps.push(s);
    }
    Ok(steps)
}

fn pair<T: std::str::FromStr>(s: &str, sep: char, name: &str) -> Result<(T, T), String> {
    let (a, b) = s.split_once(sep).ok_or_else(|| format!("{name}: expected two numbers like `4{sep}3`"))?;
    Ok((number(a, name)?, number(b, name)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(list: &[&str]) -> Result<Args, String> {
        match parse(list.iter().map(|s| s.to_string()))? {
            Parsed::Run(a) => Ok(*a),
            Parsed::Help => Err("help".into()),
        }
    }

    #[test]
    fn defaults() {
        assert_eq!(run(&[]).unwrap(), Args::default());
    }

    #[test]
    fn screenshot_options() {
        let a = run(&[
            "--screenshot",
            "out/a.png",
            "--ticks",
            "60",
            "--size",
            "800x600",
            "--zoom",
            "3",
            "--center",
            "100.5,-20",
        ])
        .unwrap();
        assert_eq!(a.screenshot, Some(PathBuf::from("out/a.png")));
        assert_eq!(a.ticks, 60);
        assert_eq!(a.size, Some((800, 600)));
        assert_eq!(a.zoom, Some(3.0));
        assert_eq!(a.center, Some((100.5, -20.0)));
    }

    #[test]
    fn ui_options() {
        let a = run(&["--ui-state", "inventory", "--ui-scale", "1.25", "--saves", "s", "--no-ui"]).unwrap();
        assert_eq!(a.ui_state, Some(UiState::Inventory));
        assert_eq!(a.ui_scale, 1.25);
        assert_eq!(a.saves, Some(PathBuf::from("s")));
        assert!(a.no_ui);
        assert_eq!(run(&[]).unwrap().start_state(), UiState::Menu);
        assert_eq!(run(&["--exit-after", "5"]).unwrap().start_state(), UiState::Playing);
        let steam = run(&["--ui-state", "steam-line"]).unwrap();
        assert_eq!(steam.ui_state, Some(UiState::SteamLine));
        assert_eq!(steam.start_mode(), GameMode::Normal);
    }

    #[test]
    fn pose_options() {
        let a = run(&["--pose", "walk:3", "--face", "left"]).unwrap();
        assert_eq!(a.pose, Some(("walk".to_string(), Some(3))));
        assert_eq!(a.face_left, Some(true));
        assert_eq!(a.mode, GameMode::Normal, "a pose needs the robot");
        assert_eq!(run(&["--pose", "dig"]).unwrap().pose, Some(("dig".to_string(), None)));
    }

    #[test]
    fn world_shape() {
        assert_eq!(run(&[]).unwrap().shape(), crate::demo::Shape::Generated { depth_chunks: 128 });
        assert_eq!(run(&["--world", "demo", "--depth", "40"]).unwrap().shape(), crate::demo::Shape::Infinite { depth_chunks: 40 });
        let b = run(&["--world", "8x4"]).unwrap().shape();
        assert_eq!(b, crate::demo::Shape::Box { width_chunks: 8, height_chunks: 4 });
        let g = run(&["--world", "gen", "--depth", "40"]).unwrap().shape();
        assert_eq!(g, crate::demo::Shape::Generated { depth_chunks: 40 });
        assert!(run(&["--world", "generated"]).is_err());
    }

    #[test]
    fn errors() {
        assert!(run(&["--world", "0x4"]).is_err());
        assert!(run(&["--depth", "0"]).is_err());
        assert!(run(&["--size", "800"]).is_err());
        assert!(run(&["--bogus"]).is_err());
        assert!(run(&["--seed"]).is_err());
        assert!(run(&["--ui-state", "bogus"]).is_err());
        assert!(run(&["--ui-scale", "3"]).is_err());
        assert!(run(&["--pose", "dance"]).is_err());
        assert!(run(&["--face", "up"]).is_err());
        assert_eq!(parse(["--help".to_string()]), Ok(Parsed::Help));
    }
}
