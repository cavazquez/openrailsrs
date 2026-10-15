//! Drive the authored exercise through the same session used by the viewer.
//! Only driver controls are prescribed; motion and coupler loads evolve freely.
use openrailsrs_sim::{CarOperation, LiveDriveSession};

fn session() -> LiveDriveSession {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/traction_operation/scenario_couplers.toml");
    let scenario = openrailsrs_scenarios::load_scenario(&file).unwrap();
    LiveDriveSession::from_scenario(file.parent().unwrap(), &scenario).unwrap()
}

fn assert_finite_contacts(s: &LiveDriveSession) {
    for c in &s.state.couplers {
        let p = c.native_profile.as_ref().unwrap();
        assert!(c.extension_m.is_finite());
        assert!(c.extension_m >= -p.compression.stroke_m() - 1e-7);
        assert!(c.broken || c.extension_m <= p.tension.stroke_m() + 1e-7);
        assert!(c.memory.previous_force_n.is_finite());
        assert!(c.memory.absorbed_energy_j.is_finite() && c.memory.absorbed_energy_j >= 0.);
    }
    assert!(
        s.state
            .vehicles
            .iter()
            .all(|v| v.velocity_mps.is_finite() && v.position_m.is_finite())
    );
}

#[test]
fn live_start_coast_brake_reverse_and_save_preserve_authored_joints_and_history() {
    let mut s = session();
    assert_eq!(s.state.vehicles.len(), 3);
    assert_eq!(s.state.couplers.len(), 2);
    assert_ne!(
        s.state.couplers[0].max_force_n,
        s.state.couplers[1].max_force_n
    );
    s.set_direction(1.).unwrap();
    s.driver_brake = 0.;
    s.driver_throttle = 0.5;
    let mut maximum_tension: f64 = 0.;
    let mut maximum_compression: f64 = 0.;
    for _ in 0..240 {
        s.step_realtime(0.05, |_| {});
        assert_finite_contacts(&s);
        maximum_tension = maximum_tension.max(s.state.couplers[0].memory.previous_force_n);
    }
    assert!(s.velocity_mps() > 1., "loaded exercise must actually move");
    assert!(
        maximum_tension > 1000.,
        "startup must take up authored slack"
    );
    let saved = serde_json::to_string(&s.snapshot()).unwrap();
    let mut restored = session();
    restored
        .restore_snapshot(serde_json::from_str(&saved).unwrap())
        .unwrap();
    for (throttle, brake, ticks) in [(0., 0., 100), (0., 0.75, 240), (0., 1., 100)] {
        for target in [&mut s, &mut restored] {
            target.driver_throttle = throttle;
            target.driver_brake = brake;
        }
        for _ in 0..ticks {
            s.step_realtime(0.05, |_| {});
            restored.step_realtime(0.05, |_| {});
            assert_finite_contacts(&s);
            maximum_compression =
                maximum_compression.min(s.state.couplers[0].memory.previous_force_n);
            assert!((s.velocity_mps() - restored.velocity_mps()).abs() < 1e-8);
            for (a, b) in s.state.couplers.iter().zip(&restored.state.couplers) {
                assert!((a.extension_m - b.extension_m).abs() < 1e-8);
                assert!((a.memory.previous_force_n - b.memory.previous_force_n).abs() < 1e-5);
                assert!((a.memory.absorbed_energy_j - b.memory.absorbed_energy_j).abs() < 1e-5);
            }
        }
    }
    assert!(
        maximum_compression < -1000.,
        "stronger locomotive brake must load the buffers"
    );
    assert!(
        s.velocity_mps() < 0.1,
        "train must stop under sustained braking"
    );
    s.set_direction(0.).unwrap();
    s.driver_brake = 0.;
    s.driver_throttle = 0.;
    // Releasing the handle is not an instantaneous emptying of every cylinder.
    // Wait for the production pneumatic circuit, without editing its pressure.
    for _ in 0..600 {
        if s.state
            .brake_system
            .cylinder_forces_n(0.)
            .iter()
            .sum::<f64>()
            < 100.
        {
            break;
        }
        s.step_realtime(0.05, |_| {});
        assert_finite_contacts(&s);
    }
    assert!(
        s.state
            .brake_system
            .cylinder_forces_n(0.)
            .iter()
            .sum::<f64>()
            < 100.
    );
    let before_reverse = s.head_chainage_m();
    s.driver_throttle = 0.2;
    for _ in 0..50 {
        s.step_realtime(0.05, |_| {});
        assert_finite_contacts(&s);
    }
    let reverse_saved = serde_json::to_string(&s.snapshot()).unwrap();
    let mut reverse_restored = session();
    reverse_restored
        .restore_snapshot(serde_json::from_str(&reverse_saved).unwrap())
        .unwrap();
    for _ in 0..50 {
        s.step_realtime(0.05, |_| {});
        reverse_restored.step_realtime(0.05, |_| {});
        assert_finite_contacts(&s);
        assert!((s.velocity_mps() - reverse_restored.velocity_mps()).abs() < 1e-8);
        for (a, b) in s
            .state
            .couplers
            .iter()
            .zip(&reverse_restored.state.couplers)
        {
            assert!((a.extension_m - b.extension_m).abs() < 1e-8);
            assert!((a.memory.previous_force_n - b.memory.previous_force_n).abs() < 1e-5);
        }
    }
    assert!(
        s.head_chainage_m() < before_reverse - 0.5,
        "reverse must move along the route: before={before_reverse}, after={}, speed={}, throttle={}, arrived={}, phase={:?}, motor={:?}, brakes={:?}",
        s.head_chainage_m(),
        s.velocity_mps(),
        s.state.throttle,
        s.arrived,
        s.gameplay.phase,
        s.state.motor_force_n,
        s.state.brake_system.cylinder_forces_n(s.velocity_mps())
    );
    assert!(s.velocity_mps() > 0.1);
    assert_eq!(s.state.couplers[0].memory.position_orientation, Some(-1.));
    assert!(
        s.state.couplers[0].memory.previous_force_n < -1000.,
        "the front locomotive pushes the following vehicles in reverse; force must be physical compression"
    );
    println!(
        "startup_tension_n={maximum_tension}; braking_compression_n={maximum_compression}; reverse_distance_m={}",
        before_reverse - s.head_chainage_m()
    );
}

