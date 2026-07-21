//! MSTS `Transfer` ground decals from `.w` tiles (issue #31).
//!
//! `FileName` is a texture (`.ace`), not a shape. Mesh follows terrain relief
//! with OR-style alpha mask and a small depth bias to reduce z-fighting.

use std::collections::HashMap;

use bevy::prelude::*;
use openrailsrs_bevy_scenery::{
    TRANSFER_ALPHA_CUTOFF, build_transfer_mesh as shared_build_transfer_mesh,
};

use crate::shapes::{RouteAssets, load_ace_image};
use crate::terrain::TerrainElevation;
use crate::viewer_log;
use crate::world::{
    RouteFocus, WorldObject, WorldScene, WorldTileBound, horizontal_distance_xz, visible_radius_m,
};

const COLOR_TRANSFER_FALLBACK: Color = Color::srgb(0.72, 0.70, 0.66);
#[cfg(test)]
const GRID_M: f32 = 8.0;

fn sample_y(terrain: Option<&TerrainElevation>, x: f32, z: f32, fallback: f32) -> f32 {
    terrain
        .and_then(|t| t.sample_world_y(x, z))
        .unwrap_or(fallback)
}

/// Terrain-following transfer mesh (parity with OR `TransferPrimitive` / render3d).
///
/// Thin adapter over [`openrailsrs_bevy_scenery::build_transfer_mesh`] (#116).
pub fn build_transfer_mesh(
    center: Vec3,
    width: f32,
    height: f32,
    inv_rot: Quat,
    terrain: Option<&TerrainElevation>,
) -> Option<Mesh> {
    let fallback = center.y;
    shared_build_transfer_mesh(center, width, height, inv_rot, &|x, z| {
        sample_y(terrain, x, z, fallback)
    })
}

fn transfer_material(
    materials: &mut Assets<StandardMaterial>,
    texture: Option<Handle<Image>>,
    tex_name: &str,
) -> Handle<StandardMaterial> {
    let lower = tex_name.to_ascii_lowercase();
    let tint = if lower.contains("chalk") {
        Color::linear_rgb(0.92, 0.90, 0.86)
    } else if lower.contains("scrub") || lower.contains("grass") {
        Color::linear_rgb(1.05, 1.08, 1.0)
    } else {
        Color::linear_rgb(1.1, 1.1, 1.05)
    };
    materials.add(StandardMaterial {
        base_color: if texture.is_some() {
            tint
        } else {
            COLOR_TRANSFER_FALLBACK
        },
        base_color_texture: texture,
        alpha_mode: AlphaMode::Mask(TRANSFER_ALPHA_CUTOFF),
        double_sided: true,
        cull_mode: None,
        depth_bias: 0.0005,
        perceptual_roughness: 0.88,
        metallic: 0.0,
        ..default()
    })
}

/// Spawn transfer decals for every `Transfer` in the world scene (startup).
#[allow(clippy::too_many_arguments)]
pub fn spawn_transfer_patches(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    world: Res<WorldScene>,
    assets: Res<RouteAssets>,
    terrain: Option<Res<TerrainElevation>>,
    focus: Res<RouteFocus>,
) {
    spawn_transfer_objects(
        &mut commands,
        &mut meshes,
        &mut images,
        &mut materials,
        &world.items,
        terrain.as_deref(),
        &assets,
        &focus,
        None,
    );
}

