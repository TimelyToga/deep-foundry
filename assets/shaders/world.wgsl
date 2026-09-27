// World pass: draws the cells of each visible chunk at 1 texel per cell into the offscreen world texture.
// One instance is one chunk (a 64 x 64 quad). The output is a premultiplied-alpha color. Air is transparent.

@group(1) @binding(0) var cells: texture_2d_array<u32>;
@group(1) @binding(1) var palette: texture_2d<f32>;
@group(1) @binding(2) var glow_lut: texture_2d<f32>;
@group(1) @binding(3) var<storage, read> materials: array<MaterialInfo>;

struct MaterialInfo {
    phase: u32,
    // Light from the material itself (0 to 1). Not used yet; the light pass will use it.
    glow: f32,
    _pad: vec2<u32>,
};

const CHUNK: f32 = 64.0;
const SHADES: u32 = 8u;
// The glow lookup texture covers GLOW_MIN_C to GLOW_MAX_C in 256 steps.
const GLOW_MIN_C: f32 = 400.0;
const GLOW_MAX_C: f32 = 2000.0;

struct ChunkInstance {
    // World cell of the chunk's top-left corner.
    @location(0) origin: vec2<i32>,
    // Layer of the chunk in the `cells` texture array.
    @location(1) layer: u32,
};

struct WorldVsOut {
    @builtin(position) position: vec4<f32>,
    // Cell position inside the chunk (0 to 64).
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) layer: u32,
    @location(2) @interpolate(flat) origin: vec2<i32>,
};

@vertex
fn vs_world(@builtin(vertex_index) vertex_index: u32, chunk: ChunkInstance) -> WorldVsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let local = corners[vertex_index] * CHUNK;
    // Texel position in the world texture. Whole numbers, so chunk edges meet with no gaps.
    let texel = vec2<f32>(chunk.origin - frame.target_origin) + local;
    let ndc = vec2<f32>(
        texel.x / frame.world_used_size.x * 2.0 - 1.0,
        1.0 - texel.y / frame.world_used_size.y * 2.0,
    );
    var out: WorldVsOut;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.local = local;
    out.layer = chunk.layer;
    out.origin = chunk.origin;
    return out;
}

// The black-body glow color (rgb) and strength (a) for a temperature in °C.
fn hot_glow(temperature: f32) -> vec4<f32> {
    let f = clamp((temperature - GLOW_MIN_C) / (GLOW_MAX_C - GLOW_MIN_C), 0.0, 1.0);
    let i = i32(round(f * 255.0));
    return textureLoad(glow_lut, vec2<i32>(i, 0), 0);
}

@fragment
fn fs_world(in: WorldVsOut) -> @location(0) vec4<f32> {
    let local = clamp(vec2<i32>(floor(in.local)), vec2<i32>(0), vec2<i32>(63));
    let texel = textureLoad(cells, local, in.layer, 0);
    let material = texel.r;
    if material == 0u {
        return vec4<f32>(0.0);
    }
    if material >= frame.material_count {
        // A material id the palette does not know. Show it clearly.
        return vec4<f32>(1.0, 0.0, 1.0, 1.0);
    }

    // The temperature is stored as the bits of an i16.
    let temperature = f32((i32(texel.g) << 16u) >> 16u);
    let shade = texel.b & 0xffu;
    let life = f32(texel.b >> 8u);
    let info = materials[material];
    let cell = vec2<f32>(in.origin + local);

    var color = textureLoad(palette, vec2<i32>(i32(shade % SHADES), i32(material)), 0);

    if info.phase == PHASE_GAS {
        // Gases with a life (smoke) fade out at the end of their life.
        if life > 0.0 {
            color.a *= clamp(life / 90.0, 0.2, 1.0);
        }
    }

    // Every cell above about 500 °C glows: dark red, then orange, then yellow, then white.
    if temperature > GLOW_MIN_C {
        let g = hot_glow(temperature);
        var strength = g.a;
        if info.phase == PHASE_GAS {
            strength *= 0.5;
        }
        // Move the color toward the glow color, but keep some of the base color so the shades still show.
        let rgb = mix(color.rgb, g.rgb, strength * 0.7) + g.rgb * (strength * 0.15);
        color = vec4<f32>(rgb, max(color.a, strength));
    }

    if info.phase == PHASE_LIQUID {
        // Slow waves of brightness move through liquids, so they look like they flow.
        let t = frame.time;
        let wave = sin(cell.x * 0.13 + t * 1.3 + sin(cell.y * 0.21 - t * 0.6) * 1.6);
        let slow = sin(cell.x * 0.047 - cell.y * 0.09 - t * 0.9);
        color = vec4<f32>(color.rgb * (1.0 + 0.045 * wave + 0.03 * slow), color.a);
    }

    let rgb = min(color.rgb, vec3<f32>(1.0));
    return vec4<f32>(rgb * color.a, color.a);
}
