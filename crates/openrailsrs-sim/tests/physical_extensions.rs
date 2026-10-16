//! Own-model conservation and real driving checks. These are not OR captures.
//! Written during the implementation-only phase; no result is asserted here
//! until the joint validation phase actually executes this target.
use openrailsrs_sim::{
    LiveDriveSession,
    steam::{BoilerState, SteamCommand, steam_step},
};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn session(name: &str) -> LiveDriveSession {
    let file = root().join(format!("examples/physics_lab/scenario_{name}.toml"));
    let def = openrailsrs_scenarios::load_scenario(&file).unwrap();
    let mut s = LiveDriveSession::from_scenario(file.parent().unwrap(), &def).unwrap();
    s.driver_direction = 1.;
    s.driver_throttle = 0.7;
    s.driver_brake = 0.;
    s
}
fn drive(s: &mut LiveDriveSession, ticks: usize) {
    for _ in 0..ticks {
        s.step_realtime(0.05, |_| {});
    }
}

#[test]
fn optional_equipment_rejects_unknown_units_impossible_curves_and_unsupported_valves() {
    let text = std::fs::read_to_string(root().join("examples/physics_lab/vehicles/hydraulic.eng"))
        .unwrap();
    let parse = |text: &str| {
        let ast = openrailsrs_formats::parse_vehicle_text(text).unwrap();
        openrailsrs_formats::EngineFile::from_ast(&ast)
    };
    assert!(parse(&text).is_ok());
    assert!(
        parse(&text.replace("1 1 0.08", "1 1.2 0.08")).is_err(),
        "converter cannot create output power"
    );
    assert!(
        parse(&text.replace(
            "0.5s ) ORTSRSHydraulicLockUpRatio",
            "0.5fortnights ) ORTSRSHydraulicLockUpRatio"
        ))
        .is_err()
    );
    for invalid in [
        text.replace("25psi/s", "25wrong/s"),
        text.replace("Air_single_pipe", "Air_twin_pipe"),
    ] {
        let ast = openrailsrs_formats::parse_vehicle_text(&invalid).unwrap();
        let profile = openrailsrs_formats::parse_vehicle_brake_profile(&ast);
        assert!(
            openrailsrs_formats::typed::vehicle_runtime::validate_advanced_emergency(
                &ast, &profile
            )
            .is_err()
        );
    }
}

#[test]
fn explicit_native_steam_startup_initializes_one_consistent_mass_energy_state() {
    use openrailsrs_formats::typed::SteamStartup;
    let loco = openrailsrs_train::load_engine_from_path(
        root().join("examples/physics_lab/vehicles/thermodynamic.eng"),
    )
    .unwrap();
    let mut p = loco.steam.unwrap();
    let working = p.working_pressure_bar;
    let old = BoilerState::from_params(&p);
    assert_eq!(old.pressure_bar, working);
    for startup in [SteamStartup::OrHot, SteamStartup::OrCold] {
        p.operation.thermodynamics.as_mut().unwrap().startup = startup;
        let b = BoilerState::from_params(&p);
        let maximum_psi = (working * 14.503773773) as f32;
        let expected_psi = if startup == SteamStartup::OrHot {
            (maximum_psi - 5.).max(0.)
        } else {
            maximum_psi * 0.66
        };
        let expected = expected_psi as f64 / 14.503773773;
        assert!((b.pressure_bar - expected).abs() < 1e-10);
        assert!(b.valid_for(&p));
        let t = b.thermodynamics.as_ref().unwrap();
        assert_eq!(t.mass_residual_kg(), 0.);
        assert_eq!(t.energy_residual_j(), 0.);
        assert_ne!(
            t.initial_mass_kg,
            old.thermodynamics.as_ref().unwrap().initial_mass_kg
        );
        let mut inconsistent = b.clone();
        inconsistent.pressure_bar = working;
        assert!(!inconsistent.valid_for(&p));
    }
    p.operation.thermodynamics = None;
    assert_eq!(
        BoilerState::from_params(&p).pressure_bar,
        working,
        "legacy factory remains unchanged"
    );
}

