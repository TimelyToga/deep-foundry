// World pass: draws the cells of each visible chunk at 1 texel per cell into the offscreen world
// textures, then the particles over them.
// One chunk instance is a 64 x 64 quad. One particle instance is a short streak, 1 cell wide.
//
// Outputs (see crates/render/src/targets.rs):
//   0 color:    cells that are not gas. sRGB with premultiplied alpha. Air is transparent.
//   1 gas:      gas cells, the same way.
//   2 emission: light the cell gives (linear rgb); a = how much the cell stops light.
//   3 data:     r = heat for the shimmer (0 to 1).

@group(1) @binding(0) var cells: texture_2d_array<u32>;
@group(1) @binding(1) var palette: texture_2d<f32>;
@group(1) @binding(2) var glow_lut: texture_2d<f32>;
@group(1) @binding(3) var<storage, read> materials: array<MaterialInfo>;

struct MaterialInfo {
    phase: u32,
    // Light from the material itself (0 to 1), from the data files.
    glow: f32,
    // How much a cell stops light (0 air, 1 rock).
    opacity: f32,
    _pad: u32,
};

const CHUNK: f32 = 64.0;
const SHADES: u32 = 8u;
// The glow lookup texture covers GLOW_MIN_C to GLOW_MAX_C in 256 steps.
const GLOW_MIN_C: f32 = 400.0;
const GLOW_MAX_C: f32 = 2000.0;
// The cell flag of a burning cell (bit 2). The reactions code sets it (FLAG_BURNING in
// crates/sim/src/chunk.rs).
const CELL_FLAG_BURNING: u32 = 4u;
// The cell flag of a building body cell (bit 1, FLAG_BUILDING in crates/sim/src/chunk.rs).
// Buildings stand in front of the world: they stop only a little light, so they do not cast a
// deep shadow and their whole face gets the light.
const CELL_FLAG_BUILDING: u32 = 2u;
// Emitted light of a material with glow 1, and of a white-hot cell.
const GLOW_LIGHT: f32 = 1.4;
const HOT_LIGHT: f32 = 1.8;

struct WorldOut {
    @location(0) color: vec4<f32>,
    @location(1) gas: vec4<f32>,
    @location(2) emission: vec4<f32>,
    @location(3) data: vec4<f32>,
};

// The black-body glow color (rgb, sRGB) and strength (a) for a temperature in °C.
fn hot_glow(temperature: f32) -> vec4<f32> {
    let f = clamp((temperature - GLOW_MIN_C) / (GLOW_MAX_C - GLOW_MIN_C), 0.0, 1.0);
    let i = i32(round(f * 255.0));
    return textureLoad(glow_lut, vec2<i32>(i, 0), 0);
}

// Debug heat map: a color for each temperature (°C).
// Blue below 0, dark green at room temperature, then yellow, orange, red and white.
fn heat_map_color(t: f32) -> vec3<f32> {
    let stops = array<vec4<f32>, 8>(
        vec4<f32>(-60.0, 0.10, 0.20, 0.90),
        vec4<f32>(0.0, 0.20, 0.55, 0.95),
        vec4<f32>(20.0, 0.10, 0.30, 0.18),
        vec4<f32>(100.0, 0.20, 0.75, 0.25),
        vec4<f32>(300.0, 0.95, 0.90, 0.20),
        vec4<f32>(600.0, 1.00, 0.50, 0.05),
        vec4<f32>(1000.0, 0.90, 0.08, 0.05),
        vec4<f32>(1600.0, 1.00, 0.95, 0.95),
    );
    if t <= stops[0].x {
        return stops[0].yzw;
    }
    for (var i = 1u; i < 8u; i++) {
        if t <= stops[i].x {
            let a = stops[i - 1u];
            let b = stops[i];
            return mix(a.yzw, b.yzw, (t - a.x) / (b.x - a.x));
        }
    }
    return stops[7].yzw;
}

// A value from 0 to 1 that changes fast over time, different for each cell. For fire.
fn flicker(cell: vec2<f32>, shade: u32, life: f32) -> f32 {
    let t = frame.time;
    let seed = hash21(cell) * 6.283 + f32(shade) * 0.7;
    let a = sin(t * 13.0 + seed + life * 0.45);
    let b = sin(t * 7.3 - seed * 1.7 + cell.y * 0.35);
    return clamp(0.5 + 0.3 * a + 0.2 * b, 0.0, 1.0);
}

// Heat for the shimmer: 0 below 350 °C, 1 above 1100 °C.
fn shimmer_heat(temperature: f32) -> f32 {
    return smoothstep(350.0, 1100.0, temperature);
}

