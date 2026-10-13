use openrailsrs_scenarios::vehicle_systems::TiltDef;
fn oracle() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../../oracles/vehicle-equipment-or-1.6.1.json"
    ))
    .unwrap()
}
#[test]
fn native_active_tilt_matches_the_unchanged_original_in_both_curve_directions() {
    let data = oracle();
    let p = TiltDef {
        native_response: true,
        maximum_rad: 8_f64.to_radians(),
        time_constant_s: 1.,
        max_rate_rad_s: 0.2,
        compensation: 1.,
        minimum_speed_mps: 50. / 3.6,
    };
    for case in data["tilt"].as_array().unwrap() {
        let radius = case["radius_m"].as_f64().unwrap();
        let curvature = if radius == 0. {
            0.
        } else {
            case["direction"].as_f64().unwrap() / radius
        };
        let mut state = openrailsrs_sim::tilting::TiltState::default();
        for sample in case["samples"].as_array().unwrap() {
            state.advance(
                &p,
                curvature,
                case["cant_rad"].as_f64().unwrap(),
                case["speed_mps"].as_f64().unwrap(),
                0.05,
            );
            assert!(
                (state.angle_rad - sample.as_f64().unwrap()).abs()
                    <= data["tolerances"]["tilt_rad"].as_f64().unwrap()
            );
        }
    }
}
#[test]
fn every_authored_collector_delay_tracks_original_raise_lower_and_midstroke_reversal() {
    let dir =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/electric_supply");
    let scenario =
        openrailsrs_scenarios::load_scenario(dir.join("scenario_two_pantographs.toml")).unwrap();
    let data = oracle();
    for case in data["pantographs"].as_array().unwrap() {
        let mut s = openrailsrs_sim::LiveDriveSession::from_scenario(&dir, &scenario).unwrap();
        s.physics.electric.cars[0].params.pantographs[1].delay_s =
            case["delay_s"].as_f64().unwrap();
        for sample in case["samples"].as_array().unwrap() {
            s.state.electric.cars[0].pantographs[1].command_up =
                Some(sample["up"].as_bool().unwrap());
            openrailsrs_sim::electric::advance(
                &mut s.state,
                &s.path_data,
                &s.physics.electric,
                sample["dt_s"].as_f64().unwrap(),
            );
            assert!(
                (s.state.electric.cars[0].pantographs[1].fraction
                    - sample["fraction"].as_f64().unwrap())
                .abs()
                    <= data["tolerances"]["pantograph_fraction"].as_f64().unwrap()
            );
        }
    }
}
#[test]
fn rheostat_native_curve_notches_and_power_cap_match_original_available_force() {
    let data = oracle();
    let speeds = data["speed_curve"].as_array().unwrap();
    let force = data["force_curve"].as_array().unwrap();
    let curve = speeds
        .iter()
        .zip(force)
        .map(|(v, f)| [v.as_f64().unwrap(), f.as_f64().unwrap()])
        .collect::<Vec<_>>();
    for case in data["brake"].as_array().unwrap() {
        let speed = case["speed_mps"].as_f64().unwrap();
        let power = case["maximum_power_w"].as_f64().unwrap();
        let raw = openrailsrs_sim::brake_equipment::interpolate(&curve, speed)
            * case["handle"].as_f64().unwrap();
        let actual = if speed <= 0. {
            0.
        } else if power > 0. {
            raw.min(power / speed)
        } else {
            raw
        };
        assert!(
            (actual - case["force_n"].as_f64().unwrap()).abs()
                <= data["tolerances"]["brake_force_n"].as_f64().unwrap()
        );
    }
}
