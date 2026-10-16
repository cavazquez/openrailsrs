//! Authority, bounded TCP and real railway snapshots, without a renderer.
use openrailsrs_game::multiplayer::*;
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

fn room() -> Secret {
    Secret("only-a-disposable-test-room".into())
}
fn credential(name: &str) -> Secret {
    Secret(format!("disposable-test-identity-{name}"))
}
fn catalog() -> Vec<TrainInfo> {
    vec![
        TrainInfo {
            id: "one".into(),
            steam: true,
            cars: 1,
            departed: true,
        },
        TrainInfo {
            id: "two".into(),
            steam: false,
            cars: 2,
            departed: true,
        },
    ]
}
fn hello(name: &str, role: Role, train: Option<&str>) -> Hello {
    Hello {
        version: PROTOCOL_VERSION,
        room: room(),
        identity: name.into(),
        credential: credential(name),
        content_id: "own-demo-v1".into(),
        role,
        train: train.map(str::to_owned),
    }
}
fn connect(
    authority: &mut Authority,
    peer: PeerId,
    name: &str,
    role: Role,
    train: Option<&str>,
) -> (u64, u64) {
    match authority
        .connect(peer, hello(name, role, train), &catalog())
        .unwrap()
    {
        ServerMessage::Welcome { epoch, lease, .. } => (epoch, lease),
        _ => panic!("expected a validated welcome"),
    }
}
fn controls() -> DriverControls {
    DriverControls {
        throttle: 0.45,
        train_brake: 0.,
        independent_brake: None,
        direction: 1.,
        headlights: 1,
        cab_light: false,
        wipers: true,
        horn: false,
    }
}
fn accepted(message: &ServerMessage) -> bool {
    matches!(
        message,
        ServerMessage::Ack {
            accepted: true,
            processed: true,
            ..
        }
    )
}

#[test]
fn two_drivers_dispatcher_and_fireman_have_separate_authority() {
    let mut authority = Authority::new(room(), "own-demo-v1".into()).unwrap();
    let (epoch, first) = connect(&mut authority, 1, "driver-one", Role::Driver, Some("one"));
    let (_, second) = connect(&mut authority, 2, "driver-two", Role::Driver, Some("two"));
    let (_, dispatcher) = connect(&mut authority, 3, "dispatcher", Role::Dispatcher, None);
    let (_, fireman) = connect(&mut authority, 4, "fireman", Role::Fireman, Some("one"));
    let (_, spectator) = connect(&mut authority, 5, "observer", Role::Spectator, None);
    assert!(
        authority
            .connect(
                6,
                hello("other-driver", Role::Driver, Some("one")),
                &catalog()
            )
            .is_err()
    );
    assert!(
        authority
            .connect(
                6,
                hello("diesel-fireman", Role::Fireman, Some("two")),
                &catalog()
            )
            .is_err()
    );
    let mut applied = vec![];
    for (peer, lease, train) in [(1, first, "one"), (2, second, "two")] {
        let response = authority.command(
            peer,
            epoch,
            lease,
            1,
            RemoteCommand::Drive {
                controls: controls(),
            },
            &catalog(),
            |member, _| {
                applied.push(member.train.clone().unwrap());
                Ok(())
            },
        );
        assert!(accepted(&response));
        assert_eq!(applied.last().unwrap(), train);
    }
    let mut dispatch_count = 0;
    assert!(accepted(&authority.command(
        3,
        epoch,
        dispatcher,
        1,
        RemoteCommand::Priority {
            train: "one".into(),
            priority: 2
        },
        &catalog(),
        |_, _| {
            dispatch_count += 1;
            Ok(())
        }
    )));
    assert_eq!(dispatch_count, 1);
    let fire = RemoteCommand::Fire {
        controls: FiremanControls {
            automatic: false,
            firing: 0.4,
            damper: 0.6,
            injector1: true,
            injector2: false,
            blower: true,
        },
    };
    let mut fire_count = 0;
    assert!(accepted(&authority.command(
        4,
        epoch,
        fireman,
        1,
        fire.clone(),
        &catalog(),
        |member, _| {
            assert_eq!(member.train.as_deref(), Some("one"));
            fire_count += 1;
            Ok(())
        }
    )));
    for (peer, lease, sequence, action) in [
        (1, first, 2, fire),
        (
            4,
            fireman,
            2,
            RemoteCommand::Drive {
                controls: controls(),
            },
        ),
        (
            5,
            spectator,
            1,
            RemoteCommand::Drive {
                controls: controls(),
            },
        ),
        (
            5,
            spectator,
            2,
            RemoteCommand::Signal {
                id: "signal".into(),
                aspect: None,
            },
        ),
    ] {
        assert!(!accepted(&authority.command(
            peer,
            epoch,
            lease,
            sequence,
            action,
            &catalog(),
            |_, _| { panic!("a rejected role must never reach railway state") }
        )));
    }
    assert_eq!(fire_count, 1);
}

