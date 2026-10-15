//! Authored profiles, persistent constitutive history and finite buffer stroke.
//! DLL comparisons are separate: these energy/history laws are Rust extensions.
use openrailsrs_formats::parse_vehicle_text;
use openrailsrs_sim::coupler::{
    CouplerState, VehicleState, active_face_constraint_force_n, inelastic_contact, multi_body_step,
    multi_body_step_distributed_oriented,
};
use openrailsrs_train::coupler_profile::{CouplerCurve, parse_vehicle_couplers};

fn joint() -> CouplerState {
    let ast = parse_vehicle_text(
        r#"Wagon ( QA Coupling ( Spring (
        ORTSTensionStiffness ( 100kN 500kN ) ORTSCompressionStiffness ( 150kN 500kN )
        ORTSTensionR0 ( 0m 0.01m ) ORTSCompressionR0 ( 0m 0.005m )
        ORTSTensionSlack ( 0.02m 0.04m ) ORTSCompressionSlack ( 0.015m 0.03m )
        ORTSBreak ( 600kN 800kN ) ) ) )"#,
    )
    .unwrap();
    let p = parse_vehicle_couplers(&ast, false).unwrap();
    CouplerState::from_vehicle_ends(&p, &p).unwrap()
}

#[test]
fn native_units_separate_advanced_forces_from_legacy_stiffness_and_orient_ends() {
    let input: serde_json::Value = serde_json::from_str(include_str!(
        "../../../docs/fixtures/compatibility/coupler-forces-or161/input.json"
    ))
    .unwrap();
    let cases = input["profiles"].as_array().unwrap();
    let profile = |id: &str| {
        let row = cases.iter().find(|r| r["id"] == id).unwrap();
        parse_vehicle_couplers(
            &parse_vehicle_text(row["text"].as_str().unwrap()).unwrap(),
            row["flipped"].as_bool().unwrap(),
        )
        .unwrap()
    };
    let metric = profile("legacy_metric");
    let end = metric.rear_in_consist().unwrap();
    assert!((end.simple_slack_m - 0.02).abs() < 1e-12);
    assert!((end.tension.stroke_m() * 2. - 0.035).abs() < 1e-12);
    assert!((end.tension.maximum_stiffness_npm() / 2. - 21e6).abs() < 1e-6);
    let normal = profile("two_ends");
    let flipped = profile("two_ends_flipped");
    assert_ne!(
        normal.rear_in_consist().unwrap().warning_force_n,
        flipped.rear_in_consist().unwrap().warning_force_n
    );
    assert_eq!(
        normal.front_in_consist().unwrap().warning_force_n,
        flipped.rear_in_consist().unwrap().warning_force_n
    );
    let advanced = profile("advanced_metric");
    let p = advanced.rear_in_consist().unwrap();
    assert!(p.advanced);
    assert!((p.compression.points[1].force_n - 150_000.).abs() < 1e-6);
    assert!((p.compression.stroke_m() - 0.05).abs() < 1e-12);
}

#[test]
fn invalid_authored_units_are_actionable_instead_of_becoming_newtons() {
    let ast = parse_vehicle_text("Wagon ( Q Coupling ( Spring ( Stiffness ( 1bogus 2N/m ) ) ) )")
        .unwrap();
    let error = parse_vehicle_couplers(&ast, false).unwrap_err().to_string();
    assert!(error.contains("coupler") && error.contains("Stiffness"));
}

#[test]
fn two_buffers_share_force_and_their_travel_and_energy_add_in_series() {
    let c = joint();
    let curve = &c.native_profile.as_ref().unwrap().compression;
    assert!((curve.stroke_m() - 0.1).abs() < 1e-12);
    assert!((curve.max_force_n() - 500_000.).abs() < 1e-8); // not 1MN
    assert!((curve.force_n(0.04) - 150_000.).abs() < 1e-8);
    assert!((curve.elastic_energy_j(0.1) - 21_750.).abs() < 1e-8);
    let pair = CouplerCurve::in_series(curve, curve);
    assert!((pair.elastic_energy_j(pair.stroke_m()) - 43_500.).abs() < 1e-7);
}

