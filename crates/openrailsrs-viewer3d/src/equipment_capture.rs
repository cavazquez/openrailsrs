//! Opt-in dev-tools actions for isolated, reproducible graphical acceptance.
use bevy::prelude::*;
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Action {
    Car {
        vehicle: usize,
        operation: openrailsrs_sim::CarOperation,
    },
    Uncouple {
        after: usize,
    },
    Recouple,
    Restore,
    TakeControl {
        service: usize,
    },
    CoupleService {
        service: usize,
    },
    Cab {
        vehicle: usize,
    },
    Doors {
        vehicle: usize,
        side: openrailsrs_scenarios::vehicle_systems::DoorSide,
        open: bool,
    },
    Collector {
        vehicle: usize,
        index: usize,
    },
    Walk {
        delta: [f32; 3],
    },
    View {
        mode: String,
    },
    Pause {
        value: bool,
    },
    /// Camera offset is absolute in the existing route-focus frame. A distant
    /// camera naturally exercises the production floating-origin path.
    FlyCamera {
        position_m: [f32; 3],
        look_at_m: [f32; 3],
    },
    Orbit {
        yaw: f32,
        pitch: f32,
        distance_m: f32,
    },
    Refill {
        station: String,
        vehicle: usize,
    },
    CancelRefill,
    /// Initial load of an owned graphical fixture, never exposed as a control.
    FixturePassengers {
        count: u32,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TimedAction {
    at_s: f64,
    operation: Action,
}
#[derive(Resource, Default)]
pub struct EquipmentCapture {
    actions: Vec<TimedAction>,
    start: Option<std::time::Instant>,
    next: usize,
    pub results: Vec<serde_json::Value>,
}
#[derive(bevy::ecs::system::SystemParam)]
pub struct CaptureTransfer<'w> {
    content: Res<'w, crate::player_launch::ActivePlayerContent>,
    settings: Res<'w, crate::player_settings::PlayerSettings>,
    look: Res<'w, crate::camera::DriverLookOffset>,
    weather: Option<Res<'w, crate::weather_state::WeatherState>>,
    launch: ResMut<'w, crate::player_launch::PlayerLaunchQueue>,
}
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn apply(
    mut live: ResMut<crate::live::LiveDrive>,
    mut state: ResMut<EquipmentCapture>,
    mut follow: ResMut<crate::camera::CameraFollowMode>,
    mut passenger: ResMut<crate::camera::PassengerCamState>,
    presentation: Res<crate::train_presentation::TrainPresentation>,
    catalog: Res<crate::camera::PassengerSeatCatalog>,
    progress: Option<Res<crate::world::WorldSpawnProgress>>,
    pipelines: Option<Res<crate::performance::ScenePipelineStatus>>,
    origin: Res<crate::floating_origin::FloatingOrigin>,
    mut camera_mode: ResMut<crate::camera::CameraMode>,
    mut cameras: Query<
        (
            &mut Transform,
            &mut crate::camera::OrbitState,
            &mut crate::camera::FlyState,
        ),
        (With<Camera3d>, Without<crate::mirrors::MirrorCamera>),
    >,
    targets: Option<Res<crate::world_operations::RefillTargets>>,
    mut transfer: CaptureTransfer,
) {
    if !crate::capture::capture_enabled()
        || progress.is_some()
        || pipelines.as_ref().is_some_and(|p| !p.ready())
    {
        return;
    }
    if state.start.is_none() {
        let Some(raw) = std::env::var("OPENRAILSRS_CAPTURE_ACTIONS")
            .ok()
            .filter(|s| s.len() <= 16384)
        else {
            return;
        };
        match serde_json::from_str::<Vec<TimedAction>>(&raw) {
            Ok(actions)
                if actions.len() <= 64
                    && actions
                        .iter()
                        .all(|a| a.at_s.is_finite() && (0. ..=600.).contains(&a.at_s))
                    && actions.windows(2).all(|a| a[0].at_s <= a[1].at_s) =>
            {
                state.actions = actions
            }
            _ => {
                crate::viewer_log!("equipment capture: invalid action sequence");
                return;
            }
        }
        state.start = Some(std::time::Instant::now());
    }
    let age = state.start.unwrap().elapsed().as_secs_f64();
    while state.actions.get(state.next).is_some_and(|a| a.at_s <= age) {
        let index = state.next;
        let result = match &state.actions[index].operation {
            Action::Car { vehicle, operation } => live.session.operate_car(*vehicle, *operation),
            Action::Uncouple { after } => live.session.uncouple_after(*after),
            Action::Recouple => live.session.recouple(),
            Action::TakeControl { service } | Action::CoupleService { service } => (|| {
                let coupling =
                    matches!(state.actions[index].operation, Action::CoupleService { .. });
                if coupling {
                    live.traffic.validate_coupling(&live.session, *service)?;
                } else {
                    live.traffic.validate_control(&live.session, *service)?;
                }
                let id = live.traffic.services[*service].id.clone();
                let (_, orbit, _) = cameras.single().map_err(|_| "No capture camera")?;
                let saved = crate::saved_game::SavedCamera::capture(
                    *follow,
                    orbit,
                    &transfer.look,
                    &origin,
                );
                let path = crate::player_settings::player_data_dir().join(if coupling {
                    "formation-transfer.json"
                } else {
                    "driver-transfer.json"
                });
                crate::saved_game::SavedGame::save(
                    &mut live,
                    &transfer.content,
                    saved,
                    &path,
                    transfer.weather.as_deref(),
                    Some(&transfer.settings),
                )?;
                if coupling {
                    crate::saved_game::SavedGame::queue_coupling(&path, &id)?;
                } else {
                    crate::saved_game::SavedGame::queue_control(&path, &id)?;
                }
                transfer.launch.0 = Some(crate::saved_game::SavedGame::prepare_resume(&path)?);
                Ok(())
            })(),
            Action::Restore => serde_json::to_vec(&live.session.snapshot())
                .map_err(|e| e.to_string())
                .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|e| e.to_string()))
                .and_then(|saved| live.session.restore_snapshot(saved)),
            Action::Cab { vehicle } => live.session.select_cab(*vehicle),
            Action::Doors {
                vehicle,
                side,
                open,
            } => live.session.command_doors(Some(*vehicle), *side, *open),
            Action::Collector { vehicle, index } => live.session.toggle_collector(*vehicle, *index),
            Action::Walk { delta } => {
                let boxes = presentation
                    .vehicles
                    .iter()
                    .find(|v| v.vehicle == passenger.consist_car)
                    .map_or(&[][..], |v| v.walk_boxes_m.as_slice());
                passenger.head_msts = crate::passenger_walk::constrained_move(
                    passenger.head_msts,
                    Vec3::from_array(*delta),
                    boxes,
                );
                Ok(())
            }
            Action::View { mode } => {
                if mode == "passenger"
                    && let Some((index, views)) = catalog.seat_cars().first()
                    && let Some(view) = views.first()
                {
                    passenger.apply_viewpoint(view, *index);
                }
                *follow = match mode.as_str() {
                    "driver" => crate::camera::CameraFollowMode::DriverCam,
                    "passenger" => crate::camera::CameraFollowMode::PassengerCam,
                    "orbit" => crate::camera::CameraFollowMode::OrbitFollow,
                    _ => {
                        state
                            .results
                            .push(serde_json::json!({"index":index,"error":"unknown view"}));
                        state.next += 1;
                        continue;
                    }
                };
                Ok(())
            }
            Action::Pause { value } => {
                live.paused = *value;
                Ok(())
            }
            Action::FlyCamera {
                position_m,
                look_at_m,
            } => {
                let eye = Vec3::from_array(*position_m);
                let target = Vec3::from_array(*look_at_m);
                if !eye.is_finite()
                    || !target.is_finite()
                    || eye.abs().max_element() > 1_000_000.
                    || target.abs().max_element() > 1_000_000.
                    || eye.distance_squared(target) < 0.01
                {
                    Err("invalid QA camera pose".into())
                } else if let Ok((mut tf, mut orbit, mut fly)) = cameras.single_mut() {
                    *follow = crate::camera::CameraFollowMode::Off;
                    *camera_mode = crate::camera::CameraMode::Fly;
                    orbit.focus = target - origin.shift;
                    *tf = Transform::from_translation(eye - origin.shift)
                        .looking_at(orbit.focus, Vec3::Y);
                    let (yaw, pitch, _) = tf.rotation.to_euler(EulerRot::YXZ);
                    fly.yaw = yaw;
                    fly.pitch = pitch;
                    Ok(())
                } else {
                    Err("QA main camera missing".into())
                }
            }
            Action::Orbit {
                yaw,
                pitch,
                distance_m,
            } => {
                if !yaw.is_finite()
                    || !pitch.is_finite()
                    || !distance_m.is_finite()
                    || !(-1.5..=1.5).contains(pitch)
                    || !(1.0..=10000.).contains(distance_m)
                {
                    Err("invalid QA orbit".into())
                } else if let Ok((mut tf, mut orbit, _)) = cameras.single_mut() {
                    *follow = crate::camera::CameraFollowMode::OrbitFollow;
                    *camera_mode = crate::camera::CameraMode::Orbit;
                    orbit.yaw = *yaw;
                    orbit.pitch = *pitch;
                    orbit.distance = *distance_m;
                    *tf = crate::camera::camera_transform_from_orbit_state(
                        orbit.focus,
                        *yaw,
                        *pitch,
                        *distance_m,
                    );
                    Ok(())
                } else {
                    Err("QA main camera missing".into())
                }
            }
            Action::Refill { station, vehicle } => {
                if let Some(target) = targets.as_ref().and_then(|t| {
                    t.0.iter()
                        .find(|t| t.station.id == *station && t.vehicle == *vehicle)
                }) {
                    live.session.begin_refill(
                        target.station.clone(),
                        *vehicle,
                        target.distance_m,
                        target.width_m,
                    )
                } else {
                    Err("QA pickup not aligned with vehicle intake".into())
                }
            }
            Action::CancelRefill => {
                live.session.cancel_refill();
                Ok(())
            }
            Action::FixturePassengers { count } => {
                if *count > 256
                    || !live
                        .scenario_path()
                        .components()
                        .any(|p| p.as_os_str() == "scenarios")
                {
                    Err("QA passenger seeding requires disposable fixture".into())
                } else {
                    live.session.state.passengers = *count;
                    Ok(())
                }
            }
        };
        state
            .results
            .push(serde_json::json!({"index":index,"result":result,
                "time_s":live.session.time_s(), "boarding_progress":live.session.gameplay.boarding_progress(),
                "passengers":live.session.state.passengers,
                "refilling":live.session.refilling,
                "camera_mode":format!("{:?}",*camera_mode),
                "origin_m":origin.shift.to_array(),
                "door_fractions":live.session.state.vehicle_systems.iter().map(|s|s.door_fraction).collect::<Vec<_>>()
            }));
        state.next += 1;
    }
}
