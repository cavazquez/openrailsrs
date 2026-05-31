//! MSTS `Forest` patches from `.w` tiles (order 11 / issue #8).
//!
//! Each forest anchor spawns a population of cross-billboard trees with RNG
//! seeded from tile + uid. Trees sample terrain height and avoid track centrelines.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;
use std::time::Instant;

use crate::shapes::load_ace_image;
use crate::terrain::TerrainElevation;
use crate::track::{SceneBounds, TrackScene, TrackSegmentIndex, forest_track_clearance_m};
use crate::world::{RouteFocus, RouteWorldOffset, WorldObject, WorldScene};
use crate::{log_step, viewer_log};

const COLOR_TREE_FALLBACK: Color = Color::srgb(0.18, 0.62, 0.22);
const MAX_SCATTER_ATTEMPTS: u32 = 12;
const DEFAULT_TREE_WIDTH_M: f32 = 5.0;
const DEFAULT_TREE_HEIGHT_M: f32 = 12.0;
const DEFAULT_PATCH_HALF_M: f32 = 128.0;

/// Tree height/width baseline in metres.
pub fn forest_tree_size(width: f32, height: f32) -> (f32, f32) {
    let w = if width > 0.0 {
        width.clamp(0.5, 50.0)
    } else {
        DEFAULT_TREE_WIDTH_M
    };
    let h = if height > 0.0 {
        height.clamp(1.0, 80.0)
    } else {
        DEFAULT_TREE_HEIGHT_M
    };
    (w, h)
}

/// Default half-extent of a forest patch when `.w` has no `Area`.
pub fn default_patch_half(bounds: &SceneBounds) -> f32 {
    DEFAULT_PATCH_HALF_M.min(bounds.ground_half().max(DEFAULT_PATCH_HALF_M))
}

/// Deterministic [0, 1) sample for tree placement (Open Rails-style seeded RNG).
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

/// One tree placement in world space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TreePlacement {
    pub position: Vec3,
    pub scale: f32,
}

/// Scatter trees inside a rectangular patch around `anchor`.
#[allow(clippy::too_many_arguments)]
pub fn scatter_trees_in_patch(
    anchor: Vec3,
    patch_half_x: f32,
    patch_half_z: f32,
    population: u32,
    scale_min: f32,
    scale_max: f32,
    tile_x: i32,
    tile_z: i32,
    uid: u32,
    track_index: &TrackSegmentIndex,
    terrain: Option<&TerrainElevation>,
    track_clearance_m: f32,
    focus: &RouteFocus,
) -> Vec<TreePlacement> {
    let mut trees = Vec::with_capacity(population as usize);
    for i in 0..population {
        let mut placed = None;
        for attempt in 0..MAX_SCATTER_ATTEMPTS {
            let ch = attempt * 4;
            let rx = forest_rng01(tile_x, tile_z, uid, i, ch) * 2.0 - 1.0;
            let rz = forest_rng01(tile_x, tile_z, uid, i, ch + 1) * 2.0 - 1.0;
            let x = anchor.x + rx * patch_half_x;
            let z = anchor.z + rz * patch_half_z;
            let clearance = if attempt + 1 == MAX_SCATTER_ATTEMPTS {
                0.0
            } else {
                track_clearance_m
            };
            if clearance > 0.0 && track_index.min_distance_xz(x, z, clearance) < clearance {
                continue;
            }
            let t = forest_rng01(tile_x, tile_z, uid, i, ch + 2);
            let scale = scale_min + (scale_max - scale_min) * t;
            let y = terrain
                .and_then(|elev| elev.sample_world_y(x, z))
                .unwrap_or_else(|| focus.scenery_y_to_msl(anchor.y));
            placed = Some(TreePlacement {
                position: Vec3::new(x, y, z),
                scale,
            });
            break;
        }
        if let Some(tree) = placed {
            trees.push(tree);
        }
    }
    trees
}

