//! Train replay: position markers from simulation CSV (same fields as viewer 2D).

use std::collections::HashMap;
use std::path::PathBuf;

use bevy::prelude::*;
use openrailsrs_track::TrackGraph;

use crate::rolling_stock::TrainConsistScene;
use crate::shapes::{
    RouteAssets, load_ace_image, load_shape_from_path, resolve_shape_path_in_dirs,
    vehicle_shape_local_transform,
};
use crate::terrain::{TerrainElevation, ground_y_at};
use crate::track::{TrackScene, graph_to_world};

const COLOR_TRAIN_PRIMARY: Color = Color::srgb(1.0, 0.25, 1.0);
const COLOR_TRAIN_FALLBACK: Color = Color::srgb(0.95, 0.25, 0.85);

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
    terrain: Option<&TerrainElevation>,
    scene: &TrackScene,
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
    world.y = ground_y_at(terrain, world.x, world.z, scene);

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
    terrain: Option<&TerrainElevation>,
    scene: &TrackScene,
) -> Option<(Vec3, f32, f64)> {
    if rows.is_empty() {
        return None;
    }
    let idx = rows
        .partition_point(|r| r.time_s <= t)
        .saturating_sub(1)
        .min(rows.len() - 1);
    let row = &rows[idx];
    let (pos, yaw) = position_on_graph(graph, &row.edge_id, row.pos_on_edge_m, terrain, scene)?;
    Some((pos, yaw, row.velocity_mps))
}

#[derive(Component)]
pub struct TrainMarker {
    pub track_index: usize,
}

type ShapeCache = HashMap<PathBuf, (Handle<Mesh>, Handle<StandardMaterial>, bool)>;

/// Spawn train visuals: consist meshes for the primary track when available, else cubes.
#[allow(clippy::too_many_arguments)]
pub fn spawn_train_markers(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    scene: Res<TrackScene>,
    replay: Res<ReplayState>,
    consist: Res<TrainConsistScene>,
    assets: Res<RouteAssets>,
    terrain: Option<Res<TerrainElevation>>,
) {
    if !replay.is_active() {
        return;
    }

    let terrain_ref = terrain.as_deref();
    let fallback_y = ground_y_at(
        terrain_ref,
        scene.bounds.center.x,
        scene.bounds.center.z,
        &scene,
    );
    let unit = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let mut shape_cache: ShapeCache = HashMap::new();
    let mut texture_cache: HashMap<PathBuf, Handle<Image>> = HashMap::new();
    let shape_dir_bufs = consist.shape_search_dirs(&assets.route_dir);
    let shape_dirs: Vec<&std::path::Path> = shape_dir_bufs.iter().map(|p| p.as_path()).collect();

    let mut shape_mesh_count = 0usize;

    for (i, track) in replay.tracks.iter().enumerate() {
        let color = if track.color == COLOR_TRAIN_PRIMARY {
            TRAIN_COLORS[i % TRAIN_COLORS.len()]
        } else {
            track.color
        };

        let (pos, yaw, _) = pose_at_time(&scene.graph, &track.rows, 0.0, terrain_ref, &scene)
            .unwrap_or((scene.bounds.center + Vec3::Y * fallback_y, 0.0, 0.0));
        let head = Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw));

        if !consist.vehicles_for(&track.label).is_empty() {
            let vehicles = consist.vehicles_for(&track.label);
            commands
                .spawn((
                    TrainMarker { track_index: i },
                    head,
                    Visibility::default(),
                    Name::new(format!("train:{}", track.label)),
                ))
                .with_children(|train| {
                    for (vi, vehicle) in vehicles.iter().enumerate() {
                        if let Some(shape_name) = vehicle.shape_file.as_deref() {
                            if let Some(shape_path) =
                                resolve_shape_path_in_dirs(&shape_dirs, shape_name)
                            {
                                let shape_path_key = shape_path.clone();
                                let (mesh, material, _) = shape_cache
                                    .entry(shape_path_key)
                                    .or_insert_with(|| {
                                        load_vehicle_shape_assets(
                                            &shape_path,
                                            &assets.route_dir,
                                            &mut meshes,
                                            &mut images,
                                            &mut materials,
                                            &mut texture_cache,
                                            color,
                                        )
                                    })
                                    .clone();
                                let local = meshes
                                    .get(&mesh)
                                    .map(|m| {
                                        vehicle_shape_local_transform(
                                            m,
                                            vehicle.offset_m,
                                            vehicle.length_m,
                                        )
                                    })
                                    .unwrap_or_else(|| {
                                        vehicle_local_transform(
                                            &scene,
                                            vehicle.offset_m,
                                            vehicle.length_m,
                                        )
                                    });
                                train.spawn((
                                    Mesh3d(mesh),
                                    MeshMaterial3d(material),
                                    local,
                                    Name::new(format!(
                                        "train:{}:car:{vi}:{}",
                                        track.label, vehicle.name
                                    )),
                                ));
                                shape_mesh_count += 1;
                                continue;
                            }
                        }

                        let local =
                            vehicle_local_transform(&scene, vehicle.offset_m, vehicle.length_m);

                        let material = materials.add(StandardMaterial {
                            base_color: color,
                            perceptual_roughness: 0.55,
                            metallic: 0.15,
                            emissive: LinearRgba::from(color) * 0.25,
                            ..default()
                        });
                        train.spawn((
                            Mesh3d(unit.clone()),
                            MeshMaterial3d(material),
                            local,
                            Name::new(format!(
                                "train:{}:car:{vi}:{}:fallback",
                                track.label, vehicle.name
                            )),
                        ));
                    }
                });
            continue;
        }

        let body_len = scene.bounds.edge_radius() * 8.0;
        let body_w = scene.bounds.edge_radius() * 3.0;
        let body_h = scene.bounds.edge_radius() * 2.5;
        let material = materials.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 0.55,
            metallic: 0.15,
            emissive: LinearRgba::from(color) * 0.35,
            ..default()
        });

        commands.spawn((
            Mesh3d(unit.clone()),
            MeshMaterial3d(material),
            head.with_scale(Vec3::new(body_len, body_h, body_w)),
            TrainMarker { track_index: i },
            Name::new(format!("train:{}", track.label)),
        ));
    }

    if !consist.is_empty() {
        eprintln!(
            "openrailsrs-viewer3d: {} consist track(s), {} vehicle(s), {shape_mesh_count} shape mesh(es)",
            consist.track_count(),
            consist.total_vehicles(),
        );
    }
}

