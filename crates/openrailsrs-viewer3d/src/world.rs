//! MSTS world tiles (`.w`) as coloured placeholder boxes (order 5 / issue #8).

use std::path::{Path, PathBuf};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use openrailsrs_formats::{WorldFile, WorldItem};

use crate::shapes::{
    RouteAssets, ShapeRenderAsset, load_shape_render_asset_from_path, resolve_shape_path_in_dirs,
    shape_search_dirs,
};
use crate::terrain::TerrainElevation;
use crate::track::TrackScene;

/// MSTS / Open Rails world tile size (metres).
pub const MSTS_TILE_SIZE_M: f64 = 2048.0;

/// Maximum distance (m) from the route centre at which world objects are spawned.
/// Objects beyond this radius are skipped to keep draw call count manageable on
/// large imported routes.
pub const VISIBLE_RADIUS_M: f32 = 8000.0;

/// Shapes closer than this use the highest LOD; farther shapes use coarser LOD.
pub const SHAPE_LOD_DISTANCE_M: f32 = 2000.0;

/// Within this radius, spawn real `.s` meshes when the file resolves; beyond it, placeholders only.
pub const SHAPE_MESH_RADIUS_M: f32 = SHAPE_LOD_DISTANCE_M;

/// Forest patch metadata from a `.w` `Forest` item.
#[derive(Clone, Debug, PartialEq)]
pub struct ForestPatch {
    pub uid: u32,
    pub tree_texture: Option<String>,
    pub scale_min: f32,
    pub scale_max: f32,
    pub population: u32,
    /// Half-width of scatter patch in metres (`Area` / 2, or 0 → viewer default).
    pub patch_half_x: f32,
    pub patch_half_z: f32,
    /// Base billboard width in metres from `TreeSize`.
    pub tree_width: f32,
    /// Base billboard height in metres from `TreeSize`.
    pub tree_height: f32,
}

/// Horizontal water metadata from a `.w` `HWater` item.
#[derive(Clone, Debug, PartialEq)]
pub struct WaterPatch {
    pub uid: u32,
    pub half_x: f32,
    pub half_z: f32,
    pub surface_y: f32,
    pub texture_file: Option<String>,
}

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
    pub forest: Option<ForestPatch>,
    pub water: Option<WaterPatch>,
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

    /// World-space centre of loaded scenery (for culling / terrain when the track graph has no `x_m`/`y_m`).
    pub fn position_center(&self) -> Option<Vec3> {
        if self.items.is_empty() {
            return None;
        }
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        for obj in &self.items {
            min = min.min(obj.position);
            max = max.max(obj.position);
        }
        Some((min + max) * 0.5)
    }
}

/// View/cull centre: MSTS world bbox when present, else track graph centre.
#[derive(Resource, Clone, Copy, Debug)]
pub struct RouteFocus {
    pub center: Vec3,
    /// Terrain MSL (metres) at route centre; use with [`Self::to_render_surface`] only.
    pub height_origin: f32,
}

impl RouteFocus {
    pub fn from_scene_and_world(scene: &TrackScene, world: &WorldScene) -> Self {
        Self::from_scene_world_and_elevation(scene, world, None)
    }

    pub fn from_scene_world_and_elevation(
        scene: &TrackScene,
        world: &WorldScene,
        elevation: Option<&TerrainElevation>,
    ) -> Self {
        let center = world.position_center().unwrap_or(scene.bounds.center);
        let height_origin = elevation
            .and_then(|t| t.sample_world_y(center.x, center.z))
            .unwrap_or(center.y);
        Self {
            center,
            height_origin,
        }
    }

    /// General world-space position to Bevy render space using the scenery bbox centre.
    /// For Y this subtracts `center.y`; prefer [`Self::to_render_surface`] (uses
    /// `height_origin`) for consistent height with terrain tiles.
    pub fn to_render(&self, world: Vec3) -> Vec3 {
        Vec3::new(
            world.x - self.center.x,
            world.y - self.center.y,
            world.z - self.center.z,
        )
    }

    /// World points on the terrain surface (MSL from `_Y.RAW` / [`crate::terrain::ground_y_at`]).
    pub fn to_render_surface(&self, world: Vec3) -> Vec3 {
        Vec3::new(
            world.x - self.center.x,
            world.y - self.height_origin,
            world.z - self.center.z,
        )
    }