#[test]
fn vessel_conserves_mass_energy_through_fire_load_injection_and_safety() {
    let loco = openrailsrs_train::load_engine_from_path(
        root().join("examples/physics_lab/vehicles/thermodynamic.eng"),
    )
    .unwrap();
    let p = loco.steam.unwrap();
    let mut b = BoilerState::from_params(&p);
    let initial_coal = b.coal_kg + b.fire_mass_kg;
    let initial_tender = b.tender_water_kg;
    let mut burned = 0.;
    let mut injected = 0.;
    let mut exhausted = 0.;
    b.controls.automatic_fireman = false;
    for tick in 0..1200 {
        b.controls.firing = 0.8;
        b.controls.damper = if tick < 300 { 1. } else { 0.5 };
        b.controls.injector1 = (600..900).contains(&tick);
        let throttle = if !(300..1000).contains(&tick) {
            0.
        } else {
            0.7
        };
        let speed = if tick < 300 { 0. } else { 10. };
        steam_step(&mut b, &p, throttle, speed, 0.05);
        burned += b.coal_burn_kg_s * 0.05;
        injected += b.injection_kg_s * 0.05;
        exhausted += b.steam_usage_kg_s * 0.05;
        let t = b.thermodynamics.as_ref().unwrap();
        assert!(b.valid_for(&p), "invalid vessel at {tick}: {b:?}");
        assert!(t.mass_residual_kg().abs() < 1e-6);
        assert!(t.energy_residual_j().abs() < 0.1);
        assert!(b.pressure_bar <= p.working_pressure_bar * 1.05 + 1e-6);
    }
    assert!((initial_coal - b.coal_kg - b.fire_mass_kg - burned).abs() < 1e-6);
    assert!((initial_tender - b.tender_water_kg - injected).abs() < 1e-6);
    assert!(burned > 0. && injected > 0. && exhausted > 0.);
    let mut corrupt = b.clone();
    corrupt.pressure_bar = (b.pressure_bar - 1.).max(0.);
    assert!(
        !corrupt.valid_for(&p),
        "pressure cannot be replaced without its vessel energy"
    );
}

#[test]
fn independent_boilers_compound_and_controls_survive_a_driven_save() {
    let mut s = session("double_steam");
    assert_eq!(s.state.steam_boilers.len(), 2);
    openrailsrs_sim::steam_equipment::set_local_regulator(&mut s.state, 0, Some(0.2)).unwrap();
    openrailsrs_sim::steam_equipment::set_local_regulator(&mut s.state, 1, Some(0.85)).unwrap();
    openrailsrs_sim::steam_equipment::command_vehicle(
        &mut s.state,
        1,
        SteamCommand::CompoundStartingValve,
    )
    .unwrap();
    drive(&mut s, 300);
    let a = &s.state.steam_boilers[0];
    let b = &s.state.steam_boilers[1];
    assert_ne!(a.boiler.water_kg, b.boiler.water_kg);
    assert!(s.state.odometer_m > 0.1);
    assert!(a.valid() && b.valid());
    assert!(!a.boiler.controls.compound_bypass && b.boiler.controls.compound_bypass);
    openrailsrs_sim::steam_equipment::command_vehicle(
        &mut s.state,
        1,
        SteamCommand::CompoundStartingValve,
    )
    .unwrap();
    let encoded = serde_json::to_vec(&s.snapshot()).unwrap();
    let mut resumed = session("double_steam");
    resumed
        .restore_snapshot(serde_json::from_slice(&encoded).unwrap())
        .unwrap();
    assert_eq!(resumed.state.steam_boilers, s.state.steam_boilers);
    drive(&mut s, 300);
    drive(&mut resumed, 300);
    assert_eq!(s.state.steam_boilers, resumed.state.steam_boilers);
    assert_eq!(s.state.velocity_mps, resumed.state.velocity_mps);
    assert!(
        s.state.steam_boilers[1]
            .boiler
            .thermodynamics
            .as_ref()
            .unwrap()
            .receiver_in_kg
            > 0.
    );
    let mut bad = s.snapshot();
    bad.state.steam_boilers[1].boiler.thermodynamics = None;
    assert!(s.restore_snapshot(bad).is_err());
    let mut missing = s.snapshot();
    missing.state.steam_boilers.clear();
    assert!(
        s.restore_snapshot(missing).is_err(),
        "new independent boilers cannot be downgraded to a lead-only saved state"
    );
}

