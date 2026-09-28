// Shared code for all render passes.
// The renderer puts this text in front of each pass shader before it compiles the shader.
// Keep `Frame` the same as `FrameUniforms` in crates/render/src/frame.rs.
//
// Coordinates: the world pass draws into offscreen textures at 1 texel per cell ("world texels").
// Texel (0, 0) is the world cell `target_origin`. The light textures have 1 texel per
// LIGHT_CELLS x LIGHT_CELLS cells, on the same origin.

struct Frame {
    // Size of the render target in pixels.
    screen_size: vec2<f32>,
    // Full size of the offscreen world textures in texels.
    world_tex_size: vec2<f32>,
    // The part of the world textures that the world pass draws this frame, in texels.
    world_used_size: vec2<f32>,
    // World texture position (in texels) of the top-left corner of the screen.
    view_offset: vec2<f32>,
    // The world cell at world texture texel (0, 0).
    target_origin: vec2<i32>,
    // `target_origin` with x wrapped to 0..65536, as f32. For patterns that must not lose precision.
    origin_wrapped: vec2<f32>,
    // World size in cells. Width 0: no limit to the left and right.
    world_cells: vec2<f32>,
    // Full size of the light textures in texels.
    light_tex_size: vec2<f32>,
    // The part of the light textures that the light pass fills this frame.
    light_used: vec2<u32>,
    // Screen pixels per cell.
    zoom: f32,
    // Seconds. Used for small animations such as the liquid color shift.
    time: f32,
    // Number of materials in the palette.
    material_count: u32,
    // 1 if the render target stores sRGB colors in linear form (an "...Srgb" format).
    output_srgb: u32,
    // FLAG_ bits below.
    flags: u32,
    // Number of point lights (light pass).
    light_count: u32,
    // Number of particles (world pass).
    particle_count: u32,
    // World row of the ground surface (about). The sky color gets lighter toward it.
    surface_y: f32,
    _pad0: u32,
    _pad1: u32,
    // Linear color of the sky light (rgb) that falls in from above.
    sky_color: vec4<f32>,
    // Linear light that is everywhere, also deep underground (rgb).
    ambient: vec4<f32>,
    // x: light kept per light texel of distance, y: bloom strength, z: heat shimmer strength
    // (in cells), w: exposure.
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;

// Keep the same bits as `flags` in crates/render/src/frame.rs.
const FLAG_LIGHTING: u32 = 1u;
const FLAG_HEAT_MAP: u32 = 2u;
const FLAG_CHUNK_GRID: u32 = 4u;
const FLAG_BLOOM: u32 = 8u;
const FLAG_SHIMMER: u32 = 16u;
const FLAG_LIGHT_ONLY: u32 = 32u;

// Cells per light texel, on each side. Keep it the same as LIGHT_CELLS in crates/render/src/targets.rs.
const LIGHT_CELLS: f32 = 4.0;

// Material phases. Keep the same numbers as `phase_code` in crates/render/src/palette.rs.
const PHASE_EMPTY: u32 = 0u;
const PHASE_SOLID: u32 = 1u;
const PHASE_POWDER: u32 = 2u;
const PHASE_LIQUID: u32 = 3u;
const PHASE_GAS: u32 = 4u;
const PHASE_FIRE: u32 = 5u;

fn has_flag(bit: u32) -> bool {
    return (frame.flags & bit) != 0u;
}

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

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3<f32>(0.0));
    let low = x * 12.92;
    let high = 1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, x <= vec3<f32>(0.0031308));
}

// Colors in the data files are sRGB values ("#rrggbb"). Convert them only when the target format
// expects linear values.
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

// Texture position in the light textures for world texel position `t`.
fn light_uv(t: vec2<f32>) -> vec2<f32> {
    return t / LIGHT_CELLS / frame.light_tex_size;
}

// The ambient light at world row `world_y`. A little more near the surface, so that the ground
// there still shows its shape.
fn ambient_at(world_y: f32) -> vec3<f32> {
    let near_surface = 1.0 - smoothstep(frame.surface_y, frame.surface_y + 500.0, world_y);
    return frame.ambient.rgb * (1.0 + 2.0 * near_surface);
}

// One value that goes smoothly toward 1 above `knee` instead of being cut off.
fn limit1(x: f32) -> f32 {
    let knee = 0.7;
    if x <= knee {
        return x;
    }
    return knee + (1.0 - knee) * (1.0 - exp(-(x - knee) / (1.0 - knee)));
}

// Bright colors go smoothly toward 1 instead of being cut off. Mostly the color keeps its hue
// (all channels get the same scale); a small part goes toward white, as in very bright light.
fn soft_limit(c: vec3<f32>) -> vec3<f32> {
    let m = max(max(c.r, c.g), c.b);
    if m <= 0.7 {
        return c;
    }
    let same_hue = c * (limit1(m) / m);
    let per_channel = vec3<f32>(limit1(c.r), limit1(c.g), limit1(c.b));
    return mix(same_hue, per_channel, 0.15);
}