#[test]
fn retries_gaps_and_reconnect_never_repeat_a_command() {
    let mut authority = Authority::new(room(), "own-demo-v1".into()).unwrap();
    let (epoch, lease) = connect(&mut authority, 1, "driver", Role::Driver, Some("one"));
    let mut applications = 0;
    let action = RemoteCommand::Drive {
        controls: controls(),
    };
    assert!(!accepted(&authority.command(
        1,
        epoch,
        lease,
        2,
        action.clone(),
        &catalog(),
        |_, _| { panic!("a gap has not been processed") }
    )));
    for _ in 0..2 {
        assert!(accepted(&authority.command(
            1,
            epoch,
            lease,
            1,
            action.clone(),
            &catalog(),
            |_, _| {
                applications += 1;
                Ok(())
            }
        )));
    }
    assert_eq!(applications, 1);
    authority.disconnect(1);
    assert!(authority.human_trains().contains("one"));
    assert_eq!(authority.disconnected_drivers(), vec!["one"]);
    assert!(
        authority
            .connect(3, hello("thief", Role::Driver, Some("one")), &catalog())
            .is_err()
    );
    let (_, new_lease) = connect(&mut authority, 2, "driver", Role::Spectator, None);
    assert_ne!(lease, new_lease);
    assert_eq!(authority.member(2).unwrap().role, Role::Driver);
    assert!(!accepted(&authority.command(
        1,
        epoch,
        lease,
        2,
        action.clone(),
        &catalog(),
        |_, _| { panic!("the replaced socket is revoked") }
    )));
    assert!(accepted(&authority.command(
        2,
        epoch,
        new_lease,
        1,
        action.clone(),
        &catalog(),
        |_, _| { panic!("the replayed command has a stored receipt") }
    )));
    assert!(accepted(&authority.command(
        2,
        epoch,
        new_lease,
        2,
        action,
        &catalog(),
        |_, _| {
            applications += 1;
            Ok(())
        }
    )));
    assert_eq!(applications, 2);
}

#[test]
fn role_changes_are_atomic_and_checkpoint_restarts_revoke_old_epochs() {
    let mut authority = Authority::new(room(), "own-demo-v1".into()).unwrap();
    let (epoch, lease) = connect(&mut authority, 1, "driver", Role::Driver, Some("one"));
    let _ = connect(&mut authority, 2, "fireman", Role::Fireman, Some("one"));
    let denied = authority.command(
        1,
        epoch,
        lease,
        1,
        RemoteCommand::Role {
            role: Role::Fireman,
            train: Some("one".into()),
        },
        &catalog(),
        |_, _| Ok(()),
    );
    assert!(!accepted(&denied));
    assert_eq!(authority.member(1).unwrap().role, Role::Driver);
    let checkpoint = authority.checkpoint();
    let bytes = serde_json::to_vec(&checkpoint).unwrap();
    let mut restored =
        Authority::restore(serde_json::from_slice(&bytes).unwrap(), &catalog()).unwrap();
    assert_eq!(restored.roster().iter().filter(|m| m.connected).count(), 0);
    let (new_epoch, new_lease) = connect(&mut restored, 3, "driver", Role::Spectator, None);
    assert_ne!(epoch, new_epoch);
    assert!(!accepted(&restored.command(
        3,
        epoch,
        new_lease,
        2,
        RemoteCommand::Drive {
            controls: controls()
        },
        &catalog(),
        |_, _| panic!("old epoch")
    )));
    assert!(!accepted(&restored.command(
        3,
        new_epoch,
        new_lease,
        1,
        RemoteCommand::Role {
            role: Role::Fireman,
            train: Some("one".into())
        },
        &catalog(),
        |_, _| panic!("stored denial")
    )));
    assert!(accepted(&restored.command(
        3,
        new_epoch,
        new_lease,
        2,
        RemoteCommand::Role {
            role: Role::Spectator,
            train: None
        },
        &catalog(),
        |_, _| Ok(())
    )));
    assert!(!restored.human_trains().contains("one"));
    let mut duplicate = checkpoint.clone();
    duplicate.members.get_mut("fireman").unwrap().lease = lease;
    assert!(Authority::restore(duplicate, &catalog()).is_err());
    let mut wrong_identity = checkpoint;
    wrong_identity.members.get_mut("driver").unwrap().identity = "other".into();
    assert!(Authority::restore(wrong_identity, &catalog()).is_err());
}

