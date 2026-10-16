//! Original OR1.6.1 tunnel forces with explicitly prescribed subsystem inputs.
//! Force rowN uses rowN−1 geometry, speed, train length/mass. Body area comes
//! from Single(width*height), never observed TrainCrossSectionAreaM2 state.
//! The closed route must discover its own tunnel positions and motion.
use openrailsrs_sim::native_tunnel::{TunnelForceInput, force_for_geometry};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const REFERENCE: &[u8] = include_bytes!(
    "../../../docs/fixtures/compatibility/twenty-20261015/tunnel-or161/reference.json"
);
const PROVENANCE: &[u8] = include_bytes!(
    "../../../docs/fixtures/compatibility/twenty-20261015/tunnel-or161/provenance.json"
);
const PROTOCOL: &[u8] = include_bytes!(
    "../../../docs/fixtures/compatibility/twenty-20261015/tunnel-or161/protocol.json"
);
const MANIFEST: &[u8] = include_bytes!(
    "../../../docs/fixtures/compatibility/twenty-20261015/tunnel-or161/native08-manifest.json"
);
const EXTRACTOR: &[u8] = include_bytes!(
    "../../../docs/fixtures/compatibility/twenty-20261015/tunnel-or161/extract-fixture.py"
);
const PROTOCOL_SHA: &str = "49eed6db5e701f9e513a5f4b15091ba5f8c17a42435136da835a850df8877705";

