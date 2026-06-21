//! 3D track graph from `track.toml`: nodes as spheres, edges as cylinders.
//!
//! Planar graph coordinates (`x_m`, `y_m`) map to Bevy world space with Y up:
//! `X = x_m`, `Z = y_m` (same convention as the 2D viewer's horizontal axes).

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;
use openrailsrs_track::{NodeKind, TrackGraph};

use crate::launch::{
    TRACK_DEV_ORBIT_DISTANCE_M, TRACK_DEV_ORBIT_MAX_M, ViewerLaunchOpts, ViewerSceneryMode,
};
use crate::shapes::RouteAssets;
use crate::terrain::{TerrainElevation, ground_y_at};
use crate::train::{ReplayState, pose_at_time};
use crate::world::WorldScene;

// ── Colours (aligned with openrailsrs-viewer 2D) ─────────────────────────────

// const COLOR_EDGE: Color = Color::srgb(1.0, 0.667, 0.2);
const COLOR_TRACK_RAIL: Color = Color::srgb(0.78, 0.86, 0.98);
const COLOR_NODE_PLAIN: Color = Color::srgb(1.0, 1.0, 1.0);
const COLOR_NODE_SWITCH: Color = Color::srgb(0.0, 1.0, 1.0);
const COLOR_NODE_STATION: Color = Color::srgb(1.0, 1.0, 0.0);
const TRACK_HALF_GAUGE_M: f32 = 0.7175;
const TRACK_RENDER_LIFT_M: f32 = 0.14;

/// Edge count above which the viewer switches to gizmo lines (no per-edge meshes).
pub const COMPACT_EDGE_THRESHOLD: usize = 800;

/// Initial orbit distance when a replay/scenario provides a train start position.
pub const REPLAY_START_ORBIT_DISTANCE_M: f32 = 400.0;

/// How track geometry is drawn (auto-selected from edge count at load time).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackRenderMode {
    /// Cylinder/sphere PBR meshes for every edge and node.
    Full,
    /// Gizmo lines for edges; node spheres only for switches and stations.
    Compact,
}

impl TrackRenderMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Compact => "compact",
        }
    }
}

/// Loaded route topology and derived scene framing data.
#[derive(Resource, Clone)]
pub struct TrackScene {
    pub graph: TrackGraph,
    pub bounds: SceneBounds,
    pub render_mode: TrackRenderMode,
    pub edge_count: usize,
}

impl TrackScene {
    pub fn from_graph(graph: TrackGraph) -> Self {
        let edge_count = graph.edges_iter().count();
        let bounds = SceneBounds::from_graph(&graph);
        let render_mode = if edge_count > COMPACT_EDGE_THRESHOLD {
            TrackRenderMode::Compact
        } else {
            TrackRenderMode::Full
        };
        Self {
            graph,
            bounds,
            render_mode,
            edge_count,
        }
    }
}

/// Axis-aligned bounds of the track graph in world space (metres).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneBounds {
    pub center: Vec3,
    pub half_extent: f32,
    pub min: Vec3,
    pub max: Vec3,
}

impl SceneBounds {
    /// Default sandbox when the graph has no positioned nodes.
    pub fn default_sandbox() -> Self {
        Self {
            center: Vec3::ZERO,
            half_extent: 100.0,
            min: Vec3::new(-100.0, 0.0, -100.0),
            max: Vec3::new(100.0, 0.0, 100.0),
        }
    }

    pub fn from_graph(graph: &TrackGraph) -> Self {
        let mut min_x = f64::MAX;
        let mut min_z = f64::MAX;
        let mut max_x = f64::MIN;
        let mut max_z = f64::MIN;
        let mut any = false;

        for (_, node) in graph.nodes_iter() {
            any = true;
            min_x = min_x.min(node.x_m);
            min_z = min_z.min(node.y_m);
            max_x = max_x.max(node.x_m);
            max_z = max_z.max(node.y_m);
        }

        if !any {
            return Self::default_sandbox();
        }

        let min = Vec3::new(min_x as f32, 0.0, min_z as f32);
        let max = Vec3::new(max_x as f32, 0.0, max_z as f32);
        let center = (min + max) * 0.5;
        let half_w = (max.x - min.x).max(1.0) * 0.5;
        let half_d = (max.z - min.z).max(1.0) * 0.5;
        let half_extent = half_w.max(half_d);

        Self {
            center,
            half_extent,
            min,
            max,
        }
    }

