//! Values that change each frame. All passes read them from one uniform buffer (group 0).

use bytemuck::{Pod, Zeroable};

/// Keep this the same as `Frame` in assets/shaders/common.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct FrameUniforms {
    pub screen_size: [f32; 2],
    pub world_tex_size: [f32; 2],
    pub world_used_size: [f32; 2],
    pub view_offset: [f32; 2],
    pub target_origin: [i32; 2],
    pub world_cells: [f32; 2],
    pub view_top_left: [f32; 2],
    pub zoom: f32,
    pub time: f32,
    pub material_count: u32,
    pub output_srgb: u32,
    pub _pad: [u32; 2],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_match_the_shader() {
        assert_eq!(std::mem::size_of::<FrameUniforms>(), 80);
        assert_eq!(std::mem::size_of::<ChunkInstance>(), 16);
    }
}
