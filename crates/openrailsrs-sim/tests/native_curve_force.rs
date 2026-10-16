//! Source-boundary curve-force checks. Original forces are expected outputs
//! here, never inputs to a train's position or longitudinal integration.
use openrailsrs_sim::native_dynamics::{NativeCurveForceInput, NativeTrainPhysics};
use std::{fs, path::PathBuf};
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn native_model(locos: &[usize], count: usize) -> NativeTrainPhysics {
    let ast = openrailsrs_formats::parse("(Engine (Mass 80000))").unwrap();
    NativeTrainPhysics {
        vehicles: (0..count)
            .map(|i| {
                openrailsrs_formats::parse_native_vehicle_physics(&ast, locos.contains(&i), 80000.)
            })
            .collect(),
        vehicle_masses_kg: vec![80000.; count],
        environment: Default::default(),
        control_vehicle_index: locos[0],
        locomotive_indices: locos.to_vec(),
        adhesion_inputs: Vec::new(),
    }
}
fn coefficient(speed: f32, base: f32) -> f32 {
    (7.5_f32 / (speed * 3.6_f32 + 44_f32) + 0.161_f32) * base
}

#[test]
fn original_dll_curve_forces_match_at_the_pure_force_boundary() {
    let fixture:serde_json::Value = serde_json::from_slice(&fs::read(root().join(
        "docs/fixtures/compatibility/twenty-20261015/chiltern-forces161-native02/curve-force-sparse.json")).unwrap()).unwrap();
    let tolerance = fixture["tolerance_n_abs"].as_f64().unwrap();
    assert_eq!(tolerance, 0.01);
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 256);
    let mut maximum_error = 0_f64;
    let mut nonzero = 0;
    for case in cases {
        let input = &case["input"];
        let f = |key: &str| input[key].as_f64().unwrap() as f32;
        let actual = NativeCurveForceInput {
            mass_kg: f("mass_kg"),
            speed_mps: f("speed_mps"),
            radius_m: f("radius_m"),
            cant_m: f("cant_m"),
            gauge_m: f("gauge_m"),
            rigid_wheelbase_m: f("rigid_wheelbase_m"),
            wagon_coefficient: f("wagon_coefficient"),
        }
        .force_n();
        let expected = case["expected_curve_force_n"].as_f64().unwrap();
        nonzero += usize::from(expected > 0.);
        let error = (f64::from(actual) - expected).abs();
        maximum_error = maximum_error.max(error);
        assert!(
            error <= tolerance,
            "tick={}, vehicle={}, expected={expected}, actual={actual}, error={error}",
            case["tick"],
            case["vehicle"]
        );
    }
    assert_eq!(nonzero, 63);
    assert!(maximum_error <= tolerance);
}

#[test]
fn curve_coefficient_uses_mutable_base_cached_speed_and_ordered_native_publication() {
    let native = native_model(&[0, 2], 3);
    let mut state = native.initial_state(2);
    state.adhesion_factor = 0.5;
    state.axles[0].integration.previous_train_speed_mps = Some(2.5);
    state.axles[1].integration.previous_train_speed_mps = Some(8.5);
    let dry = native.prepare_curve_friction(&mut state, &[0, 2], 80.);
    assert_eq!(
        dry,
        vec![
            coefficient(2.5, 0.5),
            coefficient(2.5, 0.5),
            coefficient(8.5, 0.5)
        ]
    );
    state.adhesion_factor = 0.8;
    // The IIR y[] belongs to Pacha, not Train.WagonCoefficientFriction.
    state.adhesion_filters = vec![Some(
        openrailsrs_sim::native_adhesion_input::AdhesionInputState {
            filter: openrailsrs_sim::native_adhesion_input::AdhesionFilter {
                x: [1., 1.],
                y: [2.4, 2.4],
            },
            cursor: 0,
            source_sha256: "a".repeat(64),
            source_vehicle: 0,
            base_factor: 0.8,
        },
    )];
    let changed = native.prepare_curve_friction(&mut state, &[0, 2], 0.);
    assert_eq!(
        changed,
        vec![
            coefficient(2.5, 0.8),
            coefficient(2.5, 0.8),
            coefficient(8.5, 0.8)
        ]
    );
    assert_ne!(changed, dry);
    let native = native_model(&[1], 3);
    let mut leading = native.initial_state(1);
    leading.adhesion_factor = 0.5;
    leading.axles[0].integration.previous_train_speed_mps = Some(3.5);
    assert_eq!(leading.wagon_curve_coefficient, 0.35);
    let first = native.prepare_curve_friction(&mut leading, &[1], 90.);
    assert_eq!(first[0], 0.35);
    assert_eq!(first[1], coefficient(3.5, 0.5));
    assert_eq!(first[2], first[1]);
    leading.adhesion_factor = 0.8;
    let second = native.prepare_curve_friction(&mut leading, &[1], 0.);
    assert_eq!(second[0], first[2]);
    assert_eq!(second[1], coefficient(3.5, 0.8));
}

#[test]
fn shared_curve_bus_survives_save_and_invalid_physics_history_is_rejected_transactionally() {
    let dir = root().join("examples/traction_operation");
    let mut config =
        openrailsrs_scenarios::load_scenario(dir.join("scenario_two_diesel.toml")).unwrap();
    config.route.start_offset_m = Some(100.);
    config.route.stops.clear();
    config.simulation.native_physics = Some(Default::default());
    let mut live = openrailsrs_sim::LiveDriveSession::from_scenario(&dir, &config).unwrap();
    live.step_realtime(0.05, |_| {});
    live.state
        .native_dynamics
        .as_mut()
        .unwrap()
        .wagon_curve_coefficient = 0.27;
    let saved = live.snapshot();
    let mut resumed = openrailsrs_sim::LiveDriveSession::from_scenario(&dir, &config).unwrap();
    resumed
        .restore_snapshot(serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap())
        .unwrap();
    assert_eq!(
        resumed
            .state
            .native_dynamics
            .as_ref()
            .unwrap()
            .wagon_curve_coefficient,
        0.27
    );
    let value = serde_json::to_value(resumed.snapshot()).unwrap();
    for invalid in [f32::NAN, f32::INFINITY, -0.1, 4.1] {
        let mut saved = resumed.snapshot();
        saved
            .state
            .native_dynamics
            .as_mut()
            .unwrap()
            .wagon_curve_coefficient = invalid;
        assert!(resumed.restore_snapshot(saved).is_err());
        assert_eq!(serde_json::to_value(resumed.snapshot()).unwrap(), value);
    }
    // A legacy save lacks this diagnostic bus and receives the native train's
    // initialization value, not a coefficient inferred from its trajectory.
    let mut legacy = serde_json::to_value(resumed.state.native_dynamics.as_ref().unwrap()).unwrap();
    legacy
        .as_object_mut()
        .unwrap()
        .remove("wagon_curve_coefficient");
    let legacy: openrailsrs_sim::native_dynamics::NativeDynamicsState =
        serde_json::from_value(legacy).unwrap();
    assert_eq!(legacy.wagon_curve_coefficient, 0.35);
}