#[derive(Deserialize)]
struct Fixture {
    schema: u32,
    reference_version: String,
    reference_commit: String,
    protocol_sha256: String,
    tolerance_abs_n: f64,
    route_hardware: RouteHardware,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct RouteHardware {
    authored_route_speed_limit_mps: f64,
    authored_tunnel_overrides: bool,
    single: Geometry,
    double: Geometry,
}
#[derive(Deserialize)]
struct Geometry {
    area_m2: f64,
    perimeter_m: f64,
}
#[derive(Deserialize)]
struct Case {
    force_row: usize,
    context_row: usize,
    car: usize,
    time_s: f64,
    active_previous: bool,
    input: Input,
    tunnel_previous: PreviousTunnel,
    expected_force_n: f64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    train_length_m: f64,
    train_mass_kg: f64,
    car_mass_kg: f64,
    width_m: f64,
    height_m: f64,
    speed_mps: f64,
}
#[derive(Deserialize)]
struct PreviousTunnel {
    ahead_valid: bool,
    behind_valid: bool,
    length_ahead_m: f64,
    length_behind_m: f64,
    num_paths: usize,
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn original_single_double_tunnels_match_previous_tick_context_with_prior_limit() {
    let fixture: Fixture = serde_json::from_slice(REFERENCE).unwrap();
    let provenance: Value = serde_json::from_slice(PROVENANCE).unwrap();
    let protocol: Value = serde_json::from_slice(PROTOCOL).unwrap();
    let manifest: Value = serde_json::from_slice(MANIFEST).unwrap();
    assert_eq!(fixture.schema, 1);
    assert_eq!(fixture.reference_version, "1.6.1");
    assert_eq!(
        fixture.reference_commit,
        "d16e670da333d26d2edfc97d5631a19dadf49ce5"
    );
    assert_eq!(fixture.protocol_sha256, PROTOCOL_SHA);
    assert_eq!(digest(PROTOCOL), PROTOCOL_SHA);
    assert_eq!(fixture.tolerance_abs_n, 0.01);
    assert_eq!(protocol["force_maximum_absolute_error_n"], 0.01);
    assert_eq!(protocol["original_full_service_six_limits_changed"], false);
    assert_eq!(
        provenance["raw_trace_sha256"],
        "362ecf741d06ae02e5bfb6ea67af88559d0f98adf631342d42e070c91f4eb13a"
    );
    assert_eq!(
        provenance["native07_projection_sha256"],
        "21212399f6289ff44b6d108d6312670008e6503abee0054df2d4dd150d6e2be2"
    );
    assert_eq!(provenance["original_resources_bundled"], false);
    assert_eq!(
        manifest["files_sha256"]["native/trace.csv"],
        provenance["raw_trace_sha256"]
    );
    assert_eq!(digest(MANIFEST), provenance["native08_manifest_sha256"]);
    assert_eq!(manifest["rows"], 32583);
    assert_eq!(manifest["previous_columns"], 164);
    for (name, bytes) in [
        ("reference.json", REFERENCE),
        ("protocol.json", PROTOCOL),
        ("native08-manifest.json", MANIFEST),
        ("extract-fixture.py", EXTRACTOR),
    ] {
        assert_eq!(
            provenance["files_sha256"][name],
            digest(bytes),
            "Changed evidence{name}"
        );
    }
    let route = &fixture.route_hardware;
    assert_eq!(route.authored_route_speed_limit_mps, 223.52);
    assert!(!route.authored_tunnel_overrides);
    assert_eq!((route.single.area_m2, route.single.perimeter_m), (70., 32.));
    assert_eq!(
        (route.double.area_m2, route.double.perimeter_m),
        (100., 37.5)
    );
    assert!((100..=300).contains(&fixture.cases.len()));
    let mut cars = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut calls = 0usize;
    let mut inactive = 0usize;
    let mut saw400 = false;
    let mut saw500 = false;
    for case in &fixture.cases {
        assert_eq!(case.context_row + 1, case.force_row);
        assert!(case.force_row > 0 && case.force_row < 32583);
        assert!(case.car < 7);
        assert!(case.input.width_m > 0. && case.input.height_m > 0.);
        assert!(case.input.train_mass_kg.is_finite() && case.input.car_mass_kg > 0.);
        if case.context_row == 0 {
            // Native Initialize has not published aggregate Train.MassKg yet.
            // This inactive initial context is retained as captured evidence.
            assert_eq!(case.input.train_mass_kg, 0.);
            assert!(!case.active_previous);
        } else {
            assert!(case.input.train_mass_kg > 0.);
        }
        assert!((case.time_s - case.force_row as f64 * 0.05).abs() < 1e-8);
        let previous = &case.tunnel_previous;
        if !case.active_previous {
            // This is evidence of the captured exit phase. The pure geometry
            // helper has no active flag; runtime path-presence guards have
            // their own integration tests and are not faked by this branch.
            assert_eq!(case.expected_force_n, 0.);
            inactive += 1;
            continue;
        }
        assert!(previous.ahead_valid && previous.behind_valid);
        let geometry = if previous.num_paths >= 2 {
            &route.double
        } else {
            &route.single
        };
        let input = &case.input;
        let actual = force_for_geometry(
            &TunnelForceInput {
                train_length_m: input.train_length_m,
                train_mass_kg: input.train_mass_kg,
                car_mass_kg: input.car_mass_kg,
                car_cross_section_m2: ((input.width_m as f32) * (input.height_m as f32)) as f64,
                speed_mps: input.speed_mps,
            },
            ((previous.length_ahead_m as f32) + (previous.length_behind_m as f32)) as f64,
            geometry.area_m2,
            geometry.perimeter_m,
        );
        assert!(actual.is_finite());
        assert!(
            (actual - case.expected_force_n).abs() <= fixture.tolerance_abs_n,
            "forceRow{} car{} contextRow{}: Rust{actual} versus OR{} with prior0.01N limit",
            case.force_row,
            case.car,
            case.context_row,
            case.expected_force_n
        );
        cars.insert(case.car);
        paths.insert(previous.num_paths);
        saw400 |= case.time_s == 400.;
        saw500 |= case.time_s == 500.;
        calls += 1;
    }
    assert_eq!(cars.len(), 7);
    assert!(paths.contains(&1) && paths.contains(&2));
    assert!(saw400 && saw500);
    assert!(calls >= 100 && inactive > 0);
    assert_eq!(provenance["geometry_calls"], calls);
    assert_eq!(provenance["inactive_transition_records"], inactive);
}
