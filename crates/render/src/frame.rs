//! Values that change each frame. All passes read them from one uniform buffer (group 0).

use bytemuck::{Pod, Zeroable};

/// Bits of `FrameUniforms::flags`. Keep them the same as the FLAG_ constants in common.wgsl.
pub(crate) mod flags {
    /// The light pass runs, and the composite pass uses its result.
    pub const LIGHTING: u32 = 1 << 0;
    /// Debug view: each cell has the color of its temperature.
    pub const HEAT_MAP: u32 = 1 << 1;
    /// Debug view: lines on the chunk borders.
    pub const CHUNK_GRID: u32 = 1 << 2;
    pub const BLOOM: u32 = 1 << 3;
    pub const SHIMMER: u32 = 1 << 4;
    /// Debug view: only the light map.
    pub const LIGHT_ONLY: u32 = 1 << 5;
}

/// Keep this the same as `Frame` in assets/shaders/common.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct FrameUniforms {
    pub screen_size: [f32; 2],
    pub world_tex_size: [f32; 2],
    pub world_used_size: [f32; 2],
    pub view_offset: [f32; 2],
    pub target_origin: [i32; 2],
    pub origin_wrapped: [f32; 2],
    pub world_cells: [f32; 2],
    pub light_tex_size: [f32; 2],
    pub light_used: [u32; 2],
    pub zoom: f32,
    pub time: f32,
    pub material_count: u32,
    pub output_srgb: u32,
    pub flags: u32,
    pub light_count: u32,
    pub particle_count: u32,
    pub surface_y: f32,
    pub _pad: [u32; 2],
    pub sky_color: [f32; 4],
    pub ambient: [f32; 4],
    pub params: [f32; 4],
}

/// Instance data for one chunk in the world pass. Keep it the same as `ChunkInstance` in world.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct ChunkInstance {
    /// World cell of the chunk's top-left corner.
    pub origin: [i32; 2],
    /// Layer in the cell texture array.
    pub layer: u32,
    pub _pad: u32,
}

/// Instance data for one particle in the world pass. Keep it the same as `ParticleInstance` in
/// world.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct ParticleInstance {
    /// Position in world texture texels (cells from the texture origin).
    pub pos: [f32; 2],
    /// Velocity in cells per tick.
    pub vel: [f32; 2],
    /// Material in the low 16 bits, temperature (the bits of an i16) in the high 16 bits.
    pub material_temp: u32,
    pub shade: u32,
}

/// One point light for the light pass. Keep it the same as `PointLight` in light.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct LightInstance {
    /// Position in world texture texels.
    pub pos: [f32; 2],
    pub radius: f32,
    pub _pad: f32,
    /// Linear RGB, and 0 in `a`.
    pub color: [f32; 4],
}

/// Instance data for one sprite. Keep it the same as `SpriteInstance` in sprite.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct SpriteInstance {
    /// World texture position of the pivot (cells from the texture origin).
    pub pos: [f32; 2],
    /// Source rectangle in the sheet: x, y, width, height in texels.
    pub src: [u32; 4],
    /// The pivot in sheet texels from the top-left corner of the source rectangle.
    pub pivot: [f32; 2],
    /// Cells per sheet texel, and the angle in radians.
    pub scale_angle: [f32; 2],
    /// 1: mirrored.
    pub flip: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_match_the_shader() {
        assert_eq!(std::mem::size_of::<FrameUniforms>(), 160);
        assert_eq!(std::mem::offset_of!(FrameUniforms, sky_color), 112);
        assert_eq!(std::mem::size_of::<ChunkInstance>(), 16);
        assert_eq!(std::mem::size_of::<ParticleInstance>(), 24);
        assert_eq!(std::mem::size_of::<LightInstance>(), 32);
        assert_eq!(std::mem::size_of::<SpriteInstance>(), 44);
    }
}
