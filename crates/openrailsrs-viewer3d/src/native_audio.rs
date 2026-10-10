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

fn state(session: &openrailsrs_sim::LiveDriveSession, vehicle: usize) -> SoundState {
    let t = session.cab_telemetry();
    let engine_index = session
        .physics
        .diesel_vehicle_indices
        .iter()
        .position(|&index| index == vehicle);
    let engine = engine_index.and_then(|i| session.physics.diesel_engines.get(i));
    let diesel_state = session.state.diesel.car(vehicle);
    let rpm = session
        .physics
        .diesel
        .cars
        .iter()
        .find(|c| c.vehicle == vehicle)
        .map(|c| &c.governor)
        .or_else(|| engine.and_then(|e| e.engine.as_deref()))
        .zip(
            diesel_state
                .map(|d| d.rpm)
                .or_else(|| engine_index.and_then(|i| session.state.diesel_rpm.get(i).copied())),
        )
        .map_or(t.throttle_pct / 100.0, |(engine, rpm)| {
            (rpm - engine.idle_rpm) / (engine.max_rpm - engine.idle_rpm).max(1.0)
        });
    let car = session.formation.cars.get(vehicle);
    let parked = vehicle >= session.formation.coupled_count;
    let controls_powered = car.is_some_and(|c| {
        c.powered && c.power_on && c.battery_on && (vehicle == 0 || c.mu_connected)
    }) && !parked;
    let powered = controls_powered
        && session.state.electric.power_available(vehicle)
        && session.state.diesel.power_available(vehicle);
    let velocity = if parked {
        0.0
    } else {
        session
            .state
            .vehicles
            .get(vehicle)
            .map_or(session.velocity_mps(), |v| v.velocity_mps)
    };
    let cylinder = if parked {
        session
            .formation
            .parked_brakes
            .get(vehicle - session.formation.coupled_count)
    } else {
        session.state.brake_system.cylinders.get(vehicle)
    };
    let locomotive = session.vehicle_definition(vehicle).and_then(|v| {
        if let openrailsrs_train::Vehicle::Loco(l) = v {
            Some(l)
        } else {
            None
        }
    });
    let steam = locomotive.and_then(|l| l.steam.as_ref());
    let boiler = steam.and(session.state.boiler_state.as_ref());
    let steam_working = boiler.is_some_and(|b| b.tractive_force_n > 0. && !b.low_water_failure);
    // Missing fuel-consumption data does not turn a diesel with a governor
    // into an electric. Keep the sound-variable scale tied to this motor.
    let diesel = diesel_state.is_some()
        || engine.is_some_and(|e| e.engine.is_some())
        || locomotive.is_some_and(|l| {
            l.diesel_sfc_g_per_kwh.is_some()
                || l.diesel_traction
                    .as_ref()
                    .is_some_and(|m| m.engine.is_some())
        });
    let electric = steam.is_none() && !diesel;
    SoundState {
        sander: controls_powered
            && session
                .state
                .rail_adhesion
                .as_ref()
                .and_then(|r| r.cars.get(vehicle))
                .is_some_and(|c| c.using_sand),
        speed: velocity as f32,
        distance: if parked {
            session.formation.parked_head_chainage_m.unwrap_or(0.0)
        } else {
            session.state.odometer_m
        } as f32,
        variable1: if !powered {
            0.0
        } else {
            steam.map_or(
                if electric {
                    t.throttle_pct
                } else {
                    session.driver_throttle
                },
                |s| {
                    if steam_working {
                        velocity.abs() / s.driving_wheel_radius_m / std::f64::consts::PI * 5.0
                    } else {
                        0.0
                    }
                },
            ) as f32
        },
        // OR diesel uses an RPM fraction; electric/steam programs expect 0–100.
        // Steam uses the physical boiler effort; electric demand remains a
        // proxy until a native motor-current model is available.
        variable2: if diesel_state.is_some_and(|d| d.rpm > 0.) {
            rpm.clamp(0., 1.) as f32
        } else if !powered {
            0.0
        } else if let Some(b) = boiler {
            (b.tractive_force_n / steam.unwrap().max_tractive_effort_n()).clamp(0., 1.) as f32
                * 100.
        } else if electric {
            t.throttle_pct as f32
        } else {
            rpm.clamp(0.0, 1.0) as f32
        },
        variable3: 0.0,
        steam_phase: steam.filter(|_| powered && steam_working).map(|s| {
            (session.state.odometer_m + session.render_wheel_slip_distance_m(vehicle, 0.0))
                / (std::f64::consts::TAU * s.driving_wheel_radius_m)
                * f64::from(s.cylinder_count)
                * 2.0
        }),
        engine_on: diesel_state
            .map(|d| d.command_running && d.fuel_l > 0. && car.is_some_and(|c| c.battery_on)),
        injector1: boiler.is_some_and(|b| b.controls.injector1),
        injector2: boiler.is_some_and(|b| b.controls.injector2),
        blower: boiler.is_some_and(|b| b.controls.blower),
        damper: boiler.map_or(0., |b| b.controls.damper as f32),
        cylinder_cocks: boiler.is_some_and(|b| b.controls.cylinder_cocks),
        brake_cylinder: cylinder.map_or(t.brake_cyl_bar, |c| c.pressure_bar()) as f32 * 14.503774,
        brake_pipe: cylinder
            .and_then(|c| c.pipe_pressure_bar())
            .unwrap_or(t.brake_pipe_bar) as f32,
        throttle: if powered {
            session.driver_throttle as f32
        } else {
            0.0
        },
        brake: session.driver_brake as f32,
        direction: session.driver_direction as f32,
        horn: controls_powered
            && t.horn_active
            && boiler.is_none_or(|b| b.pressure_bar > 0. && !b.low_water_failure),
        wiper: vehicle == 0 && t.wiper_active,
        doors: matches!(
            session.exterior.door,
            openrailsrs_sim::DoorState::Opening | openrailsrs_sim::DoorState::Open
        ),
        headlights: t.headlights,
    }
}

