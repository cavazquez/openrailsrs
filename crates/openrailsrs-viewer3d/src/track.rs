//! 3D track graph from `track.toml`: nodes as spheres, edges as cylinders.
//!
//! Planar graph coordinates (`x_m`, `y_m`) map to Bevy world space with Y up:
//! `X = x_m`, `Z = y_m` (same convention as the 2D viewer's horizontal axes).

use bevy::prelude::*;
use openrailsrs_track::{NodeKind, TrackGraph};

// ── Colours (aligned with openrailsrs-viewer 2D) ─────────────────────────────

pub(crate) const COLOR_EDGE: Color = Color::srgb(1.0, 0.667, 0.2);
const COLOR_NODE_PLAIN: Color = Color::srgb(1.0, 1.0, 1.0);
const COLOR_NODE_SWITCH: Color = Color::srgb(0.0, 1.0, 1.0);
const COLOR_NODE_STATION: Color = Color::srgb(1.0, 1.0, 0.0);

/// Edge count above which the viewer switches to gizmo lines (no per-edge meshes).
pub const COMPACT_EDGE_THRESHOLD: usize = 800;

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
#[inline]
pub fn graph_to_world(x_m: f64, y_m: f64) -> Vec3 {
    Vec3::new(x_m as f32, 0.0, y_m as f32)
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

/// One-shot: spawn edge cylinders and node spheres for the loaded graph.
pub fn spawn_track_meshes(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    scene: Res<TrackScene>,
) {
    let bounds = scene.bounds;
    let edge_radius = bounds.edge_radius();
    let node_radius = bounds.node_radius();

    let edge_material = materials.add(StandardMaterial {
        base_color: COLOR_EDGE,
        perceptual_roughness: 0.85,
        metallic: 0.05,
        ..default()
    });

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

    if scene.render_mode == TrackRenderMode::Full {
        let edge_mesh = meshes.add(Cylinder::new(edge_radius, 1.0));
        for (_, edge) in scene.graph.edges_iter() {
            let Some(from) = scene.graph.node(&edge.from.0) else {
                continue;
            };
            let Some(to) = scene.graph.node(&edge.to.0) else {
                continue;
            };
            let p0 = graph_to_world(from.x_m, from.y_m);
            let p1 = graph_to_world(to.x_m, to.y_m);
            commands.spawn((
                Mesh3d(edge_mesh.clone()),
                MeshMaterial3d(edge_material.clone()),
                cylinder_between(p0, p1),
                Name::new(format!("edge:{}", edge.id.0)),
            ));
        }
    }

    let node_mesh = meshes.add(Sphere::new(node_radius));
    for (_, node) in scene.graph.nodes_iter() {
        if !should_spawn_node(&node.kind, scene.render_mode) {
            continue;
        }
        let pos = graph_to_world(node.x_m, node.y_m);
        commands.spawn((
            Mesh3d(node_mesh.clone()),
            MeshMaterial3d(node_material_for(&node.kind)),
            Transform::from_translation(pos),
            Name::new(format!("node:{}", node.id.0)),
        ));
    }
}

/// Draw edge segments as gizmo lines in compact render mode.
pub fn draw_compact_edges(scene: Res<TrackScene>, mut gizmos: Gizmos) {
    if scene.render_mode != TrackRenderMode::Compact {
        return;
    }
    for (_, edge) in scene.graph.edges_iter() {
        let Some(from) = scene.graph.node(&edge.from.0) else {
            continue;
        };
        let Some(to) = scene.graph.node(&edge.to.0) else {
            continue;
        };
        let p0 = graph_to_world(from.x_m, from.y_m);
        let p1 = graph_to_world(to.x_m, to.y_m);
        gizmos.line(p0, p1, COLOR_EDGE);
    }
}

/// Point the orbit camera at the route centre with a distance that frames it.
pub fn frame_orbit_camera_on_track(
    scene: Res<TrackScene>,
    mut limit: ResMut<crate::camera::OrbitDistanceLimit>,
    mut query: Query<&mut crate::camera::OrbitState>,
) {
    let Ok(mut orbit) = query.single_mut() else {
        return;
    };
    let max = scene.bounds.orbit_distance();
    limit.max = max;
    orbit.focus = scene.bounds.center;
    orbit.distance = max;
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
}
