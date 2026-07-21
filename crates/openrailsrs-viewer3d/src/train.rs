//! Train replay: position markers from simulation CSV (same fields as viewer 2D).

use std::collections::HashMap;
use std::path::PathBuf;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use openrailsrs_track::TrackGraph;

use crate::camera::CameraFollowMode;
use crate::floating_origin::{FloatingOrigin, view_position};
use crate::launch::{ViewerSceneryMode, track_dev_render_enabled};
use crate::rolling_stock::TrainConsistScene;
use crate::shapes::{
    RouteAssets, ShapeRenderAsset, load_shape_render_asset_from_path, resolve_shape_path_in_dirs,
    vehicle_shape_local_transform, vehicle_texture_search_dirs,
};
use crate::terrain::{TerrainElevation, ground_y_at};
use crate::track::{TrackScene, graph_to_world_with_offset};
use crate::viewer_log;
use crate::world::{RouteFocus, RouteWorldOffset};

const COLOR_TRAIN_PRIMARY: Color = Color::srgb(1.0, 0.25, 1.0);
const COLOR_TRAIN_FALLBACK: Color = Color::srgb(0.95, 0.25, 0.85);

pub const TRAIN_COLORS: [Color; 4] = [
    Color::srgb(1.0, 0.25, 1.0),
    Color::srgb(0.25, 1.0, 1.0),
    Color::srgb(0.5, 1.0, 0.25),
    Color::srgb(1.0, 0.5, 0.25),
];

/// Opaque exterior parts cast sun shadows; blend/glass/additive skip (#41).
#[inline]
pub(crate) fn train_part_casts_shadow(is_transparent: bool) -> bool {
    !is_transparent
}

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

/// Absolute MSTS world position on a graph edge (terrain MSL, before render rebase).
pub fn graph_point_msts_world(
    graph: &TrackGraph,
    edge_id: &str,
    pos_on_edge_m: f64,
    terrain: Option<&TerrainElevation>,
    scene: &TrackScene,
    world_offset: Vec3,
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
    let mut world = graph_to_world_with_offset(world_offset, x_m, y_m);
    world.y = ground_y_at(terrain, world.x, world.z, scene);

    let dx = (to.x_m - from.x_m) as f32;
    let dz = (to.y_m - from.y_m) as f32;
    let yaw = if dx * dx + dz * dz > 1e-6 {
        -dz.atan2(dx)
    } else {
        0.0
    };

    Some((world, yaw))
}

/// World position and yaw (rad, around +Y) for a train on an edge.
pub fn position_on_graph(
    graph: &TrackGraph,
    edge_id: &str,
    pos_on_edge_m: f64,
    terrain: Option<&TerrainElevation>,
    scene: &TrackScene,
    world_offset: Vec3,
    focus: &RouteFocus,
) -> Option<(Vec3, f32)> {
    let (world, yaw) =
        graph_point_msts_world(graph, edge_id, pos_on_edge_m, terrain, scene, world_offset)?;
    Some((focus.to_render_surface(world), yaw))
}

/// Interpolate train world pose at simulation time `t`.
pub fn pose_at_time(
    graph: &TrackGraph,
    rows: &[CsvRow],
    t: f64,
    terrain: Option<&TerrainElevation>,
    scene: &TrackScene,
    world_offset: Vec3,
    focus: &RouteFocus,
) -> Option<(Vec3, f32, f64)> {
    if rows.is_empty() {
        return None;
    }
    let idx = rows
        .partition_point(|r| r.time_s <= t)
        .saturating_sub(1)
        .min(rows.len() - 1);
    let row = &rows[idx];
    let (pos, yaw) = position_on_graph(
        graph,
        &row.edge_id,
        row.pos_on_edge_m,
        terrain,
        scene,
        world_offset,
        focus,
    )?;
    Some((pos, yaw, row.velocity_mps))
}

#[derive(Component)]
pub struct TrainMarker {
    pub track_index: usize,
}

type ShapeCache = HashMap<PathBuf, ShapeRenderAsset>;