    /// Convert `.w` tile-local Y to terrain MSL for [`Self::to_render_surface`].
    pub fn scenery_y_to_msl(&self, scenery_y: f32) -> f32 {
        self.height_origin + (scenery_y - self.center.y)
    }

    /// Horizontal distance from route centre in render space (for culling).
    pub fn horizontal_distance(&self, world: Vec3) -> f32 {
        let local = self.to_render(world);
        Vec2::new(local.x, local.z).length()
    }
}

/// Whether a world object should be culled for being outside [`VISIBLE_RADIUS_M`].
#[inline]
pub fn should_cull_world_object(focus: &RouteFocus, world: Vec3) -> bool {
    focus.horizontal_distance(world) > VISIBLE_RADIUS_M
}

/// Translates abstract graph coordinates into MSTS world space when the two diverge.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct RouteWorldOffset {
    pub delta: Vec3,
}

impl RouteWorldOffset {
    pub fn from_scene_and_world(scene: &TrackScene, world: &WorldScene) -> Self {
        let graph_center = scene.bounds.center;
        let Some(world_center) = world.position_center() else {
            return Self::default();
        };
        if (world_center - graph_center).length() <= 2_000.0 {
            return Self::default();
        }
        let delta = world_center - graph_center;
        eprintln!(
            "openrailsrs-viewer3d: aligning track/train to MSTS scenery (offset {:.0}, {:.0}, {:.0} m)",
            delta.x, delta.y, delta.z
        );
        Self { delta }
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
        | WorldItem::Signal { qdir, .. }
        | WorldItem::Other { qdir, .. } => qdir.map(|q| qdir_to_quat(&q)).unwrap_or(Quat::IDENTITY),
        _ => Quat::IDENTITY,
    };
    let forest = match item {
        WorldItem::Forest {
            uid,
            tree_texture,
            scale_range,
            patch_size,
            tree_size,
            population,
            ..
        } => {
            let (scale_min, scale_max) = scale_range
                .map(|r| (r[0] as f32, r[1] as f32))
                .unwrap_or((0.8, 1.2));
            let (patch_half_x, patch_half_z) = patch_size
                .map(|a| ((a[0] * 0.5) as f32, (a[1] * 0.5) as f32))
                .unwrap_or((0.0, 0.0));
            let (tree_width, tree_height) = tree_size
                .map(|s| (s[0] as f32, s[1] as f32))
                .unwrap_or((0.0, 0.0));
            Some(ForestPatch {
                uid: *uid,
                tree_texture: tree_texture.clone(),
                scale_min,
                scale_max,
                population: *population,
                patch_half_x,
                patch_half_z,
                tree_width,
                tree_height,
            })
        }
        _ => None,
    };
    let water = match item {
        WorldItem::HWater {
            uid,
            file_name,
            position,
            size,
            ..
        } => Some(WaterPatch {
            uid: *uid,
            half_x: (size[0] * 0.5) as f32,
            half_z: (size[1] * 0.5) as f32,
            surface_y: position.y as f32,
            texture_file: file_name.clone(),
        }),
        _ => None,
    };
    Some(WorldObject {
        kind: item.kind(),
        label: object_label(item),
        shape_file: item.file_name().map(str::to_string),
        position,
        rotation,
        tile_x,
        tile_z,
        forest,
        water,
    })
}

