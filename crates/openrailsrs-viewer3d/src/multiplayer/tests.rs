//! The real viewer plugin and TCP endpoints, with CPU railway state and no GPU.
//! Limits are declared here before execution: state delivery <= 4 seconds;
//! reconnect <= 5 seconds; simulated transport latency is 60 ms in each direction.
use super::*;
use bevy::{input::InputPlugin, state::app::StatesPlugin};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/multiplayer/scenario.toml")
}
#[derive(Resource)]
struct Advance(bool);
fn advance_host(state: Res<MultiplayerState>, advance: Res<Advance>, mut live: ResMut<LiveDrive>) {
    if state.host() && advance.0 {
        let live = &mut *live;
        live.traffic.advance(&mut live.session, 0.05, None, |_| {});
    }
}
fn app(identity: &str, role: Role, train: Option<&str>, key: &Secret) -> App {
    app_at(&scenario_path(), identity, role, train, key)
}
fn app_at(
    path: &std::path::Path,
    identity: &str,
    role: Role,
    train: Option<&str>,
    key: &Secret,
) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin, InputPlugin));
    app.insert_state(ViewerAppState::Playing);
    app.insert_resource(LiveDrive::from_scenario_path(path).unwrap());
    app.insert_resource(crate::player_launch::ActivePlayerContent::default());
    app.insert_resource(Advance(true));
    app.insert_resource(NetworkLaunch {
        host: None,
        join: None,
        identity: None,
        role: None,
        train: None,
        key_file: None,
        restore: false,
        consumed: true,
    });
    app.add_plugins(MultiplayerPlugin);
    app.add_systems(Update, advance_host.after(poll));
    {
        let mut state = app.world_mut().resource_mut::<MultiplayerState>();
        state.render_ui = false;
        state.form.identity = identity.into();
        state.form.role = role;
        state.form.train = train.map(str::to_owned);
        state.form.room = key.clone();
        // No writes to the user's private identity store from these tests.
        state.form.credential = Some(persistence::random_secret().unwrap());
    }
    app
}
fn state(app: &App) -> &MultiplayerState {
    app.world().resource::<MultiplayerState>()
}
fn live(app: &App) -> &LiveDrive {
    app.world().resource::<LiveDrive>()
}
fn queue(app: &mut App, command: RemoteCommand) {
    app.world_mut()
        .resource_mut::<MultiplayerState>()
        .queue(command);
}
fn until(apps: &mut [App], timeout: Duration, condition: impl Fn(&[App]) -> bool) {
    let deadline = Instant::now() + timeout;
    loop {
        for app in &mut *apps {
            app.update();
        }
        if condition(apps) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "network condition did not arrive; {}",
            apps.iter()
                .map(|app| state(app).notice.as_str())
                .collect::<Vec<_>>()
                .join(" | ")
        );
        thread::sleep(Duration::from_millis(5));
    }
}
fn drive(throttle: f64) -> RemoteCommand {
    RemoteCommand::Drive {
        controls: DriverControls {
            throttle,
            train_brake: 0.,
            independent_brake: None,
            direction: 1.,
            headlights: 1,
            cab_light: false,
            wipers: false,
            horn: false,
        },
    }
}
fn host(app: &mut App) {
    {
        let mut state = app.world_mut().resource_mut::<MultiplayerState>();
        state.form.address = "127.0.0.1:0".into();
        state.actions.push(NetworkAction::Host);
    }
    app.update();
    assert!(self::state(app).host());
}
fn join(app: &mut App, address: &str) {
    let mut state = app.world_mut().resource_mut::<MultiplayerState>();
    state.form.address = address.into();
    state.actions.push(NetworkAction::Join);
}

