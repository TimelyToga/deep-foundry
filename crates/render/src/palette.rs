//! Material colors and material data for the GPU, built from `foundry_content`.

use bytemuck::{Pod, Zeroable};
use foundry_content::{Content, Phase};

/// Shades per material in the palette texture. A cell uses `shade % SHADES`.
pub const SHADES: usize = 8;

/// Number of entries in the glow lookup texture.
pub const GLOW_LUT_SIZE: usize = 256;
/// The glow lookup texture covers this range of temperatures (°C). Keep it the same as world.wgsl.
pub const GLOW_MIN_C: f32 = 400.0;
pub const GLOW_MAX_C: f32 = 2000.0;

/// Gases are drawn partly transparent. This is the highest alpha (0 to 255) a gas color can have.
const GAS_MAX_ALPHA: u8 = 200;

/// Material data for the world shader. Keep it the same as `MaterialInfo` in world.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct MaterialInfo {
    pub phase: u32,
    pub glow: f32,
    pub _pad: [u32; 2],
}

/// The number the shaders use for a phase. Keep it the same as the PHASE_ constants in common.wgsl.
pub(crate) fn phase_code(phase: Phase) -> u32 {
    match phase {
        Phase::Empty => 0,
        Phase::Solid => 1,
        Phase::Powder => 2,
        Phase::Liquid => 3,
        Phase::Gas => 4,
        Phase::Fire => 5,
    }
}

/// The palette: `SHADES` colors for each material, row by row (one row per material).
/// Shade `i` is the material's color `i % colors.len()`.
pub(crate) fn build_palette(content: &Content) -> Vec<[u8; 4]> {
    let mats = &content.materials;
    let mut out = Vec::with_capacity(mats.len() * SHADES);
    for m in 0..mats.len() {
        let colors = &mats.colors[m];
        for i in 0..SHADES {
            let mut c = colors[i % colors.len()];
            if mats.phase[m] == Phase::Gas {
                c[3] = c[3].min(GAS_MAX_ALPHA);
            }
            out.push(c);
        }
    }
    out
}

pub(crate) fn build_material_info(content: &Content) -> Vec<MaterialInfo> {
    let mats = &content.materials;
    (0..mats.len())
        .map(|m| MaterialInfo { phase: phase_code(mats.phase[m]), glow: mats.glow[m], _pad: [0; 2] })
        .collect()
}

/// Glow color (r, g, b) and strength (a) of a hot cell, from 0 to 1.
///
/// Below 500 °C there is no glow. Then the color goes from dark red to orange, yellow and white.
/// The strength grows to its full value at 1150 °C.
pub fn glow_color(temperature: f32) -> [f32; 4] {
    // (temperature °C, r, g, b)
    const STOPS: [(f32, f32, f32, f32); 8] = [
        (500.0, 0.25, 0.02, 0.00),
        (650.0, 0.55, 0.05, 0.01),
        (800.0, 0.85, 0.16, 0.02),
        (1000.0, 1.00, 0.34, 0.05),
        (1200.0, 1.00, 0.52, 0.12),
        (1350.0, 1.00, 0.74, 0.34),
        (1500.0, 1.00, 0.93, 0.76),
        (2000.0, 1.00, 1.00, 1.00),
    ];
    if temperature <= STOPS[0].0 {
        return [0.0; 4];
    }
    let mut rgb = [1.0, 1.0, 1.0];
    for w in STOPS.windows(2) {
        let (a, b) = (w[0], w[1]);
        if temperature <= b.0 {
            let f = (temperature - a.0) / (b.0 - a.0);
            rgb = [a.1 + (b.1 - a.1) * f, a.2 + (b.2 - a.2) * f, a.3 + (b.3 - a.3) * f];
            break;
        }
    }
    let x = ((temperature - 500.0) / 650.0).clamp(0.0, 1.0);
    let strength = x * x * (3.0 - 2.0 * x);
    [rgb[0], rgb[1], rgb[2], strength]
}

/// The glow lookup texture: `GLOW_LUT_SIZE` RGBA8 entries from `GLOW_MIN_C` to `GLOW_MAX_C`.
pub(crate) fn build_glow_lut() -> Vec<[u8; 4]> {
    (0..GLOW_LUT_SIZE)
        .map(|i| {
            let t = GLOW_MIN_C + (GLOW_MAX_C - GLOW_MIN_C) * i as f32 / (GLOW_LUT_SIZE - 1) as f32;
            glow_color(t).map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content() -> Content {
        Content::from_ron(
            &[r##"[
                Material(id: "air", name: "Air", phase: Empty, colors: ["#00000000"]),
                Material(id: "rock", name: "Rock", phase: Solid, colors: ["#101010", "#202020", "#303030"]),
                Material(id: "fog", name: "Fog", phase: Gas, density: 0.5, colors: ["#ffffffff"]),
            ]"##],
            &[],
        )
        .unwrap()
    }

    #[test]
    fn palette_cycles_colors() {
        let c = content();
        let p = build_palette(&c);
        assert_eq!(p.len(), 3 * SHADES);
        let rock = &p[SHADES..2 * SHADES];
        assert_eq!(rock[0], [0x10, 0x10, 0x10, 255]);
        assert_eq!(rock[3], [0x10, 0x10, 0x10, 255]);
        assert_eq!(rock[5], [0x30, 0x30, 0x30, 255]);
        // Gases are never fully opaque.
        assert_eq!(p[2 * SHADES][3], GAS_MAX_ALPHA);
    }

    #[test]
    fn material_info_has_phases() {
        let info = build_material_info(&content());
        assert_eq!(info[0].phase, 0);
        assert_eq!(info[1].phase, 1);
        assert_eq!(info[2].phase, 4);
    }

    #[test]
    fn glow_gets_brighter_and_whiter() {
        assert_eq!(glow_color(20.0)[3], 0.0);
        assert_eq!(glow_color(500.0)[3], 0.0);
        let dark = glow_color(600.0);
        let orange = glow_color(1000.0);
        let white = glow_color(1600.0);
        assert!(dark[3] < orange[3] && orange[3] <= white[3]);
        assert!(dark[1] < orange[1] && orange[1] < white[1], "green grows: red to orange to white");
        assert!(white[2] > 0.7);
        let lut = build_glow_lut();
        assert_eq!(lut.len(), GLOW_LUT_SIZE);
        assert_eq!(lut[0], [0, 0, 0, 0]);
        assert_eq!(lut[GLOW_LUT_SIZE - 1], [255, 255, 255, 255]);
    }
}