    /// Ground plane / grid half-size with a small margin around the route.
    pub fn ground_half(&self) -> f32 {
        (self.half_extent * 1.15).max(50.0)
    }

    pub fn edge_radius(&self) -> f32 {
        let base = (self.half_extent * 0.004).clamp(2.0, 30.0);
        if self.half_extent > 20_000.0 {
            base.min(8.0)
        } else {
            base
        }
    }

    pub fn node_radius(&self) -> f32 {
        let base = (self.edge_radius() * 2.0).clamp(4.0, 60.0);
        if self.half_extent > 20_000.0 {
            base.min(16.0)
        } else {
            base
        }
    }

    /// Initial orbit camera distance to frame the whole route.
    pub fn orbit_distance(&self) -> f32 {
        (self.half_extent * 2.2).clamp(20.0, 500_000.0)
    }
}

/// Map track graph coordinates to Bevy world space (Y up, route on the XZ plane).
/// Delegates to [`crate::coordinates::graph_to_world`].
pub use crate::coordinates::graph_to_world;

/// Graph node position plus optional MSTS world alignment offset.
pub fn graph_to_world_with_offset(offset: Vec3, x_m: f64, y_m: f64) -> Vec3 {
    graph_to_world(x_m, y_m) + offset
}

/// Shortest distance in the XZ plane from `(px, pz)` to segment `(x0,z0)–(x1,z1)`.
pub fn point_segment_distance_xz(px: f32, pz: f32, x0: f32, z0: f32, x1: f32, z1: f32) -> f32 {
    let dx = x1 - x0;
    let dz = z1 - z0;
    let len_sq = dx * dx + dz * dz;
    if len_sq < 1e-6 {
        let ex = px - x0;
        let ez = pz - z0;
        return (ex * ex + ez * ez).sqrt();
    }
    let t = ((px - x0) * dx + (pz - z0) * dz) / len_sq;
    let t = t.clamp(0.0, 1.0);
    let cx = x0 + t * dx;
    let cz = z0 + t * dz;
    let ex = px - cx;
    let ez = pz - cz;
    (ex * ex + ez * ez).sqrt()
}

/// Minimum distance from a world XZ point to any edge in the track graph.
pub fn min_distance_to_graph_xz(graph: &TrackGraph, x: f32, z: f32) -> f32 {
    TrackSegmentIndex::from_graph(graph, Vec3::ZERO).min_distance_xz(x, z, f32::INFINITY)
}

const TRACK_SEGMENT_CELL_M: f32 = 256.0;

#[derive(Clone, Copy, Debug)]
struct TrackSegment {
    x0: f32,
    z0: f32,
    x1: f32,
    z1: f32,
}

/// Spatial index for track edges in world XZ (for forest clearance checks).
#[derive(Clone)]
pub struct TrackSegmentIndex {
    cell_size: f32,
    segments: Vec<TrackSegment>,
    grid: std::collections::HashMap<(i32, i32), Vec<usize>>,
}

impl TrackSegmentIndex {
    pub fn from_graph(graph: &TrackGraph, world_offset: Vec3) -> Self {
        let mut segments = Vec::new();
        for (_, edge) in graph.edges_iter() {
            let Some(from) = graph.node(&edge.from.0) else {
                continue;
            };
            let Some(to) = graph.node(&edge.to.0) else {
                continue;
            };
            let w0 = graph_to_world_with_offset(world_offset, from.x_m, from.y_m);
            let w1 = graph_to_world_with_offset(world_offset, to.x_m, to.y_m);
            segments.push(TrackSegment {
                x0: w0.x,
                z0: w0.z,
                x1: w1.x,
                z1: w1.z,
            });
        }

        let cell_size = TRACK_SEGMENT_CELL_M;
        let mut grid: std::collections::HashMap<(i32, i32), Vec<usize>> =
            std::collections::HashMap::new();
        for (idx, seg) in segments.iter().enumerate() {
            let min_x = seg.x0.min(seg.x1);
            let max_x = seg.x0.max(seg.x1);
            let min_z = seg.z0.min(seg.z1);
            let max_z = seg.z0.max(seg.z1);
            let ci_min = (min_x / cell_size).floor() as i32;
            let ci_max = (max_x / cell_size).floor() as i32;
            let cz_min = (min_z / cell_size).floor() as i32;
            let cz_max = (max_z / cell_size).floor() as i32;
            for cx in ci_min..=ci_max {
                for cz in cz_min..=cz_max {
                    grid.entry((cx, cz)).or_default().push(idx);
                }
            }
        }

        Self {
            cell_size,
            segments,
            grid,
        }
    }

