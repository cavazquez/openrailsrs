//! CPU regressions against immutable calls of the original OR 1.6.1 DLLs.
//! The checked-in capture needs neither Wine nor installed original content.
//! An explicit environment override selects another complete capture; missing
//! evidence is an error, never a pass.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};
fn capture() -> (PathBuf, Value, Value, Value) {
    let dir = std::env::var_os("OPENRAILSRS_PHYSICAL_SYSTEMS_CAPTURE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../docs/fixtures/compatibility/twenty-20261015/physical-native05")
        });
    let read =
        |name: &str| serde_json::from_slice::<Value>(&fs::read(dir.join(name)).unwrap()).unwrap();
    let manifest = read("manifest.json");
    assert_eq!(manifest["capture_kind"], "original_dll_calls");
    assert_eq!(manifest["reference_version"], "1.6.1");
    assert_eq!(
        manifest["reference_commit"],
        "d16e670da333d26d2edfc97d5631a19dadf49ce5"
    );
    for required in [
        "inputs.json",
        "reference.json",
        "protocol.json",
        "registration.json",
        "capture-script.py",
        "PhysicalSystemsReference.cs",
        "wine-isolation/isolation.json",
        "wine-isolation/release.json",
    ] {
        assert!(
            manifest["files"]
                .as_object()
                .unwrap()
                .contains_key(required),
            "Missing provenance: {required}"
        );
    }
    for (name, hash) in manifest["files"].as_object().unwrap() {
        let digest = format!("{:x}", Sha256::digest(fs::read(dir.join(name)).unwrap()));
        assert_eq!(digest, hash.as_str().unwrap(), "provenance changed: {name}");
    }
    let input = read("inputs.json");
    let reference = read("reference.json");
    let protocol = read("protocol.json");
    assert_eq!(protocol["status"], "prepared_not_executed");
    assert_eq!(protocol["limits_declared_before_capture"], true);
    for (name, hash) in protocol["assemblies_sha256"].as_object().unwrap() {
        assert_eq!(
            &manifest["assemblies"][name], hash,
            "Original DLL hash: {name}"
        );
    }
    (dir, input, reference, protocol)
}
fn n(v: &Value, key: &str) -> f64 {
    v[key].as_f64().unwrap()
}
fn near(actual: f64, expected: f64, tolerance: f64, label: &str) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{label}: Rust {actual}, OR {expected}, tolerance {tolerance}"
    );
}

#[test]
fn original_steam_property_interpolators_match_at_knots_and_between_them() {
    let (_, input, reference, protocol) = capture();
    let limits = &protocol["native_limits"]["steam_tables"];
    let rows = reference["properties"].as_array().unwrap();
    assert_eq!(rows.len(), input["pressure_psi"].as_array().unwrap().len());
    assert!(rows.len() > 150);
    for r in rows {
        let p =
            openrailsrs_sim::steam_thermodynamics::properties(n(r, "pressure_psi") / 14.503773773);
        near(
            p.temperature_c * 9. / 5. + 32.,
            n(r, "temperature_f"),
            n(limits, "temperature_f_abs"),
            "saturation temperature",
        );
        near(
            p.water_enthalpy_j_kg / 2326.0003249,
            n(r, "water_heat_btu_lb"),
            n(limits, "enthalpy_btu_lb_abs"),
            "liquid heat",
        );
        near(
            p.steam_enthalpy_j_kg / 2326.0003249,
            n(r, "steam_heat_btu_lb"),
            n(limits, "enthalpy_btu_lb_abs"),
            "steam heat",
        );
        near(
            p.water_density_kg_m3 / 16.018463374,
            n(r, "water_density_lb_ft3"),
            n(limits, "density_lb_ft3_abs"),
            "liquid density",
        );
        near(
            p.steam_density_kg_m3 / 16.018463374,
            n(r, "steam_density_lb_ft3"),
            n(limits, "density_lb_ft3_abs"),
            "steam density",
        );
    }
}

