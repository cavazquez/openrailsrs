//! Verify that coupler slack produces a measurable start-up delay for trailing
//! vehicles.
//!
//! Setup: 2 vehicles.  The locomotive (vehicle 0) receives full tractive effort.
//! The wagon (vehicle 1) is initially at rest and connected via a freight
//! coupler with 0.05 m of free play.
//!
//! Expected behaviour:
//!   - Vehicle 0 starts moving immediately.
//!   - Vehicle 1 remains stationary while the coupler slack is being taken up.
//!   - After the slack is consumed, vehicle 1 receives a force and begins moving.

use openrailsrs_sim::coupler::{CouplerState, VehicleState, multi_body_step};

fn freight_coupler() -> CouplerState {
    CouplerState::freight()
}

#[test]
fn wagon_starts_later_than_locomotive() {
    let masses = vec![80_000.0_f64, 60_000.0_f64]; // loco + wagon kg
    let f_motor = 150_000.0_f64; // 150 kN tractive force on loco
    let brake_forces = vec![0.0_f64, 0.0_f64];
    let grade_resist = vec![500.0_f64, 400.0_f64]; // small rolling resistance

    let mut vehicles = vec![
        VehicleState {
            velocity_mps: 0.0,
            position_m: 0.0,
        },
        VehicleState {
            velocity_mps: 0.0,
            position_m: 0.0,
        },
    ];
    let mut couplers = vec![freight_coupler()];

    let dt = 0.01_f64;

    // After a very short time (~0.01 s) the loco should be moving but the
    // wagon should still be at rest (coupler slack not yet consumed).
    multi_body_step(
        &mut vehicles,
        &mut couplers,
        f_motor,
        &brake_forces,
        &grade_resist,
        &masses,
        dt,
        1.0,
    );
    let loco_v_initial = vehicles[0].velocity_mps;
    let wagon_v_initial = vehicles[1].velocity_mps;

    assert!(
        loco_v_initial > 0.0,
        "locomotive should start moving immediately, got v={}",
        loco_v_initial
    );
    assert_eq!(
        wagon_v_initial, 0.0,
        "wagon should remain stationary while coupler slack is taken up (one tick), got v={}",
        wagon_v_initial
    );

    // Run for 1 second — the coupler slack (0.05 m) should be consumed.
    for _ in 0..100 {
        multi_body_step(
            &mut vehicles,
            &mut couplers,
            f_motor,
            &brake_forces,
            &grade_resist,
            &masses,
            dt,
            1.0,
        );
    }

    assert!(
        vehicles[1].velocity_mps > 0.0,
        "wagon should be moving after 1 s, got v={}",
        vehicles[1].velocity_mps
    );
    // Wagon is slower than loco (still accelerating after delayed start).
    assert!(
        vehicles[0].velocity_mps >= vehicles[1].velocity_mps,
        "loco (v={}) should not be slower than wagon (v={})",
        vehicles[0].velocity_mps,
        vehicles[1].velocity_mps
    );
}

#[test]
fn single_vehicle_no_couplers() {
    let masses = vec![100_000.0_f64];
    let f_motor = 100_000.0_f64;
    let brake_forces = vec![0.0_f64];
    let grade_resist = vec![0.0_f64];

    let mut vehicles = vec![VehicleState {
        velocity_mps: 0.0,
        position_m: 0.0,
    }];
    let mut couplers: Vec<CouplerState> = vec![];

    let dt = 1.0;
    let mean_v = multi_body_step(
        &mut vehicles,
        &mut couplers,
        f_motor,
        &brake_forces,
        &grade_resist,
        &masses,
        dt,
        1.0,
    );

    assert!(
        mean_v > 0.0,
        "single vehicle should accelerate, got mean_v={}",
        mean_v
    );
    assert_eq!(mean_v, vehicles[0].velocity_mps);
}
