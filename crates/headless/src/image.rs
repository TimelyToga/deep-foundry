//! RGBA images: read and write PNG files, and draw the cell world into an image (CPU only).

use anyhow::{Context, Result, bail};
use foundry_core::{CellPos, CellRect};
use foundry_sim::Simulation;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

/// The background color for air in world pictures.
pub const BACKGROUND: [u8; 4] = [34, 40, 52, 255];

/// An RGBA image with 8 bits per channel. Pixels are stored row by row from the top.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[u8; 4]>,
}

impl Image {
    /// A new image. All pixels are transparent (air in a scene).
    pub fn new(width: u32, height: u32) -> Image {
        Image { width, height, pixels: vec![[0, 0, 0, 0]; (width * height) as usize] }
    }

    pub fn get(&self, x: u32, y: u32) -> [u8; 4] {
        self.pixels[(y * self.width + x) as usize]
    }

    /// Set one pixel. Pixels outside the image are ignored.
    pub fn set(&mut self, x: i32, y: i32, c: [u8; 4]) {
        if x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height {
            self.pixels[(y as u32 * self.width + x as u32) as usize] = c;
        }
    }

    /// Fill a rectangle. The part outside the image is ignored.
    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: [u8; 4]) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.set(xx, yy, c);
            }
        }
    }

    /// Read a PNG file. Any PNG color type works; the result is always RGBA.
    pub fn load_png(path: &Path) -> Result<Image> {
        let file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
        let mut decoder = png::Decoder::new(BufReader::new(file));
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        let mut reader = decoder.read_info().with_context(|| format!("{} is not a valid PNG", path.display()))?;
        let size = reader.output_buffer_size().context("PNG is too large")?;
        let mut buf = vec![0u8; size];
        let info = reader.next_frame(&mut buf).with_context(|| format!("cannot decode {}", path.display()))?;
        let (w, h) = (info.width, info.height);
        let mut pixels = Vec::with_capacity((w * h) as usize);
        for y in 0..h as usize {
            let row = &buf[y * info.line_size..];
            for x in 0..w as usize {
                let px = match info.color_type {
                    png::ColorType::Rgba => [row[x * 4], row[x * 4 + 1], row[x * 4 + 2], row[x * 4 + 3]],
                    png::ColorType::Rgb => [row[x * 3], row[x * 3 + 1], row[x * 3 + 2], 255],
                    png::ColorType::GrayscaleAlpha => [row[x * 2], row[x * 2], row[x * 2], row[x * 2 + 1]],
                    png::ColorType::Grayscale => [row[x], row[x], row[x], 255],
                    png::ColorType::Indexed => bail!("{}: palette PNG was not expanded", path.display()),
                };
                pixels.push(px);
            }
        }
        Ok(Image { width: w, height: h, pixels })
    }

    /// Write a PNG file (RGBA, 8 bits). Makes the parent folder if needed.
    pub fn save_png(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent()
            && !dir.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir).with_context(|| format!("cannot make folder {}", dir.display()))?;
        }
        let file = File::create(path).with_context(|| format!("cannot write {}", path.display()))?;
        let mut encoder = png::Encoder::new(BufWriter::new(file), self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(self.pixels.as_flattened())?;
        writer.finish()?;
        Ok(())
    }

    /// Each pixel becomes a `scale` × `scale` block.
    pub fn scaled(&self, scale: u32) -> Image {
        let scale = scale.max(1);
        if scale == 1 {
            return self.clone();
        }
        let mut out = Image::new(self.width * scale, self.height * scale);
        for y in 0..out.height {
            for x in 0..out.width {
                out.pixels[(y * out.width + x) as usize] = self.get(x / scale, y / scale);
            }
        }
        out
    }
}

/// Parse "#rrggbb" or "#rrggbbaa". Panics on bad input. Use it in code that draws scenes.
pub fn color(hex: &str) -> [u8; 4] {
    foundry_content::load::parse_color(hex).unwrap_or_else(|| panic!("bad color `{hex}`"))
}

