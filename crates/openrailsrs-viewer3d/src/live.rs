//! Live simulation bridge: `openrailsrs-sim` stepped each frame, train pose in 3D.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use openrailsrs_audio::{AudioCmd, AudioEngine};
use openrailsrs_scenarios::sound_regions::RegionTransition;
use openrailsrs_scenarios::{ScenarioFile, apply_scenario_runtime_overlay_dir};
use openrailsrs_sim::LiveDriveSession;

use crate::camera::{
    CHASE_PITCH, CameraFollowMode, LIVE_CHASE_DISTANCE, LiveDriverCab, OrbitState,
};
use crate::launch::LIVE_TRAIN_LOD_DISTANCE_M;
use crate::rolling_stock::TrainConsistScene;
use crate::shapes::{
    RouteAssets, load_shape_render_asset_from_path, resolve_shape_path_in_dirs,
    vehicle_shape_local_transform,
};
use crate::terrain::{TerrainElevation, ground_y_at};
use crate::track::TrackScene;
use crate::train::{TRAIN_COLORS, position_on_graph, vehicle_local_transform};

/// When present, the viewer runs the physics sim in real time instead of CSV replay.
#[derive(Resource)]
pub struct LiveDrive {
    pub session: LiveDriveSession,
    pub audio: Option<AudioEngine>,
    pub paused: bool,
    scenario_dir: PathBuf,
    scenario_path: PathBuf,
}

impl LiveDrive {
    pub fn from_scenario_path(path: &std::path::Path) -> Result<Self, String> {
        let scenario_dir = path
            .parent()
            .ok_or("scenario path has no parent directory")?;
        let mut scenario = openrailsrs_scenarios::load_scenario(path).map_err(|e| e.to_string())?;
        apply_scenario_runtime_overlay_dir(&mut scenario, scenario_dir)
            .map_err(|e| e.to_string())?;
        Self::from_scenario_with_paths(scenario_dir, path, &scenario)
    }

    pub fn from_scenario(
        scenario_dir: &std::path::Path,
        scenario: &ScenarioFile,
    ) -> Result<Self, String> {
        let scenario_path = scenario_dir.join("scenario.toml");
        Self::from_scenario_with_paths(scenario_dir, &scenario_path, scenario)
    }

    fn from_scenario_with_paths(
        scenario_dir: &std::path::Path,
        scenario_path: &std::path::Path,
        scenario: &ScenarioFile,
    ) -> Result<Self, String> {
        let session =
            LiveDriveSession::from_scenario(scenario_dir, scenario).map_err(|e| e.to_string())?;
        let audio = AudioEngine::try_start();
        if audio.is_none() {
            eprintln!("openrailsrs-viewer3d: no audio device — live drive is silent");
        }
        Ok(Self {
            session,
            audio,
            paused: false,
            scenario_dir: scenario_dir.to_path_buf(),
            scenario_path: scenario_path.to_path_buf(),
        })
    }

    /// Reset physics, gameplay and driver inputs to scenario start.
    pub fn reset(&mut self) -> Result<(), String> {
        let mut scenario =
            openrailsrs_scenarios::load_scenario(&self.scenario_path).map_err(|e| e.to_string())?;
        apply_scenario_runtime_overlay_dir(&mut scenario, &self.scenario_dir)
            .map_err(|e| e.to_string())?;
        self.session = LiveDriveSession::from_scenario(&self.scenario_dir, &scenario)
            .map_err(|e| e.to_string())?;
        self.paused = false;
        Ok(())
    }
}

fn apply_region_transition(audio: &AudioEngine, t: &RegionTransition) {
    match t {
        RegionTransition::Enter {
            id,
            kind,
            base_volume,
        } => {
            audio.send(AudioCmd::EnterRegion {
                id: id.clone(),
                kind: kind.clone(),
                base_volume: *base_volume,
            });
        }
        RegionTransition::Leave { id } => audio.send(AudioCmd::LeaveRegion { id: id.clone() }),
    }
}

pub fn live_mode_active(live: Option<Res<LiveDrive>>) -> bool {
    live.is_some()
}

pub fn live_mode_inactive(live: Option<Res<LiveDrive>>) -> bool {
    live.is_none()
}

/// Orbit framing for live mode: free pan/rotate (T cycles chase / driver later).
pub fn enable_live_defaults(
    mut follow: ResMut<CameraFollowMode>,
    mut orbit: Query<&mut OrbitState, With<Camera3d>>,
) {
    *follow = CameraFollowMode::Off;
    if let Ok(mut orbit) = orbit.single_mut() {
        orbit.distance = LIVE_CHASE_DISTANCE;
        orbit.pitch = CHASE_PITCH;
    }
}

pub fn advance_live_sim(time: Res<Time>, mut live: ResMut<LiveDrive>) {
    if live.paused {
        return;
    }
    let audio = live.audio.take();
    live.session.step_realtime(time.delta_secs() as f64, |t| {
        if let Some(ref a) = audio {
            apply_region_transition(a, t);
        }
    });
    live.audio = audio;
}