    /// Minimum XZ distance to the nearest indexed segment within `search_radius_m`.
    pub fn min_distance_xz(&self, x: f32, z: f32, search_radius_m: f32) -> f32 {
        if !search_radius_m.is_finite() {
            return self.segments.iter().fold(f32::INFINITY, |min, seg| {
                min.min(point_segment_distance_xz(
                    x, z, seg.x0, seg.z0, seg.x1, seg.z1,
                ))
            });
        }

        let search_cells = (search_radius_m / self.cell_size).ceil() as i32 + 1;
        let cx = (x / self.cell_size).floor() as i32;
        let cz = (z / self.cell_size).floor() as i32;

        let mut min = f32::INFINITY;
        for dx in -search_cells..=search_cells {
            for dz in -search_cells..=search_cells {
                let Some(indices) = self.grid.get(&(cx + dx, cz + dz)) else {
                    continue;
                };
                for &idx in indices {
                    let seg = &self.segments[idx];
                    let d = point_segment_distance_xz(x, z, seg.x0, seg.z0, seg.x1, seg.z1);
                    min = min.min(d);
                }
            }
        }
        min
    }
}

/// Default clearance when scattering trees away from track centreline.
pub fn forest_track_clearance_m(bounds: &SceneBounds) -> f32 {
    // Cap clearance: route `edge_radius` can be km-scale and would reject every tree.
    (bounds.edge_radius().max(2.0) * 0.12).clamp(2.5, 6.0)
}

/// Transform for a unit-height cylinder aligned on Y, scaled to span `from` → `to`.
pub fn cylinder_between(from: Vec3, to: Vec3) -> Transform {
    let delta = to - from;
    let length = delta.length();
    if length < 1e-4 {
        return Transform::from_translation(from);
    }
    let mid = (from + to) * 0.5;
    let rotation = Quat::from_rotation_arc(Vec3::Y, delta / length);
    Transform {
        translation: mid,
        rotation,
        scale: Vec3::new(1.0, length, 1.0),
    }
}

fn node_material_index(kind: &NodeKind) -> usize {
    match kind {
        NodeKind::Plain => 0,
        NodeKind::Switch { .. } => 1,
        NodeKind::Station { .. } => 2,
    }
}

fn should_spawn_node(kind: &NodeKind, mode: TrackRenderMode) -> bool {
    match mode {
        TrackRenderMode::Full => true,
        TrackRenderMode::Compact => !matches!(kind, NodeKind::Plain),
    }
}

