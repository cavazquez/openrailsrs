//! Train replay: position markers from simulation CSV (same fields as viewer 2D).

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use openrailsrs_track::TrackGraph;

use crate::track::{TrackScene, graph_to_world};

const COLOR_TRAIN_PRIMARY: Color = Color::srgb(1.0, 0.25, 1.0);

pub const TRAIN_COLORS: [Color; 4] = [
    Color::srgb(1.0, 0.25, 1.0),
    Color::srgb(0.25, 1.0, 1.0),
    Color::srgb(0.5, 1.0, 0.25),
    Color::srgb(1.0, 0.5, 0.25),
];

/// One CSV time series for a train (primary or extra).
#[derive(Clone, Debug)]
pub struct TrainTrack {
    pub label: String,
    pub color: Color,
    pub rows: Vec<CsvRow>,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct CsvRow {
    pub time_s: f64,
    pub velocity_mps: f64,
    #[serde(default)]
    pub edge_id: String,
    #[serde(default)]
    pub pos_on_edge_m: f64,
}

/// Playback state when at least one CSV track is loaded.
#[derive(Resource, Clone, Debug)]
pub struct ReplayState {
    pub scenario_name: String,
    pub tracks: Vec<TrainTrack>,
    pub t_sim: f64,
    pub speed: f64,
    pub paused: bool,
    pub max_t: f64,
}

impl ReplayState {
    pub fn new(scenario_name: String, tracks: Vec<TrainTrack>) -> Self {
        let max_t = tracks
            .iter()
            .filter_map(|t| t.rows.last().map(|r| r.time_s))
            .fold(0.0_f64, f64::max)
            .max(1.0);
        let paused = tracks.is_empty();
        Self {
            scenario_name,
            tracks,
            t_sim: 0.0,
            speed: 1.0,
            paused,
            max_t,
        }
    }

    pub fn is_active(&self) -> bool {
        !self.tracks.is_empty()
    }
}

impl Default for ReplayState {
    fn default() -> Self {
        Self::new(String::new(), Vec::new())
    }
}

/// Load CSV rows from a simulation output file (skips malformed lines).
pub fn load_csv(path: &std::path::Path) -> Vec<CsvRow> {
    let Ok(mut rdr) = csv::Reader::from_path(path) else {
        return vec![];
    };
    rdr.deserialize::<CsvRow>().filter_map(|r| r.ok()).collect()
}

/// World position and yaw (rad, around +Y) for a train on an edge.
pub fn position_on_graph(
    graph: &TrackGraph,
    edge_id: &str,
    pos_on_edge_m: f64,
    y_lift: f32,
) -> Option<(Vec3, f32)> {
    let edge = graph.edge(edge_id.trim())?;
    let from = graph.node(&edge.from.0)?;
    let to = graph.node(&edge.to.0)?;

    let frac = if edge.length_m > 0.0 {
        (pos_on_edge_m / edge.length_m).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let x_m = from.x_m + frac * (to.x_m - from.x_m);
    let y_m = from.y_m + frac * (to.y_m - from.y_m);
    let mut world = graph_to_world(x_m, y_m);
    world.y += y_lift;

    let dx = (to.x_m - from.x_m) as f32;
    let dz = (to.y_m - from.y_m) as f32;
    let yaw = if dx * dx + dz * dz > 1e-6 {
        dz.atan2(dx)
    } else {
        0.0
    };

    Some((world, yaw))
}

/// Interpolate train world pose at simulation time `t`.
pub fn pose_at_time(
    graph: &TrackGraph,
    rows: &[CsvRow],
    t: f64,
    y_lift: f32,
) -> Option<(Vec3, f32, f64)> {
    if rows.is_empty() {
        return None;
    }
    let idx = rows
        .partition_point(|r| r.time_s <= t)
        .saturating_sub(1)
        .min(rows.len() - 1);
    let row = &rows[idx];
    let (pos, yaw) = position_on_graph(graph, &row.edge_id, row.pos_on_edge_m, y_lift)?;
    Some((pos, yaw, row.velocity_mps))
}

#[derive(Component)]
pub struct TrainMarker {
    pub track_index: usize,
}

/// Spawn one cuboid marker per loaded CSV track.
pub fn spawn_train_markers(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    scene: Res<TrackScene>,
    replay: Res<ReplayState>,
) {
    if !replay.is_active() {
        return;
    }

    let y_lift = scene.bounds.node_radius() + scene.bounds.edge_radius() * 1.5;
    let body_len = scene.bounds.edge_radius() * 8.0;
    let body_w = scene.bounds.edge_radius() * 3.0;
    let body_h = scene.bounds.edge_radius() * 2.5;

    let mesh = meshes.add(Cuboid::new(body_len, body_h, body_w));

    for (i, track) in replay.tracks.iter().enumerate() {
        let color = if track.color == COLOR_TRAIN_PRIMARY {
            TRAIN_COLORS[i % TRAIN_COLORS.len()]
        } else {
            track.color
        };
        let material = materials.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 0.55,
            metallic: 0.15,
            emissive: LinearRgba::from(color) * 0.35,
            ..default()
        });

        let (pos, yaw, _) = pose_at_time(&scene.graph, &track.rows, 0.0, y_lift).unwrap_or((
            scene.bounds.center + Vec3::Y * y_lift,
            0.0,
            0.0,
        ));

        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)),
            TrainMarker { track_index: i },
            Name::new(format!("train:{}", track.label)),
        ));
    }
}

