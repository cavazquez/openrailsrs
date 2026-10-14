//! Explicit depot QA against the existing compiled engine. Maintenance and
//! control receive identical driver inputs/time; only maintenance transfers
//! finite oil/coolant and repairs those circuits. No runtime replacement.
use openrailsrs_sim::{CarOperation, LiveDriveSession};
use openrailsrs_sim::engine_equipment::{MotorFault, MotorOperation};
use serde_json::{Value, json};
use std::path::Path;

fn protected_state(s: &LiveDriveSession) -> Value {
    json!({
        "formation": s.formation,
        "pneumatic_brakes": s.state.brake_system,
        "brake_equipment": s.state.vehicle_systems.iter().map(|v| &v.brakes).collect::<Vec<_>>(),
        "power": s.state.power_supply,
        "native_fuel": s.state.diesel,
        "motor_fuel": s.state.vehicle_systems.iter().flat_map(|v| &v.motors)
            .map(|m| json!({"fuel_l":m.fuel_l,"consumed_l":m.consumed_l,"refilled_l":m.refilled_l}))
            .collect::<Vec<_>>(),
        "service": s.gameplay,
        "time_s": s.state.time.0,
        "head_m": s.head_chainage_m(),
    })
}

fn prepare(s: &mut LiveDriveSession) {
    s.driver_direction = 0.5;
    s.driver_throttle = 0.;
    s.driver_brake = 0.63;
    for i in 0..s.formation.cars.len() {
        if !s.formation.cars[i].handbrake {
            s.operate_car(i, CarOperation::Handbrake).unwrap();
        }
        if s.formation.cars[i].powered && s.formation.cars[i].battery_on {
            s.operate_car(i, CarOperation::Battery).unwrap();
        }
    }
    let running: Vec<_> = s.state.vehicle_systems.iter().flat_map(|v| {
        v.motors.iter().enumerate().filter(|(_, m)| m.running)
            .map(move |(i, _)| (v.definition.vehicle, i))
    }).collect();
    for (vehicle, motor) in running {
        s.operate_motor(vehicle, motor, MotorOperation::StartStop).unwrap();
    }
    for vehicle in &mut s.state.vehicle_systems {
        vehicle.brakes.temperature_c = 77.;
        vehicle.brakes.heat_j = 2300.;
        for motor in &mut vehicle.motors {
            motor.fuel_l -= 31.;
            motor.consumed_l += 31.;
            if vehicle.definition.diesel_circuits.is_some() {
                motor.oil_l -= 25.;
                motor.coolant_l -= 35.;
                motor.oil_c = 54.;
                motor.coolant_c = 63.;
                // A latched circuit fault from before preparation is retained
                // by the control and repaired only after a sufficient transfer.
                motor.fault = Some(MotorFault::OilLevel);
                motor.isolated = true;
            }
        }
    }
    s.gameplay.accrued_penalty = 37.5;
    for _ in 0..100 {
        s.step_realtime(0.05, |_| {});
    }
    assert!(s.formation.cars.iter().all(|c| c.handbrake));
    assert!(s.formation.cars.iter().filter(|c| c.powered).all(|c| !c.battery_on));
    assert!(s.state.vehicle_systems.iter().flat_map(|v| &v.motors).all(|m| !m.running && m.rpm < 20.));
    assert!(s.state.brake_system.cylinders.iter().any(|c| c.current_force_n > 0.));
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 3, "DepotStateCompare SOURCE_ROOT OUTPUT.json");
    let path = Path::new(&args[1]).join("examples/traction_operation/scenario_depot.toml");
    let root = path.parent().unwrap();
    let scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
    let id = scenario.simulation.rail_operations.depots[0].id.clone();
    let mut work = LiveDriveSession::from_scenario(root, &scenario).unwrap();
    let initial_unsafe_error = work.begin_depot_work(&id).unwrap_err();
    prepare(&mut work);
    let unknown_error = work.begin_depot_work("unknown-depot").unwrap_err();
    let baseline = work.snapshot();
    work.validate_snapshot(&baseline).unwrap();
    let before = protected_state(&work);
    let mut control = LiveDriveSession::from_scenario(root, &scenario).unwrap();
    control.restore_snapshot(baseline).unwrap();
    assert_eq!(before, protected_state(&control));
    work.begin_depot_work(&id).unwrap();
    let duplicate_error = work.begin_depot_work(&id).unwrap_err();
    for _ in 0..300 {
        work.step_realtime(0.05, |_| {});
        control.step_realtime(0.05, |_| {});
    }
    assert!(work.state.depot.active.is_some());
    assert_eq!(protected_state(&work), protected_state(&control));
    let midway = protected_state(&work);
    let checkpoint = work.snapshot();
    let checkpoint_control = control.snapshot();
    let mut restored = LiveDriveSession::from_scenario(root, &scenario).unwrap();
    let mut restored_control = LiveDriveSession::from_scenario(root, &scenario).unwrap();
    restored.restore_snapshot(checkpoint).unwrap();
    restored_control.restore_snapshot(checkpoint_control).unwrap();
    assert_eq!(midway, protected_state(&restored));
    for _ in 0..400 {
        restored.step_realtime(0.05, |_| {});
        restored_control.step_realtime(0.05, |_| {});
    }
    assert_eq!(protected_state(&restored), protected_state(&restored_control));
    assert!(restored.state.depot.completed.contains(&id));
    assert_eq!(restored.state.depot.used_oil_l[&id], 50.);
    assert_eq!(restored.state.depot.used_coolant_l[&id], 70.);
    assert!(restored.state.vehicle_systems.iter().filter(|v| v.definition.diesel_circuits.is_some())
        .flat_map(|v| &v.motors).all(|m| m.fault.is_none() && !m.isolated));
    assert!(restored_control.state.vehicle_systems.iter().filter(|v| v.definition.diesel_circuits.is_some())
        .flat_map(|v| &v.motors).all(|m| m.fault == Some(MotorFault::OilLevel) && m.isolated));
    let after = protected_state(&restored);
    assert_eq!(before["motor_fuel"], after["motor_fuel"]);
    assert_ne!(before["brake_equipment"], after["brake_equipment"]);
    let mut finite_scenario = scenario.clone();
    finite_scenario.simulation.rail_operations.depots[0].oil_l = 1.;
    finite_scenario.simulation.rail_operations.depots[0].coolant_l = 1.;
    let mut finite = LiveDriveSession::from_scenario(root, &finite_scenario).unwrap();
    prepare(&mut finite);
    finite.begin_depot_work(&id).unwrap();
    for _ in 0..700 { finite.step_realtime(0.05, |_| {}); }
    assert_eq!(finite.state.depot.used_oil_l[&id], 1.);
    assert_eq!(finite.state.depot.used_coolant_l[&id], 1.);
    let finite_motors = finite.state.vehicle_systems.iter().filter(|v| v.definition.diesel_circuits.is_some())
        .flat_map(|v| &v.motors).cloned().collect::<Vec<_>>();
    assert!(finite_motors.iter().all(|m| m.fault.is_some() && m.isolated));
    let finite_snapshot = finite.snapshot();
    let mut finite_restore = LiveDriveSession::from_scenario(root, &finite_scenario).unwrap();
    finite_restore.restore_snapshot(finite_snapshot).unwrap();
    finite_restore.begin_depot_work(&id).unwrap();
    for _ in 0..700 { finite_restore.step_realtime(0.05, |_| {}); }
    assert_eq!(finite_restore.state.depot.used_oil_l[&id], 1.);
    assert_eq!(finite_restore.state.depot.used_coolant_l[&id], 1.);
    let invalid_snapshot_error = {
        let mut bad = finite_restore.snapshot();
        bad.state.depot.used_oil_l.insert(id.clone(), f64::NAN);
        finite_restore.restore_snapshot(bad).unwrap_err().to_string()
    };
    std::fs::write(&args[2], serde_json::to_vec_pretty(&json!({
        "status":"pass","source_scope":"Compiled source32, owned depot fixture; no Cargo or runtime changes",
        "initial_unsafe_error":initial_unsafe_error,"unknown_depot_error":unknown_error,
        "active_operation_error":duplicate_error,"invalid_resource_snapshot_error":invalid_snapshot_error,
        "nondefault_initial":before,"midway_before_restore":midway,"after_recovery":after,
        "control_after_same_time":protected_state(&restored_control),
        "protected_state_matches_control_at_midway_and_finish":true,
        "checkpoint_preserves_nondefault_state":true,"elapsed_work_s":35.,
        "pneumatic_state_evolved":before["pneumatic_brakes"] != after["pneumatic_brakes"],
        "thermal_brake_state_evolved":true,
        "used_oil_l":50.,"used_coolant_l":70.,
        "circuit_fault_repaired_only_in_work":true,
        "finite_supply":{"oil_budget_l":1.,"coolant_budget_l":1.,"used_oil_l":1.,"used_coolant_l":1.,
            "faults_remain":true,"second_cycle_after_restore_creates_no_material":true,
            "status_text":finite_restore.depot_status()},
        "scope":"Fuel, battery, pneumatic/thermal brakes and service continuity compared against an identical-time control; physical pressure/cooling evolution is retained. Finite transfer ends a work cycle but does not imply every circuit was repaired."
    })).unwrap()).unwrap();
}
