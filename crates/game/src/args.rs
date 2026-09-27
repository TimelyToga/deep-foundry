//! Command line options.

use std::path::PathBuf;

pub const USAGE: &str = "\
Deep Foundry

USAGE:
    deep-foundry [OPTIONS]

OPTIONS:
    --seed N                 World seed (default 1)
    --world WxH              World size in chunks of 64 x 64 cells (default 32x16)
    --exit-after SECONDS     Quit after this time and print the average FPS and frame time
    --no-vsync               Do not wait for the display refresh (to measure the highest FPS)
    --size WxH               Window size in screen pixels (default 1600x900 points)
    --screenshot OUT.png     Render one image with no window, save it, and quit
      --ticks N              Ticks to run before the screenshot (default 0)
      --size WxH             Image size in pixels (default 1600x900)
      --zoom Z               Screen pixels per cell (default 2)
      --center X,Y           World cell at the image center (default: the middle of the demo area)
      --ui                   Also draw the side panel
    -h, --help               Show this text
";

#[derive(Debug, Clone, PartialEq)]
pub struct Args {
    pub seed: u64,
    /// World size in chunks.
    pub world: (i32, i32),
    pub exit_after: Option<f64>,
    pub no_vsync: bool,
    pub screenshot: Option<PathBuf>,
    pub ticks: u32,
    /// Image size for screenshots, or window size in pixels. `None`: the default.
    pub size: Option<(u32, u32)>,
    pub zoom: Option<f32>,
    pub center: Option<(f64, f64)>,
    /// Draw the egui panel in the screenshot.
    pub ui: bool,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            seed: 1,
            world: (32, 16),
            exit_after: None,
            no_vsync: false,
            screenshot: None,
            ticks: 0,
            size: None,
            zoom: None,
            center: None,
            ui: false,
        }
    }
}

impl Args {
    /// Size of a screenshot image.
    pub fn image_size(&self) -> (u32, u32) {
        self.size.unwrap_or((1600, 900))
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
            "--world" => {
                let (w, h) = pair::<i32>(&value("--world")?, 'x', "--world")?;
                if !(1..=256).contains(&w) || !(1..=256).contains(&h) {
                    return Err("--world: each size must be 1 to 256 chunks".into());
                }
                out.world = (w, h);
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
            "--ui" => out.ui = true,
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
    fn errors() {
        assert!(run(&["--world", "0x4"]).is_err());
        assert!(run(&["--size", "800"]).is_err());
        assert!(run(&["--bogus"]).is_err());
        assert!(run(&["--seed"]).is_err());
        assert_eq!(parse(["--help".to_string()]), Ok(Parsed::Help));
    }
}