#[test]
fn original_emergency_service_pipe3_reservoir_and_rearm_match_runtime_subsystem() {
    use openrailsrs_sim::brake::{BrakeSystem, BrakeVehicleSpec};
    let (_, input, reference, protocol) = capture();
    let tolerance = &protocol["native_limits"]["pneumatic"];
    let profiles = input["brake_profiles"].as_array().unwrap();
    let captures = reference["brakes"].as_array().unwrap();
    assert_eq!(captures.len(), 3);
    for (profile, data) in profiles.iter().zip(captures) {
        let fields = profile["fields"].as_object().unwrap();
        let mut text = "Wagon ( reference Size (3m 4m 20m) ".to_string();
        for (key, value) in fields {
            text.push_str(&format!(" {key} ( {} )", value.as_str().unwrap()));
        }
        text.push_str(" ORTSRSAdvancedEmergency (1) )");
        let ast = openrailsrs_formats::parse_vehicle_text(&text).unwrap();
        let parsed = openrailsrs_formats::parse_vehicle_brake_profile(&ast);
        openrailsrs_formats::typed::vehicle_runtime::validate_advanced_emergency(&ast, &parsed)
            .unwrap();
        let mut system = BrakeSystem::from_vehicle_specs(
            &[BrakeVehicleSpec {
                native_legacy_shoe_curve: None,
                profile: parsed,
                position_m: 0.,
                max_force_n: n(data, "max_force_n"),
                ep_instant: false,
                shoe_friction: openrailsrs_formats::BrakeShoeFrictionCurve::identity(),
                mass_kg: 40000.,
                skid_adhesion_mu: 0.,
            }],
            200.,
            false,
            3.,
        );
        let cyl = &mut system.cylinders[0];
        cyl.initialize_released_pneumatic_pressure_psi(n(&data["initial"], "pipe_psi"))
            .unwrap();
        let rows = data["points"].as_array().unwrap();
        assert!(rows.len() > 2000);
        let mut phases = std::collections::BTreeSet::new();
        let mut emergency = false;
        let mut independent = false;
        for row in rows {
            phases.insert(row["phase"].as_str().unwrap());
            cyl.prescribe_independent_pressure_psi(n(row, "input_independent_psi"))
                .unwrap();
            cyl.advance_prescribed_pneumatic_boundary(n(row, "input_pipe_psi"), n(row, "dt_s"))
                .unwrap();
            let r = cyl.emergency_pneumatic_status().unwrap();
            for (actual, key) in [
                (r.pipe_psi, "pipe_psi"),
                (r.cylinder_psi, "cylinder_psi"),
                (r.auxiliary_psi, "auxiliary_psi"),
                (r.emergency_reservoir_psi, "emergency_reservoir_psi"),
                (r.control_psi, "control_psi"),
            ] {
                near(
                    actual,
                    n(row, key),
                    n(tolerance, "pressure_psi_abs"),
                    &format!("{} tick{} {key}", profile["id"], row["tick"]),
                );
            }
            near(
                cyl.current_force_n,
                n(row, "shoe_force_n"),
                n(tolerance, "shoe_force_n_abs"),
                "shoe force",
            );
            assert_eq!(r.valve, row["valve"].as_str().unwrap());
            emergency |= r.valve == "Emergency";
            independent |= row["phase"] == "independent_only" && r.cylinder_psi > 0.;
        }
        assert!(
            phases.contains("service_hold")
                && phases.contains("emergency")
                && phases.contains("rearm")
        );
        assert!(emergency && independent);
    }
}

