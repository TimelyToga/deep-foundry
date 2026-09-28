// Light pass: compute shaders on the light textures (1 texel per LIGHT_CELLS x LIGHT_CELLS cells).
// See crates/render/src/passes/light.rs for the order of the steps.
//
// 1. cs_downsample: the emitted light and the light-stopping value of each light texel, from the
//    world pass textures. Point lights (the robot's lamp) are added here.
// 2. cs_sky: one thread for each column. Sky light comes in at the top of the column and gets
//    weaker in each texel that stops light.
// 3. cs_spread (many times) and cs_spread_last: light spreads to the 8 neighbor texels. It gets
//    a little weaker with each texel of distance, and much weaker in texels that stop light.
// 4. cs_bloom_down, cs_bloom_up: blurred copies of the emitted light at smaller sizes, for the bloom.
//
// Each step uses its own bind group with its own binding numbers (group 1).

// How much light passes through a light texel that is full of rock. A little light goes into
// walls, so their surface looks lit.
const SOLID_PASS: f32 = 0.4;

// ---------------------------------------------------------------- 1. downsample

struct PointLight {
    // Position in world texture texels.
    pos: vec2<f32>,
    radius: f32,
    _pad: f32,
    // Linear rgb.
    color: vec4<f32>,
};

@group(1) @binding(0) var cell_emission: texture_2d<f32>;
@group(1) @binding(1) var cell_data: texture_2d<f32>;
@group(1) @binding(2) var<storage, read> lights: array<PointLight>;
@group(1) @binding(3) var src_out: texture_storage_2d<rgba16float, write>;
@group(1) @binding(4) var aux_out: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(8, 8)
fn cs_downsample(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= frame.light_used.x || id.y >= frame.light_used.y {
        return;
    }
    let n = i32(LIGHT_CELLS);
    let base = vec2<i32>(id.xy) * n;
    var sum = vec3<f32>(0.0);
    var brightest = vec3<f32>(0.0);
    var opacity = 0.0;
    var heat = 0.0;
    for (var y = 0; y < n; y++) {
        for (var x = 0; x < n; x++) {
            let p = base + vec2<i32>(x, y);
            let e = textureLoad(cell_emission, p, 0);
            sum += e.rgb;
            brightest = max(brightest, e.rgb);
            opacity += e.a;
            heat = max(heat, textureLoad(cell_data, p, 0).r);
        }
    }
    let cells = f32(n * n);
    // A small bright source (one cell of fire) still gives some light.
    var emitted = mix(sum / cells, brightest, 0.3);
    let pass_through = 1.0 - (opacity / cells) * (1.0 - SOLID_PASS);

    let center = vec2<f32>(base) + LIGHT_CELLS * 0.5;
    for (var i = 0u; i < frame.light_count; i++) {
        let l = lights[i];
        let d = distance(center, l.pos);
        if d < l.radius {
            emitted += l.color.rgb * (1.0 - d / l.radius);
        }
    }
    textureStore(src_out, id.xy, vec4<f32>(emitted, pass_through));
    textureStore(aux_out, id.xy, vec4<f32>(heat, 0.0, 0.0, 1.0));
}

// ---------------------------------------------------------------- 2. sky

@group(1) @binding(10) var sky_src: texture_2d<f32>;
@group(1) @binding(11) var<storage, read> sky_entry: array<f32>;
@group(1) @binding(12) var seed_out: texture_storage_2d<rgba16float, write>;
@group(1) @binding(13) var first_out: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(64)
fn cs_sky(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = id.x;
    if x >= frame.light_used.x {
        return;
    }
    var sky = sky_entry[x];
    for (var y = 0u; y < frame.light_used.y; y++) {
        let p = vec2<u32>(x, y);
        let s = textureLoad(sky_src, p, 0);
        let seed = s.rgb + frame.sky_color.rgb * sky;
        textureStore(seed_out, p, vec4<f32>(seed, sky));
        // The light that leaves this texel: what arrives and passes, or what it emits.
        textureStore(first_out, p, vec4<f32>(max(s.rgb, seed * s.a), 0.0));
        // Sky light goes a little deeper into the ground than other light, so the lit layer
        // under the surface is thick enough to show its shape.
        sky *= pow(s.a, 0.8);
    }
}

// ---------------------------------------------------------------- 3. spread

@group(1) @binding(20) var spread_src: texture_2d<f32>;
@group(1) @binding(21) var spread_seed: texture_2d<f32>;
@group(1) @binding(22) var spread_in: texture_2d<f32>;
@group(1) @binding(23) var spread_out: texture_storage_2d<rgba16float, write>;

fn leaving(p: vec2<i32>) -> vec3<f32> {
    let last = vec2<i32>(frame.light_used) - 1;
    return textureLoad(spread_in, clamp(p, vec2<i32>(0), last), 0).rgb;
}