#[test]
fn turning_points_change_unloading_force_and_survive_save_restore() {
    let mut continuous = joint();
    for x in [0., 0.025, 0.04, 0.06, 0.10, 0.12, 0.11] {
        continuous.extension_m = x;
        continuous.advance_force_n(0., 0.05, 1.);
    }
    let checkpoint = serde_json::to_vec(&continuous).unwrap();
    let mut restored: CouplerState = serde_json::from_slice(&checkpoint).unwrap();
    for x in [0.10, 0.09, 0.08, 0.07, 0.08, 0.10, 0.07, 0.03, 0.] {
        continuous.extension_m = x;
        restored.extension_m = x;
        let a = continuous.advance_force_n(0., 0.05, 1.);
        let b = restored.advance_force_n(0., 0.05, 1.);
        assert!((a - b).abs() < 1e-8);
        assert!(
            (continuous.memory.absorbed_energy_j - restored.memory.absorbed_energy_j).abs() < 1e-8
        );
    }
    assert!(continuous.memory.absorbed_energy_j > 1.);
    assert!(continuous.memory.stored_energy_j < 1e-8);
    let mut unloading = joint();
    for x in [0., 0.06, 0.12, 0.05] {
        unloading.extension_m = x;
        unloading.advance_force_n(0., 0.05, 1.);
    }
    let mut first_loading = joint();
    first_loading.extension_m = 0.05;
    let first = first_loading.advance_force_n(0., 0.05, 1.);
    assert!(unloading.memory.previous_force_n < first);
}

#[test]
fn compressed_stroke_contact_conserves_momentum_and_flags_excess_energy() {
    let mut vehicles = vec![
        VehicleState {
            velocity_mps: 0.,
            position_m: 0.,
        },
        VehicleState {
            velocity_mps: 5.,
            position_m: 0.,
        },
    ];
    let mut couplers = vec![joint()];
    let masses = [40_000., 60_000.];
    let initial_momentum = 300_000.;
    let initial_relative_energy = 300_000.;
    for _ in 0..500 {
        multi_body_step(
            &mut vehicles,
            &mut couplers,
            0.,
            &[0., 0.],
            &[0., 0.],
            &masses,
            0.01,
            1.,
        );
        assert!(couplers[0].extension_m >= -0.10000001);
        assert!(
            (vehicles[0].position_m - vehicles[1].position_m - couplers[0].extension_m).abs()
                < 1e-8
        );
        let relative_velocity = vehicles[0].velocity_mps - vehicles[1].velocity_mps;
        let relative_energy = 0.5 * 24_000. * relative_velocity.powi(2);
        let ledger = relative_energy
            + couplers[0].memory.stored_energy_j
            + couplers[0].memory.absorbed_energy_j;
        assert!(
            ledger <= initial_relative_energy * 1.01,
            "a passive buffer must not create energy: ledger={ledger}"
        );
    }
    let momentum = masses[0] * vehicles[0].velocity_mps + masses[1] * vehicles[1].velocity_mps;
    assert!((momentum - initial_momentum).abs() < 1e-5);
    assert!(couplers[0].memory.impact_out_of_range_count > 0);
    assert!(couplers[0].memory.compression_absorbed_energy_j > 21_750.);
    assert!(
        vehicles
            .iter()
            .all(|v| v.velocity_mps.is_finite() && v.position_m.is_finite())
    );
}

#[test]
fn contact_impulse_is_relative_energy_not_absolute_train_speed() {
    let rest = inelastic_contact(40_000., 60_000., 0., 0.2);
    let moving = inelastic_contact(40_000., 60_000., 25., 25.2);
    assert!((rest.absorbed_energy_j - 480.).abs() < 1e-9);
    assert!((moving.absorbed_energy_j - rest.absorbed_energy_j).abs() < 1e-7);
    assert!((moving.leading_velocity_mps - 25. - rest.leading_velocity_mps).abs() < 1e-12);
}

