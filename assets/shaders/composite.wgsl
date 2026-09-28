// Composite pass: makes the screen image from the world textures and the light textures.
//
// For each screen pixel:
// 1. The background: the sky where the sky light falls straight in, else a dark rock wall.
// 2. The cells over it, drawn with sharp bilinear filtering: inside a texel the color is the texel
//    color. Only in a band one screen pixel wide at each texel edge are two texels mixed. So cells
//    look crisp at every zoom, and the camera can move by less than one cell with no shimmer.
// 3. Light: color = base color x (ambient + light) + emitted light. The light map has 1 texel for
//    LIGHT_CELLS x LIGHT_CELLS cells; it is smooth (bilinear).
// 4. Gas over that, a little soft. Then the bloom (blurred emitted light) and a soft limit for
//    very bright colors.
// Heat shimmer moves the cell texture sideways a little above very hot places.

@group(1) @binding(0) var world_color: texture_2d<f32>;
@group(1) @binding(1) var world_gas: texture_2d<f32>;
@group(1) @binding(2) var world_emission: texture_2d<f32>;
@group(1) @binding(3) var light_map: texture_2d<f32>;
@group(1) @binding(4) var light_aux: texture_2d<f32>;
@group(1) @binding(5) var bloom: texture_2d<f32>;
@group(1) @binding(6) var smooth_sampler: sampler;

@vertex
fn vs_composite(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    return fullscreen_position(vertex_index);
}

// Sharp bilinear texture position for world texel position `t`.
fn sharp_uv(t: vec2<f32>) -> vec2<f32> {
    // The nearest texel edge, and the distance to it.
    let edge = floor(t + 0.5);
    let d = t - edge;
    // 0 = the texel before the edge, 1 = the texel after it. The change takes one screen pixel.
    let s = clamp(d * frame.zoom + 0.5, vec2<f32>(0.0), vec2<f32>(1.0));
    return (edge - 0.5 + s) / frame.world_tex_size;
}

// Texture position in the light textures for world texel position `t`.
fn light_uv(t: vec2<f32>) -> vec2<f32> {
    return t / LIGHT_CELLS / frame.light_tex_size;
}