/// Draw an area of the world. Each cell gets the first color of its material,
/// mixed over a dark background by the color's alpha. Air shows the background.
pub fn render_cells(sim: &Simulation, area: CellRect, scale: u32) -> Image {
    let mats = &sim.content().materials;
    let mut img = Image::new(area.width().max(0) as u32, area.height().max(0) as u32);
    for y in area.y0..area.y1 {
        for x in area.x0..area.x1 {
            let m = sim.cell(CellPos::new(x, y)).material;
            let c = if m.is_air() { BACKGROUND } else { over(mats.colors[m.index()][0], BACKGROUND) };
            img.set(x - area.x0, y - area.y0, c);
        }
    }
    img.scaled(scale)
}

/// Draw the temperature of each cell in an area (all materials, also air).
/// 20 °C is near black. Cold is blue. Hot goes red, orange, yellow, then white at 2500 °C.
pub fn render_heat(sim: &Simulation, area: CellRect, scale: u32) -> Image {
    let mut img = Image::new(area.width().max(0) as u32, area.height().max(0) as u32);
    for y in area.y0..area.y1 {
        for x in area.x0..area.x1 {
            let t = sim.cell(CellPos::new(x, y)).temperature;
            img.set(x - area.x0, y - area.y0, heat_color(t));
        }
    }
    img.scaled(scale)
}

/// The color for a temperature in the heat picture.
pub fn heat_color(t: i16) -> [u8; 4] {
    const STOPS: [(f32, [f32; 3]); 7] = [
        (-100.0, [80.0, 80.0, 255.0]),
        (0.0, [20.0, 40.0, 120.0]),
        (20.0, [12.0, 12.0, 18.0]),
        (100.0, [150.0, 20.0, 20.0]),
        (500.0, [235.0, 90.0, 0.0]),
        (1200.0, [255.0, 220.0, 40.0]),
        (2500.0, [255.0, 255.0, 255.0]),
    ];
    let t = t as f32;
    if t <= STOPS[0].0 {
        let c = STOPS[0].1;
        return [c[0] as u8, c[1] as u8, c[2] as u8, 255];
    }
    for w in STOPS.windows(2) {
        let ((t0, c0), (t1, c1)) = (w[0], w[1]);
        if t <= t1 {
            let f = (t - t0) / (t1 - t0);
            let mix = |i: usize| (c0[i] + (c1[i] - c0[i]) * f) as u8;
            return [mix(0), mix(1), mix(2), 255];
        }
    }
    [255, 255, 255, 255]
}

/// Mix color `c` over an opaque background by the alpha of `c`.
fn over(c: [u8; 4], bg: [u8; 4]) -> [u8; 4] {
    let a = c[3] as u32;
    let mix = |i: usize| ((c[i] as u32 * a + bg[i] as u32 * (255 - a)) / 255) as u8;
    [mix(0), mix(1), mix(2), 255]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_round_trip() {
        let mut img = Image::new(5, 3);
        img.fill_rect(1, 1, 3, 1, color("#d9c38c"));
        img.set(4, 2, [1, 2, 3, 200]);
        let dir = std::env::temp_dir().join(format!("foundry_headless_png_{}", std::process::id()));
        let path = dir.join("round_trip.png");
        img.save_png(&path).unwrap();
        let back = Image::load_png(&path).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(img, back);
    }

    #[test]
    fn scaled_repeats_pixels() {
        let mut img = Image::new(2, 1);
        img.set(1, 0, [9, 9, 9, 255]);
        let s = img.scaled(3);
        assert_eq!((s.width, s.height), (6, 3));
        assert_eq!(s.get(2, 2), [0, 0, 0, 0]);
        assert_eq!(s.get(3, 0), [9, 9, 9, 255]);
    }

    #[test]
    fn heat_colors_are_ordered() {
        assert_eq!(heat_color(20), [12, 12, 18, 255]);
        assert!(heat_color(1000)[0] > heat_color(100)[0]);
        assert_eq!(heat_color(i16::MAX), [255, 255, 255, 255]);
        assert_eq!(heat_color(-273), [80, 80, 255, 255]);
    }
}
