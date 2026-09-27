//! Scenes: a PNG image plus a RON file with the same name.
//!
//! Each pixel color is a material (the legend in the RON file says which).
//! Transparent pixels and pure black pixels (#000000) are air.
//! The loader uses only the public `Simulation` API: `Simulation::new` and `set_cell`.

use crate::check::Check;
use crate::image::Image;
use crate::paths;
use anyhow::{Context, Result, anyhow, bail};
use foundry_content::Content;
use foundry_core::{CHUNK_SIZE, CellPos, MaterialId};
use foundry_sim::{SimConfig, Simulation};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A pixel position in the scene image. In RON: `(x: 5, y: 10)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

/// A rectangle of image pixels: top-left corner, width and height. In RON: `(x: 0, y: 40, w: 96, h: 24)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl fmt::Display for Rect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "(x {}, y {}, w {}, h {})", self.x, self.y, self.w, self.h)
    }
}

/// The world size of a scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub enum WorldSize {
    /// The smallest world (in whole chunks) that holds the image plus `margin` cells on each side.
    #[default]
    Fit,
    /// A world of this many chunks (width, height).
    Chunks(i32, i32),
}

/// One legend entry: a material id, or a material id with a start temperature.
/// In RON: `"sand"` or `(material: "lava", temperature: 1500)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegendEntry {
    pub material: String,
    pub temperature: Option<i16>,
}

impl<'de> Deserialize<'de> for LegendEntry {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Full {
            material: String,
            #[serde(default)]
            temperature: Option<i16>,
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Either {
            Name(String),
            Full(Full),
        }
        match Either::deserialize(d)? {
            Either::Name(material) => Ok(LegendEntry { material, temperature: None }),
            Either::Full(f) => Ok(LegendEntry { material: f.material, temperature: f.temperature }),
        }
    }
}

/// The RON file of a scene. The RON name is `Scene(...)`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename = "Scene", deny_unknown_fields)]
pub struct SceneDef {
    /// What the scene shows or tests. Free text.
    #[serde(default)]
    pub note: String,
    /// Pixel color "#rrggbb" to material.
    pub legend: BTreeMap<String, LegendEntry>,
    #[serde(default)]
    pub size: WorldSize,
    /// Cells of air around the image when `size` is `Fit`.
    #[serde(default = "default_margin")]
    pub margin: i32,
    /// World position of the top-left image pixel. Default: the image is in the center of the world.
    #[serde(default)]
    pub at: Option<Point>,
    /// Ticks to run before the checks.
    pub ticks: u32,
    #[serde(default = "default_seed")]
    pub seed: u64,
    /// Bedrock on the left, right and bottom edges of the world (2 cells thick).
    #[serde(default = "default_true")]
    pub bedrock_border: bool,
    #[serde(default)]
    pub checks: Vec<Check>,
}

fn default_margin() -> i32 {
    16
}

fn default_seed() -> u64 {
    1
}

fn default_true() -> bool {
    true
}

/// Read RON text with the options that scene and baseline files use.
/// `implicit_some` lets files write `min: 5` in place of `min: Some(5)`.
pub fn ron_options() -> ron::Options {
    ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
}

/// A loaded scene. It can make any number of equal simulations.
#[derive(Debug, Clone)]
pub struct Scene {
    /// The file name without extension.
    pub name: String,
    pub ron_path: PathBuf,
    pub png_path: PathBuf,
    pub def: SceneDef,
    /// Image size in pixels.
    pub width: i32,
    pub height: i32,
    /// One entry for each pixel, row by row. `None` is air at the default temperature (nothing to write).
    pub cells: Vec<Option<(MaterialId, Option<i16>)>>,
    /// World size in chunks.
    pub world_chunks: (i32, i32),
    /// World position of image pixel (0, 0).
    pub origin: CellPos,
}

impl Scene {
    /// Load a scene by name or path. See `paths::find_scene`.
    pub fn find(name: &str, content: &Content) -> Result<Scene> {
        Scene::load(&paths::find_scene(name)?, content)
    }

    /// Load `<name>.ron` and `<name>.png` from the same folder.
    pub fn load(ron_path: &Path, content: &Content) -> Result<Scene> {
        let text =
            std::fs::read_to_string(ron_path).with_context(|| format!("cannot read {}", ron_path.display()))?;
        let def: SceneDef =
            ron_options().from_str(&text).map_err(|e| anyhow!("{}: {e}", ron_path.display()))?;
        let png_path = ron_path.with_extension("png");
        let image = Image::load_png(&png_path)?;
        Scene::from_parts(paths::stem(ron_path), ron_path.to_path_buf(), png_path, def, &image, content)
    }