#[test]
fn viewer_two_drivers_dispatch_fireman_spectator_and_shutdown_use_real_tcp() {
    let key = persistence::random_secret().unwrap();
    let mut server = app("dispatcher", Role::Dispatcher, None, &key);
    host(&mut server);
    let address = state(&server).form.address.clone();
    let mut apps = vec![
        server,
        app("driver-one", Role::Driver, Some("Jugador"), &key),
        app("driver-two", Role::Driver, Some("vapor_2"), &key),
        app("fireman", Role::Fireman, Some("Jugador"), &key),
        app("observer", Role::Spectator, None, &key),
    ];
    for client in &mut apps[1..] {
        join(client, &address);
    }
    until(&mut apps, Duration::from_secs(4), |apps| {
        apps.iter()
            .all(|app| state(app).connected && state(app).frames > 0)
            && state(&apps[0]).roster.len() == 5
    });
    queue(&mut apps[1], drive(0.55));
    queue(&mut apps[2], drive(0.35));
    queue(
        &mut apps[1],
        RemoteCommand::SteamDriver {
            cutoff: 0.5,
            cylinder_cocks: true,
        },
    );
    queue(
        &mut apps[3],
        RemoteCommand::Fire {
            controls: FiremanControls {
                automatic: false,
                firing: 0.45,
                damper: 0.65,
                injector1: true,
                injector2: false,
                blower: true,
            },
        },
    );
    queue(
        &mut apps[0],
        RemoteCommand::Switch {
            node: "free_j".into(),
            position: openrailsrs_track::SwitchPosition::Diverging,
        },
    );
    queue(
        &mut apps[0],
        RemoteCommand::Signal {
            id: "signal_b".into(),
            aspect: Some(openrailsrs_track::SignalAspect::Stop),
        },
    );
    queue(
        &mut apps[0],
        RemoteCommand::Priority {
            train: "vapor_2".into(),
            priority: 2,
        },
    );
    queue(
        &mut apps[4],
        RemoteCommand::Follow {
            train: "vapor_2".into(),
        },
    );
    until(&mut apps, Duration::from_secs(4), |apps| {
        let server = live(&apps[0]);
        let boiler = server.session.state.boiler_state.as_ref().unwrap();
        (server.session.driver_throttle - 0.55).abs() < 1e-12
            && (server.traffic.services[0].session.driver_throttle - 0.35).abs() < 1e-12
            && !boiler.controls.automatic_fireman
            && (boiler.controls.firing - 0.45).abs() < 1e-12
            && (boiler.controls.cutoff - 0.5).abs() < 1e-12
            && boiler.controls.injector1
            && server.session.graph.switch_position("free_j")
                == Some(openrailsrs_track::SwitchPosition::Diverging)
            && server.session.snapshot().signal_overrides.get("signal_b")
                == Some(&openrailsrs_track::SignalAspect::Stop)
            && server.traffic.services[0].session.dispatcher.priority == 2
            && state(&apps[4]).followed.as_deref() == Some("vapor_2")
            && state(&apps[3])
                .views
                .iter()
                .any(|v| v.fireman.as_ref().is_some_and(|f| !f.automatic))
    });
    // The observer can render another service, but cannot operate it.
    let before = live(&apps[0]).session.driver_throttle;
    queue(&mut apps[4], drive(1.));
    until(&mut apps, Duration::from_secs(4), |apps| {
        state(&apps[4]).notice.contains("no tiene autoridad")
    });
    assert_eq!(live(&apps[0]).session.driver_throttle, before);
    assert!(state(&apps[4]).views.len() <= 8);
    assert!(
        state(&apps[4])
            .views
            .iter()
            .any(|view| view.id == "vapor_2")
    );
    assert!(
        live(&apps[4])
            .session
            .state
            .boiler_state
            .as_ref()
            .unwrap()
            .controls
            .automatic_fireman,
        "the observer's public pose stream must not install the host's private boiler/controller snapshot"
    );
    assert!(live(&apps[0]).traffic.remote_drivers.contains("vapor_2"));
    until(&mut apps, Duration::from_secs(4), |apps| {
        let server = live(&apps[0]);
        server.session.head_chainage_m() > 80.05
            && server.traffic.services[0].session.head_chainage_m() > 80.05
            && state(&apps[1]).rtt_ms.is_some()
    });
    let speed = live(&apps[0]).traffic.services[0].session.velocity_mps();
    assert!(speed.is_finite());
    // Stop only this client; the host retains its slot and brakes its train.
    apps[2]
        .world_mut()
        .resource_mut::<MultiplayerState>()
        .actions
        .push(NetworkAction::Disconnect);
    until(&mut apps, Duration::from_secs(4), |apps| {
        let server = live(&apps[0]);
        let train = &server.traffic.services[0].session;
        train.driver_throttle == 0.
            && train.driver_brake == 1.
            && state(&apps[0])
                .roster
                .iter()
                .any(|m| m.identity == "driver-two" && !m.connected)
    });
    join(&mut apps[2], &address);
    until(&mut apps, Duration::from_secs(5), |apps| {
        state(&apps[2]).connected && state(&apps[2]).role == Role::Driver
    });
    assert_eq!(state(&apps[2]).train.as_deref(), Some("vapor_2"));
    assert_eq!(live(&apps[0]).traffic.services.len(), 1);
    queue(
        &mut apps[3],
        RemoteCommand::Role {
            role: Role::Spectator,
            train: None,
        },
    );
    until(&mut apps, Duration::from_secs(4), |apps| {
        state(&apps[3]).role == Role::Spectator
            && live(&apps[0])
                .session
                .state
                .boiler_state
                .as_ref()
                .unwrap()
                .controls
                .automatic_fireman
    });
    for app in &mut apps {
        app.world_mut()
            .resource_mut::<NextState<ViewerAppState>>()
            .set(ViewerAppState::Menu);
        app.update();
        assert!(!state(app).active());
    }
    drop(apps);
    assert!(
        TcpStream::connect_timeout(&address.parse().unwrap(), Duration::from_millis(200)).is_err()
    );
}

