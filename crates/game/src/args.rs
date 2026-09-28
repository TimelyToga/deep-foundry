//! Command line options.

use foundry_ui::GameMode;
use std::path::PathBuf;

pub const USAGE: &str = "\
Deep Foundry

USAGE:
    deep-foundry [OPTIONS]

OPTIONS:
    --seed N                 World seed (default 1)
    --depth N                Depth of the world below the surface, in chunks of 64 x 64 cells
                             (default 128). The world has no limit to the left and right.
    --world WxH              A finite world of W x H chunks with bedrock walls, in place of the
                             world with no side limit (for tests)
    --world gen              A world made by the world generator (surface biomes, caves, ores)
                             in place of the demo world. New games and screenshots use it.
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
                             campfire (a campfire window: raw bricks, wood fuel, clay bricks)
    --mode MODE              The mode of a world that --ui-state starts: sandbox (default) or
                             normal (the robot, the factory and the Hub)
    --ui-scale S             Size of the UI, 0.75 to 2 (default: the settings file, else 1)
    --settings FILE          The settings file (default: settings.ron in the app data folder)
    --smoke-test             Play a fixed list of UI actions in the window (new game, paint, pause,
                             save, load, quit to menu, continue, delete) and quit; exit code 1 on a
                             failure. Use it with --saves and an empty folder.
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
    /// `--world gen`: new worlds come from the world generator, not the demo source.
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
        )
    }
}

impl Default for Args {
    fn default() -> Self {
        Self {
            seed: 1,
            depth: foundry_sim::DEFAULT_DEPTH_CHUNKS,
            world: None,
            generated: false,
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
                let v = value("--world")?;
                if v == "gen" {
                    out.generated = true;
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
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    Ok(Parsed::Run(Box::new(out)))
}

fn number<T: std::str::FromStr>(s: &str, name: &str) -> Result<T, String> {
    s.trim().parse().map_err(|_| format!("{name}: `{s}` is not a valid number"))
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
        assert_eq!(run(&[]).unwrap().shape(), crate::demo::Shape::Infinite { depth_chunks: 128 });
        assert_eq!(run(&["--depth", "40"]).unwrap().shape(), crate::demo::Shape::Infinite { depth_chunks: 40 });
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
