//! Recorded external noise is independent of vehicle motion/force. The native
//! y[] oracle is consumed only by the isolated filter test, never by a train.
use openrailsrs_scenarios::{NativePhysicsEnvironment, ScenarioFile};
use openrailsrs_sim::native_adhesion_input::{self, AdhesionFilter};
use openrailsrs_sim::{CarOperation, LiveDriveSession, ServicePhase};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn input_csv(last_tick: usize) -> String {
    let mut text =
        "time_s,vehicle,input_factor,base_factor,filter_x0,filter_x1,filter_y0,filter_y1\n"
            .to_owned();
    for tick in 0..=last_tick {
        for vehicle in [0, 1] {
            if tick == 0 {
                text.push_str(&format!("0,{vehicle},0,0,0,0,0,0\n"));
            } else {
                text.push_str(&format!(
                    "{:.2},{vehicle},{},0.5,,,,\n",
                    tick as f64 * 0.05,
                    if vehicle == 0 { 0.53 } else { 0.47 }
                ));
            }
        }
    }
    text
}
fn scenario(dir: &Path, last_tick: usize) -> ScenarioFile {
    let base = root().join("examples/traction_operation");
    let mut s =
        openrailsrs_scenarios::load_scenario(base.join("scenario_two_diesel.toml")).unwrap();
    s.route.path = base.join(&s.route.path).to_string_lossy().into_owned();
    s.train.consist = base.join(&s.train.consist).to_string_lossy().into_owned();
    s.route.start_offset_m = Some(100.);
    s.route.stops.clear();
    s.simulation.duration = last_tick as f64 * 0.05;
    s.simulation.multi_body = true;
    s.simulation.native_physics = Some(NativePhysicsEnvironment {
        adhesion_factor: 0.5,
        adhesion_input_file: Some("inputs.csv".into()),
        adhesion_input_step_s: Some(0.05),
        ..Default::default()
    });
    fs::write(dir.join("inputs.csv"), input_csv(last_tick)).unwrap();
    s
}
fn start(dir: &Path, s: &ScenarioFile) -> LiveDriveSession {
    let mut live = LiveDriveSession::from_scenario(dir, s).unwrap();
    live.driver_throttle = 0.;
    live.driver_brake = 1.;
    live.driver_direction = 0.5;
    live
}

#[test]
fn single_butterworth_matches_all_original_filter_outputs_without_feeding_them_to_physics() {
    let repo = root();
    let oracle: serde_json::Value = serde_json::from_slice(&fs::read(repo.join(
        "docs/fixtures/compatibility/twenty-20261015/chiltern-adhesion161-native01/filter-oracle.json")).unwrap()).unwrap();
    assert_eq!(oracle["outputs_may_drive_runtime"], false);
    let inputs = native_adhesion_input::load(
        &repo.join("examples/chiltern_local"),
        "physics/native-adhesion-inputs.csv",
        0.05,
        750.1,
        8,
        &[0, 7],
    )
    .unwrap();
    let mut filters = [AdhesionFilter::default(), AdhesionFilter::default()];
    let tolerance = oracle["filter_maximum_absolute_error"].as_f64().unwrap();
    let mut maximum_error = 0_f64;
    for row in oracle["expected"].as_array().unwrap() {
        let tick = row[0].as_u64().unwrap() as usize;
        let vehicle = row[1].as_u64().unwrap() as usize;
        let filter = &mut filters[usize::from(vehicle == 7)];
        let series = inputs[vehicle].as_ref().unwrap();
        assert_eq!(series.sha256, oracle["input_sha256"].as_str().unwrap());
        let output = if tick == 0 {
            filter.y[0]
        } else {
            filter.advance(series.samples[tick].input_factor, 0.05)
        };
        let error = (f64::from(output) - row[2].as_f64().unwrap()).abs();
        maximum_error = maximum_error.max(error);
        assert!(
            error <= tolerance,
            "tick={tick}, vehicle={vehicle}, error={error}"
        );
    }
    assert_eq!(oracle["expected"].as_array().unwrap().len(), 30006);
    assert!(maximum_error <= tolerance);
}