// The outputs for one cell. `cell` is the world position wrapped to 0..16384 (for patterns).
fn shade_cell(material: u32, temperature: f32, shade: u32, life: f32, cell_flags: u32, cell: vec2<f32>) -> WorldOut {
    var out: WorldOut;
    out.color = vec4<f32>(0.0);
    out.gas = vec4<f32>(0.0);
    out.emission = vec4<f32>(0.0);
    out.data = vec4<f32>(shimmer_heat(temperature), 0.0, 0.0, 0.0);

    if has_flag(FLAG_HEAT_MAP) {
        // Debug view: every cell (also air) has the color of its temperature.
        let alpha = select(1.0, 0.55, material == 0u);
        out.color = vec4<f32>(heat_map_color(temperature) * alpha, alpha);
        return out;
    }
    if material == 0u {
        return out;
    }
    if material >= frame.material_count {
        // A material id the palette does not know. Show it clearly.
        out.color = vec4<f32>(1.0, 0.0, 1.0, 1.0);
        out.emission = vec4<f32>(0.0, 0.0, 0.0, 1.0);
        return out;
    }

    let info = materials[material];
    var color = textureLoad(palette, vec2<i32>(i32(shade % SHADES), i32(material)), 0);
    var emission = vec3<f32>(0.0);
    var opacity = info.opacity;

    if info.phase == PHASE_GAS && life > 0.0 {
        // Gases with a life (smoke) fade out at the end of their life.
        color.a *= clamp(life / 90.0, 0.2, 1.0);
    }

    if info.phase == PHASE_FIRE {
        // Fire flickers. Young fire (much life left) is yellow; old fire is dark orange.
        let f = flicker(cell, shade, life);
        let young = clamp(life / 30.0, 0.0, 1.0);
        let old_color = color.rgb * vec3<f32>(0.95, 0.5, 0.3);
        let young_color = min(color.rgb * vec3<f32>(1.1, 1.15, 1.2) + vec3<f32>(0.05, 0.08, 0.02), vec3<f32>(1.0));
        color = vec4<f32>(mix(old_color, young_color, young) * (0.75 + 0.4 * f), color.a);
        emission = srgb_to_linear(color.rgb) * info.glow * GLOW_LIGHT * (0.6 + 0.6 * f);
    } else if info.glow > 0.0 {
        emission = srgb_to_linear(color.rgb) * info.glow * GLOW_LIGHT;
    }

    // Every cell above about 500 °C glows: dark red, then orange, then yellow, then white.
    if temperature > GLOW_MIN_C {
        let g = hot_glow(temperature);
        var strength = g.a;
        if info.phase == PHASE_GAS {
            strength *= 0.5;
        }
        // Move the color toward the glow color, but keep some of the base color so the shades still show.
        let rgb = mix(color.rgb, g.rgb, strength * 0.75) + g.rgb * (strength * 0.15);
        color = vec4<f32>(rgb, max(color.a, strength));
        emission = max(emission, srgb_to_linear(g.rgb) * strength * HOT_LIGHT);
    }

    if (cell_flags & CELL_FLAG_BURNING) != 0u {
        // A burning cell: an orange flicker on its color, and a little light.
        let f = flicker(cell, shade, life);
        color = vec4<f32>(mix(color.rgb, vec3<f32>(1.0, 0.52, 0.12), 0.3 + 0.35 * f), color.a);
        emission += vec3<f32>(1.0, 0.36, 0.06) * (0.35 + 0.5 * f);
    }

    if info.phase == PHASE_LIQUID {
        // Slow waves of brightness move through liquids, so they look like they flow.
        let t = frame.time;
        let wave = sin(cell.x * 0.13 + t * 1.3 + sin(cell.y * 0.21 - t * 0.6) * 1.6);
        let slow = sin(cell.x * 0.047 - cell.y * 0.09 - t * 0.9);
        let k = 1.0 + 0.045 * wave + 0.03 * slow;
        color = vec4<f32>(color.rgb * k, color.a);
        emission *= k;
    }

    let rgb = min(color.rgb, vec3<f32>(1.0));
    let premultiplied = vec4<f32>(rgb * color.a, color.a);
    if info.phase == PHASE_GAS {
        out.gas = premultiplied;
        opacity *= color.a;
    } else {
        out.color = premultiplied;
    }
    if (cell_flags & CELL_FLAG_BUILDING) != 0u {
        opacity *= 0.25;
    }
    out.emission = vec4<f32>(emission, opacity);
    return out;
}

// ---------------------------------------------------------------- chunks

