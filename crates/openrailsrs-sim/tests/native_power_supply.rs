use openrailsrs_core::power_supply::{
    PowerSource, PowerSupplyInput, PowerSupplyParams, PowerSupplyState,
};

#[test]
fn native_default_power_states_match_unmodified_or161_dlls() {
    let oracle: serde_json::Value =
        serde_json::from_str(include_str!("../../../oracles/openrails-brake-power.json")).unwrap();
    let params = PowerSupplyParams {
        main_delay_s: 2.,
        auxiliary_delay_s: 1.,
        ..Default::default()
    };
    for source in [
        PowerSource::Electric,
        PowerSource::Diesel,
        PowerSource::Steam,
    ] {
        let mut state = PowerSupplyState::default();
        for point in oracle["power"].as_array().unwrap() {
            let kind: PowerSource = serde_json::from_value(point["kind"].clone()).unwrap();
            if kind != source {
                continue;
            }
            let b = |key: &str| point[key].as_bool().unwrap();
            state.update(
                PowerSupplyInput {
                    source,
                    source_available: b("source"),
                    contact_closed: b("contact"),
                    battery_on: b("battery"),
                    master_key_on: b("master"),
                    train_supply_switch_on: false,
                },
                &params,
                point["time_s"].as_f64().unwrap(),
            );
            for (key, actual) in [
                ("main", state.main),
                ("auxiliary", state.auxiliary),
                ("low_voltage", state.low_voltage),
                ("cab", state.cab),
            ] {
                assert_eq!(actual, b(key), "{source:?} {} {key}", point["time_s"]);
            }
        }
    }
}

#[test]
fn class47_authored_train_supply_switch_preserves_auxiliaries_when_traction_is_open() {
    let ast = openrailsrs_formats::read_vehicle_ast(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/brake_supply_native/vehicles/43cbe1ce2075.eng"),
    )
    .unwrap();
    let params = openrailsrs_formats::typed::power_supply::parse_power_supply(&ast).unwrap();
    assert!(params.train_supply_fitted && params.manual_train_supply);
    let mut state = PowerSupplyState::default();
    let mut input = PowerSupplyInput {
        source: PowerSource::Diesel,
        source_available: true,
        contact_closed: false,
        battery_on: true,
        master_key_on: true,
        train_supply_switch_on: false,
    };
    state.hot_start(&params, 0.);
    state.update(input, &params, 0.);
    assert!(state.auxiliary && !state.main && !state.train_supply);
    input.train_supply_switch_on = true;
    state.update(input, &params, 1.);
    assert!(state.train_supply && !state.main);
    let mut resumed: PowerSupplyState =
        serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
    input.source_available = false;
    state.update(input, &params, 2.);
    resumed.update(input, &params, 2.);
    assert!(!resumed.train_supply && !resumed.auxiliary);
    assert_eq!(state, resumed);
}
