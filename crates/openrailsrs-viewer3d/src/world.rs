//! MSTS world tiles (`.w`) as coloured placeholder boxes (order 5 / issue #8).

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use openrailsrs_formats::{WorldFile, WorldItem};

use crate::shapes::{RouteAssets, load_shape_mesh, resolve_shape_path};
use crate::track::TrackScene;

/// MSTS / Open Rails world tile size (metres).
pub const MSTS_TILE_SIZE_M: f64 = 2048.0;

/// One scenery object from a loaded `.w` tile, ready for 3D spawn.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldObject {
    pub kind: &'static str,
    pub label: String,
    /// Shape filename from the world item (`FileName`), if any.
    pub shape_file: Option<String>,
    pub position: Vec3,
    pub rotation: Quat,
    pub tile_x: i32,
    pub tile_z: i32,
}

/// All world objects discovered under a route's `WORLD/` (or `world/`) folder.
#[derive(Resource, Clone, Default)]
pub struct WorldScene {
    pub tiles_loaded: usize,
    pub items: Vec<WorldObject>,
}

impl WorldScene {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Convert MSTS tile-local coordinates to Bevy world space (Y up).
///
/// Global X/Z follow the usual MSTS rule: `tile * 2048 + local`.
/// MSTS `y` maps to Bevy `Y` (height).
pub fn msts_to_bevy(tile_x: i32, tile_z: i32, local: openrailsrs_formats::Vec3) -> Vec3 {
    Vec3::new(
        (tile_x as f64 * MSTS_TILE_SIZE_M + local.x) as f32,
        local.y as f32,
        (tile_z as f64 * MSTS_TILE_SIZE_M + local.z) as f32,
    )
}

pub fn qdir_to_quat(qdir: &[f64; 4]) -> Quat {
    Quat::from_xyzw(
        qdir[0] as f32,
        qdir[1] as f32,
        qdir[2] as f32,
        qdir[3] as f32,
    )
}

fn object_label(item: &WorldItem) -> String {
    item.file_name()
        .map(str::to_string)
        .unwrap_or_else(|| item.kind().to_string())
}

fn object_from_item(tile_x: i32, tile_z: i32, item: &WorldItem) -> Option<WorldObject> {
    let position = msts_to_bevy(tile_x, tile_z, item.position()?);
    let rotation = match item {
        WorldItem::Static { qdir, .. }
        | WorldItem::Track { qdir, .. }
        | WorldItem::Dyntrack { qdir, .. }
        | WorldItem::Signal { qdir, .. } => {
            qdir.map(|q| qdir_to_quat(&q)).unwrap_or(Quat::IDENTITY)
        }
        _ => Quat::IDENTITY,
    };
    Some(WorldObject {
        kind: item.kind(),
        label: object_label(item),
        shape_file: item.file_name().map(str::to_string),
        position,
        rotation,
        tile_x,
        tile_z,
    })
}

/// Scan `route_dir/WORLD` and `route_dir/world` for `.w` files and parse them.
pub fn load_world_from_route_dir(route_dir: &Path) -> WorldScene {
    let mut paths = discover_world_files(route_dir);
    paths.sort();

    let mut scene = WorldScene::default();
    for path in paths {
        match WorldFile::from_path(&path) {
            Ok(world) => {
                scene.tiles_loaded += 1;
                for item in &world.items {
                    if let Some(obj) = object_from_item(world.tile_x, world.tile_z, item) {
                        scene.items.push(obj);
                    }
                }
            }
            Err(err) => {
                eprintln!(
                    "openrailsrs-viewer3d: skip world {} ({err})",
                    path.display()
                );
            }
        }
    }
    scene
}

fn discover_world_files(route_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for subdir in ["WORLD", "world"] {
        let dir = route_dir.join(subdir);
        if !dir.is_dir() {
            continue;
        }
        let Ok(read_dir) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("w"))
            {
                out.push(path);
            }
        }
    }
    out
}

fn kind_color(kind: &str) -> Color {
    match kind {
        "Static" => Color::srgb(0.6, 0.65, 0.75),
        "Forest" => Color::srgb(0.22, 0.72, 0.28),
        "TrackObj" => Color::srgb(0.78, 0.48, 0.18),
        "Signal" => Color::srgb(1.0, 0.85, 0.2),
        "Dyntrack" => Color::srgb(0.58, 0.32, 0.82),
        _ => Color::srgb(0.45, 0.45, 0.5),
    }
}