    /// Make a scene from a definition and an image. `load` uses it; tests can use it directly.
    pub fn from_parts(
        name: String,
        ron_path: PathBuf,
        png_path: PathBuf,
        def: SceneDef,
        image: &Image,
        content: &Content,
    ) -> Result<Scene> {
        let file = png_path.display().to_string();

        // Legend: color -> (material, temperature).
        let mut legend: HashMap<[u8; 3], (MaterialId, Option<i16>)> = HashMap::new();
        for (key, entry) in &def.legend {
            let rgb = parse_rgb(key).ok_or_else(|| anyhow!("{file}: legend color `{key}` is not \"#rrggbb\""))?;
            let m = content.material(&entry.material).ok_or_else(|| {
                anyhow!("{file}: legend color {key} names material `{}`, which does not exist", entry.material)
            })?;
            if rgb == [0, 0, 0] && (!m.is_air() || entry.temperature.is_some()) {
                bail!("{file}: #000000 is always air; pick another color for `{}`", entry.material);
            }
            if legend.insert(rgb, (m, entry.temperature)).is_some() {
                bail!("{file}: legend color {key} is listed twice");
            }
        }

        // Pixels.
        let (w, h) = (image.width as i32, image.height as i32);
        let mut cells = Vec::with_capacity(image.pixels.len());
        let mut unknown: Vec<(String, u32, u32)> = vec![];
        let mut unknown_count = 0usize;
        for y in 0..image.height {
            for x in 0..image.width {
                let p = image.get(x, y);
                let rgb = [p[0], p[1], p[2]];
                if p[3] == 0 || rgb == [0, 0, 0] {
                    cells.push(None);
                    continue;
                }
                match legend.get(&rgb) {
                    Some(&(m, temp)) => cells.push(if m.is_air() && temp.is_none() { None } else { Some((m, temp)) }),
                    None => {
                        unknown_count += 1;
                        let hex = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
                        if unknown.len() < 5 && !unknown.iter().any(|(c, _, _)| *c == hex) {
                            unknown.push((hex, x, y));
                        }
                        cells.push(None);
                    }
                }
            }
        }
        if let Some((hex, x, y)) = unknown.first() {
            let others: Vec<String> = unknown.iter().skip(1).map(|(c, x, y)| format!("{c} at ({x}, {y})")).collect();
            let more = if others.is_empty() { String::new() } else { format!("; also {}", others.join(", ")) };
            bail!(
                "{file}: unknown color {hex} at pixel ({x}, {y}) ({unknown_count} pixels have colors not in the legend{more})"
            );
        }

        // World size and image position.
        let margin = def.margin.max(0);
        let world_chunks = match def.size {
            WorldSize::Fit => {
                let cw = (w + 2 * margin + CHUNK_SIZE - 1) / CHUNK_SIZE;
                let ch = (h + 2 * margin + CHUNK_SIZE - 1) / CHUNK_SIZE;
                (cw.max(1), ch.max(1))
            }
            WorldSize::Chunks(cw, ch) => {
                if cw <= 0 || ch <= 0 {
                    bail!("{}: size Chunks({cw}, {ch}) must be at least 1 x 1", ron_path.display());
                }
                (cw, ch)
            }
        };
        let (ww, wh) = (world_chunks.0 * CHUNK_SIZE, world_chunks.1 * CHUNK_SIZE);
        let origin = match def.at {
            Some(p) => CellPos::new(p.x, p.y),
            None => CellPos::new((ww - w) / 2, (wh - h) / 2),
        };
        if origin.x < 0 || origin.y < 0 || origin.x + w > ww || origin.y + h > wh {
            bail!(
                "{}: the {w} x {h} image at ({}, {}) does not fit in the {ww} x {wh} world",
                ron_path.display(),
                origin.x,
                origin.y
            );
        }

        for (i, check) in def.checks.iter().enumerate() {
            check
                .validate(content)
                .map_err(|e| anyhow!("{}: check {} ({}): {e}", ron_path.display(), i + 1, check.describe()))?;
        }

        Ok(Scene { name, ron_path, png_path, def, width: w, height: h, cells, world_chunks, origin })
    }

    /// A new simulation with the scene's cells in it.
    pub fn build_sim(&self, content: Arc<Content>) -> Simulation {
        let config = SimConfig {
            width_chunks: self.world_chunks.0,
            height_chunks: self.world_chunks.1,
            seed: self.def.seed,
            bedrock_border: self.def.bedrock_border,
        };
        let mut sim = Simulation::new(content, config);
        for y in 0..self.height {
            for x in 0..self.width {
                if let Some((m, temp)) = self.cells[(y * self.width + x) as usize] {
                    sim.set_cell(self.origin.offset(x, y), m, temp);
                }
            }
        }
        sim
    }
}