#[test]
fn original_three_car_propagation_keeps_only_the_head_boundary_prescribed() {
    use openrailsrs_sim::brake::{BrakeSystem, BrakeVehicleSpec};
    let (_, input, reference, protocol) = capture();
    let limits = &protocol["native_limits"]["pneumatic"];
    let profiles = input["brake_profiles"].as_array().unwrap();
    let captures = reference["brake_networks"]
        .as_array()
        .expect("Missing real PropagateBrakePressure calls");
    assert_eq!(profiles.len(), captures.len());
    for (profile, data) in profiles.iter().zip(captures) {
        assert_eq!(profile["id"], data["id"]);
        let mut text = "Wagon ( reference ".to_string();
        for (key, value) in profile["fields"].as_object().unwrap() {
            text.push_str(&format!(" {key} ( {} )", value.as_str().unwrap()));
        }
        text.push_str(" ORTSRSAdvancedEmergency (1) )");
        let ast = openrailsrs_formats::parse_vehicle_text(&text).unwrap();
        let parsed = openrailsrs_formats::parse_vehicle_brake_profile(&ast);
        openrailsrs_formats::typed::vehicle_runtime::validate_advanced_emergency(&ast, &parsed)
            .unwrap();
        let hardware = data["hardware"].as_array().unwrap();
        assert_eq!(hardware.len(), 3);
        let mut position = 0.;
        let specs = hardware
            .iter()
            .map(|h| {
                let mut profile = parsed.clone();
                let emergency = profile
                    .native_air
                    .as_mut()
                    .unwrap()
                    .emergency
                    .as_mut()
                    .unwrap();
                emergency.pipe_volume_m3 = n(h, "pipe_volume_m3");
                emergency.pipe_volume_authored = true;
                let spec = BrakeVehicleSpec {
                    native_legacy_shoe_curve: None,
                    profile,
                    position_m: position,
                    max_force_n: n(h, "max_force_n"),
                    ep_instant: false,
                    shoe_friction: openrailsrs_formats::BrakeShoeFrictionCurve::identity(),
                    mass_kg: 40000.,
                    skid_adhesion_mu: 0.,
                };
                position += n(h, "length_m");
                spec
            })
            .collect::<Vec<_>>();
        let mut system = BrakeSystem::from_vehicle_specs(&specs, 200., false, 3.);
        for (cylinder, initial) in system
            .cylinders
            .iter_mut()
            .zip(data["initial"].as_array().unwrap())
        {
            cylinder
                .initialize_released_pneumatic_pressure_psi(n(initial, "pipe_psi"))
                .unwrap();
        }
        let rows = data["points"].as_array().unwrap();
        assert!(rows.len() > 2000);
        let mut nonuniform = false;
        let mut emergency_at_tail = false;
        for row in rows {
            system
                .advance_prescribed_pneumatic_network(n(row, "input_head_pipe_psi"), n(row, "dt_s"))
                .unwrap();
            let cars = row["cars"].as_array().unwrap();
            assert_eq!(cars.len(), 3);
            let actual = system
                .cylinders
                .iter()
                .map(|c| c.emergency_pneumatic_status().unwrap())
                .collect::<Vec<_>>();
            nonuniform |= (actual[0].pipe_psi - actual[2].pipe_psi).abs() > 1e-5;
            emergency_at_tail |= actual[2].valve == "Emergency";
            for (i, (c, expected)) in actual.iter().zip(cars).enumerate() {
                for (value, key) in [
                    (c.pipe_psi, "pipe_psi"),
                    (c.cylinder_psi, "cylinder_psi"),
                    (c.auxiliary_psi, "auxiliary_psi"),
                    (c.emergency_reservoir_psi, "emergency_reservoir_psi"),
                    (c.control_psi, "control_psi"),
                ] {
                    near(
                        value,
                        n(expected, key),
                        n(limits, "pressure_psi_abs"),
                        &format!("{} network tick{} car{i} {key}", profile["id"], row["tick"]),
                    );
                }
                near(
                    system.cylinders[i].current_force_n,
                    n(expected, "shoe_force_n"),
                    n(limits, "shoe_force_n_abs"),
                    "network shoe force",
                );
                assert_eq!(c.valve, expected["valve"].as_str().unwrap());
            }
        }
        assert!(
            nonuniform && emergency_at_tail,
            "No propagation/emergency coverage at the trailing car"
        );
    }
}