#[test]
fn viewer_checkpoint_preserves_world_and_revokes_old_authority_on_restore() {
    let key = persistence::random_secret().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server.json");
    let mut server = app("host-driver", Role::Driver, Some("Jugador"), &key);
    server
        .world_mut()
        .resource_mut::<MultiplayerState>()
        .save_file = path.clone();
    server
        .world_mut()
        .resource_mut::<crate::player_launch::ActivePlayerContent>()
        .weather = crate::player_launch::PlayerWeather::Rain;
    let atmosphere = crate::weather_state::Atmosphere {
        rain: 0.6,
        ..Default::default()
    };
    let checkpoint:crate::weather_state::WeatherCheckpoint=serde_json::from_value(serde_json::json!({
        "initial":crate::player_launch::PlayerWeather::Rain,"profile":crate::weather_state::WeatherProfile::SteadyRain,
        "seed":42,"journey":crate::weather_journey::JourneyOptions::default(),"elapsed_s":120.,
        "atmosphere":atmosphere,"transition_from":atmosphere,
    })).unwrap();
    server.insert_resource(checkpoint.restore(0.));
    let mut environment = crate::environment::LiveEnvironment::default();
    environment.utc = chrono::DateTime::from_timestamp(1_791_936_000, 0);
    environment.timezone = Some(chrono_tz::Europe::London);
    environment.location = Some(crate::environment::RouteLocation {
        latitude: 51.5,
        longitude: -0.1,
    });
    server.insert_resource(environment);
    host(&mut server);
    let address = state(&server).form.address.clone();
    let mut apps = vec![server, app("second", Role::Driver, Some("vapor_2"), &key)];
    join(&mut apps[1], &address);
    until(&mut apps, Duration::from_secs(4), |apps| {
        state(&apps[1]).connected && state(&apps[0]).roster.len() == 2
    });
    queue(&mut apps[0], drive(0.4));
    queue(&mut apps[1], drive(0.25));
    until(&mut apps, Duration::from_secs(4), |apps| {
        live(&apps[0]).session.driver_throttle == 0.4
            && live(&apps[0]).traffic.services[0].session.driver_throttle == 0.25
    });
    apps[0]
        .world_mut()
        .resource_mut::<MultiplayerState>()
        .actions
        .push(NetworkAction::Save);
    apps[0].update();
    let saved = persistence::read_save(&path).unwrap();
    assert_eq!(saved.authority.members.len(), 2);
    assert_eq!(saved.game.traffic.len(), 1);
    assert_eq!(
        saved.game.weather,
        crate::player_launch::PlayerWeather::Rain
    );
    assert_eq!(saved.game.weather_checkpoint.as_ref().unwrap().seed, 42);
    let pressure = saved
        .game
        .session
        .state
        .boiler_state
        .as_ref()
        .unwrap()
        .pressure_bar;
    let grants = serde_json::to_value(&saved.game.session.track_reservations).unwrap();
    let old_epoch = saved.authority.epoch;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    // Dispose endpoints, then recover into an independently loaded world.
    for app in &mut apps {
        app.world_mut()
            .resource_mut::<MultiplayerState>()
            .actions
            .push(NetworkAction::Disconnect);
        app.update();
    }
    apps[0]
        .world_mut()
        .resource_mut::<MultiplayerState>()
        .form
        .address = "127.0.0.1:0".into();
    apps[0].world_mut().resource_mut::<Advance>().0 = false;
    apps[0]
        .world_mut()
        .resource_mut::<MultiplayerState>()
        .actions
        .push(NetworkAction::Restore);
    apps[0].update();
    assert!(state(&apps[0]).host());
    let Runtime::Host(host) = state(&apps[0]).runtime.as_ref().unwrap() else {
        panic!("restored host")
    };
    assert!(host.authority.epoch() > old_epoch);
    assert_eq!(host.authority.roster().len(), 2);
    // Freeze only this test's host tick so restoration can be compared before
    // physics advances; this is not a change to the saved railway state.
    assert_eq!(
        live(&apps[0])
            .session
            .state
            .boiler_state
            .as_ref()
            .unwrap()
            .pressure_bar,
        pressure
    );
    assert_eq!(
        serde_json::to_value(&live(&apps[0]).session.snapshot().track_reservations).unwrap(),
        grants
    );
    assert_eq!(live(&apps[0]).traffic.services.len(), 1);
    assert_eq!(live(&apps[0]).session.driver_throttle, 0.);
    assert_eq!(
        live(&apps[0]).traffic.services[0].session.driver_throttle,
        0.
    );
    let restored_address = state(&apps[0]).form.address.clone();
    let previous_frames = state(&apps[1]).frames;
    apps[1].insert_resource(crate::environment::LiveEnvironment::default());
    join(&mut apps[1], &restored_address);
    until(&mut apps, Duration::from_secs(5), |apps| {
        state(&apps[1]).connected
            && state(&apps[1]).frames > previous_frames
            && state(&apps[1]).train.as_deref() == Some("vapor_2")
    });
    let Runtime::Host(host) = state(&apps[0]).runtime.as_ref().unwrap() else {
        unreachable!()
    };
    assert_eq!(
        host.authority
            .roster()
            .iter()
            .filter(|m| m.role == Role::Driver)
            .count(),
        2
    );
    assert_eq!(live(&apps[0]).traffic.services.len(), 1);
    assert_eq!(
        apps[0]
            .world()
            .resource::<crate::player_launch::ActivePlayerContent>()
            .weather,
        crate::player_launch::PlayerWeather::Rain
    );
    assert_eq!(
        apps[0]
            .world()
            .resource::<crate::weather_state::WeatherState>()
            .seed,
        42
    );
    assert_eq!(
        apps[0]
            .world()
            .resource::<crate::environment::LiveEnvironment>()
            .utc
            .unwrap()
            .timestamp(),
        1_791_936_000
    );
    assert_eq!(
        apps[1]
            .world()
            .resource::<crate::environment::LiveEnvironment>()
            .timezone,
        Some(chrono_tz::Europe::London)
    );
    drop(apps);
}