#[test]
fn double_steam_force_and_consumption_are_metered_by_locomotive_identity() {
    let mut s = session("double_steam");
    for (vehicle, regulator, firing_delta, damper_delta) in
        [(0, 0.2, -0.3, -0.2), (1, 0.85, 0.3, 0.2)]
    {
        openrailsrs_sim::steam_equipment::set_local_regulator(
            &mut s.state,
            vehicle,
            Some(regulator),
        )
        .unwrap();
        openrailsrs_sim::steam_equipment::command_vehicle(
            &mut s.state,
            vehicle,
            SteamCommand::Firing(firing_delta),
        )
        .unwrap();
        openrailsrs_sim::steam_equipment::command_vehicle(
            &mut s.state,
            vehicle,
            SteamCommand::Damper(damper_delta),
        )
        .unwrap();
    }
    let initial = s
        .state
        .steam_boilers
        .iter()
        .map(|b| {
            (
                b.boiler.coal_kg + b.boiler.fire_mass_kg,
                b.boiler.thermodynamics.as_ref().unwrap().total_mass_kg + b.boiler.tender_water_kg,
            )
        })
        .collect::<Vec<_>>();
    let mut impulses = [0.; 2];
    let mut steam = [0.; 2];
    let mut coal = [0.; 2];
    for _ in 0..300 {
        drive(&mut s, 1);
        let mut requested_total = 0.;
        for b in &s.state.steam_boilers {
            let vehicle = b.vehicle;
            let force = s.state.motor_force_n[vehicle];
            assert_eq!(force, b.boiler.tractive_force_n);
            assert!(force.is_finite() && force >= 0.);
            requested_total += b.boiler.tractive_force_n;
            impulses[vehicle] += force * 0.05;
            steam[vehicle] += b.boiler.steam_usage_kg_s * 0.05;
            coal[vehicle] += b.boiler.coal_burn_kg_s * 0.05;
        }
        assert_eq!(requested_total, s.state.motor_force_n.iter().sum::<f64>());
        assert_eq!(
            s.state.motor_force_n[2], 0.,
            "Hauled wagon cannot receive either boiler's output"
        );
    }
    assert!(s.state.odometer_m > 0.1);
    assert!(impulses.iter().all(|x| *x > 0.) && (impulses[0] - impulses[1]).abs() > 1.);
    assert!(steam.iter().all(|x| *x > 0.) && (steam[0] - steam[1]).abs() > 0.001);
    assert!((coal[0] - coal[1]).abs() > 0.001);
    let heat = s
        .state
        .steam_boilers
        .iter()
        .map(|b| b.boiler.thermodynamics.as_ref().unwrap().heat_input_j)
        .collect::<Vec<_>>();
    assert!(
        heat[0] > 0. && heat[1] > heat[0],
        "Different drafts must heat their own boilers separately"
    );
    for b in &s.state.steam_boilers {
        let vehicle = b.vehicle;
        let t = b.boiler.thermodynamics.as_ref().unwrap();
        assert!(
            (initial[vehicle].0 - b.boiler.coal_kg - b.boiler.fire_mass_kg - coal[vehicle]).abs()
                < 1e-6
        );
        assert!(
            (initial[vehicle].1 - t.total_mass_kg - b.boiler.tender_water_kg - t.outlet_steam_kg)
                .abs()
                < 1e-6
        );
        assert!((steam[vehicle] + t.safety_steam_kg - t.outlet_steam_kg).abs() < 1e-6);
        assert!(b.valid());
    }
}

