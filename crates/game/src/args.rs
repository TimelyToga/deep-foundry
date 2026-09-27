//! Command line options.

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
    --exit-after SECONDS     Quit after this time and print the average FPS and frame time
    --no-vsync               Do not wait for the display refresh (to measure the highest FPS)
    --size WxH               Window size in screen pixels (default 1600x900 points)
    --saves DIR              Folder for saved games (default: the app data folder of the system)
    --ui-state STATE         Start in this screen: menu, newgame, load, settings, pause, save,
                             playing, inventory or debug (default: menu; playing with --exit-after)
    --ui-scale S             Size of the UI, 0.75 to 2 (default 1)
    --smoke-test             Play a fixed list of UI actions in the window (new game, paint, pause,
                             save, load, quit to menu, continue, delete) and quit; exit code 1 on a
                             failure. Use it with --saves and an empty folder.
    --screenshot OUT.png     Render one image with no window, save it, and quit
      --ticks N              Ticks to run before the screenshot (default 0)
      --size WxH             Image size in pixels (default 1600x900)
      --zoom Z               Screen pixels per cell (default 2)
      --center X,Y           World cell at the image center (default: the middle of the demo area)
      --no-ui                Draw only the world, with no UI (default screen: playing)
    -h, --help               Show this text
";

#[derive(Debug, Clone, PartialEq)]
pub struct Args {
    pub seed: u64,
    /// Chunks below the surface level (for the world with no side limit).
    pub depth: i32,
    /// A finite world of this many chunks (width, height). `None`: no limit to the left and right.
    pub world: Option<(i32, i32)>,
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
    /// Folder for saves. `None`: the default folder.
    pub saves: Option<PathBuf>,
    /// The screen to start in. `None`: the default.
    pub ui_state: Option<UiState>,
    pub ui_scale: f32,
    /// Run the scripted check of the UI flow in the window.
    pub smoke_test: bool,
}

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
            _ => return None,
        })
    }

    /// True for the screens that show a world.
    pub fn has_world(self) -> bool {
        matches!(self, UiState::Pause | UiState::Save | UiState::Playing | UiState::Inventory | UiState::Debug)
    }
}

impl Default for Args {
    fn default() -> Self {
        Self {
            seed: 1,
            depth: foundry_sim::DEFAULT_DEPTH_CHUNKS,
            world: None,
            exit_after: None,
            no_vsync: false,
            screenshot: None,
            ticks: 0,
            size: None,
            zoom: None,
            center: None,
            no_ui: false,
            saves: None,
            ui_state: None,
            ui_scale: 1.0,
            smoke_test: false,
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

    /// The shape of the world to make.
    pub fn shape(&self) -> crate::demo::Shape {
        match self.world {
            Some((w, h)) => crate::demo::Shape::Box { width_chunks: w, height_chunks: h },
            None => crate::demo::Shape::Infinite { depth_chunks: self.depth },
        }
    }
}

/// What the program should do.
#[derive(Debug, Clone, PartialEq)]
pub enum Parsed {
    Run(Args),
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
                let (w, h) = pair::<i32>(&value("--world")?, 'x', "--world")?;
                if !(1..=256).contains(&w) || !(1..=256).contains(&h) {
                    return Err("--world: each size must be 1 to 256 chunks".into());
                }
                out.world = Some((w, h));
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
            "--saves" => out.saves = Some(PathBuf::from(value("--saves")?)),
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
            }
            "--center" => out.center = Some(pair::<f64>(&value("--center")?, ',', "--center")?),
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    Ok(Parsed::Run(out))
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
            Parsed::Run(a) => Ok(a),
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
    fn world_shape() {
        assert_eq!(run(&[]).unwrap().shape(), crate::demo::Shape::Infinite { depth_chunks: 128 });
        assert_eq!(run(&["--depth", "40"]).unwrap().shape(), crate::demo::Shape::Infinite { depth_chunks: 40 });
        let b = run(&["--world", "8x4"]).unwrap().shape();
        assert_eq!(b, crate::demo::Shape::Box { width_chunks: 8, height_chunks: 4 });
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
        assert_eq!(parse(["--help".to_string()]), Ok(Parsed::Help));
    }
}