/// A bounded, owned TCP proxy supplies actual latency instead of changing a
/// simulation timestamp or calling the authority directly in place of a client.
struct DelayedLink {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl DelayedLink {
    fn new(server: SocketAddr, delay: Duration) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(6);
            let mut client = loop {
                match listener.accept() {
                    Ok((client, _)) => break client,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if flag.load(Ordering::Acquire) || Instant::now() > deadline {
                            return;
                        }
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => return,
                }
            };
            let Ok(mut upstream) = TcpStream::connect_timeout(&server, Duration::from_millis(500))
            else {
                return;
            };
            client.set_nonblocking(true).unwrap();
            upstream.set_nonblocking(true).unwrap();
            let mut to_server = std::collections::VecDeque::<(Instant, Vec<u8>, usize)>::new();
            let mut to_client = std::collections::VecDeque::<(Instant, Vec<u8>, usize)>::new();
            fn transfer(
                source: &mut TcpStream,
                target: &mut TcpStream,
                queue: &mut std::collections::VecDeque<(Instant, Vec<u8>, usize)>,
                delay: Duration,
            ) -> bool {
                let mut bytes = [0u8; 8192];
                match source.read(&mut bytes) {
                    Ok(0) => return false,
                    Ok(n) => queue.push_back((Instant::now() + delay, bytes[..n].to_vec(), 0)),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(_) => return false,
                }
                if queue.iter().map(|(_, bytes, _)| bytes.len()).sum::<usize>()
                    > 2 * MAX_FRAME_BYTES
                {
                    return false;
                }
                while let Some((ready, bytes, offset)) = queue.front_mut() {
                    if *ready > Instant::now() {
                        break;
                    }
                    match target.write(&bytes[*offset..]) {
                        Ok(0) => return false,
                        Ok(n) => {
                            *offset += n;
                            if *offset == bytes.len() {
                                queue.pop_front();
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(_) => return false,
                    }
                }
                true
            }
            while !flag.load(Ordering::Acquire) {
                if !transfer(&mut client, &mut upstream, &mut to_server, delay)
                    || !transfer(&mut upstream, &mut client, &mut to_client, delay)
                {
                    break;
                }
                thread::sleep(Duration::from_millis(2));
            }
        });
        Self {
            address,
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for DelayedLink {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}

#[test]
fn viewer_with_real_tcp_latency_receives_one_authoritative_result() {
    let key = persistence::random_secret().unwrap();
    let mut server = app("host", Role::Spectator, None, &key);
    host(&mut server);
    let proxy = DelayedLink::new(
        state(&server).form.address.parse().unwrap(),
        Duration::from_millis(60),
    );
    let mut client = app("delayed-driver", Role::Driver, Some("Jugador"), &key);
    join(&mut client, &proxy.address.to_string());
    let mut apps = vec![server, client];
    until(&mut apps, Duration::from_secs(4), |apps| {
        state(&apps[1]).connected && state(&apps[1]).frames > 0
    });
    queue(&mut apps[1], drive(0.3));
    until(&mut apps, Duration::from_secs(4), |apps| {
        state(&apps[1]).rtt_ms.is_some() && live(&apps[0]).session.driver_throttle == 0.3
    });
    let latency = state(&apps[1]).rtt_ms.unwrap();
    assert!(
        (100. ..4000.).contains(&latency),
        "RTT did not include the declared two-way delay: {latency}"
    );
    let Runtime::Host(host) = state(&apps[0]).runtime.as_ref().unwrap() else {
        unreachable!()
    };
    assert_eq!(
        host.authority
            .member_by_id("delayed-driver")
            .unwrap()
            .last_sequence,
        1
    );
    assert_eq!(live(&apps[0]).traffic.services.len(), 1);
    drop(apps);
    drop(proxy);
}

#[test]
fn observer_cameras_follow_rendered_services_and_do_not_create_an_extra_train() {
    let key = persistence::random_secret().unwrap();
    let mut app = app("observer", Role::Spectator, None, &key);
    host(&mut app);
    app.insert_resource(crate::camera::CameraFollowMode::Off);
    app.insert_resource(crate::camera::CameraMode::Orbit);
    let position = Vec3::new(120., 5., 35.);
    app.world_mut().spawn((
        crate::traffic::TrafficTrainMarker(1),
        Transform::from_translation(position),
    ));
    let camera = app
        .world_mut()
        .spawn((
            Camera3d::default(),
            crate::camera::OrbitState::default(),
            Transform::default(),
        ))
        .id();
    app.world_mut().resource_mut::<MultiplayerState>().followed = Some("vapor_2".into());
    app.update();
    assert_eq!(
        app.world()
            .get::<crate::camera::OrbitState>(camera)
            .unwrap()
            .focus,
        position + Vec3::Y * 2.
    );
    app.world_mut().resource_mut::<MultiplayerState>().camera = ObserverCamera::Free;
    app.update();
    assert!(matches!(
        *app.world().resource::<crate::camera::CameraMode>(),
        crate::camera::CameraMode::Fly
    ));
    assert_eq!(live(&app).traffic.services.len(), 1);
}

#[test]
fn cooperative_double_steam_keeps_both_thermodynamic_ledgers_in_the_server_save() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/physics_lab/scenario_double_steam.toml");
    let key = persistence::random_secret().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let mut server = app_at(&path, "driver", Role::Driver, Some("Jugador"), &key);
    server
        .world_mut()
        .resource_mut::<MultiplayerState>()
        .save_file = directory.path().join("thermal-server.json");
    host(&mut server);
    let address = state(&server).form.address.clone();
    let mut fireman = app_at(&path, "fireman", Role::Fireman, Some("Jugador"), &key);
    join(&mut fireman, &address);
    let mut apps = vec![server, fireman];
    until(&mut apps, Duration::from_secs(4), |apps| {
        state(&apps[1]).connected && state(&apps[1]).frames > 0
    });
    queue(&mut apps[0], drive(0.2));
    queue(
        &mut apps[1],
        RemoteCommand::Fire {
            controls: FiremanControls {
                automatic: false,
                firing: 0.4,
                damper: 0.6,
                injector1: false,
                injector2: true,
                blower: true,
            },
        },
    );
    until(&mut apps, Duration::from_secs(4), |apps| {
        !live(&apps[0])
            .session
            .state
            .boiler_state
            .as_ref()
            .unwrap()
            .controls
            .automatic_fireman
    });
    {
        let mut live = apps[0].world_mut().resource_mut::<LiveDrive>();
        openrailsrs_sim::steam_equipment::set_local_regulator(
            &mut live.session.state,
            1,
            Some(0.3),
        )
        .unwrap();
    }
    apps[0]
        .world_mut()
        .resource_mut::<MultiplayerState>()
        .actions
        .push(NetworkAction::Save);
    apps[0].update();
    let saved = persistence::read_save(&state(&apps[0]).save_file).unwrap();
    let boilers = saved.game.session.state.steam_boilers.clone();
    assert_eq!(boilers.len(), 2);
    assert!(boilers.iter().all(|b| b.boiler.thermodynamics.is_some()));
    assert_eq!(boilers[1].local_regulator, Some(0.3));
    for app in &mut apps {
        app.world_mut()
            .resource_mut::<MultiplayerState>()
            .actions
            .push(NetworkAction::Disconnect);
        app.update();
    }
    apps[0].world_mut().resource_mut::<Advance>().0 = false;
    apps[0]
        .world_mut()
        .resource_mut::<MultiplayerState>()
        .form
        .address = "127.0.0.1:0".into();
    apps[0]
        .world_mut()
        .resource_mut::<MultiplayerState>()
        .actions
        .push(NetworkAction::Restore);
    apps[0].update();
    assert!(state(&apps[0]).host());
    let restored = &live(&apps[0]).session.state.steam_boilers;
    assert_eq!(restored.len(), 2);
    for (expected, actual) in boilers.iter().zip(restored) {
        assert_eq!(actual.vehicle, expected.vehicle);
        assert_eq!(
            actual.boiler.thermodynamics, expected.boiler.thermodynamics,
            "restoration must not reset either boiler's mass, energy or compound receiver ledger"
        );
    }
    assert_eq!(
        restored[1].local_regulator,
        Some(0.),
        "a disconnected regulator is closed without deleting its local identity"
    );
    assert_eq!(live(&apps[0]).session.formation.cars.len(), 3);
    let address = state(&apps[0]).form.address.clone();
    let previous_frames = state(&apps[1]).frames;
    join(&mut apps[1], &address);
    until(&mut apps, Duration::from_secs(5), |apps| {
        state(&apps[1]).connected
            && state(&apps[1]).frames > previous_frames
            && state(&apps[1]).role == Role::Fireman
    });
    for (expected, actual) in boilers
        .iter()
        .zip(&live(&apps[1]).session.state.steam_boilers)
    {
        assert_eq!(actual.boiler.thermodynamics, expected.boiler.thermodynamics);
    }
    assert_eq!(state(&apps[0]).roster.len(), 2);
    let position = live(&apps[0]).session.head_chainage_m();
    queue(&mut apps[0], RemoteCommand::ResumeRegulators);
    queue(&mut apps[0], drive(0.45));
    apps[0].world_mut().resource_mut::<Advance>().0 = true;
    until(&mut apps, Duration::from_secs(4), |apps| {
        let s = &live(&apps[0]).session;
        s.state
            .steam_boilers
            .iter()
            .all(|b| b.local_regulator.is_none())
            && s.state.motor_force_n.first().is_some_and(|f| *f > 1.)
            && s.state.motor_force_n.get(1).is_some_and(|f| *f > 1.)
            && s.head_chainage_m() > position + 0.05
            && s.velocity_mps() > 0.01
    });
    drop(apps);
}
