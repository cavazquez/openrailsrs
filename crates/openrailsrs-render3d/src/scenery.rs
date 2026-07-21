//! Bosque (`Forest`) y agua horizontal (`HWater`) desde tiles `.w`.

use std::collections::HashMap;
use std::path::Path;

use crate::stream::TileContent;
use bevy::math::{Affine2, Vec2};
use bevy::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use openrailsrs_bevy_scenery::{
    WATER_LIFT_M, WATER_UV_TILES, TreePlacement, scatter_trees_in_patch,
    water_material as shared_water_material, water_reflection_material,
};

use crate::objects::{ForestPatch, ObjectMarker};
use crate::terrain::TileHeight;
use crate::textures::{
    TextureEnvironment, TextureFlags, load_ace_file, texture_search_dirs_for_shape,
};
use crate::world_spawn::{AssetIndex, ObjectSpawnCtx, TextureLoadStats};

const COLOR_TREE_FALLBACK: Color = Color::srgb(0.18, 0.62, 0.22);
/// Velocidad de corriente en UV/s (U = “a lo largo”, V = componente cruzada).
const WATER_FLOW_U: f32 = 0.14;
const WATER_FLOW_V: f32 = 0.045;

/// Marca una superficie `HWater` animada (bob vertical + scroll UV opcional).
#[derive(Component, Clone, Debug)]
pub struct WaterSurface {
    pub base_y: f32,
    pub phase: f32,
    pub is_reflection: bool,
    /// Material con textura `.ace`; se anima `uv_transform` cada frame.
    pub flow_material: Option<Handle<StandardMaterial>>,
}

fn scatter_trees(
    anchor: Vec3,
    patch: &ForestPatch,
    tile_x: i32,
    tile_z: i32,
    height: &TileHeight,
) -> Vec<TreePlacement> {
    scatter_trees_in_patch(
        anchor,
        patch.patch_half_x,
        patch.patch_half_z,
        patch.population,
        patch.scale_min,
        patch.scale_max,
        tile_x,
        tile_z,
        patch.uid,
        |x, z| height.local_y(x, z),
        None,
        0.0,
    )
}