/// Engine / brake loops once per frame (independent of physics sub-steps).
pub fn live_audio_frame(live: Res<LiveDrive>) {
    let Some(ref audio) = live.audio else {
        return;
    };
    audio.send(AudioCmd::SetVelocity(live.session.velocity_mps()));
    audio.send(AudioCmd::SetBraking(live.session.driver_brake));
}

/// Driver controls (same as `openrailsrs cab`). Throttle/brake use arrow keys so W/S stay free for camera pan.
pub fn live_driver_input(keys: Res<ButtonInput<KeyCode>>, mut live: ResMut<LiveDrive>) {
    if keys.just_pressed(KeyCode::KeyP) {
        live.paused = !live.paused;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        if let Err(err) = live.reset() {
            eprintln!("openrailsrs-viewer3d: live reset failed: {err}");
        }
    }
    let throttle_up = keys.just_pressed(KeyCode::ArrowUp) || keys.just_pressed(KeyCode::PageUp);
    let throttle_down =
        keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::PageDown);
    if throttle_up {
        live.session.driver_brake = 0.0;
        live.session.driver_throttle = (live.session.driver_throttle + 0.1).min(1.0);
    }
    if throttle_down {
        if live.session.driver_throttle > 0.0 {
            live.session.driver_throttle = (live.session.driver_throttle - 0.1).max(0.0);
        } else {
            live.session.driver_brake = (live.session.driver_brake + 0.15).min(1.0);
        }
    }
    if keys.just_pressed(KeyCode::Space) {
        live.session.driver_throttle = 0.0;
        live.session.driver_brake = 1.0;
    }
    if keys.just_pressed(KeyCode::KeyH) {
        if let Some(ref audio) = live.audio {
            audio.send(AudioCmd::Horn);
        }
    }
    if keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd) {
        live.session.speed_mul = (live.session.speed_mul * 2.0).min(16.0);
    }
    if keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::NumpadSubtract) {
        live.session.speed_mul = (live.session.speed_mul / 2.0).max(0.25);
    }
}

pub fn update_live_train_marker(
    scene: Res<TrackScene>,
    offset: Res<crate::world::RouteWorldOffset>,
    focus: Res<crate::world::RouteFocus>,
    live: Res<LiveDrive>,
    terrain: Option<Res<TerrainElevation>>,
    mut query: Query<&mut Transform, With<LiveTrainMarker>>,
) {
    let Some(edge) = live.session.current_edge_id() else {
        return;
    };
    let Some((pos, yaw)) = position_on_graph(
        &scene.graph,
        edge,
        live.session.pos_on_edge_m(),
        terrain.as_deref(),
        &scene,
        offset.delta,
        &focus,
    ) else {
        return;
    };
    for mut transform in &mut query {
        transform.translation = pos;
        transform.rotation = Quat::from_rotation_y(yaw);
    }
}

#[derive(Component)]
pub struct LiveTrainMarker;

/// Visual mesh under the live train (hidden in driver view).
#[derive(Component)]
pub struct LiveTrainBody;