// A smooth noise from 0 to 1.
fn value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(i);
    let b = hash21(i + vec2<f32>(1.0, 0.0));
    let c = hash21(i + vec2<f32>(0.0, 1.0));
    let d = hash21(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// How strong the heat shimmer is at world texel `t`: heat in the texel or a little below it
// (hot air goes up).
fn shimmer_amount(t: vec2<f32>) -> f32 {
    var h = textureSampleLevel(light_aux, smooth_sampler, light_uv(t), 0.0).r;
    h = max(h, textureSampleLevel(light_aux, smooth_sampler, light_uv(t + vec2<f32>(0.0, 6.0)), 0.0).r * 0.85);
    h = max(h, textureSampleLevel(light_aux, smooth_sampler, light_uv(t + vec2<f32>(0.0, 14.0)), 0.0).r * 0.6);
    h = max(h, textureSampleLevel(light_aux, smooth_sampler, light_uv(t + vec2<f32>(0.0, 26.0)), 0.0).r * 0.3);
    return h;
}

// The background behind air cells (linear color). `cell` is the world cell position (x wrapped).
fn background(cell: vec2<f32>, world_y: f32, sky: f32, light: vec3<f32>) -> vec3<f32> {
    // The sky: deep blue high up, lighter near the ground.
    let h = smoothstep(frame.surface_y - 1100.0, frame.surface_y + 60.0, world_y);
    let sky_color = mix(vec3<f32>(0.035, 0.09, 0.26), vec3<f32>(0.20, 0.34, 0.54), h);
    // The rock wall far behind the caves: dark, with large soft spots. It gets the light too.
    let n = value_noise(cell * 0.045) * 0.6 + value_noise(cell * 0.17) * 0.4;
    let wall = vec3<f32>(0.16, 0.14, 0.13) * (0.6 + 0.8 * n);
    let wall_lit = wall * (frame.ambient.rgb + light * 0.7);
    return mix(wall_lit, sky_color * frame.sky_color.rgb, sky);
}

// The old unlit background (lighting off): a dark gradient that gets darker with depth.
fn plain_background(world_y: f32) -> vec3<f32> {
    let size = max(frame.world_cells.y, 1.0);
    let depth = clamp(world_y / size, 0.0, 1.0);
    let top = vec3<f32>(0.110, 0.140, 0.200);
    let middle = vec3<f32>(0.062, 0.068, 0.090);
    let bottom = vec3<f32>(0.030, 0.028, 0.036);
    var color = mix(top, middle, smoothstep(0.0, 0.55, depth));
    color = mix(color, bottom, smoothstep(0.55, 1.0, depth));
    return srgb_to_linear(color);
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

// Premultiplied sRGB color to straight linear color.
fn unpremultiply(c: vec4<f32>) -> vec3<f32> {
    if c.a <= 0.0 {
        return vec3<f32>(0.0);
    }
    return srgb_to_linear(c.rgb / c.a);
}

@fragment
fn fs_composite(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    // Position in the world texture, in texels. Texel i covers i to i + 1.
    var t = frame.view_offset + position.xy / frame.zoom;
    let world_y = f32(frame.target_origin.y) + t.y;
    let cell = frame.origin_wrapped + t;
    let lighting = has_flag(FLAG_LIGHTING);

    if lighting && has_flag(FLAG_SHIMMER) && frame.params.z > 0.0 {
        let amount = shimmer_amount(t);
        if amount > 0.02 {
            let time = frame.time;
            let wave = sin(cell.y * 0.9 + time * 7.0 + sin(cell.x * 0.23 + time * 1.7) * 2.0);
            let wave2 = sin(cell.y * 0.37 - time * 4.3 + cell.x * 0.11);
            t.x += amount * frame.params.z * (0.7 * wave + 0.3 * wave2);
        }
    }

    let base = textureSampleLevel(world_color, smooth_sampler, sharp_uv(t), 0.0);
    let gas = textureSampleLevel(world_gas, smooth_sampler, t / frame.world_tex_size, 0.0);

    var color: vec3<f32>;
    if lighting {
        let l = textureSampleLevel(light_map, smooth_sampler, light_uv(t), 0.0);
        let light = l.rgb * frame.params.w;
        if has_flag(FLAG_LIGHT_ONLY) {
            return vec4<f32>(to_output(linear_to_srgb(soft_limit(light))), 1.0);
        }
        // A little more light near the surface, so that the ground there still shows its shape.
        let near_surface = 1.0 - smoothstep(frame.surface_y, frame.surface_y + 500.0, world_y);
        let lit = frame.ambient.rgb * (1.0 + 2.0 * near_surface) + light;
        // The cell's own light. A cell that gives much light shows mostly its own light, not the
        // light map (which has its own light in it too), so its shades still show.
        let emission = textureSampleLevel(world_emission, smooth_sampler, sharp_uv(t), 0.0).rgb;
        let own = clamp(max(max(emission.r, emission.g), emission.b), 0.0, 1.0) * 0.8;
        color = background(cell, world_y, clamp(l.a, 0.0, 1.0), light) * (1.0 - base.a);
        color += unpremultiply(base) * lit * (1.0 - own) * base.a + emission * 0.7;
        // Gas gets the light too; hot gas glows by its emitted light (in `emission`).
        color = color * (1.0 - gas.a) + unpremultiply(gas) * lit * gas.a;
        if has_flag(FLAG_BLOOM) {
            let b = textureSampleLevel(bloom, smooth_sampler, t / (LIGHT_CELLS * 2.0) / vec2<f32>(textureDimensions(bloom)), 0.0).rgb;
            // Only bright light makes a glow.
            color += max(b - vec3<f32>(0.12), vec3<f32>(0.0)) * frame.params.y;
        }
        color = soft_limit(color);
    } else {
        color = plain_background(world_y) * (1.0 - base.a) + unpremultiply(base) * base.a;
        color = color * (1.0 - gas.a) + unpremultiply(gas) * gas.a;
    }

    // Outside a finite world: darker. A world width of 0 means no limit to the left and right.
    let world_x = f32(frame.target_origin.x) + t.x;
    let inside_x = frame.world_cells.x <= 0.0 || (world_x >= 0.0 && world_x < frame.world_cells.x);
    let inside = inside_x && world_y >= 0.0 && world_y < max(frame.world_cells.y, 1.0);
    if !inside {
        color *= 0.4;
    }

    var out = linear_to_srgb(color);
    if has_flag(FLAG_CHUNK_GRID) {
        // A line one screen pixel wide on each chunk border (64 cells), and a faint line on each
        // tile border (8 cells) when the tiles are large enough on the screen.
        let chunk_px = (fract(cell / 64.0) * 64.0) * frame.zoom;
        let tile_px = (fract(cell / 8.0) * 8.0) * frame.zoom;
        if chunk_px.x < 1.0 || chunk_px.y < 1.0 {
            out = mix(out, vec3<f32>(1.0, 0.85, 0.3), 0.6);
        } else if frame.zoom >= 3.0 && (tile_px.x < 1.0 || tile_px.y < 1.0) {
            out = mix(out, vec3<f32>(1.0), 0.12);
        }
    }
    // A very small noise stops visible bands in dark gradients. With no light, only the background
    // has gradients: the cells keep their exact colors.
    let noise = select(1.0 - base.a, 1.0, lighting);
    out += (hash21(position.xy) - 0.5) / 255.0 * noise;
    return vec4<f32>(to_output(out), 1.0);
}