pub(crate) fn vehicle_local_transform(
    scene: &TrackScene,
    offset_m: f32,
    length_m: f32,
) -> Transform {
    let edge = scene.bounds.edge_radius().max(2.0);
    Transform {
        translation: Vec3::new(offset_m, 0.0, 0.0),
        scale: Vec3::new(length_m.max(edge * 2.0), edge * 2.0, edge * 2.5),
        ..default()
    }
}

fn load_vehicle_shape_assets(
    shape_path: &std::path::Path,
    route_dir: &std::path::Path,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    texture_cache: &mut HashMap<PathBuf, Handle<Image>>,
    fallback_color: Color,
) -> (Handle<Mesh>, Handle<StandardMaterial>, bool) {
    match load_shape_from_path(shape_path, None) {
        Some(loaded) => {
            let mesh = meshes.add(loaded.mesh);
            if let Some(tex_name) = loaded.texture_file {
                if let Some(image) = load_ace_image(route_dir, &tex_name) {
                    let handle = texture_cache
                        .entry(route_dir.join("TEXTURES").join(&tex_name))
                        .or_insert_with(|| images.add(image))
                        .clone();
                    let material = materials.add(StandardMaterial {
                        base_color: Color::WHITE,
                        base_color_texture: Some(handle),
                        perceptual_roughness: 0.85,
                        metallic: 0.05,
                        double_sided: true,
                        ..default()
                    });
                    return (mesh, material, true);
                }
            }
            let material = materials.add(StandardMaterial {
                base_color: fallback_color,
                emissive: LinearRgba::from(fallback_color) * 0.35,
                perceptual_roughness: 0.75,
                metallic: 0.1,
                double_sided: true,
                ..default()
            });
            (mesh, material, false)
        }
        None => {
            let material = materials.add(StandardMaterial {
                base_color: COLOR_TRAIN_FALLBACK,
                perceptual_roughness: 0.75,
                metallic: 0.1,
                ..default()
            });
            (meshes.add(Cuboid::new(1.0, 1.0, 1.0)), material, false)
        }
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
    terrain: Option<Res<TerrainElevation>>,
    mut query: Query<(&TrainMarker, &mut Transform), Without<Camera3d>>,
) {
    if !replay.is_active() {
        return;
    }

    let terrain_ref = terrain.as_deref();

    for (marker, mut transform) in &mut query {
        let Some(track) = replay.tracks.get(marker.track_index) else {
            continue;
        };
        let Some((pos, yaw, _)) =
            pose_at_time(&scene.graph, &track.rows, replay.t_sim, terrain_ref, &scene)
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
        let scene = TrackScene::from_graph(g.clone());
        let expected_y = ground_y_at(None, 50.0, 0.0, &scene);
        let (pos, _yaw) = position_on_graph(&g, "e1", 50.0, None, &scene).unwrap();
        assert!((pos.x - 50.0).abs() < 1e-3);
        assert!((pos.y - expected_y).abs() < 1e-3);
    }

    #[test]
    fn pose_at_time_uses_last_row_at_end() {
        let g = line_graph();
        let scene = TrackScene::from_graph(g.clone());
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
        let (pos, _, vel) = pose_at_time(&g, &rows, 99.0, None, &scene).unwrap();
        assert!((pos.x - 100.0).abs() < 1e-3);
        assert!((vel - 5.0).abs() < 1e-6);
    }
}