#[test]
fn split_save_and_recouple_keep_the_correct_original_hardware() {
    let mut s = session();
    s.set_direction(1.).unwrap();
    s.driver_throttle = 0.4;
    s.driver_brake = 0.;
    s.step_realtime(8., |_| {});
    s.driver_throttle = 0.;
    s.driver_brake = 1.;
    s.step_realtime(14., |_| {});
    assert!(s.velocity_mps() < 0.1);
    let internal_memory = serde_json::to_value(&s.state.couplers[1].memory).unwrap();
    assert!(
        s.state.couplers[1].memory.stored_energy_j + s.state.couplers[1].memory.absorbed_energy_j
            > 1.,
        "the actual drive must establish a nonempty energy history before parking"
    );
    let relative_position = s.state.vehicles[1].position_m - s.state.vehicles[2].position_m;
    assert!(
        relative_position.abs() > 1e-6,
        "the internal deflection must exercise uniform rebasing"
    );
    s.operate_car(1, CarOperation::Handbrake).unwrap();
    s.operate_car(2, CarOperation::Handbrake).unwrap();
    s.uncouple_after(0).unwrap();
    assert_eq!(s.state.couplers.len(), 0);
    assert_eq!(s.formation.parked_couplers.len(), 2);
    let saved = s.snapshot();
    let mut restored = session();
    restored.restore_snapshot(saved.clone()).unwrap();
    let mut forged = saved;
    forged.formation.parked_couplers[0]
        .native_profile
        .as_mut()
        .unwrap()
        .compression
        .points[1]
        .force_n *= 2.;
    assert!(session().restore_snapshot(forged).is_err());
    restored.recouple().unwrap();
    assert_eq!(restored.state.couplers.len(), 2);
    assert!(restored.formation.parked_couplers.is_empty());
    assert_eq!(
        serde_json::to_value(&restored.state.couplers[1].memory).unwrap(),
        internal_memory,
        "reconnecting the opened cut cannot erase the parked section's internal turning points or energy"
    );
    assert!(
        (restored.state.vehicles[1].position_m
            - restored.state.vehicles[2].position_m
            - relative_position)
            .abs()
            < 1e-9
    );
    for (a, b) in restored.state.couplers.iter().zip(session().state.couplers) {
        assert!(a.valid_for(&b));
    }
}