/// Line-list mesh for compact routes (drawn once; avoids per-frame gizmo cost).
pub fn build_compact_track_line_mesh(
    graph: &TrackGraph,
    offset: Vec3,
    focus: &crate::world::RouteFocus,
    terrain: Option<&TerrainElevation>,
    scene: &TrackScene,
) -> Mesh {
    let edge_count = graph.edges_iter().count();
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(edge_count * 4);
    for (_, edge) in graph.edges_iter() {
        let Some(from) = graph.node(&edge.from.0) else {
            continue;
        };
        let Some(to) = graph.node(&edge.to.0) else {
            continue;
        };
        let w0 = graph_to_world_with_offset(offset, from.x_m, from.y_m);
        let w1 = graph_to_world_with_offset(offset, to.x_m, to.y_m);
        let p0 = track_surface_render_pos(w0, terrain, scene, focus);
        let p1 = track_surface_render_pos(w1, terrain, scene, focus);
        let dir = Vec2::new(p1.x - p0.x, p1.z - p0.z);
        let side = if dir.length_squared() > 1e-6 {
            let n = dir.normalize();
            Vec3::new(-n.y, 0.0, n.x)
        } else {
            Vec3::X
        };
        let lift = Vec3::Y * TRACK_RENDER_LIFT_M;
        for lateral in [-TRACK_HALF_GAUGE_M, TRACK_HALF_GAUGE_M] {
            let rail_offset = side * lateral + lift;
            positions.push((p0 + rail_offset).to_array());
            positions.push((p1 + rail_offset).to_array());
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh
}

fn track_surface_render_pos(
    world: Vec3,
    terrain: Option<&TerrainElevation>,
    scene: &TrackScene,
    focus: &crate::world::RouteFocus,
) -> Vec3 {
    let y = ground_y_at(terrain, world.x, world.z, scene);
    focus.to_render_surface(Vec3::new(world.x, y, world.z))
}

#[derive(Component)]
pub(crate) struct CompactTrackLines;

fn should_hide_compact_track_lines(
    opts: ViewerLaunchOpts,
    render_mode: TrackRenderMode,
    has_world_scenery: bool,
) -> bool {
    opts.live && render_mode == TrackRenderMode::Compact && has_world_scenery
}

/// One-shot: spawn edge cylinders and node spheres for the loaded graph.
#[allow(clippy::too_many_arguments)]
pub fn spawn_track_meshes(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    scene: Res<TrackScene>,
    offset: Res<crate::world::RouteWorldOffset>,
    focus: Res<crate::world::RouteFocus>,
    opts: Res<ViewerLaunchOpts>,
    mode: Res<ViewerSceneryMode>,
    terrain: Option<Res<TerrainElevation>>,
    world: Option<Res<WorldScene>>,
    assets: Res<RouteAssets>,
) {
    if mode.is_track_focused() && assets.track_db().is_some() {
        return;
    }
    let has_world_scenery = world.as_deref().is_some_and(|w| !w.is_empty());
    if should_hide_compact_track_lines(*opts, scene.render_mode, has_world_scenery) {
        crate::viewer_log!(
            "openrailsrs-viewer3d: hiding compact logical track graph in live MSTS scenery; TrackObj/TDB geometry provides visual rails"
        );
        return;
    }
    let offset = offset.delta;
    let terrain_ref = terrain.as_deref();
    let bounds = scene.bounds;
    let node_radius = bounds.node_radius();

    let mut node_materials: Vec<Handle<StandardMaterial>> = Vec::new();
    for color in [COLOR_NODE_PLAIN, COLOR_NODE_SWITCH, COLOR_NODE_STATION] {
        node_materials.push(materials.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 0.7,
            metallic: 0.1,
            ..default()
        }));
    }

    let node_material_for = |kind: &NodeKind| -> Handle<StandardMaterial> {
        node_materials[node_material_index(kind)].clone()
    };

    if scene.render_mode == TrackRenderMode::Compact {
        let line_mesh = meshes.add(build_compact_track_line_mesh(
            &scene.graph,
            offset,
            &focus,
            terrain_ref,
            &scene,
        ));
        let line_material = materials.add(StandardMaterial {
            base_color: COLOR_TRACK_RAIL,
            emissive: LinearRgba::from(COLOR_TRACK_RAIL) * 0.2,
            unlit: true,
            ..default()
        });
        let mut track = commands.spawn((
            CompactTrackLines,
            Mesh3d(line_mesh),
            MeshMaterial3d(line_material),
            Transform::IDENTITY,
            Name::new("track:compact-lines"),
        ));
        if opts.live {
            track.insert(NotShadowCaster);
        }
    } else {
        let mut segments = Vec::new();
        for (_, edge) in scene.graph.edges_iter() {
            let Some(from) = scene.graph.node(&edge.from.0) else {
                continue;
            };
            let Some(to) = scene.graph.node(&edge.to.0) else {
                continue;
            };
            let p0 = track_surface_render_pos(
                graph_to_world_with_offset(offset, from.x_m, from.y_m),
                terrain_ref,
                &scene,
                &focus,
            );
            let p1 = track_surface_render_pos(
                graph_to_world_with_offset(offset, to.x_m, to.y_m),
                terrain_ref,
                &scene,
                &focus,
            );
            let delta = p1 - p0;
            let length = delta.length();
            if length > 1e-4 {
                let rotation = Quat::from_rotation_arc(Vec3::Z, delta.normalize());
                segments.push(crate::dyntrack::ProceduralTrackSegment {
                    position: p0,
                    rotation,
                    length_m: Some(length),
                    half_gauge_m: Some(crate::dyntrack::MSTS_STANDARD_HALF_GAUGE_M),
                    curve_radius_m: None,
                    curve_angle_deg: None,
                });
            }
        }
        crate::dyntrack::spawn_procedural_track_batch(
            &mut commands,
            &mut meshes,
            &mut materials,
            &segments,
            "logical-track",
            crate::dyntrack::ProceduralTrackStyle::Full,
        );
    }

    if !opts.live {
        let node_mesh = meshes.add(Sphere::new(node_radius));
        for (_, node) in scene.graph.nodes_iter() {
            if !should_spawn_node(&node.kind, scene.render_mode) {
                continue;
            }
            let pos = track_surface_render_pos(
                graph_to_world_with_offset(offset, node.x_m, node.y_m),
                terrain_ref,
                &scene,
                &focus,
            );
            commands.spawn((
                Mesh3d(node_mesh.clone()),
                MeshMaterial3d(node_material_for(&node.kind)),
                Transform::from_translation(pos),
                Name::new(format!("node:{}", node.id.0)),
            ));
        }
    }
}

