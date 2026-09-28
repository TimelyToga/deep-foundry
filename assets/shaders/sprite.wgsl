// Sprite pass: draws sprites (the robot, its effects) into the offscreen world textures, at
// 1 texel per cell, after the cells. So a sprite is on the same cell grid as the world, and the
// composite pass shows and lights it like the cells.
//
// One instance is one sprite: a rectangle of the sprite sheet at a world cell. Each texel is
// read with textureLoad: no filtering, no blur.
//
// Outputs: location 0 is the world color texture (premultiplied sRGB). Location 1 is the emitted
// light texture (linear RGB, added): the glow mask, and the whole sprite color for glow sprites.

@group(1) @binding(0) var sheet: texture_2d<f32>;
// The glow mask: the same layout as the sheet. Transparent where nothing glows.
@group(1) @binding(1) var glow: texture_2d<f32>;

// `flags` bits. Keep them the same as in crates/render/src/sprite.rs.
const SPRITE_FLIP_X: u32 = 1u;
const SPRITE_GLOW: u32 = 2u;
// How bright emitted sprite light is. The composite pass shows emitted light at 0.7.
const SPRITE_GLOW_STRENGTH: f32 = 1.4;

struct SpriteInstance {
    // World cell of the sprite's top-left corner.
    @location(0) cell: vec2<i32>,
    // Top-left texel of the source rectangle in the sheet.
    @location(1) src: vec2<u32>,
    // Size in texels (= cells).
    @location(2) size: vec2<u32>,
    // The sheet color is multiplied by this.
    @location(3) tint: vec4<f32>,
    // SPRITE_ bits above.
    @location(4) flags: u32,
};

struct SpriteVsOut {
    @builtin(position) position: vec4<f32>,
    // Texel position inside the sprite (0 to size).
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) src: vec2<u32>,
    @location(2) @interpolate(flat) size: vec2<u32>,
    @location(3) @interpolate(flat) tint: vec4<f32>,
    @location(4) @interpolate(flat) flags: u32,
};

@vertex
fn vs_sprite(@builtin(vertex_index) vertex_index: u32, s: SpriteInstance) -> SpriteVsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let local = corners[vertex_index] * vec2<f32>(s.size);
    // Texel position in the world texture. Whole numbers, so the sprite is on the cell grid.
    let texel = vec2<f32>(s.cell - frame.target_origin) + local;
    let ndc = vec2<f32>(
        texel.x / frame.world_used_size.x * 2.0 - 1.0,
        1.0 - texel.y / frame.world_used_size.y * 2.0,
    );
    var out: SpriteVsOut;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.local = local;
    out.src = s.src;
    out.size = s.size;
    out.tint = s.tint;
    out.flags = s.flags;
    return out;
}

struct SpriteFsOut {
    @location(0) color: vec4<f32>,
    @location(1) emission: vec4<f32>,
};

@fragment
fn fs_sprite(in: SpriteVsOut) -> SpriteFsOut {
    var p = min(vec2<u32>(floor(in.local)), in.size - vec2<u32>(1u));
    if (in.flags & SPRITE_FLIP_X) != 0u {
        p.x = in.size.x - 1u - p.x;
    }
    let texel = vec2<i32>(in.src + p);
    let c = textureLoad(sheet, texel, 0) * in.tint;
    if c.a <= 0.0 {
        discard;
    }
    let g = textureLoad(glow, texel, 0);
    var light = srgb_to_linear(g.rgb) * g.a;
    if (in.flags & SPRITE_GLOW) != 0u {
        light += srgb_to_linear(c.rgb) * c.a;
    }
    var out: SpriteFsOut;
    // The world color texture has premultiplied alpha.
    out.color = vec4<f32>(c.rgb * c.a, c.a);
    // Alpha is not used: the blend keeps the alpha of the texture (how much the cell stops light).
    out.emission = vec4<f32>(light * SPRITE_GLOW_STRENGTH, 0.0);
    return out;
}
