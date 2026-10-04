//! ECS adapter: simulation emits sound state, the background audio engine owns
//! all device/sample resources. Camera changes never reload the native bank.
use crate::{
    camera::CameraFollowMode,
    live::{LiveDrive, LiveTrainMarker},
    player_settings::PlayerSettings,
    shapes::RouteAssets,
    traffic::TrafficTrainMarker,
};
use bevy::prelude::*;
use openrailsrs_audio::native::{
    ConsistSoundSpec, NativeAudioEngine, SoundFrame, SoundState, TrainSoundFrame,
};

#[derive(Resource, Default)]
pub struct NativeAudio {
    pub engine: Option<NativeAudioEngine>,
}

pub fn start_native_audio(
    live: Option<Res<LiveDrive>>,
    assets: Res<RouteAssets>,
    mut audio: ResMut<NativeAudio>,
) {
    audio.engine = None;
    let Some(live) = live else { return };
    let Ok(scenario) = openrailsrs_scenarios::load_scenario(live.scenario_path()) else {
        return;
    };
    let directory = live
        .scenario_path()
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let mut specs = vec![ConsistSoundSpec {
        id: 0,
        consist: directory.join(&scenario.train.consist),
        route: assets.route_dir.clone(),
    }];
    specs.extend(
        scenario
            .extra_trains
            .iter()
            .enumerate()
            .map(|(i, t)| ConsistSoundSpec {
                id: i + 1,
                consist: directory.join(&t.consist),
                route: assets.route_dir.clone(),
            }),
    );
    audio.engine = NativeAudioEngine::start(specs);
}
pub fn stop_native_audio(mut audio: ResMut<NativeAudio>) {
    audio.engine = None;
}

fn state(session: &openrailsrs_sim::LiveDriveSession) -> SoundState {
    let t = session.cab_telemetry();
    let rpm = session
        .physics
        .diesel_engines
        .first()
        .and_then(|e| e.engine.as_deref())
        .zip(t.diesel_rpm)
        .map_or(t.throttle_pct / 100.0, |(engine, rpm)| {
            (rpm - engine.idle_rpm) / (engine.max_rpm - engine.idle_rpm).max(1.0)
        });
    let electric =
        session.physics.steam_params.is_none() && session.physics.diesel_sfc_g_per_kwh.is_none();
    SoundState {
        speed: session.velocity_mps() as f32,
        distance: session.state.odometer_m as f32,
        variable1: session.physics.steam_params.as_ref().map_or(
            if electric {
                t.throttle_pct
            } else {
                session.driver_throttle
            },
            |s| {
                if session.driver_throttle > 0.0 {
                    session.velocity_mps().abs() / s.driving_wheel_radius_m / std::f64::consts::PI
                        * 5.0
                } else {
                    0.0
                }
            },
        ) as f32,
        // OR diesel uses an RPM fraction; electric/steam programs expect 0–100.
        // Until effort/chest-pressure telemetry is exposed, demand is a proxy
        // for those two load variables rather than a falsely normalized RPM.
        variable2: if electric || session.physics.steam_params.is_some() {
            t.throttle_pct as f32
        } else {
            rpm.clamp(0.0, 1.0) as f32
        },
        variable3: 0.0,
        steam_phase: session.physics.steam_params.as_ref().map(|s| {
            session.state.odometer_m / (std::f64::consts::TAU * s.driving_wheel_radius_m) * 8.0
        }),
        brake_cylinder: t.brake_cyl_bar as f32 * 14.503774,
        brake_pipe: t.brake_pipe_bar as f32,
        throttle: session.driver_throttle as f32,
        brake: session.driver_brake as f32,
        direction: session.driver_direction as f32,
        horn: t.horn_active,
        wiper: t.wiper_active,
        doors: matches!(
            session.exterior.door,
            openrailsrs_sim::DoorState::Opening | openrailsrs_sim::DoorState::Open
        ),
        headlights: session.headlights,
    }
}

pub fn update_native_audio(
    live: Option<Res<LiveDrive>>,
    audio: Res<NativeAudio>,
    settings: Res<PlayerSettings>,
    follow: Res<CameraFollowMode>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    player: Query<(Entity, &GlobalTransform), With<LiveTrainMarker>>,
    traffic: Query<(Entity, &TrafficTrainMarker, &GlobalTransform)>,
    cars: Query<(
        &ChildOf,
        &crate::rolling_stock::ConsistCarIndex,
        &GlobalTransform,
    )>,
) {
    let (Some(live), Some(engine)) = (live, audio.engine.as_ref()) else {
        return;
    };
    let Ok(camera) = camera.single() else { return };
    let car_distances = |head: Option<Entity>, count| {
        let mut distances = vec![f32::INFINITY; count];
        for (parent, car, transform) in &cars {
            if Some(parent.parent()) == head
                && let Some(distance) = distances.get_mut(car.0)
            {
                *distance = transform.translation().distance(camera.translation());
            }
        }
        distances
    };
    let player_pose = player.single().ok();
    let mut trains = vec![TrainSoundFrame {
        id: 0,
        state: state(&live.session),
        distance_m: player_pose
            .map_or(0.0, |(_, p)| p.translation().distance(camera.translation())),
        vehicle_distances_m: car_distances(
            player_pose.map(|(e, _)| e),
            live.session.formation.cars.len(),
        ),
    }];
    for (i, service) in live.traffic.services.iter().enumerate() {
        let pose = traffic.iter().find(|(_, m, _)| m.0 == i + 1);
        let distance = pose.map_or(f32::INFINITY, |(_, _, tf)| {
            tf.translation().distance(camera.translation())
        });
        trains.push(TrainSoundFrame {
            id: i + 1,
            state: state(&service.session),
            distance_m: if service.departed {
                distance
            } else {
                f32::INFINITY
            },
            vehicle_distances_m: if service.departed {
                car_distances(
                    pose.map(|(e, _, _)| e),
                    service.session.formation.cars.len(),
                )
            } else {
                vec![]
            },
        });
    }
    engine.send(SoundFrame {
        time_s: live.session.time_s(),
        cab: matches!(
            *follow,
            CameraFollowMode::DriverCam | CameraFollowMode::Cab2d
        ),
        passenger: *follow == CameraFollowMode::PassengerCam,
        paused: live.paused,
        volume: if settings.audio_enabled {
            settings.audio_volume
        } else {
            0.0
        },
        trains,
    });
}
