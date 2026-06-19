//! Live simulation bridge: `openrailsrs-sim` stepped each frame, train pose in 3D.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use openrailsrs_audio::{AudioCmd, AudioEngine};
use openrailsrs_scenarios::sound_regions::RegionTransition;
use openrailsrs_scenarios::{ScenarioFile, apply_scenario_runtime_overlay_dir};
use openrailsrs_sim::LiveDriveSession;

use crate::cab_view::{CabLeadVehicle, CabTrainParent, orts_3d_cab_for_vehicle};
use crate::camera::{
    CHASE_PITCH, CameraFollowMode, LIVE_CHASE_DISTANCE, LiveDriverCab, OrbitDistanceLimit,
    OrbitState, camera_transform_from_orbit_state, chase_yaw_from_train, clamp_distance_to_limit,
    orbit_user_zoom_max, yaw_from_transform,
};
use crate::floating_origin::{FloatingOrigin, view_position};
use crate::launch::{LIVE_TRAIN_LOD_DISTANCE_M, ViewerSceneryMode, track_dev_render_enabled};
use crate::rolling_stock::TrainConsistScene;
use crate::shapes::{
    RouteAssets, load_shape_from_path, load_shape_render_asset_from_path,
    resolve_vehicle_shape_path, vehicle_cab_frame_and_exterior_scale,
    vehicle_shape_local_transform, vehicle_texture_root_for_shape_path,
};
use crate::terrain::{TerrainElevation, ground_y_at};
use crate::track::TrackScene;
use crate::train::{TRAIN_COLORS, position_on_graph, vehicle_local_transform};
use crate::world::visible_radius_m;
use crate::{log_step, viewer_log};

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
        let mut session =
            LiveDriveSession::from_scenario(scenario_dir, scenario).map_err(|e| e.to_string())?;
        // Optional time-compression for demos / headless capture (e.g. the ~29 km Chiltern run).
        if let Some(mul) = std::env::var("OPENRAILSRS_SPEED_MUL")
            .ok()
            .and_then(|v| v.trim().parse::<f64>().ok())
            .filter(|m| *m >= 0.1 && *m <= 64.0)
        {
            session.speed_mul = mul;
            viewer_log!(
                "openrailsrs-viewer3d: live sim speed_mul = {mul:.1}x (OPENRAILSRS_SPEED_MUL)"
            );
        }
        let audio = None; // AudioEngine::try_start(); // Desactivado por pedido del usuario para evitar ruido
        if audio.is_none() {
            viewer_log!(
                "openrailsrs-viewer3d: no audio device or audio disabled — live drive is silent"
            );
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
#[allow(clippy::type_complexity)]
pub fn enable_live_defaults(
    mut follow: ResMut<CameraFollowMode>,
    scene: Res<crate::track::TrackScene>,
    focus: Res<crate::world::RouteFocus>,
    mode: Res<ViewerSceneryMode>,
    mut limit: ResMut<OrbitDistanceLimit>,
    train: Query<&Transform, (With<LiveTrainMarker>, Without<Camera3d>)>,
    mut cam: Query<(&mut Transform, &mut OrbitState), (With<Camera3d>, Without<LiveTrainMarker>)>,
) {
    *follow = if mode.is_run_corridor() {
        CameraFollowMode::ChaseCam
    } else {
        CameraFollowMode::Off
    };
    // Optional override for demos / capture: keep the camera locked to the train.
    if let Ok(mode) = std::env::var("OPENRAILSRS_FOLLOW") {
        match mode.trim().to_ascii_lowercase().as_str() {
            "chase" => *follow = CameraFollowMode::ChaseCam,
            "driver" | "cab" => *follow = CameraFollowMode::DriverCam,
            "orbit" | "orbitfollow" => *follow = CameraFollowMode::OrbitFollow,
            "off" => *follow = CameraFollowMode::Off,
            _ => {}
        }
    }
    limit.max = scene
        .bounds
        .orbit_distance()
        .max(visible_radius_m())
        .min(orbit_user_zoom_max());
    let Ok((mut transform, mut orbit)) = cam.single_mut() else {
        return;
    };
    let (focus_pt, yaw) = if let Ok(train_tf) = train.single() {
        (
            train_tf.translation + Vec3::Y * 2.0,
            yaw_from_transform(train_tf),
        )
    } else {
        (focus.to_render_surface(focus.center), 0.0)
    };
    orbit.focus = focus_pt;
    orbit.yaw = chase_yaw_from_train(yaw);
    orbit.pitch = if mode.is_run_corridor() {
        0.12
    } else {
        CHASE_PITCH
    };
    orbit.distance = clamp_distance_to_limit(
        if mode.is_run_corridor() {
            32.0
        } else {
            LIVE_CHASE_DISTANCE
        },
        orbit_user_zoom_max(),
    );
    *orbit = crate::camera::orbit_state_with_env_overrides(*orbit);
    *transform =
        camera_transform_from_orbit_state(orbit.focus, orbit.yaw, orbit.pitch, orbit.distance);
    if mode.is_run_corridor() {
        viewer_log!(
            "openrailsrs-viewer3d: run_corridor — camera chase init dist={:.0}m pitch={:.0}°",
            orbit.distance,
            orbit.pitch.to_degrees()
        );
    }
}

pub fn advance_live_sim(time: Res<Time>, mut live: ResMut<LiveDrive>) {
    if live.paused {
        return;
    }
    let was_arrived = live.session.arrived;
    let audio = live.audio.take();
    live.session.step_realtime(time.delta_secs() as f64, |t| {
        if let Some(ref a) = audio {
            apply_region_transition(a, t);
        }
    });
    live.audio = audio;
    if live.session.arrived && !was_arrived {
        viewer_log!(
            "openrailsrs-viewer3d: train ARRIVED at destination \"{}\" — t={:.0}s, {:.0}m travelled",
            live.session.gameplay.destination,
            live.session.time_s(),
            live.session.state.odometer_m,
        );
    }
}

/// Autopilot for demos / headless capture: drives toward the destination holding a
/// target notch, easing off near the speed limit. Opt in with `OPENRAILSRS_AUTODRIVE`
/// (truthy, or a `0..1` notch). Lets the train reach the next station hands-free.
pub fn autodrive_enabled() -> bool {
    std::env::var_os("OPENRAILSRS_AUTODRIVE").is_some_and(|v| !v.is_empty() && v != "0")
}

fn autodrive_notch() -> f64 {
    std::env::var("OPENRAILSRS_AUTODRIVE")
        .ok()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|v| *v > 0.0 && *v <= 1.0)
        .unwrap_or(1.0)
}