fn box_size_for_kind(kind: &str, base: f32) -> Vec3 {
    match kind {
        "Forest" => Vec3::new(base * 1.6, base * 2.4, base * 1.6),
        "Static" => Vec3::new(base * 1.4, base * 1.8, base * 1.4),
        "TrackObj" | "Dyntrack" => Vec3::new(base * 2.4, base * 0.35, base * 0.35),
        _ => Vec3::splat(base),
    }
}

fn shape_eligible(obj: &WorldObject) -> bool {
    obj.shape_file
        .as_ref()
        .is_some_and(|f| f.to_ascii_lowercase().ends_with(".s"))
}

/// Spawn world objects: real meshes for resolvable `.s` shapes, cuboids otherwise.
pub fn spawn_world_boxes(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    world: Res<WorldScene>,
    scene: Res<TrackScene>,
    assets: Res<RouteAssets>,
) {
    if world.is_empty() {
        return;
    }

    let base = scene.bounds.edge_radius().max(2.0) * 1.5;
    let unit = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let mut box_material_cache: std::collections::HashMap<&'static str, Handle<StandardMaterial>> =
        std::collections::HashMap::new();
    let mut shape_mesh_cache: std::collections::HashMap<PathBuf, Handle<Mesh>> =
        std::collections::HashMap::new();
    let mut shape_material_cache: std::collections::HashMap<PathBuf, Handle<StandardMaterial>> =
        std::collections::HashMap::new();

    let shape_material_color = Color::srgb(0.45, 0.72, 0.95);

    for obj in &world.items {
        if shape_eligible(obj) {
            let file_name = obj.shape_file.as_deref().unwrap_or("");
            if let Some(shape_path) = resolve_shape_path(&assets.route_dir, file_name) {
                let mesh = shape_mesh_cache
                    .entry(shape_path.clone())
                    .or_insert_with(|| {
                        if let Some(mesh) = load_shape_mesh(&shape_path) {
                            meshes.add(mesh)
                        } else {
                            eprintln!(
                                "openrailsrs-viewer3d: shape {} failed, using placeholder cube",
                                shape_path.display()
                            );
                            unit.clone()
                        }
                    });
                let material = shape_material_cache
                    .entry(shape_path)
                    .or_insert_with(|| {
                        materials.add(StandardMaterial {
                            base_color: shape_material_color,
                            perceptual_roughness: 0.75,
                            metallic: 0.1,
                            double_sided: true,
                            ..default()
                        })
                    })
                    .clone();

                commands.spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material),
                    Transform {
                        translation: obj.position,
                        rotation: obj.rotation,
                        scale: Vec3::ONE,
                    },
                    Name::new(format!("world:{}:{}", obj.kind, obj.label)),
                ));
                continue;
            }
        }

        let size = box_size_for_kind(obj.kind, base);
        let material = box_material_cache
            .entry(obj.kind)
            .or_insert_with(|| {
                materials.add(StandardMaterial {
                    base_color: kind_color(obj.kind),
                    perceptual_roughness: 0.85,
                    metallic: 0.05,
                    ..default()
                })
            })
            .clone();

        let ground_y = obj.position.y + size.y * 0.5;
        let translation = Vec3::new(obj.position.x, ground_y, obj.position.z);

        commands.spawn((
            Mesh3d(unit.clone()),
            MeshMaterial3d(material),
            Transform {
                translation,
                rotation: obj.rotation,
                scale: size,
            },
            Name::new(format!("world:{}:{}", obj.kind, obj.label)),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::Vec3 as FVec3;

    #[test]
    fn msts_tile_zero_uses_local_coords() {
        let p = msts_to_bevy(
            0,
            0,
            FVec3 {
                x: 100.0,
                y: 5.0,
                z: -3.0,
            },
        );
        assert_eq!(p, Vec3::new(100.0, 5.0, -3.0));
    }

    #[test]
    fn msts_tile_offset_scales_by_2048() {
        let p = msts_to_bevy(
            2,
            1,
            FVec3 {
                x: 10.0,
                y: 0.0,
                z: 20.0,
            },
        );
        assert_eq!(p, Vec3::new(4106.0, 0.0, 2068.0));
    }

    #[test]
    fn load_fixture_world_from_route_dir() {
        let route_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        let scene = load_world_from_route_dir(&route_dir);
        assert_eq!(scene.tiles_loaded, 1);
        assert_eq!(scene.items.len(), 5);
        assert!(scene.items.iter().any(|o| o.kind == "Static"));
        assert!(scene.items.iter().any(|o| o.kind == "Forest"));
    }
}
