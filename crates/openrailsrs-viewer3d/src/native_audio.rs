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
    ConsistSoundSpec, NativeAudioEngine, SoundFrame, SoundPlacement, SoundState, TrainSoundFrame,
};

#[derive(Resource, Default)]
pub struct NativeAudio {
    pub engine: Option<NativeAudioEngine>,
    initial_announcements: std::collections::HashSet<String>,
    last_clock: f64,
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
    let source = |id: &str| {
        scenario
            .extra_trains
            .iter()
            .find(|entry| entry.id == id)
            .map_or(&scenario.train.consist, |entry| &entry.consist)
    };
    let mut specs = vec![ConsistSoundSpec {
        id: 0,
        vehicle_offset: 0,
        consist: directory.join(source(&live.session.service_id)),
        route: assets.route_dir.clone(),
    }];
    for receipt in &live.traffic.couplings {
        specs.push(ConsistSoundSpec {
            id: 0,
            vehicle_offset: receipt.before.formation.cars.len(),
            consist: directory.join(source(&receipt.donor)),
            route: assets.route_dir.clone(),
        });
    }
    specs.extend(
        live.traffic
            .services
            .iter()
            .enumerate()
            .map(|(i, t)| ConsistSoundSpec {
                id: i + 1,
                vehicle_offset: 0,
                consist: directory.join(source(&t.id)),
                route: assets.route_dir.clone(),
            }),
    );
    audio.initial_announcements = live
        .session
        .state
        .announcements
        .played
        .iter()
        .cloned()
        .collect();
    audio.last_clock = live.session.time_s();
    let root = directory.join(&scenario.train.announcements.root);
    let con = directory.join(&scenario.train.consist);
    let scopes = [
        openrailsrs_train::consist_asset_root(&con).to_path_buf(),
        assets.route_dir.clone(),
        assets
            .route_dir
            .parent()
            .and_then(std::path::Path::parent)
            .unwrap_or(&assets.route_dir)
            .to_path_buf(),
        directory.to_path_buf(),
    ];
    let authorized = root.canonicalize().ok().is_some_and(|root| {
        scopes
            .iter()
            .filter_map(|s| s.canonicalize().ok())
            .any(|s| root.starts_with(s))
    });
    let clips = if authorized {
        scenario
            .train
            .announcements
            .events
            .iter()
            .map(|a| openrailsrs_audio::announcements::LicensedSample {
                id: a.id.clone(),
                path: root.join(&a.file),
                scope: root.clone(),
            })
            .collect()
    } else {
        vec![]
    };
    audio.engine = NativeAudioEngine::start_with_announcements(specs, clips);
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
        c.powered
            && c.power_on
            && c.battery_on
            && (vehicle == session.cab_vehicle() || c.mu_connected)
    }) && !parked;
    let mut powered = controls_powered
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
    // Steam regulator/valve gear does not depend on the cab's battery.
    powered |= steam.is_some() && !parked && car.is_some_and(|c| c.power_on);
    let boiler = steam.and(session.state.boiler_state.as_ref());
    let steam_motion = session.steam_motion(vehicle, 0.0);
    let steam_working = steam_motion.is_some_and(|m| m.working);
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
        air_leak: session.leaking_hose(vehicle).is_some(),
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
                |_s| {
                    if steam_working {
                        steam_motion.map_or(0.0, |m| m.frequency_hz)
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
        steam_phase: steam_motion.filter(|m| m.working).map(|m| m.stroke_phase),
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
        wiper: vehicle == session.cab_vehicle() && t.wiper_active,
        wiper_rate: (1.8 / session.selected_wiper_mode().sweep_s()) as f32,
        doors: matches!(
            session.exterior.door,
            openrailsrs_sim::DoorState::Opening | openrailsrs_sim::DoorState::Open
        ),
        headlights: t.headlights,
        curve_squeal: false,
        coupler_contact: session.state.couplers.get(vehicle).map_or(0, |c| {
            if c.broken || c.extension_m.abs() <= c.free_play_m {
                0
            } else if c.extension_m > 0. {
                1
            } else {
                -1
            }
        }),
        coupler_force_n: session.state.couplers.get(vehicle).map_or(0., |c| {
            c.force_n(
                session
                    .state
                    .vehicles
                    .get(vehicle + 1)
                    .map_or(0., |v| v.velocity_mps)
                    - velocity,
            )
        }) as f32,
        coupler_relative_speed: (session
            .state
            .vehicles
            .get(vehicle + 1)
            .map_or(velocity, |v| v.velocity_mps)
            - velocity) as f32,
        coupling_event: if vehicle + 1 >= session.formation.cars.len() {
            0
        } else if vehicle + 1 == session.formation.coupled_count {
            61
        } else {
            58
        },
    }
}

