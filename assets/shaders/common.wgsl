// Shared code for all render passes.
// The renderer puts this text in front of each pass shader before it compiles the shader.
// Keep `Frame` the same as `FrameUniforms` in crates/render/src/frame.rs.

struct Frame {
    // Size of the render target in pixels.
    screen_size: vec2<f32>,
    // Full size of the offscreen world texture in texels.
    world_tex_size: vec2<f32>,
    // The part of the world texture that the world pass draws this frame, in texels.
    world_used_size: vec2<f32>,
    // World texture position (in texels) of the top-left corner of the screen.
    view_offset: vec2<f32>,
    // The world cell at world texture texel (0, 0).
    target_origin: vec2<i32>,
    // World size in cells. Width 0: no limit to the left and right.
    world_cells: vec2<f32>,
    // World position (in cells) of the top-left corner of the screen.
    view_top_left: vec2<f32>,
    // Screen pixels per cell.
    zoom: f32,
    // Seconds. Used for small animations such as the liquid color shift.
    time: f32,
    // Number of materials in the palette.
    material_count: u32,
    // 1 if the render target stores sRGB colors in linear form (an "...Srgb" format).
    output_srgb: u32,
    _pad: vec2<u32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;

// Material phases. Keep the same numbers as `phase_code` in crates/render/src/palette.rs.
const PHASE_EMPTY: u32 = 0u;
const PHASE_SOLID: u32 = 1u;
const PHASE_POWDER: u32 = 2u;
const PHASE_LIQUID: u32 = 3u;
const PHASE_GAS: u32 = 4u;
const PHASE_FIRE: u32 = 5u;

// One triangle that covers the whole target. Use it with `draw(0..3, 0..1)`.
fn fullscreen_position(vertex_index: u32) -> vec4<f32> {
    let x = f32((vertex_index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(vertex_index & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

// Colors in the shaders are sRGB values (the same numbers as the "#rrggbb" colors in the data files).
// Convert them only when the target format expects linear values.
fn to_output(c: vec3<f32>) -> vec3<f32> {
    if frame.output_srgb != 0u {
        return srgb_to_linear(c);
    }
    return c;
}

// A number from 0 to 1 that looks random, from a 2D position.
fn hash21(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(0.1031, 0.1030));
    let r = q + dot(q, q.yx + 33.33);
    return fract((r.x + r.y) * r.x);
}