#[test]
fn local_regulator_obeys_the_same_operational_and_safety_interlocks() {
    let mut s = session("double_steam");
    s.driver_throttle = 0.;
    openrailsrs_sim::steam_equipment::set_local_regulator(&mut s.state, 0, Some(0.8)).unwrap();
    drive(&mut s, 10);
    assert!(
        s.state.motor_force_n[0] > 0.,
        "a crewed regulator is independent of the common zero command"
    );
    s.driver_direction = 0.5;
    drive(&mut s, 1);
    assert!(s.state.traction_inhibited && s.state.motor_force_n[0] == 0.);
    s.driver_direction = 1.;
    s.exterior.door = openrailsrs_sim::exterior::DoorState::Open;
    drive(&mut s, 1);
    assert!(s.state.traction_inhibited && s.state.motor_force_n[0] == 0.);
    s.exterior.door = openrailsrs_sim::exterior::DoorState::Closed;
    s.gameplay.departure_hold = true;
    drive(&mut s, 1);
    assert!(s.state.traction_inhibited && s.state.motor_force_n[0] == 0.);
    s.gameplay.departure_hold = false;
    drive(&mut s, 1);
    assert!(!s.state.traction_inhibited && s.state.motor_force_n[0] > 0.);
    s.state.traction_inhibited = true;
    assert_eq!(
        openrailsrs_sim::steam_equipment::effective_regulator(
            &s.state,
            &s.physics,
            0,
            0.,
            Some(1.)
        ),
        0.
    );
    s.state.traction_inhibited = false;
    s.physics.power_supply.cars[0].enabled = false;
    drive(&mut s, 1);
    assert_eq!(
        s.state.motor_force_n[0], 0.,
        "an isolated boiler may burn fuel but cannot apply traction"
    );
    assert!(s.state.steam_boilers[0].boiler.coal_burn_kg_s > 0.);
    s.physics.power_supply.cars[0].enabled = true;
    s.state.brake_system.emergency = true;
    drive(&mut s, 1);
    assert_eq!(s.state.motor_force_n[0], 0.);
}

#[test]
fn hydraulic_launch_load_fuel_heat_and_shifts_share_the_physics_tick() {
    let mut s = session("hydraulic");
    let fuel = s.state.diesel.car(0).unwrap().fuel_l;
    let mut gears = std::collections::BTreeSet::new();
    let mut launch_slip = false;
    for _ in 0..1200 {
        s.step_realtime(0.05, |_| {});
        let d = s.state.diesel.car(0).unwrap();
        let h = d.hydraulic.as_ref().unwrap();
        assert!(h.valid_for(s.physics.diesel.cars[0].params.hydraulic.as_ref().unwrap()));
        assert!(h.output_power_w <= h.input_power_w + 1e-6);
        assert!((h.input_power_w - h.output_power_w - h.loss_power_w).abs() < 1e-5);
        launch_slip |= s.state.velocity_mps < 2. && h.loss_power_w > 100.;
        gears.insert(h.gear);
    }
    assert!(launch_slip && s.state.odometer_m > 1.);
    assert!(s.state.diesel.car(0).unwrap().fuel_l < fuel);
    assert!(
        gears.len() > 1,
        "transmission did not reach a regime transition: {gears:?}"
    );
    let saved = s.snapshot();
    let mut continued = session("hydraulic");
    continued.restore_snapshot(saved).unwrap();
    drive(&mut s, 20);
    drive(&mut continued, 20);
    assert_eq!(s.state.diesel, continued.state.diesel);
}

#[test]
fn rack_positive_contact_brakes_gradient_without_an_adhesion_cap() {
    let mut s = session("rack");
    let mut engaged = false;
    let mut transmitted = false;
    for _ in 0..400 {
        s.step_realtime(0.05, |_| {});
        let r = &s.state.rack.cars[0];
        engaged |= r.phase == openrailsrs_sim::rack::RackPhase::Engaged;
        transmitted |= r.transmitted_force_n > 100.;
        assert!(s.state.rack.valid_for(&s.physics.rack));
    }
    assert!(engaged && transmitted && s.state.odometer_m > 1.);
    s.driver_throttle = 0.;
    s.driver_brake = 1.;
    drive(&mut s, 200);
    assert!(s.state.velocity_mps < 0.1);
    assert!(s.state.rack.cars[0].brake_work_j > 1.);
    let mut bad = s.snapshot();
    bad.state.rack.cars.clear();
    assert!(s.restore_snapshot(bad).is_err());
    let mut mismatch = s.physics.rack.clone();
    mismatch.sections[0].system = "Riggenbach-other".into();
    assert!(s.physics.rack.append(&mismatch, 2).is_err());
}

