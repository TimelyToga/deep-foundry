// Sprite pass: pictures from the sprite sheet (for example the robot) over the world, at screen
// resolution, after the composite pass. They get the same light as the cells around them.
//
// One instance is one sprite: a quad that can be scaled, turned and mirrored around its pivot.
// The sheet has two textures with the same layout: the colors (sRGB, straight alpha) and the
// emitted light (parts that glow in the dark, such as a visor).

@group(1) @binding(0) var sheet_color: texture_2d<f32>;
@group(1) @binding(1) var sheet_emission: texture_2d<f32>;
@group(1) @binding(2) var light_map: texture_2d<f32>;
@group(1) @binding(3) var smooth_sampler: sampler;

struct SpriteInstance {
    // World texture position (texels = cells from the texture origin) of the pivot.
    @location(0) pos: vec2<f32>,
    // Source rectangle in the sheet: x, y, width, height in sheet texels.
    @location(1) src: vec4<u32>,
    // The pivot in sheet texels from the top-left corner of the source rectangle.
    @location(2) pivot: vec2<f32>,
    // x: cells per sheet texel. y: angle in radians (clockwise on the screen).
    @location(3) scale_angle: vec2<f32>,
    // 1: mirror left and right (around the pivot).
    @location(4) flip: u32,
};

struct SpriteVsOut {
    @builtin(position) position: vec4<f32>,
    // Position in the sheet, in texels.
    @location(0) texel: vec2<f32>,
    // Position in the world texture, in texels.
    @location(1) world: vec2<f32>,
    // Screen pixels per sheet texel.
    @location(2) @interpolate(flat) density: f32,
};

@vertex
fn vs_sprite(@builtin(vertex_index) vertex_index: u32, s: SpriteInstance) -> SpriteVsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let size = vec2<f32>(s.src.zw);
    let local = corners[vertex_index] * size;
    var d = local - s.pivot;
    if s.flip != 0u {
        d.x = -d.x;
    }
    let c = cos(s.scale_angle.y);
    let n = sin(s.scale_angle.y);
    let turned = vec2<f32>(d.x * c - d.y * n, d.x * n + d.y * c) * s.scale_angle.x;
    let world = s.pos + turned;
    let px = (world - frame.view_offset) * frame.zoom;
    var out: SpriteVsOut;
    out.position = vec4<f32>(px.x / frame.screen_size.x * 2.0 - 1.0, 1.0 - px.y / frame.screen_size.y * 2.0, 0.0, 1.0);
    out.texel = vec2<f32>(s.src.xy) + local;
    out.world = world;
    out.density = frame.zoom * s.scale_angle.x;
    return out;
}

@fragment
fn fs_sprite(in: SpriteVsOut) -> @location(0) vec4<f32> {
    // Sharp bilinear filtering in sheet texels (see composite.wgsl).
    let sheet_size = vec2<f32>(textureDimensions(sheet_color));
    let edge = floor(in.texel + 0.5);
    let s = clamp((in.texel - edge) * in.density + 0.5, vec2<f32>(0.0), vec2<f32>(1.0));
    let uv = (edge - 0.5 + s) / sheet_size;
    let c = textureSampleLevel(sheet_color, smooth_sampler, uv, 0.0);
    let e = textureSampleLevel(sheet_emission, smooth_sampler, uv, 0.0);
    let alpha = max(c.a, e.a);
    if alpha < 0.004 {
        discard;
    }
    let base = srgb_to_linear(c.rgb / max(c.a, 1e-4)) * (c.a / alpha);
    let glow = srgb_to_linear(e.rgb / max(e.a, 1e-4)) * (e.a / alpha);
    var color: vec3<f32>;
    if has_flag(FLAG_LIGHTING) {
        let l = textureSampleLevel(light_map, smooth_sampler, light_uv(in.world), 0.0).rgb * frame.params.w;
        let world_y = f32(frame.target_origin.y) + in.world.y;
        color = soft_limit(base * (ambient_at(world_y) + l) + glow * 1.3);
    } else {
        color = min(base + glow, vec3<f32>(1.0));
    }
    return vec4<f32>(to_output(linear_to_srgb(color)) * alpha, alpha);
}