pub fn live_autodrive(mut live: ResMut<LiveDrive>) {
    if live.paused || live.session.arrived {
        return;
    }
    let target = autodrive_notch();
    let v = live.session.velocity_mps();
    let limit = live.session.effective_speed_limit_mps();
    let cap = if limit.is_finite() {
        limit * 0.95
    } else {
        f64::INFINITY
    };
    if v < cap {
        live.session.driver_brake = 0.0;
        live.session.driver_throttle = target;
    } else {
        live.session.driver_throttle = 0.0;
        live.session.driver_brake = 0.2;
    }
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
            viewer_log!("openrailsrs-viewer3d: live reset failed: {err}");
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
    origin: Res<FloatingOrigin>,
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
    let pos = view_position(pos, &origin);
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

/// Tracks the last driver-cam state to emit the diagnostic log only once per transition.
#[derive(Resource, Default)]
pub struct DriverCamState {
    /// `true` when driver view was active on the previous frame.
    pub was_driver: bool,
}

/// Hide the consist mesh in first-person driver view (cab interior stays visible).
///
/// CAB-P2: traverses the **full** `LiveTrainBody` hierarchy (including children
/// of children) so that any mesh entity under the consist root is hidden.  Only
/// entities carrying [`CabInteriorMarker`] remain visible in driver view.
///
/// Emits a single diagnostic log line each time the view mode changes.
pub fn update_driver_train_visibility(
    follow: Res<CameraFollowMode>,
    mut cam_state: ResMut<DriverCamState>,
    bodies: Query<
        Entity,
        (
            With<LiveTrainBody>,
            Without<crate::cab_view::CabInteriorMarker>,
        ),
    >,
    children_query: Query<&Children>,
    mut visibility_query: Query<&mut Visibility, Without<crate::cab_view::CabInteriorMarker>>,
    mut cab_parts: Query<&mut Visibility, With<crate::cab_view::CabInteriorMarker>>,
) {
    let hide = *follow == CameraFollowMode::DriverCam;
    let mode_changed = hide != cam_state.was_driver;
    cam_state.was_driver = hide;

    let visibility = if hide {
        Visibility::Hidden
    } else {
        Visibility::Visible
    };

    let mut exterior_count = 0usize;
    for entity in &bodies {
        set_visibility_recursive(
            entity,
            visibility,
            &children_query,
            &mut visibility_query,
            &mut exterior_count,
        );
    }

    let mut cab_count = 0usize;
    if hide {
        for mut vis in &mut cab_parts {
            *vis = Visibility::Visible;
            cab_count += 1;
        }
    }

    if mode_changed {
        if hide {
            viewer_log!(
                "openrailsrs-viewer3d: driver view → {exterior_count} exterior hidden, \
                 {cab_count} cab parts visible"
            );
        } else {
            viewer_log!("openrailsrs-viewer3d: chase view → {exterior_count} exterior visible");
        }
    }
}

fn set_visibility_recursive(
    entity: Entity,
    visibility: Visibility,
    children_query: &Query<&Children>,
    visibility_query: &mut Query<&mut Visibility, Without<crate::cab_view::CabInteriorMarker>>,
    count: &mut usize,
) {
    if let Ok(mut vis) = visibility_query.get_mut(entity) {
        if *vis != visibility {
            *vis = visibility;
        }
        *count += 1;
    }
    if let Ok(children) = children_query.get(entity) {
        for &child in children {
            set_visibility_recursive(child, visibility, children_query, visibility_query, count);
        }
    }
}

pub(crate) fn driver_cab_from_lead_vehicle(
    vehicle: &crate::rolling_stock::ConsistVehicleVisual,
    shape_dirs: &[&std::path::Path],
    route_dir: &std::path::Path,
    lead_mesh: &bevy::mesh::Mesh,
) -> LiveDriverCab {
    let head_len = vehicle.length_m;
    let placement =
        crate::shapes::cab_shape_placement_transform(lead_mesh, vehicle.offset_m, vehicle.length_m);
    let mut cab = LiveDriverCab {
        back_m: (head_len * 0.15).clamp(1.8, 4.5),
        height_m: (head_len * 0.14).clamp(2.4, 3.2),
        interior_placement: placement,
        ..Default::default()
    };
    if let Some(shape_name) = vehicle.shape_file.as_deref() {
        if let Some(orts) = orts_3d_cab_for_vehicle(shape_dirs, shape_name, route_dir) {
            cab.head_msts = Some(orts.head_pos_msts);
            let head_bevy = crate::shapes::msts_shape_vec3_to_bevy(orts.head_pos_msts);
            cab.head_lead_local = Some(head_bevy);
            cab.head_pos_train = Some(placement.transform_point(head_bevy));
            cab.look_pitch = orts.look_pitch;
        }
    }
    cab
}

pub(crate) fn live_driver_cab_from_vehicles(
    vehicles: &[crate::rolling_stock::ConsistVehicleVisual],
    shape_dirs: &[&std::path::Path],
    route_dir: &std::path::Path,
) -> LiveDriverCab {
    let head_len = vehicles.first().map(|v| v.length_m).unwrap_or(20.0);
    let mut cab = LiveDriverCab {
        back_m: (head_len * 0.15).clamp(1.8, 4.5),
        height_m: (head_len * 0.14).clamp(2.4, 3.2),
        ..Default::default()
    };
    if let Some(vehicle) = vehicles.first() {
        if let Some(shape_name) = vehicle.shape_file.as_deref() {
            if let Some(shape_path) = resolve_vehicle_shape_path(shape_dirs, shape_name, route_dir)
            {
                if let Some(loaded) =
                    load_shape_from_path(&shape_path, Some(LIVE_TRAIN_LOD_DISTANCE_M))
                {
                    return driver_cab_from_lead_vehicle(
                        vehicle,
                        shape_dirs,
                        route_dir,
                        &loaded.mesh,
                    );
                }
            }
            if let Some(orts) = orts_3d_cab_for_vehicle(shape_dirs, shape_name, route_dir) {
                cab.look_pitch = orts.look_pitch;
            }
        }
    }
    cab
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
    mode: Res<ViewerSceneryMode>,
    terrain: Option<Res<TerrainElevation>>,
    live: Res<LiveDrive>,
) {
    viewer_log!("openrailsrs-viewer3d: spawning live train");
    let spawn_start = Instant::now();

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
    let shape_dir_bufs = consist.shape_search_dirs(&assets.route_dir);
    let shape_dirs: Vec<&std::path::Path> = shape_dir_bufs.iter().map(|p| p.as_path()).collect();
    let driver_cab = live_driver_cab_from_vehicles(vehicles, &shape_dirs, &assets.route_dir);
    commands.insert_resource(driver_cab);

    if mode.is_track_dev() && !track_dev_render_enabled() {
        let unit = meshes.add(Cuboid::new(2.0, 2.5, 14.0));
        let material = materials.add(StandardMaterial {
            base_color: TRAIN_COLORS[0],
            emissive: LinearRgba::from(TRAIN_COLORS[0]) * 0.5,
            ..default()
        });
        commands.spawn((
            LiveTrainMarker,
            LiveTrainBody,
            NotShadowCaster,
            Mesh3d(unit),
            MeshMaterial3d(material),
            Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)),
            Name::new("train:live:track_dev"),
        ));
        viewer_log!("openrailsrs-viewer3d: track_dev — live train as box (no Pullman meshes)");
        log_step("spawned live train (track_dev box)", spawn_start);
        return;
    }

    if let Some(head) = driver_cab.head_pos_train {
        viewer_log!(
            "openrailsrs-viewer3d: ORTS cab head train-local ({:.2}, {:.2}, {:.2}) pitch={:.1}°",
            head.x,
            head.y,
            head.z,
            driver_cab.look_pitch.to_degrees(),
        );
    } else {
        viewer_log!(
            "openrailsrs-viewer3d: cab fallback eye back={:.1}m height={:.1}m (no ORTS3DCabHeadPos)",
            driver_cab.back_m,
            driver_cab.height_m,
        );
    }
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
            NotShadowCaster,
            Mesh3d(unit),
            MeshMaterial3d(material),
            Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)),
            Name::new("train:live:fallback"),
        ));
        viewer_log!("openrailsrs-viewer3d: live mode — consist mesh missing, using cube");
        log_step("spawned live train (fallback cube)", spawn_start);
        return;
    }

    let head = Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw));
    const TRAIN_SHAPE_FALLBACK: Color = Color::srgb(0.55, 0.58, 0.62);
    let mut texture_cache: HashMap<PathBuf, Handle<Image>> = HashMap::new();
    let mut shape_cars = 0usize;
    let mut fallback_cars = 0usize;
    let mut shape_parts = 0usize;
    let mut textured_parts = 0usize;

    let locator = meshes.add(Sphere::new(1.2));
    let locator_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.9, 0.3),
        emissive: LinearRgba::new(0.2, 1.8, 0.3, 1.0),
        ..default()
    });

    commands
        .spawn((
            LiveTrainMarker,
            CabTrainParent,
            head,
            Visibility::default(),
            Name::new("train:live"),
        ))
        .with_children(|train| {
            if !mode.is_run_corridor() {
                train.spawn((
                    LiveTrainBody,
                    NotShadowCaster,
                    Mesh3d(locator.clone()),
                    MeshMaterial3d(locator_mat),
                    Transform::from_translation(Vec3::new(0.0, 4.0, 0.0)),
                    Visibility::default(),
                    Name::new("train:live:locator"),
                ));
            }
            for (vi, vehicle) in vehicles.iter().enumerate() {
                if let Some(shape_name) = vehicle
                    .shape_file
                    .as_deref()
                    .filter(|s| !s.eq_ignore_ascii_case("test.s"))
                {
                    if let Some(shape_path) =
                        resolve_vehicle_shape_path(&shape_dirs, shape_name, &assets.route_dir)
                    {
                        let trainset_root = vehicle_texture_root_for_shape_path(&shape_path)
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
                            TRAIN_SHAPE_FALLBACK,
                        ) {
                            shape_cars += 1;
                            shape_parts += asset.parts.len();
                            textured_parts +=
                                asset.parts.iter().filter(|part| part.has_texture).count();
                            let is_lead = vi == 0;
                            let mesh_ref = meshes.get(&asset.combined_mesh);
                            let (car_transform, exterior_scale) = if is_lead {
                                mesh_ref
                                    .map(|m| {
                                        vehicle_cab_frame_and_exterior_scale(
                                            m,
                                            vehicle.offset_m,
                                            vehicle.length_m,
                                        )
                                    })
                                    .map(|(t, s)| (t, Some(s)))
                                    .unwrap_or_else(|| {
                                        (
                                            vehicle_local_transform(
                                                &scene,
                                                vehicle.offset_m,
                                                vehicle.length_m,
                                            ),
                                            None,
                                        )
                                    })
                            } else {
                                (
                                    mesh_ref
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
                                        }),
                                    None,
                                )
                            };
                            let mut car = train.spawn((
                                car_transform,
                                Visibility::default(),
                                Name::new(format!("train:live:car:{vi}")),
                            ));
                            if is_lead {
                                car.insert(CabLeadVehicle);
                            }
                            car.with_children(|car| {
                                let spawn_parts = |parent: &mut ChildSpawnerCommands<'_>,
                                                   parts: &[crate::shapes::ShapePartAsset]| {
                                    for (pi, part) in parts.iter().enumerate() {
                                        parent.spawn((
                                            LiveTrainBody,
                                            Mesh3d(part.mesh.clone()),
                                            MeshMaterial3d(part.material.clone()),
                                            Transform::default(),
                                            Visibility::default(),
                                            Name::new(format!(
                                                "train:live:car:{vi}:part:{pi}:{}",
                                                part.prim_state_idx
                                            )),
                                        ));
                                    }
                                };
                                if let Some(scale) = exterior_scale {
                                    car.spawn((
                                        Transform::from_scale(Vec3::splat(scale)),
                                        Visibility::default(),
                                        Name::new(format!("train:live:car:{vi}:exterior_scale")),
                                    ))
                                    .with_children(|scaled| spawn_parts(scaled, &asset.parts));
                                } else {
                                    spawn_parts(car, &asset.parts);
                                }
                            });
                            continue;
                        }
                    }
                }
                let is_lead = vi == 0;
                let is_last = vi == vehicles.len() - 1;
                let body_color = if is_lead {
                    Color::srgb(0.55, 0.08, 0.12) // Crimson Red / Burgundy
                } else if vi % 2 == 1 {
                    Color::srgb(0.12, 0.28, 0.18) // Dark Forest Green
                } else {
                    Color::srgb(0.10, 0.22, 0.38) // Midnight Blue
                };

                let body_material = materials.add(StandardMaterial {
                    base_color: body_color,
                    perceptual_roughness: 0.45,
                    metallic: 0.3,
                    ..default()
                });

                let roof_material = materials.add(StandardMaterial {
                    base_color: Color::srgb(0.15, 0.16, 0.17),
                    perceptual_roughness: 0.6,
                    metallic: 0.5,
                    ..default()
                });

                let glass_material = materials.add(StandardMaterial {
                    base_color: Color::srgb(0.02, 0.05, 0.1),
                    perceptual_roughness: 0.1,
                    metallic: 0.9,
                    ..default()
                });

                let metal_material = materials.add(StandardMaterial {
                    base_color: Color::srgb(0.12, 0.12, 0.13),
                    perceptual_roughness: 0.8,
                    metallic: 0.2,
                    ..default()
                });

                let wheel_material = materials.add(StandardMaterial {
                    base_color: Color::srgb(0.65, 0.66, 0.68),
                    perceptual_roughness: 0.2,
                    metallic: 0.9,
                    ..default()
                });

                let headlight_material = materials.add(StandardMaterial {
                    base_color: Color::srgb(1.0, 1.0, 0.85),
                    emissive: LinearRgba::new(3.0, 3.0, 1.5, 1.0),
                    perceptual_roughness: 0.1,
                    ..default()
                });

                let taillight_material = materials.add(StandardMaterial {
                    base_color: Color::srgb(1.0, 0.15, 0.15),
                    emissive: LinearRgba::new(3.0, 0.3, 0.3, 1.0),
                    perceptual_roughness: 0.1,
                    ..default()
                });

                let main_mesh = meshes.add(Cuboid::new(1.0, 0.8, 0.95));
                let roof_mesh = meshes.add(Cuboid::new(1.0, 0.15, 0.95));
                let window_mesh = meshes.add(Cuboid::new(0.06, 0.22, 1.01));
                let bogey_mesh = meshes.add(Cuboid::new(0.2, 0.08, 0.8));
                let wheel_mesh = meshes.add(Sphere::new(0.08));
                let light_mesh = meshes.add(Sphere::new(0.035));
                let windshield_mesh = meshes.add(Cuboid::new(0.12, 0.28, 0.96));

                let real_width = 3.0_f32;
                let real_height = 3.6_f32;
                let real_length = vehicle.length_m.max(4.0);
                let local = Transform {
                    translation: Vec3::new(vehicle.offset_m, real_height * 0.5, 0.0),
                    scale: Vec3::new(real_length, real_height, real_width),
                    ..default()
                };
                fallback_cars += 1;

                let mut fallback = train.spawn((
                    local,
                    Visibility::default(),
                    Name::new(format!("train:live:car:{vi}:fallback")),
                ));
                if is_lead {
                    fallback.insert(CabLeadVehicle);
                }

                fallback.with_children(|parent| {
                    // 1. Main body
                    parent.spawn((
                        LiveTrainBody,
                        Mesh3d(main_mesh.clone()),
                        MeshMaterial3d(body_material.clone()),
                        Transform::from_xyz(0.0, -0.05, 0.0),
                        Visibility::default(),
                        Name::new("body"),
                    ));

                    // 2. Roof
                    parent.spawn((
                        LiveTrainBody,
                        Mesh3d(roof_mesh.clone()),
                        MeshMaterial3d(roof_material.clone()),
                        Transform::from_xyz(0.0, 0.425, 0.0),
                        Visibility::default(),
                        Name::new("roof"),
                    ));

                    // 3. Side Windows
                    for &x_pos in &[-0.32_f32, -0.16_f32, 0.0_f32, 0.16_f32, 0.32_f32] {
                        if is_lead && x_pos > 0.25 {
                            continue; // Leave room for cab
                        }
                        parent.spawn((
                            LiveTrainBody,
                            Mesh3d(window_mesh.clone()),
                            MeshMaterial3d(glass_material.clone()),
                            Transform::from_xyz(x_pos, 0.1, 0.0),
                            Visibility::default(),
                            Name::new("window"),
                        ));
                    }

                    // 4. Bogeys & Wheels
                    for &bogey_x in &[-0.3_f32, 0.3_f32] {
                        parent.spawn((
                            LiveTrainBody,
                            Mesh3d(bogey_mesh.clone()),
                            MeshMaterial3d(metal_material.clone()),
                            Transform::from_xyz(bogey_x, -0.4, 0.0),
                            Visibility::default(),
                            Name::new("bogey"),
                        ));

                        // 4 wheels per bogey aligned on 1.435m track gauge (Z = ±0.239 inside 3.0m wide car)
                        for &wheel_z in &[-0.239_f32, 0.239_f32] {
                            for &wheel_offset_x in &[-0.06_f32, 0.06_f32] {
                                parent.spawn((
                                    LiveTrainBody,
                                    Mesh3d(wheel_mesh.clone()),
                                    MeshMaterial3d(wheel_material.clone()),
                                    Transform::from_xyz(bogey_x + wheel_offset_x, -0.5, wheel_z),
                                    Visibility::default(),
                                    Name::new("wheel"),
                                ));
                            }
                        }
                    }

                    // 5. Locomotive features (lead)
                    if is_lead {
                        // Windshield
                        parent.spawn((
                            LiveTrainBody,
                            Mesh3d(windshield_mesh.clone()),
                            MeshMaterial3d(glass_material.clone()),
                            Transform::from_xyz(0.42, 0.2, 0.0),
                            Visibility::default(),
                            Name::new("windshield"),
                        ));

                        // Glowing Headlights
                        parent.spawn((
                            LiveTrainBody,
                            Mesh3d(light_mesh.clone()),
                            MeshMaterial3d(headlight_material.clone()),
                            Transform::from_xyz(0.505, -0.1, -0.28),
                            Visibility::default(),
                            Name::new("headlight_l"),
                        ));
                        parent.spawn((
                            LiveTrainBody,
                            Mesh3d(light_mesh.clone()),
                            MeshMaterial3d(headlight_material.clone()),
                            Transform::from_xyz(0.505, -0.1, 0.28),
                            Visibility::default(),
                            Name::new("headlight_r"),
                        ));
                        parent.spawn((
                            LiveTrainBody,
                            Mesh3d(light_mesh.clone()),
                            MeshMaterial3d(headlight_material.clone()),
                            Transform::from_xyz(0.505, 0.3, 0.0),
                            Visibility::default(),
                            Name::new("headlight_top"),
                        ));
                    }

                    // 6. Taillights (last car)
                    if is_last {
                        parent.spawn((
                            LiveTrainBody,
                            Mesh3d(light_mesh.clone()),
                            MeshMaterial3d(taillight_material.clone()),
                            Transform::from_xyz(-0.505, 0.0, -0.28),
                            Visibility::default(),
                            Name::new("taillight_l"),
                        ));
                        parent.spawn((
                            LiveTrainBody,
                            Mesh3d(light_mesh.clone()),
                            MeshMaterial3d(taillight_material.clone()),
                            Transform::from_xyz(-0.505, 0.0, 0.28),
                            Visibility::default(),
                            Name::new("taillight_r"),
                        ));
                    }
                });
            }
        });

    viewer_log!(
        "openrailsrs-viewer3d: live drive — {} vehicle(s) ({} shape / {} fallback, {} textured part(s) / {}), dt={:.2}s, audio={}, cab back={:.1}m height={:.1}m",
        vehicles.len(),
        shape_cars,
        fallback_cars,
        textured_parts,
        shape_parts,
        live.session.dt,
        live.audio.is_some(),
        driver_cab.back_m,
        driver_cab.height_m,
    );
    log_step("spawned live train", spawn_start);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rolling_stock::ConsistVehicleVisual;
    use crate::rolling_stock::TrainConsistScene;

    #[test]
    fn pullman_dmbsa_driver_cab_near_front() {
        let route = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        let mut consist = TrainConsistScene::default();
        consist.set_scenario_dir(route.clone());
        let mut shape_dirs_bufs = consist.shape_search_dirs(&route);
        let content = PathBuf::from("/home/cristian/Documentos/Open Rails/Content");
        if content.is_dir() {
            shape_dirs_bufs.push(content.join("Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman"));
        }
        let shape_dirs: Vec<&Path> = shape_dirs_bufs.iter().map(|p| p.as_path()).collect();
        let cab = live_driver_cab_from_vehicles(
            &[ConsistVehicleVisual {
                name: "DMBSA".into(),
                shape_file: Some("RF_WP_DMBSA.s".into()),
                length_m: 20.879,
                offset_m: 0.0,
            }],
            &shape_dirs,
            &route,
        );
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
        if content.is_dir() {
            let head = cab.head_pos_train.expect("ORTS3DCabHeadPos");
            assert!(head.y > 2.0 && head.y < 4.5, "head height={head:?}");
            assert!(head.x.abs() < 12.0, "head forward={head:?}");
        }
    }
}
