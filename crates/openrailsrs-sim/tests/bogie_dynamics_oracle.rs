use std::path::PathBuf;

use openrailsrs_sim::bogie_dynamics::{NativeLateralInput, NativeLateralState};
use openrailsrs_train::bogie_geometry::VehicleBogieModel;
use serde_json::Value;
use sha2::{Digest, Sha256};

fn number(v: &Value, key: &str) -> f64 {
    v[key].as_f64().unwrap()
}

#[test]
fn original_dll_lateral_risk_matches_all_48_prescribed_states() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = root.join("docs/fixtures/compatibility/bogie-dynamics-or161");
    let input: Value =
        serde_json::from_slice(&std::fs::read(dir.join("input.json")).unwrap()).unwrap();
    let reference: Value =
        serde_json::from_slice(&std::fs::read(dir.join("reference.json")).unwrap()).unwrap();
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["reference_version"], "1.6.1");
    assert_eq!(
        manifest["reference_commit"],
        "d16e670da333d26d2edfc97d5631a19dadf49ce5"
    );
    assert_eq!(
        manifest["assemblies"]["Orts.Simulation.dll"],
        "dac17afff95141664ee87f734b64e89649699dc2c634196f38b0125d5df09679"
    );
    for (path, key) in [
        (
            "tools/openrails-reference/BogieDynamicsReference.cs",
            "helper_sha256",
        ),
        (
            "scripts/capture_bogie_dynamics_reference.py",
            "capture_script_sha256",
        ),
        ("scripts/oracle_wine.py", "wine_isolation_helper_sha256"),
    ] {
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(std::fs::read(root.join(path)).unwrap())
            ),
            manifest[key].as_str().unwrap()
        );
    }
    let isolation: Value =
        serde_json::from_slice(&std::fs::read(dir.join("wine-isolation/isolation.json")).unwrap())
            .unwrap();
    let release: Value =
        serde_json::from_slice(&std::fs::read(dir.join("wine-isolation/release.json")).unwrap())
            .unwrap();
    assert_eq!(isolation["registry_verified"], true);
    assert_eq!(isolation["graphics_driver"], "null");
    assert_eq!(isolation["show_crash_dialog"], 0);
    assert_eq!(release["remaining_processes"], serde_json::json!([]));
    assert_eq!(release["forbidden_drivers_seen"], serde_json::json!([]));
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(std::fs::read(dir.join("wine-isolation/release.json")).unwrap())
        ),
        manifest["wine_isolation_release_sha256"].as_str().unwrap()
    );
    for (file, key) in [
        ("input.json", "input_sha256"),
        ("reference.json", "output_sha256"),
        ("protocol.json", "protocol_sha256"),
    ] {
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(std::fs::read(dir.join(file)).unwrap())
            ),
            manifest[key].as_str().unwrap()
        );
    }
    assert_eq!(input["tolerances_predeclared"]["force_n_abs"], 0.25);
    assert_eq!(input["tolerances_predeclared"]["angle_rad_abs"], 1e-7);
    let rows = reference["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 48);
    assert_eq!(
        rows.iter()
            .map(|r| (r["profile"].as_str().unwrap(), r["id"].as_str().unwrap()))
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        48
    );
    assert!(
        rows.iter()
            .any(|r| r["possible"] == true && number(r, "elapsed_s") > 0.)
    );
    for row in rows {
        let p = input["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == row["profile"])
            .unwrap();
        let c = input["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == row["id"])
            .unwrap();
        let mut model = VehicleBogieModel::for_length(20.);
        model.mass_kg = number(p, "mass_kg");
        model.gauge_m = number(p, "gauge_m");
        model.cog_height_m = number(p, "cog_height_m");
        model.rigid_wheelbase_m = number(p, "rigid_wheelbase_m");
        model.flange_angle_rad = number(p, "flange_angle_rad");
        model.flange_height_m = number(p, "flange_height_m");
        let seeded = c["seed_flags"].as_bool().unwrap_or(false);
        let mut state = NativeLateralState {
            lateral_force_n: c["initial_lateral_n"].as_f64().unwrap_or(0.),
            possible: seeded,
            expected: seeded,
            has_derailed: seeded,
            ..Default::default()
        };
        state.advance(
            &model,
            NativeLateralInput {
                mass_kg: number(p, "mass_kg"),
                speed_mps: number(c, "speed_mps"),
                radius_m: number(c, "radius_m"),
                cant_angle_rad: number(c, "cant_angle_rad"),
                friction: number(c, "friction"),
                ahead_force_n: c["ahead"]
                    .as_bool()
                    .unwrap()
                    .then(|| number(c, "ahead_force_n")),
                coupler_angle_rad: number(c, "coupler_angle_rad"),
                buff: c["buff"].as_bool().unwrap(),
                axles: c["axles_override"]
                    .as_u64()
                    .unwrap_or(p["axles"].as_u64().unwrap()) as u32,
                bogies: c["bogies_override"]
                    .as_u64()
                    .unwrap_or(p["bogies"].as_u64().unwrap()) as u32,
            },
            number(c, "dt_s"),
        );
        let actual = serde_json::to_value(state).unwrap();
        for key in [
            "vertical_force_n",
            "lateral_force_n",
            "derailment_coefficient",
            "nadal_coefficient",
            "angle_of_attack_rad",
            "climb_distance_m",
            "elapsed_s",
        ] {
            let expected = number(row, key);
            let tolerance = match key {
                "vertical_force_n" | "lateral_force_n" => 0.25 + 3e-6 * expected.abs(),
                "angle_of_attack_rad" => 1e-7,
                "elapsed_s" => 1e-6,
                _ => 1e-5,
            };
            assert!(
                (number(&actual, key) - expected).abs() <= tolerance,
                "{}/{} {key}: Rust={} OR={} tolerance={tolerance}",
                row["profile"],
                row["id"],
                actual[key],
                expected
            );
        }
        for key in [
            "possible",
            "expected",
            "has_derailed",
            "buff_force_exceeded",
        ] {
            assert_eq!(
                actual[key], row[key],
                "{}/{} {key}",
                row["profile"], row["id"]
            );
        }
    }
}