#[test]
fn vertical_profile_is_spatial_reproducible_and_affects_contact_and_motion() {
    let mut a = session("vertical");
    let mut b = session("vertical");
    let mut disabled = session("vertical");
    disabled.set_vertical_irregularities_enabled(false);
    let mut variable_load = false;
    let mut resistance = false;
    for _ in 0..400 {
        a.step_realtime(0.05, |_| {});
        b.step_realtime(0.05, |_| {});
        disabled.step_realtime(0.05, |_| {});
        assert_eq!(a.state.bogies, b.state.bogies);
        assert_eq!(a.state.velocity_mps, b.state.velocity_mps);
        let v = a.state.bogies[0].vertical.as_ref().unwrap();
        variable_load |= v
            .supports
            .iter()
            .flat_map(|s| &s.wheels)
            .any(|w| (w.normal_force_n - w.reference_load_n).abs() > 1.);
        resistance |= v
            .supports
            .iter()
            .flat_map(|s| &s.wheels)
            .any(|w| w.longitudinal_force_n.abs() > 1.);
        assert!(a.state.bogies[0].valid_for(&a.physics.bogies[0]));
    }
    assert!(variable_load && resistance);
    assert_ne!(a.state.velocity_mps, disabled.state.velocity_mps);
    let p = a.physics.bogies[0].vertical_suspension.as_ref().unwrap();
    let original = openrailsrs_sim::track_irregularity::sample(&p.sections, 123.45, false);
    assert_eq!(
        original,
        openrailsrs_sim::track_irregularity::sample(&p.sections, 123.45, false)
    );
    let mut other = p.sections.clone();
    other[0].seed += 1;
    assert_ne!(
        original,
        openrailsrs_sim::track_irregularity::sample(&other, 123.45, false)
    );
    let saved = a.snapshot();
    let mut resumed = session("vertical");
    resumed.restore_snapshot(saved).unwrap();
    drive(&mut a, 20);
    drive(&mut resumed, 20);
    assert_eq!(a.state.bogies, resumed.state.bogies);
    let mut bad = a.snapshot();
    bad.state.bogies[0].vertical = None;
    assert!(a.restore_snapshot(bad).is_err());
}

#[test]
fn optional_derailment_records_cause_cuts_force_and_keeps_the_failure_on_restore() {
    let mut active = session("derailment");
    let mut disabled = session("derailment");
    disabled.set_derailment_enabled(false);
    for s in [&mut active, &mut disabled] {
        s.state.velocity_mps = 35.;
        for v in &mut s.state.vehicles {
            v.velocity_mps = 35.;
        }
    }
    drive(&mut active, 80);
    drive(&mut disabled, 80);
    assert!(!active.derailment_events().is_empty());
    assert!(disabled.derailment_events().is_empty());
    assert!(active.state.brake_system.emergency);
    assert!(active.state.motor_force_n.iter().all(|f| *f == 0.));
    assert!(active.state.velocity_mps < disabled.state.velocity_mps);
    let events = active
        .derailment_events()
        .iter()
        .map(|(i, s)| (*i, (**s).clone()))
        .collect::<Vec<_>>();
    let mut resumed = session("derailment");
    resumed.restore_snapshot(active.snapshot()).unwrap();
    assert_eq!(
        events,
        resumed
            .derailment_events()
            .iter()
            .map(|(i, s)| (*i, (**s).clone()))
            .collect::<Vec<_>>()
    );
    resumed.set_derailment_enabled(false);
    assert_eq!(resumed.derailment_events().len(), events.len());
    let mut invalid = resumed.snapshot();
    invalid.state.bogies[0]
        .derailment
        .as_mut()
        .unwrap()
        .event_time_s = None;
    assert!(resumed.restore_snapshot(invalid).is_err());
}