struct ChunkInstance {
    // World cell of the chunk's top-left corner.
    @location(0) origin: vec2<i32>,
    // Layer of the chunk in the `cells` texture array.
    @location(1) layer: u32,
};

struct ChunkVsOut {
    @builtin(position) position: vec4<f32>,
    // Cell position inside the chunk (0 to 64).
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) layer: u32,
    @location(2) @interpolate(flat) origin: vec2<i32>,
};

// Clip position of a world texture texel position.
fn texel_to_clip(texel: vec2<f32>) -> vec4<f32> {
    let ndc = vec2<f32>(
        texel.x / frame.world_used_size.x * 2.0 - 1.0,
        1.0 - texel.y / frame.world_used_size.y * 2.0,
    );
    return vec4<f32>(ndc, 0.0, 1.0);
}

@vertex
fn vs_world(@builtin(vertex_index) vertex_index: u32, chunk: ChunkInstance) -> ChunkVsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let local = corners[vertex_index] * CHUNK;
    // Texel position in the world texture. Whole numbers, so chunk edges meet with no gaps.
    let texel = vec2<f32>(chunk.origin - frame.target_origin) + local;
    var out: ChunkVsOut;
    out.position = texel_to_clip(texel);
    out.local = local;
    out.layer = chunk.layer;
    out.origin = chunk.origin;
    return out;
}

@fragment
fn fs_world(in: ChunkVsOut) -> WorldOut {
    let local = clamp(vec2<i32>(floor(in.local)), vec2<i32>(0), vec2<i32>(63));
    let texel = textureLoad(cells, local, in.layer, 0);
    // The temperature is stored as the bits of an i16.
    let temperature = f32((i32(texel.g) << 16u) >> 16u);
    // The cell position, wrapped to 0..16384 so that f32 keeps whole cells far from x = 0.
    // (The world has no limit to the left and right.) The patterns jump once every 16384 cells.
    let cell = vec2<f32>((in.origin + local) & vec2<i32>(0x3fff));
    return shade_cell(texel.r, temperature, texel.b & 0xffu, f32(texel.b >> 8u), texel.a, cell);
}

// ---------------------------------------------------------------- particles

struct ParticleInstance {
    // Position in world texture texels.
    @location(0) pos: vec2<f32>,
    // Velocity in cells per tick.
    @location(1) vel: vec2<f32>,
    // x: material (low 16 bits) and temperature (high 16 bits, the bits of an i16). y: shade.
    @location(2) info: vec2<u32>,
};

struct ParticleVsOut {
    @builtin(position) position: vec4<f32>,
    // 0 at the head of the streak, 1 at its tail.
    @location(0) along: f32,
    @location(1) @interpolate(flat) info: vec2<u32>,
    @location(2) @interpolate(flat) cell: vec2<f32>,
};

// A streak behind a moving particle shows its motion. Its length is the distance of this many ticks.
const STREAK_TICKS: f32 = 1.6;

@vertex
fn vs_particle(@builtin(vertex_index) vertex_index: u32, p: ParticleInstance) -> ParticleVsOut {
    let speed = length(p.vel);
    let dir = select(vec2<f32>(0.0, 1.0), p.vel / max(speed, 1e-5), speed > 0.05);
    let side = vec2<f32>(-dir.y, dir.x);
    // The head is the cell of the particle; the tail goes back along the velocity.
    let head = floor(p.pos) + 0.5;
    let tail_len = min(speed * STREAK_TICKS, 8.0);
    // Corners: (along 0 or 1, side -1 or 1).
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
    );
    let c = corners[vertex_index];
    let pos = head + dir * (0.5 - c.x * (tail_len + 1.0)) + side * (c.y * 0.5);
    var out: ParticleVsOut;
    out.position = texel_to_clip(pos);
    out.along = c.x;
    out.info = p.info;
    out.cell = floor(p.pos) + frame.origin_wrapped;
    return out;
}

@fragment
fn fs_particle(in: ParticleVsOut) -> WorldOut {
    let material = in.info.x & 0xffffu;
    let temperature = f32(i32(in.info.x) >> 16u);
    let shade = in.info.y;
    var out = shade_cell(material, temperature, shade, 0.0, 0u, in.cell);
    // The streak fades toward its tail. The head is a little brighter than a resting cell.
    let fade = mix(1.0, 0.15, in.along);
    out.color = (out.color + out.gas) * fade;
    out.color = vec4<f32>(min(out.color.rgb * 1.15, vec3<f32>(out.color.a)), out.color.a);
    out.gas = vec4<f32>(0.0);
    out.emission = vec4<f32>(out.emission.rgb * fade, 0.0);
    return out;
}