#[test]
fn malformed_commands_and_client_authoritative_snapshots_are_rejected() {
    let mut bad = controls();
    bad.throttle = f64::NAN;
    assert!(!RemoteCommand::Drive { controls: bad }.valid());
    assert!(
        !RemoteCommand::SteamDriver {
            cutoff: 0.8,
            cylinder_cocks: false
        }
        .valid()
    );
    assert!(
        !RemoteCommand::Fire {
            controls: FiremanControls {
                automatic: false,
                firing: 1.1,
                damper: 0.,
                injector1: false,
                injector2: false,
                blower: false
            }
        }
        .valid()
    );
    assert!(serde_json::from_str::<ClientMessage>(r#"{"type":"frame","world":{}}"#).is_err());
    assert!(serde_json::from_str::<ClientMessage>(r#"{"type":"command","epoch":1,"lease":1,"sequence":1,"action":{"command":"ping","client_millis":0},"world":{}}"#).is_err());
    assert_eq!(format!("{:?}", room()), "<redacted>");
    assert!(
        openrailsrs_game::multiplayer::transport::encode(
            &vec![0u8; MAX_REQUEST_BYTES],
            MAX_REQUEST_BYTES
        )
        .is_err()
    );
}

fn wait_event(host: &HostTransport, timeout: Duration) -> TransportEvent {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(event) = host.poll() {
            return event;
        }
        assert!(
            Instant::now() < deadline,
            "TCP request did not arrive in time"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn tcp_transport_reconnects_and_drop_releases_the_port() {
    let host = HostTransport::bind("127.0.0.1:0".parse::<SocketAddr>().unwrap()).unwrap();
    let address = host.address();
    let client =
        ClientTransport::connect(address, hello("observer", Role::Spectator, None)).unwrap();
    let peer = match wait_event(&host, Duration::from_secs(3)) {
        TransportEvent::Request {
            peer,
            message: ClientMessage::Hello { .. },
        } => peer,
        other => panic!("unexpected TCP event: {other:?}"),
    };
    assert!(host.connected_peers().contains(&peer));
    host.close(peer);
    let deadline = Instant::now() + Duration::from_secs(4);
    let replacement = loop {
        if let Some(TransportEvent::Request {
            peer: other,
            message: ClientMessage::Hello { .. },
        }) = host.poll()
            && other != peer
        {
            break other;
        }
        assert!(
            Instant::now() < deadline,
            "automatic TCP reconnect did not occur"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(replacement > peer);
    let dropping = Instant::now();
    drop(client);
    drop(host);
    assert!(dropping.elapsed() < Duration::from_secs(2));
    assert!(
        TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_err(),
        "host worker must release its listener on drop"
    );
}

#[test]
fn an_oversized_wire_frame_is_closed_before_deserializing_a_body() {
    use std::io::Write;
    let host = HostTransport::bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let mut socket = TcpStream::connect(host.address()).unwrap();
    socket
        .write_all(&((MAX_REQUEST_BYTES + 1) as u32).to_be_bytes())
        .unwrap();
    assert!(matches!(
        wait_event(&host, Duration::from_secs(3)),
        TransportEvent::Disconnected { .. }
    ));
    let deadline = Instant::now() + Duration::from_secs(1);
    while !host.connected_peers().is_empty() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn driver_and_fireman_commands_reach_the_same_real_boiler_then_restore() {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/multiplayer");
    let scenario = openrailsrs_scenarios::load_scenario(directory.join("scenario.toml")).unwrap();
    let mut player =
        openrailsrs_sim::LiveDriveSession::from_scenario(&directory, &scenario).unwrap();
    let mut traffic = openrailsrs_sim::LiveTraffic::from_scenario(&directory, &scenario).unwrap();
    let member = Member {
        identity: "driver".into(),
        credential: credential("driver"),
        role: Role::Driver,
        train: Some(player.service_id.clone()),
        followed: None,
        last_sequence: 0,
        lease: 1,
        receipts: vec![],
        peer: Some(1),
    };
    apply_command(
        &mut player,
        &mut traffic,
        &member,
        &RemoteCommand::Drive {
            controls: controls(),
        },
    )
    .unwrap();
    apply_command(
        &mut player,
        &mut traffic,
        &member,
        &RemoteCommand::SteamDriver {
            cutoff: 0.5,
            cylinder_cocks: true,
        },
    )
    .unwrap();
    let mut fireman = member.clone();
    fireman.role = Role::Fireman;
    apply_command(
        &mut player,
        &mut traffic,
        &fireman,
        &RemoteCommand::Fire {
            controls: FiremanControls {
                automatic: false,
                firing: 0.4,
                damper: 0.6,
                injector1: true,
                injector2: false,
                blower: true,
            },
        },
    )
    .unwrap();
    let boiler = player.state.boiler_state.as_ref().unwrap();
    assert_eq!(boiler.controls.cutoff, 0.5);
    assert!(boiler.controls.cylinder_cocks && boiler.controls.injector1);
    assert!(!boiler.controls.automatic_fireman);
    assert_eq!(player.driver_throttle, 0.45);
    traffic.advance(&mut player, 1., None, |_| {});
    let saved = capture_world(&mut player, &mut traffic, false).unwrap();
    let total_cars: usize = train_catalog(&player, &traffic)
        .iter()
        .map(|t| t.cars)
        .sum();
    let mut new_player =
        openrailsrs_sim::LiveDriveSession::from_scenario(&directory, &scenario).unwrap();
    let mut new_traffic =
        openrailsrs_sim::LiveTraffic::from_scenario(&directory, &scenario).unwrap();
    restore_world(&mut new_player, &mut new_traffic, saved).unwrap();
    assert_eq!(
        train_catalog(&new_player, &new_traffic)
            .iter()
            .map(|t| t.cars)
            .sum::<usize>(),
        total_cars
    );
    assert_eq!(new_player.driver_throttle, 0.45);
    assert_eq!(
        new_player
            .state
            .boiler_state
            .as_ref()
            .unwrap()
            .controls
            .cutoff,
        0.5
    );
    assert_eq!(new_player.state.time.0, player.state.time.0);
}

#[test]
fn native_manual_fireman_cannot_be_changed_to_an_unsupported_automatic_state() {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/physics_lab");
    let scenario =
        openrailsrs_scenarios::load_scenario(directory.join("scenario_native_steam.toml")).unwrap();
    let mut player =
        openrailsrs_sim::LiveDriveSession::from_scenario(&directory, &scenario).unwrap();
    let mut traffic = openrailsrs_sim::LiveTraffic::from_scenario(&directory, &scenario).unwrap();
    let id = player.service_id.clone();
    let member = Member {
        identity: "native-fireman".into(),
        credential: credential("native-fireman"),
        role: Role::Fireman,
        train: Some(id.clone()),
        followed: None,
        last_sequence: 0,
        lease: 1,
        receipts: vec![],
        peer: Some(1),
    };
    let controls = FiremanControls {
        automatic: true,
        firing: 0.5,
        damper: 0.5,
        injector1: false,
        injector2: false,
        blower: false,
    };
    let before = player.state.steam_boilers.clone();
    assert!(
        apply_command(
            &mut player,
            &mut traffic,
            &member,
            &RemoteCommand::Fire { controls }
        )
        .is_err()
    );
    assert_eq!(before, player.state.steam_boilers);
    player.driver_throttle = 0.7;
    player.driver_brake = 0.;
    openrailsrs_sim::steam_equipment::set_local_regulator(&mut player.state, 0, Some(0.6)).unwrap();
    player.trigger_horn(1.);
    world::set_automatic_fireman(&mut player, &mut traffic, &id, true);
    assert_eq!(player.driver_throttle, 0.);
    assert_eq!(player.driver_brake, 1.);
    assert_eq!(player.state.steam_boilers[0].local_regulator, Some(0.));
    assert!(!player.horn_active());
    assert!(
        !player
            .state
            .boiler_state
            .as_ref()
            .unwrap()
            .controls
            .automatic_fireman
    );
    assert!(player.state.steam_boilers[0].valid());
    let saved: openrailsrs_sim::SessionSnapshot =
        serde_json::from_slice(&serde_json::to_vec(&player.snapshot()).unwrap()).unwrap();
    let mut restored =
        openrailsrs_sim::LiveDriveSession::from_scenario(&directory, &scenario).unwrap();
    restored.restore_snapshot(saved).unwrap();
    assert_eq!(restored.state.steam_boilers[0].local_regulator, Some(0.));
    assert!(
        !restored.state.steam_boilers[0]
            .boiler
            .controls
            .automatic_fireman
    );
    assert_eq!(restored.state.boiler_state, player.state.boiler_state);
}
