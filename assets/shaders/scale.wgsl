// Scale pass: draws the offscreen world texture on the screen, over the background.
//
// Sharp bilinear filtering: inside a texel the color is the texel color. Only in a band one screen pixel
// wide at each texel edge are the two texels mixed. So cells look crisp at every zoom, and the
// camera can move by less than one cell with no shimmer.

@group(1) @binding(0) var world_color: texture_2d<f32>;
@group(1) @binding(1) var world_sampler: sampler;

@vertex
fn vs_scale(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    return fullscreen_position(vertex_index);
}

@fragment
fn fs_scale(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    // Position in the world texture, in texels. Texel i covers i to i + 1.
    let t = frame.view_offset + position.xy / frame.zoom;
    // The nearest texel edge, and the distance to it.
    let edge = floor(t + 0.5);
    let d = t - edge;
    // 0 = the texel before the edge, 1 = the texel after it. The change takes one screen pixel.
    let s = clamp(d * frame.zoom + 0.5, vec2<f32>(0.0), vec2<f32>(1.0));
    let uv = (edge - 0.5 + s) / frame.world_tex_size;
    let c = textureSampleLevel(world_color, world_sampler, uv, 0.0);
    if frame.output_srgb != 0u && c.a > 0.0 {
        return vec4<f32>(srgb_to_linear(c.rgb / c.a) * c.a, c.a);
    }
    return c;
}