#[test]
fn emergency_has_pipe_reservoir_and_rearm_states_separate_from_independent() {
    let mut s = session("hydraulic");
    s.driver_throttle = 0.;
    s.driver_brake = 0.;
    drive(&mut s, 200);
    assert_eq!(s.state.brake_system.circuits.len(), 1);
    assert_eq!(s.state.brake_system.circuits[0].vehicle, 0);
    let before_independent = s.state.brake_system.circuits[0].clone();
    let baseline = s.state.brake_system.cylinders[1]
        .emergency_pneumatic_status()
        .unwrap();
    s.state.brake_system.independent_command = 0.5;
    let supply = s.state.power_supply.clone();
    for _ in 0..20 {
        s.state.brake_system.step_circuits(&supply, 0.05);
    }
    let independent = s.state.brake_system.cylinders[1]
        .emergency_pneumatic_status()
        .unwrap();
    assert_eq!(
        independent.pipe_psi, baseline.pipe_psi,
        "independent command vented the train pipe"
    );
    assert!(
        s.state.brake_system.cylinders[0].pressure_bar() > 0.,
        "Independent failed: before={before_independent:?}; after={:?}; cylinder={:?}",
        s.state.brake_system.circuits[0],
        s.state.brake_system.cylinders[0].emergency_pneumatic_status()
    );
    s.state.brake_system.independent_command = 0.;
    for _ in 0..100 {
        s.state.brake_system.step(0.5, 0.05);
    }
    let service = s.state.brake_system.cylinders[1]
        .emergency_pneumatic_status()
        .unwrap();
    assert_ne!(service.valve, "Emergency");
    s.state.brake_system.emergency = true;
    let mut tripped = false;
    let mut dumping = false;
    for _ in 0..200 {
        s.state.brake_system.step(1., 0.05);
        let e = s.state.brake_system.cylinders[1]
            .emergency_pneumatic_status()
            .unwrap();
        tripped |= e.valve == "Emergency";
        dumping |= e.dump_open;
    }
    let emergency = s.state.brake_system.cylinders[1]
        .emergency_pneumatic_status()
        .unwrap();
    assert!(tripped && dumping);
    assert!(emergency.pipe_psi < service.pipe_psi);
    assert!(emergency.emergency_reservoir_psi < baseline.emergency_reservoir_psi);
    s.state.brake_system.emergency = false;
    for _ in 0..2000 {
        s.state.brake_system.step(0., 0.05);
        s.state.brake_system.step_circuits(&supply, 0.05);
    }
    let rearmed = s.state.brake_system.cylinders[1]
        .emergency_pneumatic_status()
        .unwrap();
    assert_eq!(rearmed.valve, "Release");
    assert!(
        rearmed.cylinder_psi < 1.
            && rearmed.emergency_reservoir_psi > emergency.emergency_reservoir_psi
    );
}

#[test]
fn one_shared_vertical_support_preserves_both_body_loads_and_spatial_reversal() {
    let file = root().join("examples/player_lab/scenario_bogie_mechanics.toml");
    let mut def = openrailsrs_scenarios::load_scenario(&file).unwrap();
    let vertical = root().join("examples/physics_lab/scenario_vertical.toml");
    let source = openrailsrs_scenarios::load_scenario(&vertical).unwrap();
    let mut p = source.train.bogie_dynamics[0]
        .vertical_suspension
        .clone()
        .unwrap();
    for section in &mut p.sections {
        for band in &mut section.spectrum {
            band[1] = 0.;
        }
    }
    for bogie in &mut def.train.bogie_dynamics {
        bogie.vertical_suspension = Some(p.clone());
    }
    let mut s = LiveDriveSession::from_scenario(file.parent().unwrap(), &def).unwrap();
    s.driver_direction = 1.;
    s.driver_brake = 0.;
    s.driver_throttle = 0.5;
    drive(&mut s, 40);
    let a = &s.state.bogies[0].vertical.as_ref().unwrap().supports[1];
    let b = &s.state.bogies[1].vertical.as_ref().unwrap().supports[0];
    assert_eq!(a.wheels[0].body_z_m, b.wheels[0].body_z_m);
    assert_eq!(a.wheels[0].axle_z_m, b.wheels[0].axle_z_m);
    for support in [a, b] {
        for w in &support.wheels {
            assert!((w.normal_force_n - w.reference_load_n).abs() < 0.01);
        }
    }
    let mut reversed = s.state.bogies[0].clone();
    let original = reversed.clone();
    reversed.reverse();
    reversed.reverse();
    assert_eq!(reversed, original);
}