/// Hide the consist mesh in first-person driver view (avoids clipping through the cab).
pub fn update_driver_train_visibility(
    follow: Res<CameraFollowMode>,
    mut bodies: Query<&mut Visibility, With<LiveTrainBody>>,
) {
    let hide = *follow == CameraFollowMode::DriverCam;
    for mut vis in &mut bodies {
        *vis = if hide {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
}

fn live_driver_cab_from_vehicles(
    vehicles: &[crate::rolling_stock::ConsistVehicleVisual],
) -> LiveDriverCab {
    let head_len = vehicles.first().map(|v| v.length_m).unwrap_or(20.0);
    // Lead cab: eye a short way behind the nose (DMU/loco front is at train head).
    LiveDriverCab {
        back_m: (head_len * 0.15).clamp(1.8, 4.5),
        height_m: (head_len * 0.14).clamp(2.4, 3.2),
    }
}

/// Spawn the player consist (reuses rolling-stock mesh path from `spawn_train_markers`).
#[allow(clippy::too_many_arguments)]
pub fn spawn_live_train(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    scene: Res<TrackScene>,
    offset: Res<crate::world::RouteWorldOffset>,
    focus: Res<crate::world::RouteFocus>,
    consist: Res<TrainConsistScene>,
    assets: Res<RouteAssets>,
    terrain: Option<Res<TerrainElevation>>,
    live: Res<LiveDrive>,
) {
    let terrain_ref = terrain.as_deref();
    let edge = live.session.current_edge_id().unwrap_or(
        scene
            .graph
            .edges_iter()
            .next()
            .map(|(id, _)| id)
            .unwrap_or(""),
    );
    let (pos, yaw) = position_on_graph(
        &scene.graph,
        edge,
        live.session.pos_on_edge_m(),
        terrain_ref,
        &scene,
        offset.delta,
        &focus,
    )
    .unwrap_or({
        let w = scene.bounds.center + offset.delta;
        let y = ground_y_at(terrain_ref, w.x, w.z, &scene);
        (focus.to_render_surface(w + Vec3::Y * y), 0.0)
    });

    let vehicles = consist.vehicles_for("primary");
    let driver_cab = live_driver_cab_from_vehicles(vehicles);
    commands.insert_resource(driver_cab);
    if vehicles.is_empty() {
        let unit = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
        let material = materials.add(StandardMaterial {
            base_color: TRAIN_COLORS[0],
            emissive: LinearRgba::from(TRAIN_COLORS[0]) * 0.85,
            ..default()
        });
        commands.spawn((
            LiveTrainMarker,
            LiveTrainBody,
            Mesh3d(unit),
            MeshMaterial3d(material),
            Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)),
            Name::new("train:live:fallback"),
        ));
        eprintln!("openrailsrs-viewer3d: live mode — consist mesh missing, using cube");
        return;
    }

    let head = Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw));
    let shape_dir_bufs = consist.shape_search_dirs(&assets.route_dir);
    let shape_dirs: Vec<&std::path::Path> = shape_dir_bufs.iter().map(|p| p.as_path()).collect();
    let unit = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let color = TRAIN_COLORS[0];
    let mut texture_cache: HashMap<PathBuf, Handle<Image>> = HashMap::new();

    let locator = meshes.add(Sphere::new(6.0));
    let locator_mat = materials.add(StandardMaterial {
        base_color: TRAIN_COLORS[0],
        emissive: LinearRgba::from(TRAIN_COLORS[0]) * 2.0,
        ..default()
    });

    commands
        .spawn((
            LiveTrainMarker,
            head,
            Visibility::default(),
            Name::new("train:live"),
        ))
        .with_children(|train| {
            train.spawn((
                LiveTrainBody,
                NotShadowCaster,
                Mesh3d(locator.clone()),
                MeshMaterial3d(locator_mat),
                Transform::from_translation(Vec3::new(0.0, 4.0, 0.0)),
                Name::new("train:live:locator"),
            ));
            for (vi, vehicle) in vehicles.iter().enumerate() {
                if let Some(shape_name) = vehicle.shape_file.as_deref() {
                    if let Some(shape_path) = resolve_shape_path_in_dirs(&shape_dirs, shape_name) {
                        let trainset_root = shape_path
                            .parent()
                            .and_then(|p| p.parent())
                            .filter(|p| *p != assets.route_dir.as_path());
                        let tex_dirs: Vec<&Path> = std::iter::once(assets.route_dir.as_path())
                            .chain(trainset_root)
                            .collect();
                        if let Some(asset) = load_shape_render_asset_from_path(
                            &shape_path,
                            &tex_dirs,
                            Some(LIVE_TRAIN_LOD_DISTANCE_M),
                            &mut meshes,
                            &mut images,
                            &mut materials,
                            &mut texture_cache,
                            color,
                        ) {
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
                                    LiveTrainBody,
                                    local,
                                    Visibility::default(),
                                    Name::new(format!("train:live:car:{vi}")),
                                ))
                                .with_children(|car| {
                                    for (pi, part) in asset.parts.iter().enumerate() {
                                        car.spawn((
                                            LiveTrainBody,
                                            NotShadowCaster,
                                            Mesh3d(part.mesh.clone()),
                                            MeshMaterial3d(part.material.clone()),
                                            Transform::default(),
                                            Name::new(format!(
                                                "train:live:car:{vi}:part:{pi}:{}",
                                                part.prim_state_idx
                                            )),
                                        ));
                                    }
                                });
                            continue;
                        }
                    }
                }
                let material = materials.add(StandardMaterial {
                    base_color: color,
                    emissive: LinearRgba::from(color) * 0.85,
                    ..default()
                });
                let mut local = vehicle_local_transform(&scene, vehicle.offset_m, vehicle.length_m);
                local.scale *= 1.15;
                train.spawn((
                    LiveTrainBody,
                    NotShadowCaster,
                    Mesh3d(unit.clone()),
                    MeshMaterial3d(material),
                    local,
                    Name::new(format!("train:live:car:{vi}:fallback")),
                ));
            }
        });

    eprintln!(
        "openrailsrs-viewer3d: live drive — {} vehicle(s), dt={:.2}s, audio={}, cab back={:.1}m height={:.1}m",
        vehicles.len(),
        live.session.dt,
        live.audio.is_some(),
        driver_cab.back_m,
        driver_cab.height_m,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rolling_stock::ConsistVehicleVisual;

    #[test]
    fn pullman_dmbsa_driver_cab_near_front() {
        let cab = live_driver_cab_from_vehicles(&[ConsistVehicleVisual {
            name: "DMBSA".into(),
            shape_file: Some("RF_WP_DMBSA.s".into()),
            length_m: 20.879,
            offset_m: 0.0,
        }]);
        assert!(
            cab.back_m >= 1.8 && cab.back_m <= 4.5,
            "back={}",
            cab.back_m
        );
        assert!(
            cab.height_m >= 2.4 && cab.height_m <= 3.2,
            "height={}",
            cab.height_m
        );
        assert!((cab.back_m - 3.13).abs() < 0.2);
    }
}
