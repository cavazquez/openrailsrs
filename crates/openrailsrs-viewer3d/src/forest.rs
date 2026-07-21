//! MSTS `Forest` patches from `.w` tiles (order 11 / issue #8 / #38).
//!
//! Each forest anchor spawns a population of OR-style camera-facing billboards
//! (`VSForest`): one quad per tree, expanded in the vertex shader from base
//! position + size packed in NORMAL. RNG seeded from tile + uid.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use openrailsrs_bevy_scenery::{
    OrForestMaterial, create_or_forest_material,
    scatter_trees_in_patch as shared_scatter_trees_in_patch,
};
use std::time::Instant;

use crate::shapes::load_ace_image;
use crate::terrain::TerrainElevation;
use crate::track::{SceneBounds, TrackScene, TrackSegmentIndex, forest_track_clearance_m};
use crate::world::{
    RouteFocus, RouteWorldOffset, WorldObject, WorldScene, horizontal_distance_xz, visible_radius_m,
};
use crate::{log_step, viewer_log};

pub use openrailsrs_bevy_scenery::{
    DEFAULT_TREE_HEIGHT_M, DEFAULT_TREE_WIDTH_M, TreePlacement, append_tree_billboard,
    build_forest_patch_mesh, forest_rng01, forest_tree_size,
};

const COLOR_TREE_FALLBACK: Color = Color::srgb(0.18, 0.62, 0.22);
const DEFAULT_PATCH_HALF_M: f32 = 128.0;

/// Default half-extent of a forest patch when `.w` has no `Area`.
pub fn default_patch_half(bounds: &SceneBounds) -> f32 {
    DEFAULT_PATCH_HALF_M.min(bounds.ground_half().max(DEFAULT_PATCH_HALF_M))
}

/// Scatter trees inside a rectangular patch around `anchor`.
///
/// Viewer adapter: terrain MSL + track clearance via [`TrackSegmentIndex`] (#117).
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
    let fallback_y = focus.scenery_y_to_msl(anchor.y);
    let blocked =
        |x: f32, z: f32, clearance: f32| track_index.min_distance_xz(x, z, clearance) < clearance;
    shared_scatter_trees_in_patch(
        anchor,
        patch_half_x,
        patch_half_z,
        population,
        scale_min,
        scale_max,
        tile_x,
        tile_z,
        uid,
        |x, z| {
            terrain
                .and_then(|elev| elev.sample_world_y(x, z))
                .unwrap_or(fallback_y)
        },
        Some(&blocked),
        track_clearance_m,
    )
}

fn solid_color_image(color: Color) -> Image {
    let linear = color.to_linear();
    let rgba = [
        (linear.red * 255.0) as u8,
        (linear.green * 255.0) as u8,
        (linear.blue * 255.0) as u8,
        255,
    ];
    Image::new(
        bevy::render::render_resource::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        rgba.to_vec(),
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// Spawn cross-billboard tree patches for every `Forest` in the world scene.
#[allow(clippy::too_many_arguments)]
pub fn spawn_forest_patches(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<OrForestMaterial>>,
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
        None,
    );
}

/// Spawn forest patches for a slice of world objects (tile streaming).
#[allow(clippy::too_many_arguments)]
pub fn spawn_forest_objects(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<OrForestMaterial>,
    items: &[WorldObject],
    track: &TrackScene,
    terrain: Option<&TerrainElevation>,
    assets: &crate::shapes::RouteAssets,
    focus: &RouteFocus,
    offset: &RouteWorldOffset,
    cull_center: Option<Vec3>,
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
    let mut material_cache: std::collections::HashMap<String, Handle<OrForestMaterial>> =
        std::collections::HashMap::new();

    let fallback_tex = images.add(solid_color_image(COLOR_TREE_FALLBACK));
    let fallback_material = create_or_forest_material(materials, fallback_tex);

    let patch_count = forests.len();
    let mut tree_count = 0usize;
    let cull_at = cull_center.unwrap_or(focus.center);

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
        if horizontal_distance_xz(cull_at, obj.position) > visible_radius_m() {
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
                        create_or_forest_material(materials, handle)
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
    fn scatter_fallback_y_keeps_absolute_anchor_y() {
        let focus = RouteFocus {
            center: Vec3::new(-12_450_948.0, 35.7818, -30_566_982.0),
            height_origin: 28.5,
        };
        let g = TrackGraph::new();
        let idx = TrackSegmentIndex::from_graph(&g, Vec3::ZERO);
        let anchor = Vec3::new(focus.center.x, 35.7818, focus.center.z);
        let trees = scatter_trees_in_patch(
            anchor, 40.0, 40.0, 8, 1.0, 1.0, 0, 0, 1, &idx, None, 0.0, &focus,
        );
        assert!(!trees.is_empty());
        let expected_y = focus.scenery_y_to_msl(35.7818);
        for tree in &trees {
            assert!(
                (tree.position.y - expected_y).abs() < 1.0,
                "fallback tree y must keep absolute WORLD Y (~{expected_y}), got {}",
                tree.position.y
            );
            let render_y = focus.to_render_surface(tree.position).y;
            assert!(
                (render_y - (35.7818 - 28.5)).abs() < 1.0,
                "render y must preserve embankment offset, got {render_y}"
            );
        }
    }

    #[test]
    fn billboard_mesh_has_one_quad_per_tree() {
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
        // OR layout: 4 shared-base verts per tree (expanded in VS).
        assert_eq!(positions.len(), 8);
        let normals = mesh.attribute(Mesh::ATTRIBUTE_NORMAL).unwrap();
        if let bevy::mesh::VertexAttributeValues::Float32x3(n) = normals {
            assert!((n[0][0] - 4.0).abs() < 1e-3);
            assert!((n[0][1] - 12.0).abs() < 1e-3);
        } else {
            panic!("expected Float32x3 normals");
        }
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