/// Spawn transfers for a slice of world objects (tile streaming).
///
/// `cull_center`: when set (live view window), distance cull uses that XZ instead of
/// [`RouteFocus::center`] so streamed tiles far from the route anchor still spawn.
#[allow(clippy::too_many_arguments)]
pub fn spawn_transfer_objects(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    items: &[WorldObject],
    terrain: Option<&TerrainElevation>,
    assets: &RouteAssets,
    focus: &RouteFocus,
    cull_center: Option<Vec3>,
) {
    let patches: Vec<_> = items
        .iter()
        .filter(|obj| obj.kind == "Transfer" && obj.transfer.is_some())
        .collect();
    if patches.is_empty() {
        return;
    }

    let mut mat_cache: HashMap<String, (Handle<StandardMaterial>, bool)> = HashMap::new();
    let mut spawned = 0usize;
    let mut textured = 0usize;
    let cull_at = cull_center.unwrap_or(focus.center);

    for obj in patches {
        if horizontal_distance_xz(cull_at, obj.position) > visible_radius_m() {
            continue;
        }
        let patch = obj.transfer.as_ref().expect("filtered");
        let center_y = sample_y(
            terrain,
            obj.position.x,
            obj.position.z,
            focus.scenery_y_to_msl(obj.position.y),
        );
        let center = Vec3::new(obj.position.x, center_y, obj.position.z);
        let inv_rot = obj.rotation.conjugate();
        let Some(mesh) = build_transfer_mesh(center, patch.width, patch.height, inv_rot, terrain)
        else {
            continue;
        };

        let tex_key = patch.texture.as_deref().unwrap_or("").to_string();
        let material = if tex_key.is_empty() {
            transfer_material(materials, None, "")
        } else if let Some((cached, has_tex)) = mat_cache.get(&tex_key) {
            if *has_tex {
                textured += 1;
            }
            cached.clone()
        } else {
            let texture =
                load_ace_image(&assets.route_dir, &tex_key).map(|image| images.add(image));
            let has_tex = texture.is_some();
            if has_tex {
                textured += 1;
            } else {
                viewer_log!("openrailsrs-viewer3d: transfer texture unresolved: {tex_key}");
            }
            let mat = transfer_material(materials, texture, &tex_key);
            mat_cache.insert(tex_key.clone(), (mat.clone(), has_tex));
            mat
        };

        let render = focus.to_render_surface(center);
        commands.spawn((
            WorldTileBound {
                tile_x: obj.tile_x,
                tile_z: obj.tile_z,
            },
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material),
            Transform::from_translation(render),
            Name::new(format!("transfer:{}:{}", obj.label, patch.uid)),
        ));
        spawned += 1;
    }

    if spawned > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: {spawned} transfer patch(es){}",
            if textured > 0 {
                format!(" ({textured} textured)")
            } else {
                String::new()
            }
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::shapes::resolve_texture_path;
    use crate::terrain::TerrainElevation;
    use crate::world::load_world_from_route_dir_near;

    fn chiltern_route() -> Option<PathBuf> {
        std::env::var_os("CHILTERN_ROUTE")
            .map(PathBuf::from)
            .or_else(|| {
                let home = std::env::var_os("HOME")?;
                let p = PathBuf::from(home)
                    .join("Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern");
                p.join("WORLD").is_dir().then_some(p)
            })
    }

    #[test]
    fn transfer_mesh_has_triangles_without_terrain() {
        let mesh = build_transfer_mesh(Vec3::ZERO, 30.0, 10.0, Quat::IDENTITY, None);
        assert!(mesh.is_some());
        let mesh = mesh.expect("mesh");
        assert!(mesh.indices().is_some());
    }

    #[test]
    fn transfer_vertices_are_relative_to_center() {
        use bevy::mesh::VertexAttributeValues;

        let center = Vec3::new(100.0, 20.0, -50.0);
        let mesh = build_transfer_mesh(center, 30.0, 10.0, Quat::IDENTITY, None).expect("mesh");
        let positions = mesh.attribute(Mesh::ATTRIBUTE_POSITION).expect("positions");
        let VertexAttributeValues::Float32x3(positions) = positions else {
            panic!("expected float positions");
        };
        let radius = (30.0f32 * 30.0 + 10.0 * 10.0).sqrt() * 0.5 + GRID_M;
        for p in positions {
            assert!(
                p[0].abs() <= radius && p[2].abs() <= radius,
                "vertex outside relative patch: {p:?}"
            );
            assert!(
                p[1].abs() < 1e-3,
                "flat fallback Y should be ~0, got {}",
                p[1]
            );
        }
    }

    #[test]
    fn transfer_material_uses_alpha_mask() {
        let mut materials = Assets::<StandardMaterial>::default();
        let handle = transfer_material(&mut materials, None, "ChalkCliff.ace");
        let m = materials.get(&handle).expect("mat");
        assert!(matches!(m.alpha_mode, AlphaMode::Mask(_)));
        assert!((m.depth_bias - 0.0005).abs() < 1e-6);
    }

    #[test]
    fn chiltern_fixture_tile_five_transfers_build_meshes() {
        let Some(route) = chiltern_route() else {
            return;
        };
        // Fixture tile from issue #31: w-006084+014930.w → 5 Transfer (stream.ace).
        let (ox, oz) = openrailsrs_formats::msts_tile_world_origin(-6084, 14930);
        let center = Vec3::new(ox + 1024.0, 0.0, oz + 1024.0);
        let elev = TerrainElevation::load_from_route_dir_near(&route, Some(center), 3000.0);
        let scene = load_world_from_route_dir_near(&route, Some(center), 50.0);
        let transfers: Vec<_> = scene
            .items
            .iter()
            .filter(|o| o.kind == "Transfer" && o.tile_x == -6084 && o.tile_z == 14930)
            .collect();
        assert_eq!(
            transfers.len(),
            5,
            "expected 5 Transfer on fixture tile, got {}",
            transfers.len()
        );
        assert!(
            resolve_texture_path(&route, "stream.ace").is_some(),
            "stream.ace must resolve under Chiltern TEXTURES"
        );
        for obj in &transfers {
            let patch = obj.transfer.as_ref().expect("transfer meta");
            assert_eq!(patch.texture.as_deref(), Some("stream.ace"));
            let center_y = elev
                .sample_world_y(obj.position.x, obj.position.z)
                .unwrap_or(obj.position.y);
            let center = Vec3::new(obj.position.x, center_y, obj.position.z);
            let mesh = build_transfer_mesh(
                center,
                patch.width,
                patch.height,
                obj.rotation.conjugate(),
                Some(&elev),
            );
            assert!(
                mesh.is_some(),
                "transfer uid {:?} should build a mesh",
                obj.uid
            );
        }
    }
}