#[test]
fn rack_rejects_an_incompatible_system_and_refuses_engagement_at_speed() {
    let mut s = session("rack");
    s.physics.rack.sections[0].system = "other".into();
    drive(&mut s, 2);
    assert_eq!(
        s.state.rack.cars[0].phase,
        openrailsrs_sim::rack::RackPhase::Fault
    );
    assert_eq!(s.state.rack.cars[0].transmitted_force_n, 0.);
    let mut s = session("rack");
    s.state.velocity_mps = 6.;
    for v in &mut s.state.vehicles {
        v.velocity_mps = 6.;
    }
    drive(&mut s, 2);
    assert_eq!(
        s.state.rack.cars[0].phase,
        openrailsrs_sim::rack::RackPhase::Fault
    );
    s.state.rack.request(0, false).unwrap();
    s.state.rack.request(0, true).unwrap();
    drive(&mut s, 2);
    assert_eq!(s.state.rack.cars[0].transmitted_force_n, 0.);
}

#[test]
fn reversing_on_the_second_edge_keeps_rack_and_roughness_in_world_chainage() {
    let base = root().join("examples/physics_lab");
    let mut def = openrailsrs_scenarios::load_scenario(base.join("scenario_rack.toml")).unwrap();
    let vertical =
        openrailsrs_scenarios::load_scenario(base.join("scenario_vertical.toml")).unwrap();
    def.train.bogie_dynamics = vertical.train.bogie_dynamics;
    let sections = &mut def.train.bogie_dynamics[0]
        .vertical_suspension
        .as_mut()
        .unwrap()
        .sections;
    sections[0].start_chainage_m = 1500.;
    sections[0].end_chainage_m = 2500.;
    let rack = &mut def.train.rack.as_mut().unwrap().sections[0];
    rack.start_chainage_m = 1500.;
    rack.end_chainage_m = 2500.;
    let route = tempfile::tempdir().unwrap();
    std::fs::write(
        route.path().join("track.toml"),
        r#"
[route]
id="two-physical-edges"
[[nodes]]
id="a"
x_m=0.0
y_m=0.0
[[nodes]]
id="b"
x_m=1500.0
y_m=0.0
[[nodes]]
id="c"
x_m=3000.0
y_m=0.0
[[edges]]
id="first"
from="a"
to="b"
length_m=1500.0
speed_limit_kmh=80.0
grade_percent=0.0
[[edges]]
id="second"
from="b"
to="c"
length_m=1500.0
speed_limit_kmh=80.0
grade_percent=-2.0
"#,
    )
    .unwrap();
    def.route.path = route.path().to_string_lossy().into_owned();
    def.route.destination = "c".into();
    def.route.start_offset_m = Some(1900.);
    let mut s = LiveDriveSession::from_scenario(&base, &def).unwrap();
    s.driver_direction = 0.;
    s.driver_throttle = 0.;
    s.driver_brake = 0.;
    s.state.velocity_mps = 1.;
    for v in &mut s.state.vehicles {
        v.velocity_mps = 1.;
    }
    assert_eq!(s.current_edge_id(), Some("second"));
    let before = s.head_chainage_m();
    drive(&mut s, 20);
    assert!(s.head_chainage_m() < before && s.head_chainage_m() > 1800.);
    assert_eq!(s.current_edge_id(), Some("second"));
    assert_eq!(s.state.rack.cars[0].section, Some(0));
    assert_eq!(
        s.state.rack.cars[0].phase,
        openrailsrs_sim::rack::RackPhase::Engaged
    );
    assert!(
        s.state.bogies[0]
            .vertical
            .as_ref()
            .unwrap()
            .supports
            .iter()
            .flat_map(|s| &s.wheels)
            .any(|w| w.track_height_m.abs() > 1e-6),
        "roughness was sampled using local/fictitious chainage"
    );
}