/// Fixed cross-quads for `StandardMaterial` lab path (viewer uses OrForest billboards).
fn build_forest_cross_mesh(trees: &[TreePlacement], base_width: f32, base_height: f32) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    for tree in trees {
        let origin = tree.position;
        let width = base_width * tree.scale;
        let height = base_height * tree.scale;
        let hw = width * 0.5;
        let base = positions.len() as u32;
        positions.push([origin.x - hw, origin.y, origin.z]);
        positions.push([origin.x + hw, origin.y, origin.z]);
        positions.push([origin.x + hw, origin.y + height, origin.z]);
        positions.push([origin.x - hw, origin.y + height, origin.z]);
        for _ in 0..4 {
            normals.push([0.0, 0.0, 1.0]);
        }
        uvs.extend([[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);

        let base2 = positions.len() as u32;
        positions.push([origin.x, origin.y, origin.z - hw]);
        positions.push([origin.x, origin.y, origin.z + hw]);
        positions.push([origin.x, origin.y + height, origin.z + hw]);
        positions.push([origin.x, origin.y + height, origin.z - hw]);
        for _ in 0..4 {
            normals.push([1.0, 0.0, 0.0]);
        }
        uvs.extend([[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
        indices.extend([base2, base2 + 1, base2 + 2, base2, base2 + 2, base2 + 3]);
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

fn forest_material(
    materials: &mut Assets<StandardMaterial>,
    texture: Option<Handle<Image>>,
    cache: &mut HashMap<String, Handle<StandardMaterial>>,
    tex_name: Option<&str>,
    lit: bool,
) -> Handle<StandardMaterial> {
    if let Some(name) = tex_name {
        if let Some(mat) = cache.get(name) {
            return mat.clone();
        }
    }
    let mat = materials.add(StandardMaterial {
        base_color: COLOR_TREE_FALLBACK,
        base_color_texture: texture,
        alpha_mode: AlphaMode::Mask(0.45),
        double_sided: true,
        cull_mode: None,
        unlit: !lit,
        fog_enabled: lit,
        ..default()
    });
    if let Some(name) = tex_name {
        cache.insert(name.to_string(), mat.clone());
    }
    mat
}

fn water_uv_transform(t: f32, phase: f32) -> Affine2 {
    let u = (t * WATER_FLOW_U + phase * 0.11).fract();
    let v = (t * WATER_FLOW_V + phase * 0.07).fract();
    Affine2::from_scale(Vec2::splat(WATER_UV_TILES)) * Affine2::from_translation(Vec2::new(u, v))
}

fn water_material(
    materials: &mut Assets<StandardMaterial>,
    texture: Option<Handle<Image>>,
) -> Handle<StandardMaterial> {
    let handle = shared_water_material(materials, texture);
    if let Some(mut mat) = materials.get_mut(&handle) {
        mat.fog_enabled = true;
        mat.unlit = false;
        mat.uv_transform = Affine2::IDENTITY;
    }
    handle
}

fn reflection_material(materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
    let handle = water_reflection_material(materials);
    if let Some(mut mat) = materials.get_mut(&handle) {
        mat.unlit = false;
    }
    handle
}

/// Ondas suaves en Y + scroll UV en texturas `.ace` (corriente).
pub fn update_water_surfaces(
    time: Res<Time>,
    mut surfaces: Query<(&mut Transform, &WaterSurface)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let t = time.elapsed_secs();
    for (mut transform, surface) in &mut surfaces {
        let amp = if surface.is_reflection { 0.018 } else { 0.07 };
        let wave = (t * 1.65 + surface.phase).sin() * amp;
        transform.translation.y = surface.base_y + wave;

        if surface.is_reflection {
            continue;
        }
        let Some(handle) = &surface.flow_material else {
            continue;
        };
        let Some(mut mat) = materials.get_mut(handle) else {
            continue;
        };
        mat.uv_transform = water_uv_transform(t, surface.phase);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_tile_scenery(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    index: &AssetIndex,
    _ctx: &mut ObjectSpawnCtx,
    route_dir: &Path,
    msts_root: &Path,
    objects: &[ObjectMarker],
    height: &TileHeight,
    tile_x: i32,
    tile_z: i32,
    tile_offset: Vec3,
    tex_stats: &mut TextureLoadStats,
    texture_env: &TextureEnvironment,
    materials_lit: bool,
) -> (usize, usize) {
    let mut forests = 0usize;
    let mut waters = 0usize;
    let mut forest_mat_cache: HashMap<String, Handle<StandardMaterial>> = HashMap::new();
    let reflect_material = reflection_material(materials);

    for obj in objects {
        if let Some(patch) = &obj.forest {
            let trees = scatter_trees(obj.position, patch, tile_x, tile_z, height);
            if trees.is_empty() {
                continue;
            }
            let mesh = meshes.add(build_forest_cross_mesh(
                &trees,
                patch.tree_width,
                patch.tree_height,
            ));
            let texture = patch.tree_texture.as_ref().and_then(|name| {
                let dirs = texture_search_dirs_for_shape(route_dir, route_dir, msts_root);
                let refs: Vec<&Path> = dirs.iter().map(|p| p.as_path()).collect();
                let flags = TextureFlags::from_raw(TextureFlags::FOREST);
                index
                    .resolve_texture(&refs, name, texture_env, flags)
                    .and_then(|path| {
                        load_ace_file(&path)
                            .map(|ace| images.add(crate::textures::ace_to_image(&ace)))
                    })
            });
            if texture.is_some() {
                tex_stats.record_resolved();
            }
            let material = forest_material(
                materials,
                texture,
                &mut forest_mat_cache,
                patch.tree_texture.as_deref(),
                materials_lit,
            );
            commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_translation(tile_offset),
                TileContent { tile_x, tile_z },
                Name::new(format!("forest:{}", patch.uid)),
            ));
            forests += 1;
        }

        if let Some(patch) = &obj.hwater {
            let width = patch.half_x * 2.0;
            let depth = patch.half_z * 2.0;
            if width <= 0.0 || depth <= 0.0 {
                continue;
            }
            let mesh = meshes.add(Plane3d::default().mesh().size(width, depth));
            let texture = patch.texture.as_ref().and_then(|name| {
                let dirs = texture_search_dirs_for_shape(route_dir, route_dir, msts_root);
                let refs: Vec<&Path> = dirs.iter().map(|p| p.as_path()).collect();
                let flags = TextureFlags::from_raw(TextureFlags::NONE);
                index
                    .resolve_texture(&refs, name, texture_env, flags)
                    .and_then(|path| {
                        load_ace_file(&path)
                            .map(|ace| images.add(crate::textures::ace_to_image(&ace)))
                    })
            });
            if texture.is_some() {
                tex_stats.record_resolved();
            }
            let material = water_material(materials, texture.clone());
            let flow_material = texture.map(|_| material.clone());
            let base = obj.position + tile_offset;
            let surface_y = base.y + WATER_LIFT_M;
            let phase = (patch.uid as f32 * 0.73).fract() * std::f32::consts::TAU;
            let mut translation = base;
            translation.y = surface_y;
            commands.spawn((
                WaterSurface {
                    base_y: surface_y,
                    phase,
                    is_reflection: false,
                    flow_material,
                },
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material),
                Transform {
                    translation,
                    rotation: obj.rotation,
                    scale: obj.scale,
                },
                TileContent { tile_x, tile_z },
                Name::new(format!("hwater:{}", patch.uid)),
            ));
            let reflect_y = surface_y - 0.05;
            commands.spawn((
                WaterSurface {
                    base_y: reflect_y,
                    phase: phase + 1.1,
                    is_reflection: true,
                    flow_material: None,
                },
                Mesh3d(mesh),
                MeshMaterial3d(reflect_material.clone()),
                Transform {
                    translation: Vec3::new(base.x, reflect_y, base.z),
                    rotation: obj.rotation * Quat::from_rotation_x(std::f32::consts::PI),
                    scale: obj.scale,
                },
                TileContent { tile_x, tile_z },
                Name::new(format!("hwater-reflect:{}", patch.uid)),
            ));
            waters += 1;
        }
    }

    (forests, waters)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forest_rng_is_deterministic() {
        use openrailsrs_bevy_scenery::forest_rng01;
        let a = forest_rng01(-6131, 14898, 42, 3, 0);
        let b = forest_rng01(-6131, 14898, 42, 3, 0);
        assert!((a - b).abs() < f32::EPSILON);
        assert!((0.0..1.0).contains(&a));
    }

    #[test]
    fn water_wave_oscillates() {
        let phase = 0.0_f32;
        let a = (0.5 * 1.65 + phase).sin() * 0.07;
        let b = (1.0 * 1.65 + phase).sin() * 0.07;
        assert_ne!(a, b);
    }

    #[test]
    fn water_uv_scroll_changes_over_time() {
        let a = water_uv_transform(0.0, 0.5);
        let b = water_uv_transform(2.5, 0.5);
        assert_ne!(a.translation, b.translation);
    }
}
