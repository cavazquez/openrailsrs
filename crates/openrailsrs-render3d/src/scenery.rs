//! Bosque (`Forest`) y agua horizontal (`HWater`) desde tiles `.w`.

use std::collections::HashMap;
use std::path::Path;

use crate::stream::TileContent;
use bevy::asset::RenderAssetUsages;
use bevy::math::{Affine2, Vec2};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::objects::{ForestPatch, ObjectMarker};
use crate::terrain::TileHeight;
use crate::textures::{
    TextureEnvironment, TextureFlags, load_ace_file, texture_search_dirs_for_shape,
};
use crate::world_spawn::{AssetIndex, ObjectSpawnCtx, TextureLoadStats};

const COLOR_TREE_FALLBACK: Color = Color::srgb(0.18, 0.62, 0.22);
const COLOR_WATER: Color = Color::srgba(0.08, 0.38, 0.62, 0.68);
const COLOR_WATER_REFLECT: Color = Color::srgba(0.04, 0.22, 0.38, 0.28);
const WATER_LIFT_M: f32 = 0.08;
/// Repeticiones de la textura `.ace` sobre el plano de agua.
const WATER_UV_TILES: f32 = 3.0;
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

#[derive(Clone, Copy, Debug)]
struct TreePlacement {
    position: Vec3,
    scale: f32,
}

pub fn forest_rng01(tile_x: i32, tile_z: i32, uid: u32, tree_index: u32, channel: u32) -> f32 {
    let mut x = (tile_x as u32)
        ^ (tile_z as u32).rotate_left(7)
        ^ uid.rotate_left(13)
        ^ tree_index.rotate_left(3)
        ^ channel.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 16;
    (x as f32) / (u32::MAX as f32)
}

fn scatter_trees(
    anchor: Vec3,
    patch: &ForestPatch,
    tile_x: i32,
    tile_z: i32,
    height: &TileHeight,
) -> Vec<TreePlacement> {
    let mut trees = Vec::with_capacity(patch.population as usize);
    for i in 0..patch.population {
        let ch = 0;
        let rx = forest_rng01(tile_x, tile_z, patch.uid, i, ch) * 2.0 - 1.0;
        let rz = forest_rng01(tile_x, tile_z, patch.uid, i, ch + 1) * 2.0 - 1.0;
        let x = anchor.x + rx * patch.patch_half_x;
        let z = anchor.z + rz * patch.patch_half_z;
        let t = forest_rng01(tile_x, tile_z, patch.uid, i, ch + 2);
        let scale = patch.scale_min + (patch.scale_max - patch.scale_min) * t;
        let y = height.local_y(x, z);
        trees.push(TreePlacement {
            position: Vec3::new(x, y, z),
            scale,
        });
    }
    trees
}

fn append_tree_cross(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
    origin: Vec3,
    width: f32,
    height: f32,
) {
    let base = positions.len() as u32;
    let hw = width * 0.5;
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

fn build_forest_mesh(trees: &[TreePlacement], base_width: f32, base_height: f32) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    for tree in trees {
        append_tree_cross(
            &mut positions,
            &mut normals,
            &mut uvs,
            &mut indices,
            tree.position,
            base_width * tree.scale,
            base_height * tree.scale,
        );
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
    materials.add(StandardMaterial {
        base_color: COLOR_WATER,
        base_color_texture: texture,
        emissive: LinearRgba::from(Color::srgb(0.08, 0.24, 0.42)) * 0.45,
        perceptual_roughness: 0.06,
        metallic: 0.05,
        reflectance: 0.75,
        alpha_mode: AlphaMode::Blend,
        double_sided: true,
        unlit: false,
        fog_enabled: true,
        uv_transform: Affine2::IDENTITY,
        ..default()
    })
}

fn reflection_material(materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: COLOR_WATER_REFLECT,
        emissive: LinearRgba::from(Color::srgb(0.05, 0.16, 0.28)) * 0.2,
        perceptual_roughness: 0.02,
        reflectance: 0.85,
        alpha_mode: AlphaMode::Blend,
        double_sided: true,
        unlit: false,
        ..default()
    })
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
            let mesh = meshes.add(build_forest_mesh(
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
