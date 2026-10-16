//! Compare production gearbox parsing/state/forces with real OR 1.6.1 calls.
//! Expected values are immutable captured DLL outputs, never Rust-generated.
use openrailsrs_formats::{
    GearboxOperation, NativeGearboxProfile, parse_native_gearbox, parse_vehicle_text,
};
use openrailsrs_sim::diesel_operation::{
    DieselCar, DieselTrainConfig, DieselTrainState, EnginePhase, advance_at_speed,
};
use openrailsrs_sim::gearbox::{GearboxInput, GearboxState};
use openrailsrs_train::diesel::DieselEngineParams;
use serde_json::Value;
use std::path::PathBuf;

fn fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/fixtures/compatibility/gearbox-or161")
        .join(name);
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn num(value: &Value, key: &str) -> f64 {
    value[key]
        .as_f64()
        .unwrap_or_else(|| panic!("missing number {key}: {value}"))
}

fn parsed_profile(inputs: &Value, reference: &Value, id: &str) -> (NativeGearboxProfile, Value) {
    let raw = inputs["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|profile| profile["id"] == id)
        .unwrap();
    let native = reference["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|profile| profile["id"] == id)
        .unwrap();
    let fields = raw["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|field| {
            let name = field["token"]
                .as_str()
                .unwrap()
                .strip_prefix("engine(")
                .unwrap();
            format!("{name} ( {} )", field["raw"].as_str().unwrap())
        })
        .collect::<Vec<_>>()
        .join("\n");
    let ast = parse_vehicle_text(&format!("Engine ( NativeCapture\n{fields}\n)"))
        .unwrap_or_else(|error| panic!("{id}: numeric fixture AST: {error}"));
    let params = &native["parameters"];
    let profile = parse_native_gearbox(&ast, num(params, "idle_rpm"), num(params, "max_rpm"))
        .unwrap_or_else(|error| panic!("{id}: native captured authored units: {error}"))
        .expect("captured gearbox must not fall back to ungeared traction");
    (profile, params.clone())
}

fn near(actual: f64, expected: f64, tolerance: f64, context: &str) {
    assert!(actual.is_finite() && expected.is_finite());
    assert!(
        (actual - expected).abs() <= tolerance,
        "{context}: Rust {actual:.10}, native OR {expected:.10}, difference {:.10} > prior limit {tolerance}",
        (actual - expected).abs()
    );
}

fn pairs(value: &Value, key: &str) -> Vec<(f64, f64)> {
    value[key]
        .as_array()
        .unwrap()
        .iter()
        .map(|pair| (pair[0].as_f64().unwrap(), pair[1].as_f64().unwrap()))
        .collect()
}

#[test]
fn authored_gear_units_limits_and_ratios_match_native_dll_initialization() {
    let inputs = fixture("inputs.json");
    let reference = fixture("reference.json");
    let tolerances = fixture("tolerances.json");
    assert_eq!(reference["reference"], "Open Rails 1.6.1");
    assert_eq!(reference["profiles"].as_array().unwrap().len(), 2);
    for native in reference["profiles"].as_array().unwrap() {
        let id = native["id"].as_str().unwrap();
        let (profile, params) = parsed_profile(&inputs, &reference, id);
        assert_eq!(profile.mechanical, params["transmission"] == "Mechanic");
        assert_eq!(
            profile.max_speeds_mps.len(),
            params["number_of_gears"].as_u64().unwrap() as usize
        );
        assert_eq!(
            profile.direct_drive_gear,
            params["direct_drive_gear"].as_u64().unwrap() as usize
        );
        assert_eq!(
            profile.reverse_indication,
            params["reverse_indication"].as_bool().unwrap()
        );
        assert_eq!(
            profile.freewheel,
            params["freewheel_fitted"].as_bool().unwrap()
        );
        near(
            profile.overspeed_percent,
            num(&params, "overspeed_pct"),
            1e-5,
            id,
        );
        near(
            profile.back_load_force_n,
            num(&params, "backload_force_n"),
            num(&tolerances, "force_n_abs"),
            id,
        );
        near(
            profile.coasting_force_n,
            num(&params, "coasting_force_n"),
            num(&tolerances, "force_n_abs"),
            id,
        );
        near(
            profile.up_proportion,
            num(&params, "up_gear_proportion"),
            1e-6,
            id,
        );
        near(
            profile.down_proportion,
            num(&params, "down_gear_proportion"),
            1e-6,
            id,
        );
        near(
            profile.wheel_radius_m,
            num(&params, "wheel_radius_m"),
            num(&tolerances, "speed_mps_abs"),
            id,
        );
        for native_gear in native["gears"].as_array().unwrap() {
            let index = native_gear["index"].as_u64().unwrap() as usize;
            let context = format!("{id} gear {}", index + 1);
            near(
                profile.max_speeds_mps[index],
                num(native_gear, "max_speed_mps"),
                num(&tolerances, "speed_mps_abs"),
                &context,
            );
            near(
                profile.max_forces_n[index],
                num(native_gear, "max_force_n"),
                num(&tolerances, "force_n_abs"),
                &context,
            );
            near(
                GearboxState::ratio(&profile, num(&params, "max_rpm"), index),
                num(native_gear, "ratio"),
                num(&tolerances, "gear_ratio_abs"),
                &context,
            );
        }
    }
}

#[test]
fn native_manual_automatic_clutch_neutral_overspeed_and_force_checkpoints() {
    let inputs = fixture("inputs.json");
    let reference = fixture("reference.json");
    let tolerances = fixture("tolerances.json");
    let mut compared = 0;
    for scenario in inputs["scenarios"].as_array().unwrap() {
        // DieselEngine.Update has a separate production integration test below.
        if scenario["method"] != "GearBox.Update" {
            continue;
        }
        let id = scenario["id"].as_str().unwrap();
        let (mut profile, params) = parsed_profile(
            &inputs,
            &reference,
            scenario["profile_id"].as_str().unwrap(),
        );
        profile.operation = serde_json::from_value(scenario["operation"].clone()).unwrap();
        let captured = reference["scenarios"]
            .as_array()
            .unwrap()
            .iter()
            .find(|sample| sample["id"] == id)
            .unwrap();
        let native_points = captured["checkpoints"].as_array().unwrap();
        let prescribed = scenario["inputs"].as_array().unwrap();
        assert_eq!(native_points.len(), prescribed.len());
        let max_rpm = num(&params, "max_rpm");
        let idle_rpm = num(&params, "idle_rpm");
        let starting_rpm = num(&params, "starting_rpm");
        let mut state = GearboxState::default();
        for (input, expected) in prescribed.iter().zip(native_points) {
            let tick = input["tick"].as_i64().unwrap();
            assert_eq!(input["tick"], expected["tick"]);
            let context = format!("{id} tick {tick}");
            if let Some(clutch) = input["clutch_pct"].as_f64() {
                // This is the native setter's input clamp, not expected physics.
                state.clutch_pct = clutch.clamp(-100., 100.);
            }
            if let Some(next) = input["next_gear_index"].as_i64() {
                assert_ne!(profile.operation, GearboxOperation::Automatic);
                state
                    .request(&profile, next as i32, num(input, "throttle_pct") / 100.)
                    .unwrap();
            }
            if let Some(up) = input["manual_up"].as_bool() {
                state.manual_up = up;
            }
            if let Some(down) = input["manual_down"].as_bool() {
                state.manual_down = down;
            }
            let speed = num(input, "speed_mps")
                * if input["direction"] == "Reverse" {
                    -1.
                } else {
                    1.
                };
            let values = GearboxInput {
                speed_mps: speed,
                throttle: num(input, "throttle_pct") / 100.,
                rpm: num(input, "rpm"),
                output_power_w: num(input, "output_power_w"),
                running: true,
            };
            state.update(&profile, max_rpm, values, num(scenario, "step_s"));
            let shaft = state.shaft_rpm(&profile, max_rpm, values.rpm, speed);
            let clutch = state.clutch_pct;
            let clutch_on = state.current_gear_index >= 0
                && state.observe_clutch(&profile, max_rpm, starting_rpm, values);
            let force = state.force_n(&profile, idle_rpm, max_rpm, values);
            let (warning, error) = state.overspeed(&profile, values.rpm, max_rpm);
            assert_eq!(
                state.current_gear_index,
                expected["current_gear_index"].as_i64().unwrap() as i32,
                "{context}: engaged gear"
            );
            assert_eq!(
                state.next_gear_index,
                expected["next_gear_index"].as_i64().unwrap() as i32,
                "{context}: requested gear"
            );
            near(
                shaft,
                num(expected, "shaft_rpm"),
                num(&tolerances, "shaft_rpm_abs"),
                &format!("{context} shaft RPM"),
            );
            near(
                clutch,
                num(expected, "clutch_pct"),
                num(&tolerances, "clutch_pct_abs"),
                &format!("{context} clutch %"),
            );
            near(
                force,
                num(expected, "tractive_force_n"),
                num(&tolerances, "force_n_abs"),
                &format!("{context} force N"),
            );
            assert_eq!(
                clutch_on,
                expected["clutch_on"].as_bool().unwrap(),
                "{context}: latched clutch"
            );
            assert_eq!(
                warning,
                expected["overspeed_warning"].as_bool().unwrap(),
                "{context}: overspeed warning"
            );
            assert_eq!(
                error,
                expected["overspeed_error"].as_bool().unwrap(),
                "{context}: overspeed failure"
            );
            assert_eq!(
                state.indication(&profile),
                expected["indication"].as_u64().unwrap() as usize,
                "{context}: gear display"
            );
            assert_eq!(
                state.geared_up,
                expected["geared_up"].as_bool().unwrap(),
                "{context}: auto up latch"
            );
            assert_eq!(
                state.geared_down,
                expected["geared_down"].as_bool().unwrap(),
                "{context}: auto down latch"
            );
            compared += 1;
        }
    }
    assert_eq!(
        compared, 493,
        "all captured native GearBox.Update points must be compared"
    );
}

#[test]
fn native_geared_diesel_engine_rpm_clutch_fuel_and_force_at_800_prescribed_ticks() {
    let inputs = fixture("inputs.json");
    let reference = fixture("reference.json");
    let tolerances = fixture("tolerances.json");
    let scenario = inputs["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .find(|scenario| scenario["id"] == "native_geared_engine_prescribed_speed")
        .unwrap();
    assert_eq!(scenario["method"], "DieselEngine.Update");
    let (profile, params) = parsed_profile(
        &inputs,
        &reference,
        scenario["profile_id"].as_str().unwrap(),
    );
    let mut governor = DieselEngineParams::from_msts_defaults(
        num(&params, "maximum_power_w"),
        num(&params, "idle_rpm"),
        num(&params, "max_rpm"),
        num(&params, "change_up_rpm_s"),
    );
    // Native-generated normalized tables are explicit inputs to the engine
    // dynamics comparison; no force/RPM/flow checkpoint is used as input.
    governor.power_tab = pairs(&params, "diesel_power_w");
    governor.throttle_rpm_tab = pairs(&params, "throttle_rpm")
        .into_iter()
        .map(|(throttle, rpm)| (throttle / 100., rpm))
        .collect();
    governor.reverse_throttle_rpm_tab = pairs(&params, "reverse_throttle_rpm")
        .into_iter()
        .map(|(rpm, throttle)| (rpm, throttle / 100.))
        .collect();
    let config = DieselTrainConfig {
        cars: vec![DieselCar {
            vehicle: 0,
            connected: true,
            battery: true,
            params: openrailsrs_formats::DieselOperatingParams {
                hydraulic: None,
                gearbox: Some(profile.clone()),
                thermal: Default::default(),
                // The native subsystem updates engine flow, not the locomotive
                // tank. A generous owned tank keeps depletion outside this case.
                capacity_l: 10_000.,
                starting_rpm: num(&params, "starting_rpm"),
                confirmation_rpm: num(&params, "idle_rpm") * 1.1,
                consumption_lph: pairs(&params, "consumption_lph"),
            },
            governor,
        }],
    };
    let mut state = DieselTrainState::default();
    state.initialize(&config);
    let captured = reference["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .find(|scenario| scenario["id"] == "native_geared_engine_prescribed_speed")
        .unwrap();
    let rows = scenario["inputs"].as_array().unwrap();
    let checkpoints = captured["checkpoints"].as_array().unwrap();
    assert_eq!(rows.len(), 800);
    assert_eq!(checkpoints.len(), 800);
    for (input, expected) in rows.iter().zip(checkpoints) {
        assert!(input["rpm"].is_null(), "RPM must evolve independently");
        assert!(
            input["clutch_pct"].is_null(),
            "clutch must evolve independently"
        );
        let context = format!("native geared engine tick {}", input["tick"]);
        let throttle = num(input, "throttle_pct") / 100.;
        state.cars[0]
            .gearbox
            .as_mut()
            .unwrap()
            .request(
                &profile,
                input["next_gear_index"].as_i64().unwrap() as i32,
                throttle,
            )
            .unwrap();
        let speed = num(input, "speed_mps")
            * if input["direction"] == "Reverse" {
                -1.
            } else {
                1.
            };
        advance_at_speed(
            &mut state,
            &config,
            throttle,
            num(scenario, "step_s"),
            true,
            &Default::default(),
            &Default::default(),
            speed,
        );
        let car = &mut state.cars[0];
        let gearbox = car.gearbox.as_mut().unwrap();
        let values = GearboxInput {
            speed_mps: speed,
            throttle,
            rpm: car.rpm,
            // DieselEngine.Update replaces the pre-update supplied output
            // power with its native power table before the force getter.
            output_power_w: config.cars[0].governor.power_at_rpm(car.rpm),
            running: true,
        };
        let shaft = gearbox.shaft_rpm(&profile, config.cars[0].governor.max_rpm, car.rpm, speed);
        let clutch = gearbox.clutch_pct;
        let clutch_on = gearbox.current_gear_index >= 0
            && gearbox.observe_clutch(
                &profile,
                config.cars[0].governor.max_rpm,
                config.cars[0].params.starting_rpm,
                values,
            );
        let force = gearbox.force_n(
            &profile,
            config.cars[0].governor.idle_rpm,
            config.cars[0].governor.max_rpm,
            values,
        );
        near(
            car.rpm,
            num(expected, "rpm"),
            num(&tolerances, "rpm_abs"),
            &format!("{context} RPM"),
        );
        near(
            car.demanded_rpm,
            num(expected, "demanded_rpm"),
            num(&tolerances, "rpm_abs"),
            &format!("{context} demanded RPM"),
        );
        near(
            car.flow_lps,
            num(expected, "flow_lps"),
            num(&tolerances, "flow_lps_abs"),
            &format!("{context} native flow L/s"),
        );
        near(
            shaft,
            num(expected, "shaft_rpm"),
            num(&tolerances, "shaft_rpm_abs"),
            &format!("{context} shaft RPM"),
        );
        near(
            clutch,
            num(expected, "clutch_pct"),
            num(&tolerances, "clutch_pct_abs"),
            &format!("{context} clutch %"),
        );
        near(
            force,
            num(expected, "tractive_force_n"),
            num(&tolerances, "force_n_abs"),
            &format!("{context} force N"),
        );
        assert_eq!(car.phase, EnginePhase::Running, "{context}: engine phase");
        assert_eq!(expected["state"], "Running");
        assert_eq!(
            gearbox.current_gear_index,
            expected["current_gear_index"].as_i64().unwrap() as i32,
            "{context}: engaged gear"
        );
        assert_eq!(
            gearbox.next_gear_index,
            expected["next_gear_index"].as_i64().unwrap() as i32,
            "{context}: requested gear"
        );
        assert_eq!(
            clutch_on,
            expected["clutch_on"].as_bool().unwrap(),
            "{context}: clutch latch"
        );
    }
}
