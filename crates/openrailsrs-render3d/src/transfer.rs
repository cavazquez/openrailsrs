//! Transfer MSTS: texturas `.ace` proyectadas sobre el terreno (vallas, taludes, hierba).

use std::collections::HashMap;
use std::path::Path;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::objects::{ObjectKind, ObjectMarker};
use crate::stream::TileContent;
use crate::terrain::TileHeight;
use crate::textures::{
    TextureEnvironment, TextureFlags, load_ace_file, texture_search_dirs_for_shape,
};
use crate::world_spawn::{AssetIndex, TextureLoadStats};

const GRID_M: f32 = 8.0;
/// Open Rails `TransferMaterial.ReferenceAlpha = 10` (0-255).
const TRANSFER_ALPHA_CUTOFF: f32 = 10.0 / 255.0;

/// Malla transfer drapada sobre el relieve (paridad OR `TransferPrimitive`).
pub fn build_transfer_mesh(
    center: Vec3,
    width: f32,
    height: f32,
    inv_rot: Quat,
    height_field: &TileHeight,
) -> Option<Mesh> {
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let radius = (width * width + height * height).sqrt() * 0.5;
    let min_ix = ((center.x - radius) / GRID_M).floor() as i32;
    let max_ix = ((center.x + radius) / GRID_M).ceil() as i32;
    // OR indexa Z en coords MSTS (+Z sur); Bevy usa `-z`.
    let center_msts_z = -center.z;
    let min_iz = ((center_msts_z - radius) / GRID_M).floor() as i32;
    let max_iz = ((center_msts_z + radius) / GRID_M).ceil() as i32;
    if min_ix >= max_ix || min_iz >= max_iz {
        return None;
    }

    let nx = (max_ix - min_ix + 1) as usize;
    let nz = (max_iz - min_iz + 1) as usize;
    let mut positions = Vec::with_capacity(nx * nz);
    let mut normals = Vec::with_capacity(nx * nz);
    let mut uvs = Vec::with_capacity(nx * nz);

    const NORMAL_SAMPLE_M: f32 = 4.0;

    for ix in min_ix..=max_ix {
        for iz in min_iz..=max_iz {
            let wx = ix as f32 * GRID_M;
            let wz = -(iz as f32 * GRID_M);
            let rel_x = wx - center.x;
            let rel_z = wz - center.z;
            let y = height_field.local_y(wx, wz) - center.y;
            positions.push([rel_x, y, rel_z]);

            let y_dx0 = height_field.local_y(wx - NORMAL_SAMPLE_M, wz);
            let y_dx1 = height_field.local_y(wx + NORMAL_SAMPLE_M, wz);
            let y_dz0 = height_field.local_y(wx, wz - NORMAL_SAMPLE_M);
            let y_dz1 = height_field.local_y(wx, wz + NORMAL_SAMPLE_M);
            let n =
                Vec3::new(y_dx0 - y_dx1, 2.0 * NORMAL_SAMPLE_M, y_dz0 - y_dz1).normalize_or_zero();
            normals.push([n.x, n.y, n.z]);

            let tc = inv_rot * Vec3::new(rel_x, 0.0, rel_z);
            uvs.push([tc.x / width + 0.5, tc.z / height + 0.5]);
        }
    }

    let mut indices = Vec::new();
    let cols = nz;
    let dx = (max_ix - min_ix) as usize;
    let dz = (max_iz - min_iz) as usize;
    for x in 0..dx {
        for z in 0..dz {
            let i00 = (x * cols + z) as u32;
            let i10 = ((x + 1) * cols + z) as u32;
            let i01 = (x * cols + z + 1) as u32;
            let i11 = ((x + 1) * cols + z + 1) as u32;
            if (x as i32 + min_ix) & 1 == (z as i32 + min_iz) & 1 {
                indices.extend([i00, i11, i10, i00, i01, i11]);
            } else {
                indices.extend([i01, i11, i10, i01, i00, i10]);
            }
        }
    }

    if indices.is_empty() {
        return None;
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    Some(mesh)
}

fn transfer_material_for_ace(
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    ace: &openrailsrs_ace::AceFile,
    tex_name: &str,
    lit: bool,
) -> Handle<StandardMaterial> {
    let image = images.add(crate::textures::ace_to_image(ace));
    let lower = tex_name.to_ascii_lowercase();
    let tint = if lower.contains("chalk") {
        Color::linear_rgb(0.92, 0.90, 0.86)
    } else if lower.contains("scrub") || lower.contains("grass") {
        Color::linear_rgb(1.05, 1.08, 1.0)
    } else {
        Color::linear_rgb(1.1, 1.1, 1.05)
    };
    materials.add(StandardMaterial {
        base_color: tint,
        base_color_texture: Some(image),
        // OR siempre usa ReferenceAlpha=10 en TransferMaterial.
        alpha_mode: AlphaMode::Mask(TRANSFER_ALPHA_CUTOFF),
        unlit: !lit,
        fog_enabled: lit,
        double_sided: true,
        cull_mode: None,
        depth_bias: 0.0005,
        perceptual_roughness: if lit { 0.88 } else { 1.0 },
        ..default()
    })
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_tile_transfers(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    index: &AssetIndex,
    route_dir: &Path,
    msts_root: &Path,
    objects: &[ObjectMarker],
    height: &TileHeight,
    tile_x: i32,
    tile_z: i32,
    tile_offset: Vec3,
    tex_stats: &mut TextureLoadStats,
    texture_env: &TextureEnvironment,
    lit: bool,
) -> usize {
    let mut count = 0usize;
    let mut mat_cache: HashMap<String, Handle<StandardMaterial>> = HashMap::new();

    for obj in objects {
        if obj.kind != ObjectKind::Transfer {
            continue;
        }
        let Some(patch) = &obj.transfer else {
            continue;
        };
        let Some(tex_name) = &patch.texture else {
            continue;
        };
        let local_center = obj.position;
        let inv_rot = obj.rotation.conjugate();
        let Some(mesh) =
            build_transfer_mesh(local_center, patch.width, patch.height, inv_rot, height)
        else {
            continue;
        };

        let dirs = texture_search_dirs_for_shape(route_dir, route_dir, msts_root);
        let refs: Vec<&Path> = dirs.iter().map(|p| p.as_path()).collect();
        let material = if let Some(cached) = mat_cache.get(tex_name) {
            cached.clone()
        } else {
            let mat = index
                .resolve_texture(&refs, tex_name, texture_env, TextureFlags::from_raw(0))
                .and_then(|path| load_ace_file(&path))
                .map(|ace| {
                    tex_stats.record_resolved();
                    transfer_material_for_ace(materials, images, &ace, tex_name, lit)
                })
                .unwrap_or_else(|| {
                    tex_stats.record_unresolved("transfer", tex_name, route_dir);
                    materials.add(StandardMaterial {
                        base_color: Color::srgb(0.72, 0.70, 0.66),
                        unlit: !lit,
                        double_sided: true,
                        ..default()
                    })
                });
            mat_cache.insert(tex_name.clone(), mat.clone());
            mat
        };

        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material),
            Transform::from_translation(tile_offset + local_center),
            TileContent { tile_x, tile_z },
            Name::new(format!("transfer:{tex_name}")),
        ));
        count += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::load_tile_geometry;

    #[test]
    fn transfer_material_uses_alpha_test_not_blend() {
        use openrailsrs_ace::read_ace;
        use std::path::PathBuf;

        let path = PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join("routes/NewForestRouteV3/Routes/Watersnake/TEXTURES/ChalkCliff.ace");
        if !path.is_file() {
            return;
        }
        let ace = read_ace(&path).expect("chalk ace");
        let mut materials = Assets::<StandardMaterial>::default();
        let mut images = Assets::<Image>::default();
        let mat =
            transfer_material_for_ace(&mut materials, &mut images, &ace, "ChalkCliff.ace", true);
        let m = materials.get(&mat).expect("mat");
        assert!(matches!(m.alpha_mode, AlphaMode::Mask(_)));
    }

    #[test]
    fn transfer_vertices_are_relative_to_center() {
        use bevy::mesh::VertexAttributeValues;
        use std::path::PathBuf;
        let route = std::env::var_os("NEW_FOREST_V3_ROUTE")
            .map(PathBuf::from)
            .or_else(|| {
                let home = std::env::var_os("HOME")?;
                let p = PathBuf::from(home).join("routes/NewForestRouteV3/Routes/Watersnake");
                p.join("world").is_dir().then_some(p)
            });
        let Some(route) = route else {
            return;
        };
        let loaded = load_tile_geometry(&route, -6144, 14900).expect("tile");
        let center = Vec3::new(-322.1, 26.2, -65.1);
        let mesh =
            build_transfer_mesh(center, 30.0, 10.0, Quat::IDENTITY, &loaded.height).expect("mesh");
        let positions = mesh.attribute(Mesh::ATTRIBUTE_POSITION).expect("positions");
        let VertexAttributeValues::Float32x3(positions) = positions else {
            panic!("expected float positions");
        };
        let radius = (30.0f32 * 30.0 + 10.0 * 10.0).sqrt() * 0.5 + GRID_M;
        for p in positions {
            assert!(
                p[0].abs() <= radius && p[2].abs() <= radius,
                "vértice fuera del parche relativo: {p:?}"
            );
            assert!(p[1].abs() < 30.0, "Y relativo demasiado grande: {}", p[1]);
        }
    }

    #[test]
    fn transfer_mesh_has_triangles() {
        use std::path::PathBuf;
        let route = std::env::var_os("NEW_FOREST_V3_ROUTE")
            .map(PathBuf::from)
            .or_else(|| {
                let home = std::env::var_os("HOME")?;
                let p = PathBuf::from(home).join("routes/NewForestRouteV3/Routes/Watersnake");
                p.join("world").is_dir().then_some(p)
            });
        let Some(route) = route else {
            return;
        };
        let loaded = load_tile_geometry(&route, -6144, 14900).expect("tile");
        let mesh = build_transfer_mesh(Vec3::ZERO, 30.0, 10.0, Quat::IDENTITY, &loaded.height);
        assert!(mesh.is_some());
    }

    #[test]
    fn new_forest_transfer_tile_parses() {
        use crate::objects::{ObjectKind, load_objects};
        use std::path::PathBuf;

        let route = std::env::var_os("NEW_FOREST_V3_ROUTE")
            .map(PathBuf::from)
            .or_else(|| {
                let home = std::env::var_os("HOME")?;
                let p = PathBuf::from(home).join("routes/NewForestRouteV3/Routes/Watersnake");
                p.join("world").is_dir().then_some(p)
            });
        let Some(route) = route else {
            return;
        };
        let objs = load_objects(&route, -6144, 14900, 0.0);
        let n = objs
            .iter()
            .filter(|o| o.kind == ObjectKind::Transfer)
            .count();
        assert!(n > 0, "tile NF con transfers deberia parsear >0, got {n}");
    }
}
