//! Shared GPU uniform layouts for OR materials.

/// Tint + shadow brightness + shader kind (scenery + cab).
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct OrSceneryGpuParams {
    pub tint: [f32; 4],
    pub shadow_brightness: f32,
    pub shader_kind: f32,
    pub flags: f32,
    pub _pad: [f32; 2],
}
