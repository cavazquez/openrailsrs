//! New CPU regressions. Prepared during the native-port implementation;
//! no result is claimed until Root compiles/runs this target.
use openrailsrs_formats::{
    NativeSteamBoilerType, NativeSteamBrakeKind, parse_native_steam161, parse_vehicle_text,
};
use openrailsrs_sim::native_steam::{
    NativeSteamHardware, NativeSteamInput, NativeSteamState, NativeSteamVacuumCar,
};

fn profile() -> openrailsrs_formats::NativeSteamParams {
    let text = include_str!("../../../examples/physics_lab/vehicles/native_steam.eng");
    parse_native_steam161(&parse_vehicle_text(text).unwrap()).unwrap()
}
fn vacuum_car(p: &openrailsrs_formats::NativeSteamParams) -> NativeSteamVacuumCar {
    NativeSteamVacuumCar {
        pipe_psi: 0.,
        cylinder_psi: 0.,
        reservoir_psi: 0.,
        pipe_volume_m3: 0.02,
        cylinder_volume_m3: 0.04,
        reservoir_volume_m3: 0.12,
        max_force_psi: p.maximum_vacuum_psi,
        application_psi_s: 4.9,
        release_psi_s: 4.9,
        direct_admission: false,
        isolated: false,
        vented: false,
        bleed: false,
        cylinder_fraction: 0.,
    }
}
#[test]
fn native_parser_accepts_both_normalized_and_named_engine_roots() {
    let text = include_str!("../../../examples/physics_lab/vehicles/native_steam.eng");
    let normalized = parse_native_steam161(&parse_vehicle_text(text).unwrap()).unwrap();
    let named =
        parse_native_steam161(&openrailsrs_formats::parse_named_stf(text).unwrap()).unwrap();
    assert_eq!(normalized, named);
    let mut invalid = normalized.clone();
    invalid.boiler_efficiency = vec![(0., 0.), (100., 0.)];
    assert!(!invalid.valid());
    assert!(NativeSteamState::new(&invalid, true).is_err());
    let mut invalid = normalized;
    invalid.train_brake_notches.clear();
    assert!(!invalid.valid());
    assert!(NativeSteamState::new(&invalid, true).is_err());
}
#[test]
fn native_empty_vacuum_network_is_rejected_before_state_mutation() {
    let p = profile();
    let mut n = NativeSteamState::new(&p, true).unwrap();
    let before = n.clone();
    assert!(
        n.advance_vacuum_network(&mut [], 0., false, 0., 0.05)
            .is_err()
    );
    assert_eq!(n, before);
}
#[test]
fn native_vacuum_nonfinite_height_and_handle_are_rejected_before_any_mutation() {
    let p = profile();
    let mut n = NativeSteamState::new(&p, true).unwrap();
    let mut cars = [vacuum_car(&p)];
    n.initialize_vacuum_network(&mut cars);
    let before = n.clone();
    let before_cars = cars;
    for (handle, height) in [(0., f32::NAN), (0., f32::INFINITY), (f32::NAN, 0.)] {
        assert!(
            n.advance_vacuum_network(&mut cars, handle, false, height, 0.05)
                .is_err()
        );
        assert_eq!(n, before);
        assert_eq!(cars, before_cars);
    }
}
#[test]
fn native_vacuum_uses_previous_kind_but_current_ejector_control_and_saves_the_phase() {
    let p = profile();
    let mut n = NativeSteamState::new(&p, true).unwrap();
    let mut cars = [vacuum_car(&p)];
    n.initialize_vacuum_network(&mut cars);
    let initial_pipe = cars[0].pipe_psi;
    assert_eq!(
        n.brake_kind(p.initial_train_brake, false),
        NativeSteamBrakeKind::Apply
    );
    assert_eq!(n.previous_train_brake_kind, NativeSteamBrakeKind::Release);

    // The authored initial Apply handle has not yet been published by the
    // wrapper: first propagation is Release with initially zero ejector flow.
    n.advance_vacuum_network(&mut cars, 1., false, 0., 0.05)
        .unwrap();
    assert_eq!(cars[0].pipe_psi, initial_pipe);
    assert_eq!(n.previous_train_brake_kind, NativeSteamBrakeKind::Apply);
    assert!(!n.feedback.brake_release);
    let mut i = input(0);
    i.brake_release = n.feedback.brake_release;
    i.brake_pipe_absolute_psi = n.feedback.brake_pipe_absolute_psi;
    n.advance(i, 0.05);
    assert!(!n.LargeSteamEjectorIsOn);

    // New Release controls the ejector now, while the network still executes
    // the previously published Apply. Both are source phases, not prescribed
    // native pressures or observed flow samples.
    n.advance_vacuum_network(&mut cars, 0., false, 0., 0.05)
        .unwrap();
    assert!(cars[0].pipe_psi > initial_pipe);
    assert!(n.feedback.brake_release);
    assert_eq!(n.previous_train_brake_kind, NativeSteamBrakeKind::Release);
    i.brake_release = n.feedback.brake_release;
    i.brake_pipe_absolute_psi = n.feedback.brake_pipe_absolute_psi;
    n.advance(i, 0.05);
    assert!(n.LargeSteamEjectorIsOn);
    let before_release = cars[0].pipe_psi;
    let mut resumed: NativeSteamState =
        serde_json::from_str(&serde_json::to_string(&n).unwrap()).unwrap();
    let mut resumed_cars = cars;
    assert!(n.valid_for(&p) && resumed.valid_for(&p));
    n.advance_vacuum_network(&mut cars, 0., false, 0., 0.05)
        .unwrap();
    resumed
        .advance_vacuum_network(&mut resumed_cars, 0., false, 0., 0.05)
        .unwrap();
    assert!(cars[0].pipe_psi < before_release);
    assert_eq!(cars, resumed_cars);
    assert_eq!(n, resumed);

    // A new-format snapshot must retain this delayed input. Neither deleting
    // it nor substituting a notch unavailable in this hardware is accepted.
    let mut json = serde_json::to_value(&n).unwrap();
    json.as_object_mut()
        .unwrap()
        .remove("previous_train_brake_kind");
    assert!(serde_json::from_value::<NativeSteamState>(json).is_err());
    n.previous_train_brake_kind = NativeSteamBrakeKind::Lap;
    assert!(!n.valid_for(&p));
}
fn input(tick: usize) -> NativeSteamInput {
    let (throttle, cutoff, damper, firing, inject, speed) = match tick {
        0..=199 => (0., 0.65, 0.2, 0.2, false, 0.),
        200..=799 => (0.65, 0.65, 0.9, 0.8, false, 8.),
        800..=1099 => (0.65, 0.45, 0.9, 0.8, true, 16.),
        1100..=1399 => (0., 0.45, 0.3, 0.3, false, 12.),
        _ => (0., 0.45, 0.3, 0.3, false, 4.),
    };
    NativeSteamInput {
        regulator: throttle,
        cutoff,
        damper,
        firing,
        blower: 0.2,
        speed_mps: speed,
        signed_speed_mps: speed,
        injectors_on: [inject, false],
        injectors: [1., 0.],
        outside_c: 20.,
        has_tender_coupled: true,
        brake_release: (200..1400).contains(&tick),
        brake_pipe_absolute_psi: if (200..1400).contains(&tick) {
            4.2
        } else {
            14.503_774
        },
        ..Default::default()
    }
}
#[test]
fn native_initialize_retains_legacy_superheater_table_selection_and_filters() {
    let p = profile();
    let h = NativeSteamHardware::new(&p).unwrap();
    assert_eq!(p.boiler_type, NativeSteamBoilerType::LegacyInferred);
    assert!(h.HasSuperheater);
    assert!(!h.EfficiencyTableSuperheated && !h.InitialDropTableSuperheated);
    assert_eq!(h.CylinderClearancePC, 0.09);
    assert_eq!(h.MaxSuperheatRefTempF, 250.);
    let n = NativeSteamState::new(&p, true).unwrap();
    assert_eq!(n.BoilerPressurePSI, 155.);
    assert!(n.BoilerHeatSmooth.value.is_none());
    assert!(n.BurnRateCoalSmooth.value.is_none());
    assert_eq!(n.CylinderSteamUsageLBpS, 1.);
    assert_eq!(n.engine.CylinderSteamUsageLBpS, 0.);
    assert_eq!(
        NativeSteamState::new(&p, false).unwrap().BoilerPressurePSI,
        160. * 0.66
    );
}
#[test]
fn original05_initialized_hardware_is_derived_from_authored_input() {
    // This uses only initial/static native fields; no post-start trajectory
    // row supplies speed, heat, pressure, mass, force or flow to this model.
    let original:serde_json::Value=serde_json::from_str(include_str!(
        "../../../docs/fixtures/compatibility/twenty-20261015/physical-native05/native-steam/steam-drive.json")).unwrap();
    let p = profile();
    let n = NativeSteamState::new(&p, true).unwrap();
    let h = &n.hardware;
    let data = &original["parameters"];
    for (name, value, limit) in [
        ("boiler_volume_ft3", h.BoilerVolumeFT3, 0.0001),
        ("max_fire_mass_kg", h.MaxFireMassKG, 0.001),
        ("max_firing_rate_kg_s", h.MaxFiringRateKGpS, 0.00001),
        ("max_fuel_burn_grate_kg_s", h.MaxFuelBurnGrateKGpS, 0.00001),
        (
            "max_steam_output_lb_s",
            h.TheoreticalMaxSteamOutputLBpS,
            0.0001,
        ),
        ("evaporation_area_m2", h.EvaporationAreaM2, 0.0001),
    ] {
        assert!(
            (data[name].as_f64().unwrap() - value as f64).abs() <= limit,
            "{name}: {value} vs {}",
            data[name]
        );
    }
}
#[test]
fn manual_injection_and_tender_accounting_keep_the_native_tick_lag() {
    let p = profile();
    let mut n = NativeSteamState::new(&p, true).unwrap();
    let mut i = input(800);
    i.injectors_on = [false, false];
    n.advance(i, 0.05);
    let before = n.tender_water_ukg();
    i.injectors_on = [true, false];
    n.advance(i, 0.05);
    assert_eq!(
        n.BoilerWaterInputLB, 0.,
        "fraction copied by UpdateFiring at end of first enabled tick"
    );
    assert_eq!(n.tender_water_ukg(), before);
    n.advance(i, 0.05);
    assert!(n.BoilerWaterInputLB > 0.);
    assert_eq!(
        n.WaterConsumptionLbpS, 0.,
        "tender consumes previous tick water input"
    );
    n.advance(i, 0.05);
    assert!(n.WaterConsumptionLbpS > 0.);
    assert!(n.tender_water_ukg() < before);
}
#[test]
fn all_native_filters_and_delayed_inputs_survive_save_resume_exactly() {
    let p = profile();
    let mut a = NativeSteamState::new(&p, true).unwrap();
    for tick in 0..901 {
        a.advance(input(tick), 0.05);
    }
    let json = serde_json::to_string(&a).unwrap();
    let mut b: NativeSteamState = serde_json::from_str(&json).unwrap();
    assert!(a.valid_for(&p) && b.valid_for(&p));
    for tick in 901..1600 {
        assert_eq!(a.advance(input(tick), 0.05), b.advance(input(tick), 0.05));
        assert_eq!(a, b);
    }
    let mut bad = b.clone();
    bad.hardware.InitialDropTableSuperheated = true;
    assert!(!bad.valid_for(&p));
    let mut bad = b;
    bad.HotStart = false;
    assert!(!bad.valid_for(&p));
}

#[test]
fn native_usage_counter_integrates_the_published_usage_phase() {
    let p = profile();
    let mut n = NativeSteamState::new(&p, true).unwrap();
    for tick in 0..250 {
        let before = n.CumulativeTotalSteamConsumptionLbs;
        n.advance(input(tick), 0.05);
        assert_eq!(
            n.CumulativeTotalSteamConsumptionLbs,
            before + n.PreviousTotalSteamUsageLBpS * 0.05
        );
    }
}