fn vehicle_states(session: &openrailsrs_sim::LiveDriveSession) -> Vec<SoundState> {
    (0..session.formation.cars.len())
        .map(|index| state(session, index))
        .collect()
}

pub fn update_native_audio(
    live: Option<Res<LiveDrive>>,
    audio: Res<NativeAudio>,
    weather: Option<Res<crate::weather_state::WeatherState>>,
    settings: Res<PlayerSettings>,
    follow: Res<CameraFollowMode>,
    passenger: Option<Res<crate::camera::PassengerCamState>>,
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
        state: state(&live.session, 0),
        vehicle_states: vehicle_states(&live.session),
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
            state: state(&service.session, 0),
            vehicle_states: vehicle_states(&service.session),
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
    if let Some(weather) = weather.as_ref() {
        engine.weather(
            weather.atmosphere.rain,
            weather.atmosphere.wind_mps.length(),
        );
    }
    engine.send(SoundFrame {
        time_s: live.session.time_s(),
        cab: matches!(
            *follow,
            CameraFollowMode::DriverCam | CameraFollowMode::Cab2d
        ),
        passenger: *follow == CameraFollowMode::PassengerCam,
        listener_vehicle: if *follow == CameraFollowMode::PassengerCam {
            passenger.as_ref().map_or(0, |p| p.consist_car)
        } else {
            0
        },
        paused: live.paused,
        volume: if settings.audio_enabled {
            settings.audio_volume
        } else {
            0.0
        },
        trains,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_sim::{BrakeCylinder, LiveDriveSession};
    use openrailsrs_train::diesel::{DieselEngineParams, DieselTractionModel};

    fn session() -> LiveDriveSession {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chiltern_extended/scenario.toml");
        let scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
        LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap()
    }

    fn cylinder(force: f64) -> BrakeCylinder {
        let mut c = BrakeCylinder::new(
            0.0,
            1000.0,
            false,
            openrailsrs_formats::BrakeShoeFrictionCurve::identity(),
            10000.0,
            0.0,
        );
        c.full_pressure_bar = 2.5;
        c.current_force_n = force;
        c
    }

    #[test]
    fn native_audio_uses_each_motor_rpm_and_each_vehicle_brake_pressure() {
        let mut s = session();
        let engine = |idle, max| DieselTractionModel {
            engine: Some(Box::new(DieselEngineParams::from_msts_defaults(
                1000000.0, idle, max, 40.0,
            ))),
            ..Default::default()
        };
        s.physics.diesel_vehicle_indices = vec![0, 7];
        s.physics.diesel_engines = vec![engine(300.0, 900.0), engine(500.0, 1500.0)];
        s.state.diesel_rpm = vec![300.0, 1500.0];
        // Canonical per-car operation state survives filtered traction arrays.
        s.state.diesel.cars = [(0, 300.), (7, 1500.)]
            .into_iter()
            .map(
                |(vehicle, rpm)| openrailsrs_sim::diesel_operation::DieselCarState {
                    vehicle,
                    rpm,
                    demanded_rpm: rpm,
                    command_running: true,
                    phase: openrailsrs_sim::diesel_operation::EnginePhase::Running,
                    fuel_l: 25.,
                    consumed_l: 0.,
                    refilled_l: 0.,
                    flow_lps: 0.,
                },
            )
            .collect();
        s.physics.diesel.cars.clear(); // This test supplies the two governor fixtures above.
        s.driver_throttle = 1.0;
        s.state.velocity_mps = 12.0;
        s.state.vehicles.clear();
        s.state.brake_system.cylinders[0] = cylinder(0.0);
        s.state.brake_system.cylinders[7] = cylinder(500.0);
        s.formation.cars[7].powered = true;
        s.formation.cars[7].mu_connected = true;
        let frames = vehicle_states(&s);
        assert_eq!(frames.len(), 8);
        assert_eq!(frames[0].variable2, 0.0);
        assert_eq!(frames[7].variable2, 1.0);
        assert_eq!(frames[0].brake_cylinder, 0.0);
        assert!((frames[7].brake_cylinder - 1.25 * 14.503774).abs() < 1e-4);
        assert_eq!(frames[7].speed, 12.0);
        s.formation.cars[7].mu_connected = false;
        assert_eq!(state(&s, 7).variable2, 1.0); // Engine still turns; MU isolation cuts traction.
        assert_eq!(state(&s, 7).throttle, 0.0);
    }

    #[test]
    fn diesel_with_default_rpm_parameters_keeps_diesel_sound_scale() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/traction_operation/scenario_low_fuel.toml");
        let scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
        let mut s =
            openrailsrs_sim::LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario)
                .unwrap();
        s.driver_throttle = 0.5;
        let sound = state(&s, 0);
        assert_eq!(
            sound.variable1, 0.5,
            "Type Diesel must not use the electric 0–100 scale when its governor fields are absent"
        );
        assert_eq!(sound.engine_on, Some(true));
        assert!(sound.variable2.is_finite());
    }

    #[test]
    fn a_parked_car_keeps_its_own_brakes_and_has_no_running_or_traction_sound() {
        let mut s = session();
        s.driver_throttle = 1.0;
        s.state.velocity_mps = 15.0;
        s.formation.coupled_count = 7;
        s.formation.parked_head_chainage_m = Some(1200.0);
        s.formation.parked_brakes = vec![cylinder(1000.0)];
        let parked = state(&s, 7);
        assert_eq!(parked.speed, 0.0);
        assert_eq!(parked.variable1, 0.0);
        assert_eq!(parked.variable2, 0.0);
        assert_eq!(parked.throttle, 0.0);
        assert!(parked.steam_phase.is_none());
        assert!((parked.brake_cylinder - 2.5 * 14.503774).abs() < 1e-4);
        s.state.odometer_m += 100.0;
        assert_eq!(state(&s, 7).distance, parked.distance);
        s.formation.cars[1].powered = false;
        assert_eq!(state(&s, 1).variable2, 0.0);
    }
}