fn vehicle_states(session: &openrailsrs_sim::LiveDriveSession) -> Vec<SoundState> {
    (0..session.formation.cars.len())
        .map(|index| state(session, index))
        .collect()
}

pub fn update_native_audio(
    live: Option<Res<LiveDrive>>,
    mut audio: ResMut<NativeAudio>,
    weather: Option<Res<crate::weather_state::WeatherState>>,
    settings: Res<PlayerSettings>,
    time: Res<Time>,
    follow: Res<CameraFollowMode>,
    passenger: Option<Res<crate::camera::PassengerCamState>>,
    presentation: Option<Res<crate::train_presentation::TrainPresentation>>,
    camera: Query<&GlobalTransform, (With<Camera3d>, Without<crate::mirrors::MirrorCamera>)>,
    player: Query<(Entity, &GlobalTransform), With<LiveTrainMarker>>,
    traffic: Query<(Entity, &TrafficTrainMarker, &GlobalTransform)>,
    geometry: (
        Res<crate::shapes::RouteAssets>,
        Res<crate::track::TrackScene>,
        Res<crate::track_position::TrackPositionResolverCache>,
        Option<Res<crate::effect_obstacles::EffectObstacles>>,
    ),
    cars: Query<(
        &ChildOf,
        &crate::rolling_stock::ConsistCarIndex,
        &GlobalTransform,
        &crate::rolling_stock_anim::TrainCarTrackOffset,
    )>,
) {
    let Some(live) = live else { return };
    if audio.engine.is_none() {
        return;
    }
    if live.session.time_s() < audio.last_clock {
        audio.initial_announcements = live
            .session
            .state
            .announcements
            .played
            .iter()
            .cloned()
            .collect();
    }
    audio.last_clock = live.session.time_s();
    let Some(_engine) = audio.engine.as_ref() else {
        return;
    };
    let Ok(camera) = camera.single() else { return };
    let car_distances = |head: Option<Entity>, count| {
        let mut distances = vec![f32::INFINITY; count];
        for (parent, car, transform, _) in &cars {
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
        vehicle_placement: vec![],
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
            vehicle_placement: vec![],
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
    let right = camera.rotation() * Vec3::X;
    let (assets, scene, cache, obstacles) = geometry;
    let resolver = assets
        .track_db()
        .map(|t| cache.resolver(t, Some(assets.tsection())));
    let listener_car = if *follow == CameraFollowMode::PassengerCam {
        passenger.as_ref().map_or(0, |p| p.consist_car)
    } else {
        live.session.cab_vehicle()
    };
    let listener_velocity = if *follow == CameraFollowMode::Off {
        Vec3::ZERO
    } else {
        cars.iter()
            .find(|(p, i, _, _)| {
                player_pose.is_some_and(|(e, _)| e == p.parent()) && i.0 == listener_car
            })
            .map_or(Vec3::ZERO, |(_, i, t, o)| {
                crate::train_effects::vehicle_world_velocity(t, o, &live.session, i.0)
            })
    };
    let mut spatial = std::collections::HashMap::new();
    for (parent, car, transform, offset) in &cars {
        let id = if player_pose.is_some_and(|(entity, _)| entity == parent.parent()) {
            0
        } else if let Some((_, marker, _)) = traffic
            .iter()
            .find(|(entity, _, _)| *entity == parent.parent())
        {
            marker.0
        } else {
            continue;
        };
        let session = if id == 0 {
            Some(&live.session)
        } else {
            live.traffic.services.get(id - 1).map(|s| &s.session)
        };
        let Some(session) = session else {
            continue;
        };
        let velocity =
            crate::train_effects::vehicle_world_velocity(transform, offset, session, car.0);
        let hose = session.leaking_hose(car.0);
        let length = session.formation.cars[car.0].length_m as f32;
        let source = transform.transform_point(Vec3::new(0., 1.8, 0.));
        let delta = source - camera.translation();
        let radial = if live.paused {
            0.
        } else {
            (velocity - listener_velocity).dot(delta.normalize_or_zero())
        };
        let blocked = obstacles
            .as_ref()
            .is_some_and(|o| o.blocked(source, camera.translation(), Some(parent.parent())));

        let mut placement =
            SoundPlacement::from_radial_velocity(delta.normalize_or_zero().dot(right), radial);
        placement.blocked = blocked;
        let emitter = |local: Vec3| {
            let source = transform.transform_point(local);
            let d = source - camera.translation();
            let radial = if live.paused {
                0.
            } else {
                (velocity - listener_velocity).dot(d.normalize_or_zero())
            };
            let blocked = obstacles
                .as_ref()
                .is_some_and(|o| o.blocked(source, camera.translation(), Some(parent.parent())));
            openrailsrs_audio::native::EmitterPlacement {
                pan: d.normalize_or_zero().dot(right),
                distance_m: d.length(),
                doppler: SoundPlacement::from_radial_velocity(0., radial).doppler,
                blocked,
            }
        };
        if let Some(horn) = presentation
            .as_ref()
            .filter(|_| id == 0)
            .and_then(|p| p.vehicles.iter().find(|p| p.vehicle == car.0))
            .and_then(|p| p.horn.as_ref())
        {
            let source = emitter(Vec3::from_array(horn.position_m));
            let direction = transform.rotation() * Vec3::from_array(horn.direction).normalize();
            let to_listener = (camera.translation()
                - transform.transform_point(Vec3::from_array(horn.position_m)))
            .normalize_or_zero();
            placement.horn_gain =
                openrailsrs_audio::acoustics::horn_gain(direction.dot(to_listener), source.blocked);
            placement.horn_source = Some(source);
        }
        if let Some(front) = hose {
            placement.leak_source = Some(emitter(Vec3::new(
                0.,
                0.5,
                if front { -length * 0.5 } else { length * 0.5 },
            )));
        }
        spatial.insert((id, car.0), placement);
        if let Some((edge, position)) = session.presentation_position_for_car(
            f64::from(offset.offset_m),
            f64::from(offset.offset_m),
            0.0,
        ) && let Some((radius, _, _)) = resolver
            .as_ref()
            .and_then(|r| r.curve_on_graph_edge(&scene.graph, &edge, position))
            && let Some(train) = trains.iter_mut().find(|t| t.id == id)
            && let Some(state) = train.vehicle_states.get_mut(car.0)
        {
            state.curve_squeal =
                radius.abs() > 0. && radius.abs() < 301. && state.speed.abs() > 0.1;
        }
    }
    for train in &mut trains {
        train.vehicle_placement = (0..train.vehicle_states.len())
            .map(|i| spatial.get(&(train.id, i)).copied().unwrap_or_default())
            .collect();
    }
    let Some(engine) = audio.engine.as_ref() else {
        return;
    };
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
        listener_vehicle: listener_car,
        paused: live.paused,
        volume: if settings.audio_enabled {
            settings.audio_volume
        } else {
            0.0
        },
        mix: settings.audio_mix,
        presentation_dt_s: time.delta_secs(),
        announcements: live
            .session
            .state
            .announcements
            .recent
            .iter()
            .filter(|a| !audio.initial_announcements.contains(&a.key))
            .map(|a| (a.key.clone(), a.id.clone()))
            .collect(),
        window_opening: live
            .session
            .state
            .cab_equipment
            .iter()
            .find(|c| c.config.vehicle == listener_car)
            .map_or(0., |c| c.windows[0].max(c.windows[1]).max(c.door) as f32),
        cover: obstacles.as_ref().map_or(0., |o| {
            if o.overhead(camera.translation()) {
                0.3 + 0.7 * o.enclosure(camera.translation())
            } else {
                0.
            }
        }),
        rain_surface: if matches!(
            *follow,
            CameraFollowMode::DriverCam | CameraFollowMode::Cab2d
        ) {
            openrailsrs_audio::weather::RainSurface::Glass
        } else if *follow == CameraFollowMode::PassengerCam {
            openrailsrs_audio::weather::RainSurface::MetalRoof
        } else if obstacles
            .as_ref()
            .is_some_and(|o| o.overhead(camera.translation()))
        {
            openrailsrs_audio::weather::RainSurface::Shelter
        } else {
            openrailsrs_audio::weather::RainSurface::Outside
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
                    temperature_c: 40.0,
                    cooling_on: false,
                    rpm,
                    demanded_rpm: rpm,
                    command_running: true,
                    phase: openrailsrs_sim::diesel_operation::EnginePhase::Running,
                    fuel_l: 25.,
                    consumed_l: 0.,
                    refilled_l: 0.,
                    flow_lps: 0.,
                    gearbox: None,
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