/// Parse "#rrggbb" (upper or lower case).
fn parse_rgb(s: &str) -> Option<[u8; 3]> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::color;

    fn content() -> Content {
        Content::load_default().unwrap()
    }

    fn def(text: &str) -> SceneDef {
        ron_options().from_str(text).unwrap()
    }

    fn scene(text: &str, img: &Image) -> Result<Scene> {
        Scene::from_parts("t".into(), "t.ron".into(), "t.png".into(), def(text), img, &content())
    }

    #[test]
    fn legend_accepts_both_forms_and_checks_parse() {
        let d = def(r##"Scene(
            legend: { "#D9C38C": "sand", "#ff5a1a": (material: "lava", temperature: 1500) },
            ticks: 10,
            checks: [
                Count(material: "sand", rect: (x: 0, y: 0, w: 4, h: 4), min: 1, pending: true),
                Total(material: "sand", exact: 3),
                Temperature(material: "lava", of: Each, max: 2000),
                Deterministic(),
            ],
        )"##);
        assert_eq!(d.legend["#D9C38C"], LegendEntry { material: "sand".into(), temperature: None });
        assert_eq!(d.legend["#ff5a1a"].temperature, Some(1500));
        assert_eq!(d.checks.len(), 4);
        assert!(d.checks[0].pending());
        assert_eq!(d.checks[3], Check::Deterministic { every: 100, pending: false });
        assert_eq!(d.size, WorldSize::Fit);
        assert_eq!(d.margin, 16);
    }

    #[test]
    fn unknown_field_is_an_error() {
        let r: Result<SceneDef, _> =
            ron_options().from_str(r#"Scene(legend: {}, ticks: 1, checks: [Total(material: "sand", exat: 3)])"#);
        assert!(r.is_err());
    }

    #[test]
    fn unknown_color_names_color_and_pixel() {
        let mut img = Image::new(4, 3);
        img.set(2, 1, color("#123456"));
        let err = scene(r##"Scene(legend: { "#d9c38c": "sand" }, ticks: 1)"##, &img).unwrap_err().to_string();
        assert!(err.contains("#123456") && err.contains("(2, 1)"), "{err}");
    }

    #[test]
    fn unknown_material_is_an_error() {
        let img = Image::new(1, 1);
        let err = scene(r##"Scene(legend: { "#d9c38c": "sandd" }, ticks: 1)"##, &img).unwrap_err().to_string();
        assert!(err.contains("sandd"), "{err}");
    }

    #[test]
    fn bad_check_is_an_error() {
        let img = Image::new(1, 1);
        let err = scene(r##"Scene(legend: {}, ticks: 1, checks: [Total(material: "sand")])"##, &img)
            .unwrap_err()
            .to_string();
        assert!(err.contains("check 1"), "{err}");
    }

    #[test]
    fn fit_size_and_center() {
        let mut img = Image::new(100, 20);
        img.set(0, 0, color("#d9c38c"));
        img.set(99, 19, [0, 0, 0, 255]); // black is air
        let s = scene(r##"Scene(legend: { "#d9c38c": "sand" }, ticks: 1)"##, &img).unwrap();
        // 100 + 2 * 16 = 132 cells -> 3 chunks. 20 + 32 = 52 -> 1 chunk.
        assert_eq!(s.world_chunks, (3, 1));
        assert_eq!(s.origin, CellPos::new((192 - 100) / 2, (64 - 20) / 2));
        let sim = s.build_sim(Arc::new(content()));
        let sand = sim.content().expect_material("sand");
        assert_eq!(sim.cell(s.origin).material, sand);
        assert!(sim.cell(s.origin.offset(99, 19)).material.is_air());
    }

    #[test]
    fn legend_temperature_is_used() {
        let mut img = Image::new(2, 1);
        img.set(1, 0, color("#ff5a1a"));
        let s = scene(
            r##"Scene(legend: { "#ff5a1a": (material: "stone", temperature: 900) }, size: Chunks(1, 1), at: (x: 10, y: 10), ticks: 1)"##,
            &img,
        )
        .unwrap();
        let sim = s.build_sim(Arc::new(content()));
        let c = sim.cell(CellPos::new(11, 10));
        assert_eq!(c.temperature, 900);
        assert_eq!(c.material, sim.content().expect_material("stone"));
    }

    #[test]
    fn readme_example_parses() {
        let readme = include_str!("../README.md");
        let start = readme.find("```ron\n").expect("README has a ron block") + "```ron\n".len();
        let end = start + readme[start..].find("```").unwrap();
        let d = def(&readme[start..end]);
        assert_eq!(d.legend["#ff5a1a"].temperature, Some(1500));
    }

    #[test]
    fn image_must_fit() {
        let img = Image::new(70, 10);
        assert!(scene(r##"Scene(legend: {}, size: Chunks(1, 1), ticks: 1)"##, &img).is_err());
    }
}
