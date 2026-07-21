//! Shared HWater geometry helpers (#118).

use bevy::prelude::*;

pub const COLOR_WATER: Color = Color::srgba(0.08, 0.38, 0.62, 0.68);
pub const COLOR_WATER_REFLECT: Color = Color::srgba(0.04, 0.22, 0.38, 0.28);
pub const WATER_LIFT_M: f32 = 0.08;
/// Texture tile repeats across the water plane.
pub const WATER_UV_TILES: f32 = 3.0;

/// Build a horizontal water plane mesh of size `(width, depth)` metres.
pub fn build_water_plane_mesh(meshes: &mut Assets<Mesh>, width: f32, depth: f32) -> Handle<Mesh> {
    meshes.add(Plane3d::default().mesh().size(width.max(0.1), depth.max(0.1)))
}

/// Default translucent water material (apps may override fog/UV animation).
pub fn water_material(
    materials: &mut Assets<StandardMaterial>,
    texture: Option<Handle<Image>>,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: COLOR_WATER,
        base_color_texture: texture,
        emissive: LinearRgba::from(Color::srgb(0.08, 0.24, 0.42)) * 0.45,
        perceptual_roughness: 0.06,
        metallic: 0.05,
        reflectance: 0.75,
        alpha_mode: AlphaMode::Blend,
        double_sided: true,
        ..default()
    })
}

/// Soft reflection plane material.
pub fn reflection_material(materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: COLOR_WATER_REFLECT,
        emissive: LinearRgba::from(Color::srgb(0.05, 0.16, 0.28)) * 0.2,
        perceptual_roughness: 0.02,
        reflectance: 0.85,
        alpha_mode: AlphaMode::Blend,
        double_sided: true,
        ..default()
    })
}