// The light that arrives in texel `p`: its own light, or the light that leaves a neighbor.
fn arriving(p: vec2<i32>) -> vec3<f32> {
    let keep = frame.params.x;
    let keep_diagonal = pow(keep, 1.4142);
    var a = textureLoad(spread_seed, p, 0).rgb;
    a = max(a, leaving(p + vec2<i32>(1, 0)) * keep);
    a = max(a, leaving(p + vec2<i32>(-1, 0)) * keep);
    a = max(a, leaving(p + vec2<i32>(0, 1)) * keep);
    a = max(a, leaving(p + vec2<i32>(0, -1)) * keep);
    a = max(a, leaving(p + vec2<i32>(1, 1)) * keep_diagonal);
    a = max(a, leaving(p + vec2<i32>(-1, 1)) * keep_diagonal);
    a = max(a, leaving(p + vec2<i32>(1, -1)) * keep_diagonal);
    a = max(a, leaving(p + vec2<i32>(-1, -1)) * keep_diagonal);
    return a;
}

@compute @workgroup_size(8, 8)
fn cs_spread(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= frame.light_used.x || id.y >= frame.light_used.y {
        return;
    }
    let p = vec2<i32>(id.xy);
    let s = textureLoad(spread_src, p, 0);
    let a = arriving(p);
    textureStore(spread_out, p, vec4<f32>(max(s.rgb, a * s.a), 0.0));
}

// The last step writes the light that arrives in each texel (for the colors of its cells), and
// the direct sky light in `a` (for the background).
@compute @workgroup_size(8, 8)
fn cs_spread_last(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= frame.light_used.x || id.y >= frame.light_used.y {
        return;
    }
    let p = vec2<i32>(id.xy);
    let sky = textureLoad(spread_seed, p, 0).a;
    textureStore(spread_out, p, vec4<f32>(arriving(p), sky));
}

// ---------------------------------------------------------------- 4. bloom

@group(1) @binding(30) var bloom_in: texture_2d<f32>;
@group(1) @binding(31) var bloom_sampler: sampler;
@group(1) @binding(32) var bloom_out: texture_storage_2d<rgba16float, write>;
// For cs_bloom_up: the level of the same size as `bloom_out`, to add.
@group(1) @binding(33) var bloom_same: texture_2d<f32>;

fn bloom_sample(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(bloom_in, bloom_sampler, uv, 0.0).rgb;
}

// Half size: each output texel is a blur of about 4 x 4 input texels.
@compute @workgroup_size(8, 8)
fn cs_bloom_down(@builtin(global_invocation_id) id: vec3<u32>) {
    let out_size = textureDimensions(bloom_out);
    if id.x >= out_size.x || id.y >= out_size.y {
        return;
    }
    let in_size = vec2<f32>(textureDimensions(bloom_in));
    let px = 1.0 / in_size;
    let uv = (vec2<f32>(id.xy) * 2.0 + 1.0) * px;
    var c = bloom_sample(uv) * 4.0;
    c += bloom_sample(uv + vec2<f32>(-px.x, -px.y));
    c += bloom_sample(uv + vec2<f32>(px.x, -px.y));
    c += bloom_sample(uv + vec2<f32>(-px.x, px.y));
    c += bloom_sample(uv + vec2<f32>(px.x, px.y));
    textureStore(bloom_out, id.xy, vec4<f32>(c / 8.0, 1.0));
}

// Double size: a smooth blur of the smaller level, plus the level of the same size.
@compute @workgroup_size(8, 8)
fn cs_bloom_up(@builtin(global_invocation_id) id: vec3<u32>) {
    let out_size = textureDimensions(bloom_out);
    if id.x >= out_size.x || id.y >= out_size.y {
        return;
    }
    let in_size = vec2<f32>(textureDimensions(bloom_in));
    let px = 1.0 / in_size;
    // The output texel center in input texel units is (id + 0.5) / 2.
    let uv = (vec2<f32>(id.xy) + 0.5) * 0.5 * px;
    let h = px * 0.5;
    var c = bloom_sample(uv + vec2<f32>(-h.x * 2.0, 0.0));
    c += bloom_sample(uv + vec2<f32>(-h.x, h.y)) * 2.0;
    c += bloom_sample(uv + vec2<f32>(0.0, h.y * 2.0));
    c += bloom_sample(uv + vec2<f32>(h.x, h.y)) * 2.0;
    c += bloom_sample(uv + vec2<f32>(h.x * 2.0, 0.0));
    c += bloom_sample(uv + vec2<f32>(h.x, -h.y)) * 2.0;
    c += bloom_sample(uv + vec2<f32>(0.0, -h.y * 2.0));
    c += bloom_sample(uv + vec2<f32>(-h.x, -h.y)) * 2.0;
    let same = textureLoad(bloom_same, id.xy, 0).rgb;
    // The larger (more blurred) levels count a little less, so the glow stays near the light.
    textureStore(bloom_out, id.xy, vec4<f32>(c / 12.0 * 0.6 + same, 1.0));
}