#[test]
fn csv_refuses_external_paths_gaps_output_feedback_and_wrong_vehicle_identities() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("inputs.csv"), input_csv(2)).unwrap();
    let load = |path: &str, vehicles: &[usize]| {
        native_adhesion_input::load(dir.path(), path, 0.05, 0.1, 2, vehicles)
    };
    assert!(load("inputs.csv", &[0, 1]).is_ok());
    for path in [
        "../inputs.csv",
        "/tmp/inputs.csv",
        "a/../../inputs.csv",
        r"a\inputs.csv",
    ] {
        assert!(load(path, &[0, 1]).is_err());
    }
    for ids in [&[][..], &[0, 0][..], &[0][..], &[0, 2][..]] {
        assert!(load("inputs.csv", ids).is_err());
    }
    for contents in [
        input_csv(2).replace("0.05,0,0.53,0.5,,,,", "0.05,0,0.53,0.5,,,0.1,"),
        input_csv(2).replace("0.05,0,0.53,0.5,,,,\n", ""),
        input_csv(2).replace("0.05,0,0.53,0.5,,,,", "0.05,0,NaN,0.5,,,,"),
    ] {
        fs::write(dir.path().join("inputs.csv"), contents).unwrap();
        assert!(load("inputs.csv", &[0, 1]).is_err());
    }
    #[cfg(unix)]
    {
        let external = tempfile::NamedTempFile::new().unwrap();
        std::os::unix::fs::symlink(external.path(), dir.path().join("escape.csv")).unwrap();
        assert!(load("escape.csv", &[0, 1]).is_err());
    }
}

#[test]
fn filter_history_continues_after_save_and_rejects_source_sha_cursor_and_car_tampering() {
    let dir = tempfile::tempdir().unwrap();
    let config = scenario(dir.path(), 40);
    let mut original = start(dir.path(), &config);
    original.set_rail_weather(openrailsrs_sim::adhesion::RailWeather::Dry);
    for _ in 0..4 {
        original.step_realtime(0.05, |_| {});
    }
    // When LiveDrive uses wheel/rail contacts, they must consume the input
    // instead of silently replacing both vehicle filters with the mean base.
    for (car, filter) in original
        .state
        .rail_adhesion
        .as_ref()
        .unwrap()
        .cars
        .iter()
        .zip(
            &original
                .state
                .native_dynamics
                .as_ref()
                .unwrap()
                .adhesion_filters,
        )
    {
        if let Some(filter) = filter {
            assert_eq!(car.factor, f64::from(filter.filter.y[0].clamp(0.05, 2.5)));
            assert_ne!(car.factor, 0.5);
        }
    }
    let saved = original.snapshot();
    let mut resumed = start(dir.path(), &config);
    resumed.set_rail_weather(openrailsrs_sim::adhesion::RailWeather::Dry);
    resumed
        .restore_snapshot(serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap())
        .unwrap();
    for _ in 0..5 {
        original.step_realtime(0.05, |_| {});
        resumed.step_realtime(0.05, |_| {});
        assert_eq!(
            original
                .state
                .native_dynamics
                .as_ref()
                .unwrap()
                .adhesion_filters,
            resumed
                .state
                .native_dynamics
                .as_ref()
                .unwrap()
                .adhesion_filters
        );
        assert_eq!(original.state.odometer_m, resumed.state.odometer_m);
    }
    let before = serde_json::to_value(resumed.snapshot()).unwrap();
    for field in ["sha", "cursor", "vehicle", "filter", "missing"] {
        let mut invalid = resumed.snapshot();
        let filters = &mut invalid
            .state
            .native_dynamics
            .as_mut()
            .unwrap()
            .adhesion_filters;
        match field {
            "sha" => filters[0].as_mut().unwrap().source_sha256 = "0".repeat(64),
            "cursor" => filters[0].as_mut().unwrap().cursor += 1,
            "vehicle" => filters[0].as_mut().unwrap().source_vehicle = 1,
            "filter" => filters[0].as_mut().unwrap().filter.y[0] = f32::NAN,
            _ => filters.clear(),
        }
        assert!(
            resumed.restore_snapshot(invalid).is_err(),
            "accepted {field}"
        );
        assert_eq!(serde_json::to_value(resumed.snapshot()).unwrap(), before);
    }
    // Same pathname and numeric samples, different bytes: a fresh content
    // binding must not claim continuity with the previously saved recording.
    let saved = resumed.snapshot();
    let contents = fs::read_to_string(dir.path().join("inputs.csv")).unwrap();
    fs::write(dir.path().join("inputs.csv"), contents + "\n").unwrap();
    let mut changed = start(dir.path(), &config);
    assert!(changed.restore_snapshot(saved).is_err());
}