/// Scan `route_dir/WORLD` and `route_dir/world` for `.w` files and parse them.
pub fn load_world_from_route_dir(route_dir: &Path) -> WorldScene {
    let mut paths = discover_world_files(route_dir);
    paths.sort();

    let mut scene = WorldScene::default();
    let mut skip_count = 0usize;
    let mut skip_sample: Option<String> = None;
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
                skip_count += 1;
                if skip_sample.is_none() {
                    skip_sample = Some(format!("{} ({err})", path.display()));
                }
            }
        }
    }
    if skip_count > 0 {
        eprintln!(
            "openrailsrs-viewer3d: skipped {skip_count} world tile(s){}",
            skip_sample
                .as_ref()
                .map(|s| format!(" (e.g. {s})"))
                .unwrap_or_default()
        );
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

struct MergedBoxGroup {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
    color: Color,
}

fn push_cuboid(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
    tf: &Transform,
    size: Vec3,
) {
    let hx = size.x * 0.5;
    let hy = size.y * 0.5;
    let hz = size.z * 0.5;
    let local = [
        Vec3::new(-hx, -hy, -hz),
        Vec3::new(hx, -hy, -hz),
        Vec3::new(hx, hy, -hz),
        Vec3::new(-hx, hy, -hz),
        Vec3::new(-hx, -hy, hz),
        Vec3::new(hx, -hy, hz),
        Vec3::new(hx, hy, hz),
        Vec3::new(-hx, hy, hz),
    ];
    let world: [Vec3; 8] = local.map(|c| tf.transform_point(c));
    let faces: [(usize, usize, usize, usize, Vec3); 6] = [
        (4, 5, 6, 7, Vec3::new(0.0, 0.0, 1.0)),
        (1, 0, 3, 2, Vec3::new(0.0, 0.0, -1.0)),
        (3, 7, 6, 2, Vec3::new(0.0, 1.0, 0.0)),
        (0, 1, 5, 4, Vec3::new(0.0, -1.0, 0.0)),
        (1, 2, 6, 5, Vec3::new(1.0, 0.0, 0.0)),
        (0, 4, 7, 3, Vec3::new(-1.0, 0.0, 0.0)),
    ];
    let base = positions.len() as u32;
    for (v0, v1, v2, v3, normal) in &faces {
        let wn = tf.rotation * *normal;
        let wn_arr = [wn.x, wn.y, wn.z];
        positions.push(world[*v0].to_array());
        positions.push(world[*v1].to_array());
        positions.push(world[*v2].to_array());
        positions.push(world[*v3].to_array());
        for _ in 0..4 {
            normals.push(wn_arr);
        }
        uvs.push([0.0, 0.0]);
        uvs.push([1.0, 0.0]);
        uvs.push([1.0, 1.0]);
        uvs.push([0.0, 1.0]);
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// Spawn world objects: real meshes for resolvable `.s` shapes, cuboids otherwise.
#[allow(clippy::too_many_arguments)]
pub fn spawn_world_boxes(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    world: Res<WorldScene>,
    focus: Res<RouteFocus>,
    scene: Res<TrackScene>,
    assets: Res<RouteAssets>,
) {
    if world.is_empty() {
        return;
    }

    let shape_dirs: Vec<PathBuf> = shape_search_dirs(&assets.route_dir);
    let shape_dir_refs: Vec<&Path> = shape_dirs.iter().map(|p| p.as_path()).collect();
    let base = scene.bounds.edge_radius().max(2.0) * 1.5;
    let mut shape_cache: std::collections::HashMap<PathBuf, ShapeRenderAsset> =
        std::collections::HashMap::new();
    let mut texture_image_cache: std::collections::HashMap<PathBuf, Handle<Image>> =
        std::collections::HashMap::new();

    let mut merged_boxes: std::collections::HashMap<&str, MergedBoxGroup> =
        std::collections::HashMap::new();

    let shape_fallback_color = Color::srgb(0.95, 0.25, 0.85);
    let shape_fallback_material = materials.add(StandardMaterial {
        base_color: shape_fallback_color,
        emissive: LinearRgba::from(shape_fallback_color) * 0.35,
        perceptual_roughness: 0.75,
        metallic: 0.1,
        double_sided: true,
        ..default()
    });

    let mut shape_mesh_count = 0usize;
    let mut shape_texture_count = 0usize;
    let mut culled_count = 0usize;

    for obj in &world.items {
        if obj.kind == "Dyntrack" || obj.kind == "Forest" || obj.kind == "HWater" {
            continue;
        }

        if should_cull_world_object(&focus, obj.position) {
            culled_count += 1;
            continue;
        }

        let dist = focus.horizontal_distance(obj.position);
        let lod_distance = if dist > SHAPE_LOD_DISTANCE_M {
            Some(dist)
        } else {
            None
        };

        if shape_eligible(obj) && dist <= SHAPE_MESH_RADIUS_M {
            let file_name = obj.shape_file.as_deref().unwrap_or("");
            if let Some(shape_path) = resolve_shape_path_in_dirs(&shape_dir_refs, file_name) {
                let asset = shape_cache
                    .entry(shape_path.clone())
                    .or_insert_with(|| {
                        load_shape_render_asset_from_path(
                            &shape_path,
                            &[assets.route_dir.as_path()],
                            lod_distance,
                            &mut meshes,
                            &mut images,
                            &mut materials,
                            &mut texture_image_cache,
                            shape_fallback_color,
                        )
                        .unwrap_or_else(|| {
                            eprintln!(
                                "openrailsrs-viewer3d: shape {} failed, using placeholder cube",
                                shape_path.display()
                            );
                            let unit: Handle<Mesh> = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
                            ShapeRenderAsset {
                                combined_mesh: unit.clone(),
                                parts: vec![crate::shapes::ShapePartAsset {
                                    prim_state_idx: -1,
                                    mesh: unit,
                                    material: shape_fallback_material.clone(),
                                    has_texture: false,
                                    is_transparent: false,
                                }],
                                has_texture: false,
                            }
                        })
                    })
                    .clone();
                if asset.has_texture {
                    shape_texture_count += 1;
                }

                // Use the .w file Y directly — it is already the object's world-space
                // height (≈ MSL). Open Rails does the same: it uses Location.Location.Y
                // straight from the .w file without any terrain lookup. Sampling terrain
                // and ignoring Position.Y caused objects to appear at the wrong elevation
                // (floating or sunken) whenever the terrain height differed from the .w Y.
                let render_pos = focus.to_render_surface(Vec3::new(
                    obj.position.x,
                    obj.position.y,
                    obj.position.z,
                ));
                commands
                    .spawn((
                        Transform {
                            translation: render_pos,
                            rotation: obj.rotation,
                            scale: Vec3::ONE,
                        },
                        Visibility::default(),
                        Name::new(format!("world:{}:{}", obj.kind, obj.label)),
                    ))
                    .with_children(|parent| {
                        for (pi, part) in asset.parts.iter().enumerate() {
                            parent.spawn((
                                Mesh3d(part.mesh.clone()),
                                MeshMaterial3d(part.material.clone()),
                                Transform::default(),
                                Name::new(format!(
                                    "world:{}:{}:part:{pi}:{}",
                                    obj.kind, obj.label, part.prim_state_idx
                                )),
                            ));
                        }
                    });
                shape_mesh_count += asset.parts.len();
                continue;
            }
        }

        let size = box_size_for_kind(obj.kind, base);
        // Position.Y from the .w file is the object's base height in world space (≈ MSL).
        // Add half the placeholder-box height to get its visual centre.
        let translation = focus.to_render_surface(Vec3::new(
            obj.position.x,
            obj.position.y + size.y * 0.5,
            obj.position.z,
        ));
        let tf = Transform {
            translation,
            rotation: obj.rotation,
            scale: size,
        };
        let kind_entry = merged_boxes.entry(obj.kind).or_insert_with(|| {
            let color = kind_color(obj.kind);
            MergedBoxGroup {
                positions: Vec::new(),
                normals: Vec::new(),
                uvs: Vec::new(),
                indices: Vec::new(),
                color,
            }
        });
        push_cuboid(
            &mut kind_entry.positions,
            &mut kind_entry.normals,
            &mut kind_entry.uvs,
            &mut kind_entry.indices,
            &tf,
            size,
        );
    }

    for (kind, group) in &merged_boxes {
        let material = materials.add(StandardMaterial {
            base_color: group.color,
            perceptual_roughness: 0.85,
            metallic: 0.05,
            ..default()
        });
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, group.positions.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, group.normals.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, group.uvs.clone());
        mesh.insert_indices(Indices::U32(group.indices.clone()));
        let count = group.indices.len() / 6;
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material),
            Transform::IDENTITY,
            Name::new(format!("world-boxes:{}", kind)),
        ));
        eprintln!(
            "openrailsrs-viewer3d: merged {count} {} placeholder(s)",
            kind
        );
    }

    if culled_count > 0 {
        eprintln!(
            "openrailsrs-viewer3d: {culled_count} world object(s) culled (>{VISIBLE_RADIUS_M:.0}m from centre)"
        );
    }
    if shape_mesh_count > 0 {
        eprintln!("openrailsrs-viewer3d: {shape_mesh_count} world object(s) using .s mesh");
    }
    if shape_texture_count > 0 {
        eprintln!("openrailsrs-viewer3d: {shape_texture_count} world object(s) with .ace texture");
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
        assert_eq!(scene.items.len(), 6);
        assert!(scene.items.iter().any(|o| o.kind == "Static"));
        assert!(scene.items.iter().any(|o| o.kind == "Forest"));
        assert!(scene.items.iter().any(|o| o.kind == "HWater"));
    }

    /// Chiltern-like focus: MSTS bbox `y` is tile-local (~80 m) but terrain MSL is ~13 km.
    fn chiltern_like_focus() -> RouteFocus {
        RouteFocus {
            center: Vec3::new(12_494_846.0, 82.0, 30_600_240.0),
            height_origin: 13_184.0,
        }
    }

    #[test]
    fn route_focus_scenery_uses_bbox_y_not_msl() {
        let focus = chiltern_like_focus();
        let obj = Vec3::new(12_494_900.0, 55.0, 30_600_300.0);
        let local = focus.to_render(obj);
        assert!(
            local.y.abs() < 200.0,
            "scenery local y should be O(100 m), got {}",
            local.y
        );
        assert!((local.y - (55.0 - 82.0)).abs() < 1.0);
        assert!(
            local.x.abs() < 500.0 && local.z.abs() < 500.0,
            "horizontal rebasing failed: {:?}",
            local
        );
    }

    #[test]
    fn scenery_y_to_msl_maps_tile_local_to_height_origin() {
        let focus = chiltern_like_focus();
        assert!((focus.scenery_y_to_msl(55.0) - 13_157.0).abs() < 1.0);
        assert!((focus.to_render_surface(Vec3::new(0.0, 13_157.0, 0.0)).y - (-27.0)).abs() < 1.0);
    }

    #[test]
    fn route_focus_surface_uses_height_origin() {
        let focus = chiltern_like_focus();
        let rail = Vec3::new(focus.center.x, 13_190.0, focus.center.z);
        let local = focus.to_render_surface(rail);
        assert!(
            (local.y - 6.0).abs() < 1.0,
            "MSL rail height should be ~0 local, got {}",
            local.y
        );
    }

    #[test]
    fn culling_uses_horizontal_distance_not_msl_y() {
        let focus = chiltern_like_focus();
        let obj = Vec3::new(focus.center.x + 100.0, 55.0, focus.center.z + 80.0);
        assert!(
            !should_cull_world_object(&focus, obj),
            "object 130 m away horizontally must not be culled"
        );
        let wrongly_vertical = Vec3::new(focus.center.x, 13_190.0, focus.center.z);
        assert!(
            !should_cull_world_object(&focus, wrongly_vertical),
            "same xz as centre must not be culled despite MSL y"
        );
    }

    #[test]
    fn using_height_origin_for_scenery_y_would_cull_everything() {
        let focus = chiltern_like_focus();
        let obj = Vec3::new(focus.center.x + 50.0, 55.0, focus.center.z);
        let buggy_y = obj.y - focus.height_origin;
        assert!(
            buggy_y.abs() > 10_000.0,
            "sanity: old bug shifted scenery y by ~-13 km"
        );
        assert!(!should_cull_world_object(&focus, obj));
    }

    #[test]
    fn from_scene_world_and_elevation_prefers_terrain_msl() {
        let route_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        if !route_dir.join("TILES").is_dir() {
            return;
        }
        let world = load_world_from_route_dir(&route_dir);
        let scene = TrackScene::from_graph(openrailsrs_track::TrackGraph::new());
        let elev = TerrainElevation::load_from_route_dir_near(&route_dir, None, f32::MAX);
        let focus_no_elev = RouteFocus::from_scene_world_and_elevation(&scene, &world, None);
        let focus = RouteFocus::from_scene_world_and_elevation(&scene, &world, Some(&elev));

        // With terrain, height_origin should be a terrain sample — a realistic MSL elevation
        // for Chiltern (~50-250 m). Without terrain it falls back to centre.y.
        assert!(
            focus.height_origin > 10.0 && focus.height_origin < 500.0,
            "height_origin should be a realistic Chiltern MSL elevation, got {}",
            focus.height_origin
        );
        // The terrain sample should differ from the bare scenery bbox fallback.
        assert!(
            (focus.height_origin - focus_no_elev.height_origin).abs() > 0.1,
            "with elevation, height_origin ({}) should differ from fallback ({})",
            focus.height_origin,
            focus_no_elev.height_origin
        );
    }
}