#[test]
fn original_or161_dll_profiles_forces_and_stroke_impulses_match_declared_tolerances() {
    let input: serde_json::Value = serde_json::from_str(include_str!(
        "../../../docs/fixtures/compatibility/coupler-forces-or161/input.json"
    ))
    .unwrap();
    let reference: serde_json::Value = serde_json::from_str(include_str!(
        "../../../docs/fixtures/compatibility/coupler-forces-or161/reference.json"
    ))
    .unwrap();
    let tolerance = &input["tolerances_predeclared"];
    let distance = tolerance["distance_abs_m"].as_f64().unwrap();
    let force_abs = tolerance["force_abs_n"].as_f64().unwrap();
    let force_rel = tolerance["force_relative"].as_f64().unwrap();
    let check = |label: &str, actual: f64, expected: f64, abs: f64, rel: f64| {
        assert!(
            (actual - expected).abs() <= abs + rel * expected.abs(),
            "{label}: Rust {actual}, original OR {expected}, tolerance {}",
            abs + rel * expected.abs()
        );
    };
    for row in input["profiles"].as_array().unwrap() {
        let expected = reference["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == row["id"])
            .unwrap();
        let profile = parse_vehicle_couplers(
            &parse_vehicle_text(row["text"].as_str().unwrap()).unwrap(),
            row["flipped"].as_bool().unwrap(),
        )
        .unwrap();
        let end = profile.rear_in_consist().unwrap();
        assert_eq!(end.advanced, expected["advanced"].as_bool().unwrap());
        assert_eq!(end.rigid, expected["rigid"].as_bool().unwrap());
        assert_eq!(
            usize::from(profile.rear.is_some()) + usize::from(profile.front.is_some()),
            expected["count"].as_u64().unwrap() as usize
        );
        for (label, value) in [
            ("zero_length_m", end.zero_length_m),
            ("simple_slack_m", end.simple_slack_m),
        ] {
            check(
                label,
                value,
                expected[label].as_f64().unwrap(),
                distance,
                0.,
            );
        }
        for (label, value) in [
            ("warning_force_n", end.warning_force_n),
            ("break_force_n", end.break_force_n),
        ] {
            check(
                label,
                value,
                expected[label].as_f64().unwrap(),
                force_abs,
                force_rel,
            );
        }
        let simple_k = if end.simple_r0_m[0] == 0. {
            7. * end.simple_stiffness_npm.iter().sum::<f64>()
        } else {
            2e7
        };
        check(
            "simple_stiffness_npm",
            simple_k,
            expected["simple_stiffness_npm"].as_f64().unwrap(),
            force_abs,
            force_rel,
        );
        if end.advanced {
            for (curve, zones, forces) in [
                (&end.tension, "tension_zones_m", "tension_forces_n"),
                (
                    &end.compression,
                    "compression_zones_m",
                    "compression_forces_n",
                ),
            ] {
                for (i, point) in curve.points.iter().enumerate() {
                    check(
                        zones,
                        point.displacement_m,
                        expected[zones][i].as_f64().unwrap(),
                        distance,
                        0.,
                    );
                    if i > 0 {
                        check(
                            forces,
                            point.force_n,
                            expected[forces][i - 1].as_f64().unwrap(),
                            force_abs,
                            force_rel,
                        );
                    }
                }
            }
        } else {
            check(
                "legacy loading stiffness",
                end.tension.maximum_stiffness_npm() / 2.,
                expected["simple_stiffness_npm"].as_f64().unwrap(),
                force_abs,
                force_rel,
            );
            check(
                "simple_stroke_m",
                end.tension.stroke_m() * 2.,
                expected["simple_stroke_m"].as_f64().unwrap(),
                distance,
                0.,
            );
        }
    }
    for row in input["forces"].as_array().unwrap() {
        let expected = reference["forces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == row["id"])
            .unwrap();
        // This two-body constraint is the production rigid-contact path. The
        // advanced elastic return-point model has separate extension tests.
        let force = active_face_constraint_force_n(
            row["leading_mass_kg"].as_f64().unwrap(),
            row["trailing_mass_kg"].as_f64().unwrap(),
            row["leading_force_n"].as_f64().unwrap(),
            row["trailing_force_n"].as_f64().unwrap(),
            row["slack_m"].as_f64().unwrap(),
            0.02,
            0.02,
        );
        check(
            row["id"].as_str().unwrap(),
            force,
            expected["tension_positive_force_n"].as_f64().unwrap(),
            force_abs,
            force_rel,
        );
    }
    for row in input["impacts"].as_array().unwrap() {
        let expected = reference["impacts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == row["id"])
            .unwrap();
        let response = inelastic_contact(
            row["leading_mass_kg"].as_f64().unwrap(),
            row["trailing_mass_kg"].as_f64().unwrap(),
            row["leading_velocity_mps"].as_f64().unwrap(),
            row["trailing_velocity_mps"].as_f64().unwrap(),
        );
        check(
            "contact impulse",
            response.impulse_ns,
            expected["tension_positive_impulse_ns"].as_f64().unwrap(),
            tolerance["impulse_abs_ns"].as_f64().unwrap(),
            0.,
        );
        for (key, value) in [
            ("leading_velocity_mps", response.leading_velocity_mps),
            ("trailing_velocity_mps", response.trailing_velocity_mps),
        ] {
            check(
                key,
                value,
                expected[key].as_f64().unwrap(),
                tolerance["velocity_abs_mps"].as_f64().unwrap(),
                0.,
            );
        }
    }
}

#[test]
fn restore_rejects_changed_hardware_nonfinite_history_and_impossible_buffer_states() {
    let expected = joint();
    let mut valid = expected.clone();
    valid.extension_m = 0.08;
    valid.advance_force_n(0., 0.05, 1.);
    assert!(valid.valid_for(&expected));
    valid.native_profile.as_mut().unwrap().sources = vec!["moved content library".into()];
    assert!(valid.valid_for(&expected));
    let mut bad = valid.clone();
    bad.memory.stored_energy_j = f64::NAN;
    assert!(!bad.valid_for(&expected));
    let mut bad = valid.clone();
    bad.native_profile.as_mut().unwrap().compression.points[1].force_n += 1.;
    assert!(!bad.valid_for(&expected));
    let mut bad = valid.clone();
    bad.extension_m = -0.11;
    assert!(!bad.valid_for(&expected));
    let mut bad = valid.clone();
    bad.memory.reversal_extension_m = -99.;
    assert!(!bad.valid_for(&expected));
    let mut bad = valid.clone();
    bad.memory.stored_energy_j = 1e15;
    assert!(!bad.valid_for(&expected));
    let mut bad = valid.clone();
    bad.memory.previous_force_n = f64::INFINITY;
    assert!(!bad.valid_for(&expected));
    let mut bad = valid.clone();
    bad.memory.constraint_force_n = f64::NAN;
    assert!(!bad.valid_for(&expected));
    let mut bad = valid.clone();
    bad.native_profile = None;
    assert!(!bad.valid_for(&expected));
    let mut bad = valid;
    bad.memory.compression_absorbed_energy_j = 1.;
    bad.memory.absorbed_energy_j = 0.;
    assert!(!bad.valid_for(&expected));
}

#[test]
fn absent_advanced_compression_force_does_not_invent_an_elastic_spring() {
    let ast =
        parse_vehicle_text("Wagon ( Q Coupling ( Spring ( ORTSTensionSlack ( 0.01m 0.02m ) ) ) )")
            .unwrap();
    let p = parse_vehicle_couplers(&ast, false).unwrap();
    let mut c = CouplerState::from_vehicle_ends(&p, &p).unwrap();
    c.extension_m = -0.08;
    assert_eq!(c.advance_force_n(0., 0.05, 1.), 0.);
    assert!(c.valid_for(&CouplerState::from_vehicle_ends(&p, &p).unwrap()));
}

#[test]
fn sustained_brakes_cannot_accelerate_a_stationary_native_formation_backwards() {
    let mut vehicles = vec![
        VehicleState {
            velocity_mps: 0.,
            position_m: 0.,
        },
        VehicleState {
            velocity_mps: 0.,
            position_m: 0.,
        },
    ];
    let mut couplers = vec![joint()];
    for _ in 0..100 {
        multi_body_step(
            &mut vehicles,
            &mut couplers,
            0.,
            &[180_000., 50_000.],
            &[0., 0.],
            &[40_000., 60_000.],
            0.05,
            1.,
        );
    }
    assert!(
        vehicles
            .iter()
            .all(|v| v.velocity_mps == 0. && v.position_m == 0.)
    );
    assert_eq!(couplers[0].memory.absorbed_energy_j, 0.);
}

#[test]
fn rigid_constraint_diagnostic_is_not_clipped_to_an_elastic_loading_envelope() {
    let ast = parse_vehicle_text("Wagon ( Q Coupling ( CouplingHasRigidConnection ( 1 ) Spring ( Stiffness ( 1000000N/m 2000000N/m ) R0 ( 0m 0.06m ) Break ( 400kN 900kN ) ) ) )").unwrap();
    let profile = parse_vehicle_couplers(&ast, false).unwrap();
    let expected = CouplerState::from_vehicle_ends(&profile, &profile).unwrap();
    let mut c = expected.clone();
    c.extension_m = 0.00015;
    let mut couplers = vec![c];
    let mut vehicles = vec![
        VehicleState {
            velocity_mps: 0.,
            position_m: 0.,
        },
        VehicleState {
            velocity_mps: 0.,
            position_m: 0.,
        },
    ];
    multi_body_step(
        &mut vehicles,
        &mut couplers,
        120_000.,
        &[0., 0.],
        &[0., 0.],
        &[40_000., 60_000.],
        0.01,
        1.,
    );
    assert!((couplers[0].memory.constraint_force_n - 72_000.).abs() < 1e-6);
    assert!(
        couplers[0].memory.previous_force_n < 3000.,
        "elastic history is bounded independently"
    );
    assert!(couplers[0].valid_for(&expected));
    let restored: CouplerState =
        serde_json::from_str(&serde_json::to_string(&couplers[0]).unwrap()).unwrap();
    assert!(restored.valid_for(&expected));
    assert_eq!(
        restored.memory.constraint_force_n,
        couplers[0].memory.constraint_force_n
    );
}

#[test]
fn reversing_a_stationary_virtual_axis_preserves_physical_deflection_and_history() {
    let mut c = joint();
    c.extension_m = 0.08;
    c.advance_force_n(0., 0.05, 1.);
    let mut couplers = vec![c];
    let mut vehicles = vec![
        VehicleState {
            velocity_mps: 0.,
            position_m: 0.,
        },
        VehicleState {
            velocity_mps: 0.,
            position_m: 0.,
        },
    ];
    multi_body_step_distributed_oriented(
        &mut vehicles,
        &mut couplers,
        &[0., 0.],
        &[1e9, 1e9],
        &[0., 0.],
        &[40_000., 60_000.],
        0.01,
        1.,
        1.,
    );
    let before = couplers[0].clone();
    multi_body_step_distributed_oriented(
        &mut vehicles,
        &mut couplers,
        &[0., 0.],
        &[1e9, 1e9],
        &[0., 0.],
        &[40_000., 60_000.],
        0.01,
        1.,
        -1.,
    );
    assert_eq!(couplers[0].memory.position_orientation, Some(-1.));
    assert_eq!(couplers[0].extension_m, before.extension_m);
    assert_eq!(
        couplers[0].memory.previous_force_n,
        before.memory.previous_force_n
    );
    assert_eq!(
        couplers[0].memory.stored_energy_j,
        before.memory.stored_energy_j
    );
    assert_eq!(
        couplers[0].memory.reversal_extension_m,
        before.memory.reversal_extension_m
    );
    assert_eq!(
        couplers[0].memory.absorbed_energy_j,
        before.memory.absorbed_energy_j
    );
    assert_ne!(
        couplers[0].memory.position_reference_m,
        before.memory.position_reference_m
    );
}

#[test]
fn mixed_rigid_contact_reaction_checks_warning_and_break_limits() {
    let ast = parse_vehicle_text("Wagon ( WeakRigid Coupling ( CouplingHasRigidConnection ( 1 ) Spring ( Stiffness ( 1000000N/m 2000000N/m ) R0 ( 0m 0.06m ) Break ( 1kN 2kN ) ) ) )").unwrap();
    let profile = parse_vehicle_couplers(&ast, false).unwrap();
    let rigid = CouplerState::from_vehicle_ends(&profile, &profile).unwrap();
    let expected = rigid.clone();
    let mut couplers = vec![rigid, joint()];
    let mut vehicles = vec![
        VehicleState {
            velocity_mps: 1.,
            position_m: 0.
        };
        3
    ];
    multi_body_step(
        &mut vehicles,
        &mut couplers,
        0.,
        &[1e9, 0., 0.],
        &[0., 0., 0.],
        &[40_000., 60_000., 50_000.],
        0.01,
        1.,
    );
    let c = &couplers[0];
    assert!(
        c.memory.last_impulse_ns < -100.,
        "unequal braking must exercise real stroke contact"
    );
    assert!(
        c.memory.constraint_force_n < -2000.,
        "the contact reaction must not be lost after the end kick"
    );
    assert!(c.memory.warning_exceeded);
    assert!(
        c.broken,
        "the mixed formation cannot bypass the authored break force"
    );
    assert!(
        c.valid_for(&expected),
        "a recorded failure must remain saveable"
    );
    assert!(!couplers[1].broken);
    assert!(
        vehicles
            .iter()
            .all(|v| v.velocity_mps.is_finite() && v.position_m.is_finite())
    );
}