#[test]
fn detached_locomotive_retains_its_own_source_and_filter_clock_when_collected() {
    let dir = tempfile::tempdir().unwrap();
    let config = scenario(dir.path(), 40);
    let mut live = start(dir.path(), &config);
    live.operate_car(1, CarOperation::Handbrake).unwrap();
    for _ in 0..3 {
        live.step_realtime(0.05, |_| {});
    }
    live.uncouple_after_named(0, "motor-B").unwrap();
    assert_eq!(live.formation.coupled_count, 1);
    for _ in 0..3 {
        live.step_realtime(0.05, |_| {});
    }
    let filters = live
        .state
        .native_dynamics
        .as_ref()
        .unwrap()
        .adhesion_filters
        .clone();
    assert_eq!(filters[0].as_ref().unwrap().source_vehicle, 0);
    assert_eq!(filters[1].as_ref().unwrap().source_vehicle, 1);
    assert_eq!(filters[1].as_ref().unwrap().cursor, 6);
    assert_ne!(
        filters[0].as_ref().unwrap().filter.y,
        filters[1].as_ref().unwrap().filter.y
    );
    let mut resumed = start(dir.path(), &config);
    resumed.restore_snapshot(live.snapshot()).unwrap();
    resumed
        .recouple_section("motor-B", openrailsrs_sim::sections::CouplingEnd::Rear)
        .unwrap();
    assert_eq!(
        resumed
            .state
            .native_dynamics
            .as_ref()
            .unwrap()
            .adhesion_filters,
        filters
    );
    resumed.step_realtime(0.05, |_| {});
    resumed.validate_snapshot(&resumed.snapshot()).unwrap();
    assert_eq!(
        resumed
            .state
            .native_dynamics
            .as_ref()
            .unwrap()
            .adhesion_filters[1]
            .as_ref()
            .unwrap()
            .cursor,
        7
    );
}

#[test]
fn environmental_noise_does_not_prescribe_the_slippery_spot_transition() {
    let dir = tempfile::tempdir().unwrap();
    let config = scenario(dir.path(), 4);
    let live = start(dir.path(), &config);
    let native = live.physics.native.as_ref().unwrap();
    let mut state = native.initial_state(2);
    state.adhesion_factor = 0.8;
    native.advance_adhesion_inputs(&mut state, 0., 0.05);
    let mut expected = AdhesionFilter::default();
    let expected = expected.advance(0.8_f32 + (0.53_f32 - 0.5_f32), 0.05);
    assert_eq!(
        state.adhesion_filters[0].as_ref().unwrap().filter.y[0],
        expected
    );
    assert_eq!(state.adhesion_filters[0].as_ref().unwrap().base_factor, 0.8);
}

#[test]
fn an_exhausted_recording_or_wrong_tick_fails_clearly_without_advancing_physics() {
    let dir = tempfile::tempdir().unwrap();
    let config = scenario(dir.path(), 2);
    let mut live = start(dir.path(), &config);
    assert!(
        live.physics
            .native
            .as_ref()
            .unwrap()
            .validate_adhesion_tick(&live.state, 0.025)
            .is_err()
    );
    live.step_realtime(0.1, |_| {});
    assert!((live.time_s() - 0.1).abs() < 1e-10);
    let before = live
        .state
        .native_dynamics
        .as_ref()
        .unwrap()
        .adhesion_filters
        .clone();
    live.step_realtime(0.05, |_| {});
    assert_eq!(live.gameplay.phase, ServicePhase::Failed);
    assert!(live.state.traction_inhibited);
    assert_eq!(live.driver_brake, 1.);
    assert!((live.time_s() - 0.1).abs() < 1e-10);
    assert_eq!(
        live.state
            .native_dynamics
            .as_ref()
            .unwrap()
            .adhesion_filters,
        before
    );
}

#[test]
fn native_force_uses_cached_abs_speed_until_wheel_slip_selects_the_wheel_channel() {
    use openrailsrs_sim::native_dynamics::NativeAxleState;
    let mut axle = NativeAxleState {
        speed_mps: 1.012903,
        ..Default::default()
    };
    axle.integration.previous_train_speed_mps = Some(0.6926235);
    assert_eq!(axle.traction_speed_mps(0.703804433), 0.6926235);
    axle.integration.wheel_slip = true;
    assert_eq!(axle.traction_speed_mps(0.703804433), 1.012903);
    let restored: NativeAxleState =
        serde_json::from_slice(&serde_json::to_vec(&axle).unwrap()).unwrap();
    assert_eq!(restored.traction_speed_mps(0.703804433), 1.012903);
}