/// `--tile-lab`: encuadre robusto en Update (la cámara puede no existir aún en
/// Startup según el orden de sistemas); corre una sola vez.
pub fn tile_lab_frame_camera_once(
    mode: Res<ViewerSceneryMode>,
    mut done: Local<bool>,
    mut limit: ResMut<crate::camera::OrbitDistanceLimit>,
    mut query: Query<(&mut Transform, &mut crate::camera::OrbitState), With<Camera3d>>,
) {
    if *done || !mode.is_tile_lab() {
        return;
    }
    let Ok((mut transform, mut orbit)) = query.single_mut() else {
        return;
    };
    limit.max = crate::launch::TILE_LAB_ORBIT_MAX_M;
    orbit.focus = Vec3::ZERO;
    orbit.pitch = 0.9;
    orbit.distance = std::env::var("OPENRAILSRS_TILE_LAB_DIST_M")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|d| *d >= 50.0 && *d <= crate::launch::TILE_LAB_ORBIT_MAX_M)
        .unwrap_or(crate::launch::TILE_LAB_ORBIT_DISTANCE_M);
    *orbit = crate::camera::orbit_state_with_env_overrides(*orbit);
    *transform = crate::camera::camera_transform_from_orbit_state(
        orbit.focus,
        orbit.yaw,
        orbit.pitch,
        orbit.distance,
    );
    *done = true;
}