pub fn advance_replay_time(time: Res<Time>, mut replay: ResMut<ReplayState>) {
    if !replay.is_active() || replay.paused {
        return;
    }
    replay.t_sim += time.delta_secs() as f64 * replay.speed;
    if replay.t_sim > replay.max_t {
        replay.t_sim = replay.max_t;
        replay.paused = true;
    }
}

pub fn update_train_markers(
    scene: Res<TrackScene>,
    replay: Res<ReplayState>,
    mut query: Query<(&TrainMarker, &mut Transform)>,
) {
    if !replay.is_active() {
        return;
    }

    let y_lift = scene.bounds.node_radius() + scene.bounds.edge_radius() * 1.5;

    for (marker, mut transform) in &mut query {
        let Some(track) = replay.tracks.get(marker.track_index) else {
            continue;
        };
        let Some((pos, yaw, _)) = pose_at_time(&scene.graph, &track.rows, replay.t_sim, y_lift)
        else {
            continue;
        };
        transform.translation = pos;
        transform.rotation = Quat::from_rotation_y(yaw);
    }
}

pub fn replay_controls(keys: Res<ButtonInput<KeyCode>>, mut replay: ResMut<ReplayState>) {
    if !replay.is_active() {
        return;
    }
    if keys.just_pressed(KeyCode::Space) {
        replay.paused = !replay.paused;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        replay.t_sim = 0.0;
        replay.paused = false;
    }
    if keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd) {
        replay.speed = (replay.speed * 2.0).min(64.0);
    }
    if keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::NumpadSubtract) {
        replay.speed = (replay.speed / 2.0).max(0.125);
    }
}

pub fn update_window_hud(
    replay: Res<ReplayState>,
    scene: Res<TrackScene>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if !replay.is_active() {
        return;
    }
    let Ok(mut window) = windows.single_mut() else {
        return;
    };

    let y_lift = scene.bounds.node_radius() + scene.bounds.edge_radius() * 1.5;
    let mut vel_kmh = 0.0;
    if let Some(track) = replay.tracks.first() {
        if let Some((_, _, v)) = pose_at_time(&scene.graph, &track.rows, replay.t_sim, y_lift) {
            vel_kmh = v * 3.6;
        }
    }

    let status = if replay.paused { "PAUSED" } else { "PLAY" };
    window.title = format!(
        "openrailsrs-viewer3d — {} | t={:.1}s {:.0} km/h {} {:.0}x",
        replay.scenario_name, replay.t_sim, vel_kmh, status, replay.speed
    );
}

pub fn replay_is_active(replay: Res<ReplayState>) -> bool {
    replay.is_active()
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_core::{EdgeId, NodeId};
    use openrailsrs_track::{Edge, Node, NodeKind, TrackGraph};

    fn line_graph() -> TrackGraph {
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
            kind: NodeKind::Plain,
            x_m: 100.0,
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
    fn position_on_graph_mid_edge() {
        let g = line_graph();
        let (pos, _yaw) = position_on_graph(&g, "e1", 50.0, 2.0).unwrap();
        assert!((pos.x - 50.0).abs() < 1e-3);
        assert!((pos.y - 2.0).abs() < 1e-3);
    }

    #[test]
    fn pose_at_time_uses_last_row_at_end() {
        let g = line_graph();
        let rows = vec![
            CsvRow {
                time_s: 0.0,
                velocity_mps: 0.0,
                edge_id: "e1".into(),
                pos_on_edge_m: 0.0,
            },
            CsvRow {
                time_s: 10.0,
                velocity_mps: 5.0,
                edge_id: "e1".into(),
                pos_on_edge_m: 100.0,
            },
        ];
        let (pos, _, vel) = pose_at_time(&g, &rows, 99.0, 0.0).unwrap();
        assert!((pos.x - 100.0).abs() < 1e-3);
        assert!((vel - 5.0).abs() < 1e-6);
    }
}