/// Spawn train visuals: consist meshes for the primary track when available, else cubes.
#[allow(clippy::too_many_arguments)]
pub fn spawn_train_markers(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    scene: Res<TrackScene>,
    offset: Res<RouteWorldOffset>,
    focus: Res<crate::world::RouteFocus>,
    replay: Res<ReplayState>,
    consist: Res<TrainConsistScene>,
    assets: Res<RouteAssets>,
    terrain: Option<Res<TerrainElevation>>,
    mode: Res<ViewerSceneryMode>,
) {
    if !replay.is_active() {
        return;
    }

    let terrain_ref = terrain.as_deref();
    let graph_world = scene.bounds.center + offset.delta;
    let fallback_y = ground_y_at(terrain_ref, graph_world.x, graph_world.z, &scene);
    let unit = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let mut shape_cache: ShapeCache = HashMap::new();
    let mut texture_cache: HashMap<PathBuf, Handle<Image>> = HashMap::new();
    let shape_dir_bufs = consist.shape_search_dirs(&assets.route_dir);
    let shape_dirs: Vec<&std::path::Path> = shape_dir_bufs.iter().map(|p| p.as_path()).collect();

    let driver_label = replay
        .tracks
        .first()
        .map(|t| t.label.as_str())
        .unwrap_or("primary");
    let driver_vehicles = consist.vehicles_for(driver_label);
    if !driver_vehicles.is_empty() {
        commands.insert_resource(crate::live::live_driver_cab_from_vehicles(
            driver_vehicles,
            &shape_dirs,
            &assets.route_dir,
        ));
    }

    let mut shape_mesh_count = 0usize;

    for (i, track) in replay.tracks.iter().enumerate() {
        let color = if track.color == COLOR_TRAIN_PRIMARY {
            TRAIN_COLORS[i % TRAIN_COLORS.len()]
        } else {
            track.color
        };

        let (pos, yaw, _) = pose_at_time(
            &scene.graph,
            &track.rows,
            0.0,
            terrain_ref,
            &scene,
            offset.delta,
            &focus,
        )
        .unwrap_or((
            focus.to_render_surface(scene.bounds.center + offset.delta + Vec3::Y * fallback_y),
            0.0,
            0.0,
        ));
        let head = Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw));

        if mode.is_track_dev() && !track_dev_render_enabled() {
            let unit = meshes.add(Cuboid::new(3.0, 3.5, 16.0));
            let material = materials.add(StandardMaterial {
                base_color: color,
                emissive: LinearRgba::from(color) * 2.0,
                unlit: true,
                ..default()
            });
            commands.spawn((
                Mesh3d(unit),
                MeshMaterial3d(material),
                head,
                TrainMarker { track_index: i },
                Name::new(format!("train:{}:track_dev", track.label)),
            ));
            continue;
        }

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
                        if let Some(shape_name) = vehicle
                            .shape_file
                            .as_deref()
                            .filter(|s| !s.eq_ignore_ascii_case("test.s"))
                        {
                            if let Some(shape_path) =
                                resolve_shape_path_in_dirs(&shape_dirs, shape_name)
                            {
                                let shape_path_key = shape_path.clone();
                                let asset = shape_cache
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
                                    .get(&asset.combined_mesh)
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
                                train
                                    .spawn((
                                        local,
                                        Visibility::default(),
                                        Name::new(format!(
                                            "train:{}:car:{vi}:{}",
                                            track.label, vehicle.name
                                        )),
                                    ))
                                    .with_children(|car| {
                                        for (pi, part) in asset.parts.iter().enumerate() {
                                            // Opaque exterior casts onto terrain (#41); glass/blend skip.
                                            let mut part_entity = car.spawn((
                                                Mesh3d(part.mesh.clone()),
                                                MeshMaterial3d(part.material.clone()),
                                                Transform::default(),
                                                Name::new(format!(
                                                    "train:{}:car:{vi}:{}:part:{pi}:{}",
                                                    track.label, vehicle.name, part.prim_state_idx
                                                )),
                                            ));
                                            if !train_part_casts_shadow(part.is_transparent) {
                                                part_entity.insert(NotShadowCaster);
                                            }
                                        }
                                    });
                                shape_mesh_count += asset.parts.len();
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
        viewer_log!(
            "openrailsrs-viewer3d: {} consist track(s), {} vehicle(s), {shape_mesh_count} shape mesh part(s)",
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
) -> ShapeRenderAsset {
    // Open Rails resolves rolling-stock textures from ReferencePath (trainset root),
    // not route TEXTURES/ — see `vehicle_texture_search_dirs`.
    let tex_dirs_owned = vehicle_texture_search_dirs(shape_path, route_dir);
    let tex_dirs: Vec<&std::path::Path> = tex_dirs_owned.iter().map(|p| p.as_path()).collect();

    load_shape_render_asset_from_path(
        shape_path,
        &tex_dirs,
        None,
        meshes,
        images,
        materials,
        texture_cache,
        fallback_color,
        true,
    )
    .unwrap_or_else(|| {
        let mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
        let material = materials.add(StandardMaterial {
            base_color: COLOR_TRAIN_FALLBACK,
            perceptual_roughness: 0.75,
            metallic: 0.1,
            ..default()
        });
        ShapeRenderAsset {
            combined_mesh: mesh.clone(),
            parts: vec![crate::shapes::ShapePartAsset {
                prim_state_idx: -1,
                sub_object_idx: u32::MAX,
                cab_matrix_idx: None,
                mesh,
                material,
                or_cab_material: None,
                has_texture: false,
                is_transparent: false,
                texture_name: None,
                shader_name: None,
                light_mat_idx: None,
                solid_color: None,
                lever_pivot_at_mesh_center: false,
                lever_local_axis: None,
                bounds_center: None,
            }],
            has_texture: false,
        }
    })
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
    offset: Res<RouteWorldOffset>,
    focus: Res<crate::world::RouteFocus>,
    replay: Res<ReplayState>,
    terrain: Option<Res<TerrainElevation>>,
    origin: Res<FloatingOrigin>,
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
        let Some((pos, yaw, _)) = pose_at_time(
            &scene.graph,
            &track.rows,
            replay.t_sim,
            terrain_ref,
            &scene,
            offset.delta,
            &focus,
        ) else {
            continue;
        };
        transform.translation = view_position(pos, &origin);
        transform.rotation = Quat::from_rotation_y(yaw);
    }
}

/// Hide the replay consist mesh in first-person driver view.
pub fn update_replay_train_visibility(
    follow: Res<crate::camera::CameraFollowMode>,
    mut markers: Query<&mut Visibility, With<TrainMarker>>,
) {
    let hide = *follow == CameraFollowMode::DriverCam;
    for mut vis in &mut markers {
        *vis = if hide {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
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

    fn test_focus() -> RouteFocus {
        RouteFocus {
            center: Vec3::ZERO,
            height_origin: 0.0,
        }
    }

    #[test]
    fn opaque_train_parts_cast_shadows_transparent_do_not() {
        assert!(train_part_casts_shadow(false));
        assert!(!train_part_casts_shadow(true));
    }

    #[test]
    fn position_on_graph_mid_edge() {
        let g = line_graph();
        let scene = TrackScene::from_graph(g.clone());
        let focus = test_focus();
        let expected_y = ground_y_at(None, 50.0, 0.0, &scene);
        let (pos, _yaw) =
            position_on_graph(&g, "e1", 50.0, None, &scene, Vec3::ZERO, &focus).unwrap();
        assert!((pos.x - 50.0).abs() < 1e-3);
        assert!((pos.y - expected_y).abs() < 1e-3);
    }

    #[test]
    fn position_on_graph_uses_to_render_surface_not_scenery_y() {
        let focus = RouteFocus {
            center: Vec3::new(1_000_000.0, 80.0, 2_000_000.0),
            height_origin: 1_050.0,
        };
        let g = line_graph();
        let scene = TrackScene::from_graph(g.clone());
        let offset = Vec3::new(1_000_000.0, 80.0, 2_000_000.0);
        let (pos, _) = position_on_graph(&g, "e1", 0.0, None, &scene, offset, &focus).unwrap();
        let msl_y = ground_y_at(None, 1_000_000.0, 2_000_000.0, &scene);
        assert!(
            (pos.y - (msl_y - focus.height_origin)).abs() < 1e-3,
            "expected MSL rebase, got {}",
            pos.y
        );
        let wrong_scenery_y = msl_y - focus.center.y;
        assert!(
            (pos.y - wrong_scenery_y).abs() > 500.0,
            "must not subtract bbox center.y ({}) for rail height",
            focus.center.y
        );
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
        let focus = test_focus();
        let (pos, _, vel) =
            pose_at_time(&g, &rows, 99.0, None, &scene, Vec3::ZERO, &focus).unwrap();
        assert!((pos.x - 100.0).abs() < 1e-3);
        assert!((vel - 5.0).abs() < 1e-6);
    }
}