/// Point the orbit camera at the route centre with a distance that frames it.
#[allow(clippy::too_many_arguments)]
pub fn frame_orbit_camera_on_track(
    scene: Res<TrackScene>,
    focus: Res<crate::world::RouteFocus>,
    mode: Res<ViewerSceneryMode>,
    replay: Res<ReplayState>,
    offset: Res<crate::world::RouteWorldOffset>,
    terrain: Option<Res<TerrainElevation>>,
    mut limit: ResMut<crate::camera::OrbitDistanceLimit>,
    mut query: Query<(&mut Transform, &mut crate::camera::OrbitState), With<Camera3d>>,
) {
    let Ok((mut transform, mut orbit)) = query.single_mut() else {
        return;
    };
    if mode.is_tile_lab() {
        // The focus centre is the tile centre, i.e. render-space origin: park the
        // camera above it looking down so the whole 2048 m tile is framed.
        limit.max = crate::launch::TILE_LAB_ORBIT_MAX_M;
        orbit.focus = Vec3::ZERO;
        orbit.pitch = 0.9;
        orbit.distance = std::env::var("OPENRAILSRS_TILE_LAB_DIST_M")
            .ok()
            .and_then(|s| s.parse().ok())
            .filter(|d| *d >= 50.0 && *d <= crate::launch::TILE_LAB_ORBIT_MAX_M)
            .unwrap_or(crate::launch::TILE_LAB_ORBIT_DISTANCE_M);
        *transform = crate::camera::camera_transform_from_orbit_state(
            orbit.focus,
            orbit.yaw,
            orbit.pitch,
            orbit.distance,
        );
        return;
    }
    if mode.is_track_focused() {
        limit.max = TRACK_DEV_ORBIT_MAX_M;
        orbit.focus = replay
            .tracks
            .first()
            .and_then(|track| {
                pose_at_time(
                    &scene.graph,
                    &track.rows,
                    0.0,
                    None,
                    &scene,
                    offset.delta,
                    &focus,
                )
                .map(|(pos, _, _)| pos)
            })
            .unwrap_or_else(|| focus.to_render_surface(scene.bounds.center));
        orbit.distance = TRACK_DEV_ORBIT_DISTANCE_M;
    } else {
        let max = scene.bounds.orbit_distance();
        limit.max = max;
        // Start at the player's position (first replay row) instead of framing the
        // whole route bbox: on big MSTS routes the bbox distance puts the camera
        // outside the sky sphere and far from the 8 km scenery radius.
        let start_pose = replay.tracks.first().and_then(|track| {
            pose_at_time(
                &scene.graph,
                &track.rows,
                0.0,
                terrain.as_deref(),
                &scene,
                offset.delta,
                &focus,
            )
            .map(|(pos, _, _)| pos)
        });
        match start_pose {
            Some(pos) => {
                orbit.focus = pos;
                orbit.distance = REPLAY_START_ORBIT_DISTANCE_M.min(max);
            }
            None => {
                orbit.focus = Vec3::ZERO;
                orbit.distance = max;
            }
        }
    }
    *transform = crate::camera::camera_transform_from_orbit_state(
        orbit.focus,
        orbit.yaw,
        orbit.pitch,
        orbit.distance,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_core::{EdgeId, NodeId};
    use openrailsrs_track::{Edge, Node, TrackGraph};

    fn sample_graph() -> TrackGraph {
        let mut g = TrackGraph::new();
        g.insert_node(Node {
            id: NodeId("a".into()),
            kind: NodeKind::Plain,
            x_m: 0.0,
            y_m: 0.0,
        })
        .unwrap();
        g.insert_node(Node {
            id: NodeId("b".into()),
            kind: NodeKind::Switch {
                stem_edge: EdgeId("e1".into()),
                diverging_edge: EdgeId("e2".into()),
            },
            x_m: 100.0,
            y_m: 50.0,
        })
        .unwrap();
        g.insert_edge(Edge {
            id: EdgeId("e1".into()),
            from: NodeId("a".into()),
            to: NodeId("b".into()),
            length_m: 111.8,
            speed_limit_mps: 22.0,
            grade_percent: 0.0,
        })
        .unwrap();
        g
    }

    #[test]
    fn graph_to_world_maps_y_m_to_z() {
        let p = graph_to_world(10.0, -3.0);
        assert_eq!(p, Vec3::new(10.0, 0.0, -3.0));
    }

    #[test]
    fn bounds_from_graph_centers_route() {
        let bounds = SceneBounds::from_graph(&sample_graph());
        assert!((bounds.center.x - 50.0).abs() < 1e-3);
        assert!((bounds.center.z - 25.0).abs() < 1e-3);
        assert!(bounds.half_extent >= 50.0);
    }

    #[test]
    fn cylinder_between_unit_y_span() {
        let from = Vec3::new(0.0, 0.0, 0.0);
        let to = Vec3::new(0.0, 0.0, 10.0);
        let t = cylinder_between(from, to);
        assert!((t.scale.y - 10.0).abs() < 1e-4);
        assert!((t.translation.z - 5.0).abs() < 1e-4);
    }

    #[test]
    fn radii_scale_with_extent() {
        let bounds = SceneBounds::from_graph(&sample_graph());
        assert!(bounds.edge_radius() >= 2.0);
        assert!(bounds.node_radius() > bounds.edge_radius());
    }

    #[test]
    fn compact_mode_selected_above_threshold() {
        let mut g = TrackGraph::new();
        let n = COMPACT_EDGE_THRESHOLD + 1;
        for i in 0..=n {
            let id = format!("n{i}");
            g.insert_node(Node {
                id: NodeId(id.clone()),
                kind: NodeKind::Plain,
                x_m: i as f64,
                y_m: 0.0,
            })
            .unwrap();
            if i > 0 {
                g.insert_edge(Edge {
                    id: EdgeId(format!("e{i}")),
                    from: NodeId(format!("n{}", i - 1)),
                    to: NodeId(id),
                    length_m: 1.0,
                    speed_limit_mps: 10.0,
                    grade_percent: 0.0,
                })
                .unwrap();
            }
        }
        let scene = TrackScene::from_graph(g);
        assert_eq!(scene.render_mode, TrackRenderMode::Compact);
    }

    #[test]
    fn compact_skips_plain_nodes() {
        assert!(!should_spawn_node(
            &NodeKind::Plain,
            TrackRenderMode::Compact
        ));
        assert!(should_spawn_node(
            &NodeKind::Switch {
                stem_edge: EdgeId("e".into()),
                diverging_edge: EdgeId("e2".into()),
            },
            TrackRenderMode::Compact,
        ));
    }

    #[test]
    fn full_mode_spawns_all_node_kinds() {
        assert!(should_spawn_node(&NodeKind::Plain, TrackRenderMode::Full));
        assert!(should_spawn_node(
            &NodeKind::Station { name: "st".into() },
            TrackRenderMode::Full,
        ));
    }

    #[test]
    fn compact_keeps_station_nodes() {
        assert!(should_spawn_node(
            &NodeKind::Station { name: "st".into() },
            TrackRenderMode::Compact,
        ));
    }

    #[test]
    fn node_material_index_maps_kinds() {
        assert_eq!(node_material_index(&NodeKind::Plain), 0);
        assert_eq!(
            node_material_index(&NodeKind::Switch {
                stem_edge: EdgeId("a".into()),
                diverging_edge: EdgeId("b".into()),
            }),
            1
        );
        assert_eq!(
            node_material_index(&NodeKind::Station { name: "x".into() }),
            2
        );
    }

    #[test]
    fn render_mode_labels() {
        assert_eq!(TrackRenderMode::Full.label(), "full");
        assert_eq!(TrackRenderMode::Compact.label(), "compact");
    }

    #[test]
    fn compact_line_mesh_has_two_rail_segments_per_edge() {
        let focus = crate::world::RouteFocus {
            center: Vec3::ZERO,
            height_origin: 0.0,
        };
        let scene = TrackScene::from_graph(sample_graph());
        let mesh = build_compact_track_line_mesh(&scene.graph, Vec3::ZERO, &focus, None, &scene);
        let positions = mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap();
        assert_eq!(positions.len(), 4);
    }

    #[test]
    fn live_msts_scenery_hides_compact_graph_overlay() {
        let opts = ViewerLaunchOpts { live: true };
        assert!(should_hide_compact_track_lines(
            opts,
            TrackRenderMode::Compact,
            true
        ));
        assert!(!should_hide_compact_track_lines(
            opts,
            TrackRenderMode::Compact,
            false
        ));
        assert!(!should_hide_compact_track_lines(
            ViewerLaunchOpts { live: false },
            TrackRenderMode::Compact,
            true
        ));
        assert!(!should_hide_compact_track_lines(
            opts,
            TrackRenderMode::Full,
            true
        ));
    }

    #[test]
    fn default_sandbox_when_graph_empty() {
        let bounds = SceneBounds::from_graph(&TrackGraph::new());
        assert_eq!(bounds.half_extent, 100.0);
        assert_eq!(bounds.center, Vec3::ZERO);
    }

    #[test]
    fn large_route_caps_mesh_radii() {
        let bounds = SceneBounds {
            center: Vec3::ZERO,
            half_extent: 25_000.0,
            min: Vec3::new(-25_000.0, 0.0, -25_000.0),
            max: Vec3::new(25_000.0, 0.0, 25_000.0),
        };
        assert!((bounds.edge_radius() - 8.0).abs() < 1e-5);
        assert!((bounds.node_radius() - 16.0).abs() < 1e-5);
    }

    #[test]
    fn ground_half_and_orbit_distance_scale() {
        let bounds = SceneBounds::from_graph(&sample_graph());
        assert!(bounds.ground_half() >= bounds.half_extent);
        assert!(bounds.orbit_distance() >= bounds.half_extent * 2.0);
    }

    #[test]
    fn point_segment_distance_at_midpoint_is_zero() {
        let d = point_segment_distance_xz(50.0, 0.0, 0.0, 0.0, 100.0, 0.0);
        assert!(d.abs() < 1e-5);
    }

    #[test]
    fn min_distance_to_graph_on_edge_is_small() {
        let g = sample_graph();
        let d = min_distance_to_graph_xz(&g, 50.0, 25.0);
        assert!(d < 1.0);
    }
}