/// Append a vertical cross billboard (two quads) centred at `origin` with given size.
pub fn append_tree_cross(
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

    // Quad in X–Y plane (normal +Z).
    positions.push([origin.x - hw, origin.y, origin.z]);
    positions.push([origin.x + hw, origin.y, origin.z]);
    positions.push([origin.x + hw, origin.y + height, origin.z]);
    positions.push([origin.x - hw, origin.y + height, origin.z]);
    for _ in 0..4 {
        normals.push([0.0, 0.0, 1.0]);
    }
    uvs.extend([[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
    indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);

    // Quad in Z–Y plane (normal +X).
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

/// Merge cross-billboards for all trees into one mesh.
pub fn build_forest_patch_mesh(trees: &[TreePlacement], base_width: f32, base_height: f32) -> Mesh {
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
    mesh.insert_indices(bevy::mesh::Indices::U32(indices));
    mesh
}

/// Spawn cross-billboard tree patches for every `Forest` in the world scene.
#[allow(clippy::too_many_arguments)]
pub fn spawn_forest_patches(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    world: Res<WorldScene>,
    track: Res<TrackScene>,
    terrain: Option<Res<TerrainElevation>>,
    assets: Res<crate::shapes::RouteAssets>,
    focus: Res<crate::world::RouteFocus>,
    offset: Res<RouteWorldOffset>,
) {
    viewer_log!(
        "openrailsrs-viewer3d: spawning forest patches ({} anchor(s))",
        world
            .items
            .iter()
            .filter(|obj| obj.kind == "Forest" && obj.forest.is_some())
            .count()
    );
    spawn_forest_objects(
        &mut commands,
        &mut meshes,
        &mut images,
        &mut materials,
        &world.items,
        &track,
        terrain.as_deref(),
        &assets,
        &focus,
        &offset,
    );
}

/// Spawn forest patches for a slice of world objects (tile streaming).
#[allow(clippy::too_many_arguments)]
pub fn spawn_forest_objects(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    items: &[WorldObject],
    track: &TrackScene,
    terrain: Option<&TerrainElevation>,
    assets: &crate::shapes::RouteAssets,
    focus: &RouteFocus,
    offset: &RouteWorldOffset,
) {
    let spawn_start = Instant::now();
    let forests: Vec<_> = items
        .iter()
        .filter(|obj| obj.kind == "Forest" && obj.forest.is_some())
        .collect();
    if forests.is_empty() {
        return;
    }

    let default_half = default_patch_half(&track.bounds);
    let track_clearance = forest_track_clearance_m(&track.bounds);
    let track_index = TrackSegmentIndex::from_graph(&track.graph, offset.delta);
    let mut material_cache: std::collections::HashMap<String, Handle<StandardMaterial>> =
        std::collections::HashMap::new();

    let fallback_material = materials.add(StandardMaterial {
        base_color: COLOR_TREE_FALLBACK,
        emissive: LinearRgba::from(COLOR_TREE_FALLBACK) * 0.15,
        perceptual_roughness: 0.95,
        metallic: 0.0,
        double_sided: true,
        alpha_mode: AlphaMode::Mask(0.5),
        ..default()
    });

    let patch_count = forests.len();
    let mut tree_count = 0usize;

    for obj in forests {
        let patch = obj.forest.as_ref().expect("filtered");
        let patch_half_x = if patch.patch_half_x > 0.0 {
            patch.patch_half_x
        } else {
            default_half
        };
        let patch_half_z = if patch.patch_half_z > 0.0 {
            patch.patch_half_z
        } else {
            default_half
        };
        let (base_w, base_h) = forest_tree_size(patch.tree_width, patch.tree_height);
        if focus.horizontal_distance(obj.position) > crate::world::VISIBLE_RADIUS_M {
            continue;
        }
        let trees_world = scatter_trees_in_patch(
            obj.position,
            patch_half_x,
            patch_half_z,
            patch.population,
            patch.scale_min,
            patch.scale_max,
            obj.tile_x,
            obj.tile_z,
            patch.uid,
            &track_index,
            terrain,
            track_clearance,
            focus,
        );
        let trees: Vec<TreePlacement> = trees_world
            .iter()
            .map(|t| TreePlacement {
                position: focus.to_render_surface(t.position),
                scale: t.scale,
            })
            .collect();
        tree_count += trees.len();

        let mesh = meshes.add(build_forest_patch_mesh(&trees, base_w, base_h));
        let material = if let Some(tex_name) = patch.tree_texture.as_deref() {
            material_cache
                .entry(tex_name.to_string())
                .or_insert_with(|| {
                    if let Some(image) = load_ace_image(&assets.route_dir, tex_name) {
                        let handle = images.add(image);
                        materials.add(StandardMaterial {
                            base_color: Color::WHITE,
                            base_color_texture: Some(handle),
                            perceptual_roughness: 0.95,
                            metallic: 0.0,
                            double_sided: true,
                            alpha_mode: AlphaMode::Mask(0.35),
                            ..default()
                        })
                    } else {
                        fallback_material.clone()
                    }
                })
                .clone()
        } else {
            fallback_material.clone()
        };

        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::IDENTITY,
            Name::new(format!("forest:{}:{}", obj.label, patch.uid)),
        ));
    }

    viewer_log!("openrailsrs-viewer3d: {patch_count} forest patch(es), {tree_count} tree(s)");
    log_step("spawned forest patches", spawn_start);
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_core::{EdgeId, NodeId};
    use openrailsrs_track::{Edge, Node, NodeKind, TrackGraph};

    use crate::terrain::TerrainElevation;
    use crate::world::load_world_from_route_dir;
    use std::path::PathBuf;

    fn zero_focus() -> RouteFocus {
        RouteFocus {
            center: Vec3::ZERO,
            height_origin: 0.0,
        }
    }

    fn line_graph_through_origin() -> TrackGraph {
        let mut g = TrackGraph::new();
        g.insert_node(Node {
            id: NodeId("a".into()),
            kind: NodeKind::Plain,
            x_m: -50.0,
            y_m: 0.0,
        })
        .unwrap();
        g.insert_node(Node {
            id: NodeId("b".into()),
            kind: NodeKind::Plain,
            x_m: 50.0,
            y_m: 0.0,
        })
        .unwrap();
        g.insert_edge(Edge {
            id: EdgeId("e1".into()),
            from: NodeId("a".into()),
            to: NodeId("b".into()),
            length_m: 100.0,
            speed_limit_mps: 30.0,
            grade_percent: 0.0,
        })
        .unwrap();
        g
    }

    #[test]
    fn rng_is_deterministic_per_tree() {
        let a = forest_rng01(0, 0, 5, 2, 0);
        let b = forest_rng01(0, 0, 5, 2, 0);
        let c = forest_rng01(0, 0, 5, 3, 0);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn tree_size_does_not_scale_with_route_extent() {
        let small = forest_tree_size(0.0, 0.0);
        let explicit = forest_tree_size(4.0, 9.0);
        let clamped = forest_tree_size(400.0, 900.0);

        assert_eq!(small, (DEFAULT_TREE_WIDTH_M, DEFAULT_TREE_HEIGHT_M));
        assert_eq!(explicit, (4.0, 9.0));
        assert_eq!(clamped, (50.0, 80.0));
    }

    #[test]
    fn scatter_respects_population_without_obstacles() {
        let g = TrackGraph::new();
        let idx = TrackSegmentIndex::from_graph(&g, Vec3::ZERO);
        let focus = zero_focus();
        let trees = scatter_trees_in_patch(
            Vec3::new(5000.0, 0.0, 5000.0),
            50.0,
            50.0,
            12,
            0.8,
            1.2,
            0,
            0,
            7,
            &idx,
            None,
            5.0,
            &focus,
        );
        assert_eq!(trees.len(), 12);
        assert!(trees.iter().all(|t| t.scale >= 0.8 && t.scale <= 1.2));
    }

    #[test]
    fn scatter_places_trees_when_track_fills_patch() {
        let g = line_graph_through_origin();
        let idx = TrackSegmentIndex::from_graph(&g, Vec3::ZERO);
        let focus = zero_focus();
        let trees = scatter_trees_in_patch(
            Vec3::ZERO,
            20.0,
            20.0,
            8,
            1.0,
            1.0,
            0,
            0,
            3,
            &idx,
            None,
            50.0,
            &focus,
        );
        assert_eq!(trees.len(), 8);
    }

    #[test]
    fn scatter_avoids_track_centreline() {
        let g = line_graph_through_origin();
        let idx = TrackSegmentIndex::from_graph(&g, Vec3::ZERO);
        let focus = zero_focus();
        let clearance = 8.0;
        let trees = scatter_trees_in_patch(
            Vec3::ZERO,
            40.0,
            40.0,
            24,
            1.0,
            1.0,
            0,
            0,
            1,
            &idx,
            None,
            clearance,
            &focus,
        );
        assert!(!trees.is_empty());
        for tree in &trees {
            assert!(idx.min_distance_xz(tree.position.x, tree.position.z, clearance) >= clearance);
        }
    }

    #[test]
    fn scatter_uses_terrain_height() {
        let route_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        let g = TrackGraph::new();
        let idx = TrackSegmentIndex::from_graph(&g, Vec3::ZERO);
        let elev = TerrainElevation::load_from_route_dir(&route_dir);
        let focus = zero_focus();
        let anchor = Vec3::new(180.0, 999.0, 55.0);
        let trees = scatter_trees_in_patch(
            anchor,
            20.0,
            20.0,
            4,
            1.0,
            1.0,
            2,
            0,
            3,
            &idx,
            Some(&elev),
            0.0,
            &focus,
        );
        assert!(!trees.is_empty());
        for tree in &trees {
            assert!((tree.position.y - 999.0).abs() > 0.1);
        }
    }

    #[test]
    fn scatter_fallback_y_is_msl_not_scenery_local() {
        let focus = RouteFocus {
            center: Vec3::new(12_494_846.0, 82.0, 30_600_240.0),
            height_origin: 13_184.0,
        };
        let g = TrackGraph::new();
        let idx = TrackSegmentIndex::from_graph(&g, Vec3::ZERO);
        let anchor = Vec3::new(focus.center.x, 55.0, focus.center.z);
        let trees = scatter_trees_in_patch(
            anchor, 40.0, 40.0, 8, 1.0, 1.0, 0, 0, 1, &idx, None, 0.0, &focus,
        );
        assert!(!trees.is_empty());
        let expected_msl = focus.scenery_y_to_msl(55.0);
        for tree in &trees {
            assert!(
                (tree.position.y - expected_msl).abs() < 1.0,
                "fallback tree y must be MSL (~{expected_msl}), got {}",
                tree.position.y
            );
            let render_y = focus.to_render_surface(tree.position).y;
            assert!(
                render_y.abs() < 50.0,
                "render y must stay near ground, got {render_y}"
            );
        }
    }

    #[test]
    fn cross_mesh_has_triangles_per_tree() {
        let g = TrackGraph::new();
        let idx = TrackSegmentIndex::from_graph(&g, Vec3::ZERO);
        let focus = zero_focus();
        let trees = scatter_trees_in_patch(
            Vec3::ZERO,
            10.0,
            10.0,
            2,
            1.0,
            1.0,
            0,
            0,
            1,
            &idx,
            None,
            0.0,
            &focus,
        );
        let mesh = build_forest_patch_mesh(&trees, 4.0, 12.0);
        let positions = mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap();
        assert_eq!(positions.len(), 16);
    }

    #[test]
    fn smoke_route_has_forest_patch() {
        let route_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        let scene = load_world_from_route_dir(&route_dir);
        let forest = scene
            .items
            .iter()
            .find(|o| o.kind == "Forest")
            .expect("forest");
        let patch = forest.forest.as_ref().expect("forest meta");
        assert_eq!(patch.tree_texture.as_deref(), Some("pine.ace"));
        assert_eq!(patch.scale_min, 0.8);
        assert!((forest.position.x - 180.0).abs() < 0.1);
        assert!((forest.position.z - (-55.0)).abs() < 0.1);
    }
}
