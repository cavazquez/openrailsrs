//! Verify that the air-brake propagation model introduces a measurable delay
//! between the front and rear cylinders of a long train.
//!
//! Setup: 3-vehicle train, 15 m apart → rear cylinder at 30 m.
//! Pipe speed: 200 m/s → rear delay = 30/200 = 0.15 s.
//! After applying the brake we check that:
//!   - after 0.05 s the front cylinder has started applying (force > 0)
//!   - after 0.05 s the rear cylinder has NOT yet applied (force == 0)
//!   - after 0.25 s (well past 0.15 s) the rear cylinder also has force > 0

use openrailsrs_sim::brake::BrakeSystem;

#[test]
fn rear_cylinder_lags_front() {
    // Three vehicles: front at 0 m, middle at 15 m, rear at 30 m.
    let vehicles: Vec<(f64, f64, bool)> = vec![
        (0.0, 50_000.0, true),
        (15.0, 40_000.0, false),
        (30.0, 40_000.0, false),
    ];
    let mut sys = BrakeSystem::from_vehicles(&vehicles, 200.0);

    // Apply full brake.
    let dt = 0.01_f64;

    // Advance for 0.05 s (5 ticks).  Front delay = 0/200 = 0 s → front already started.
    // Middle delay = 15/200 = 0.075 s → NOT yet.
    // Rear delay = 30/200 = 0.15 s → NOT yet.
    for _ in 0..5 {
        sys.step(1.0, dt);
    }
    // After 0.05 s: front cylinder should be actively applying.
    assert!(
        sys.cylinders[0].current_force_n > 0.0,
        "front cylinder should have started applying after 0.05 s, got {}",
        sys.cylinders[0].current_force_n
    );
    // Rear cylinder should not have applied yet (pipe signal hasn't arrived).
    assert_eq!(
        sys.cylinders[2].current_force_n, 0.0,
        "rear cylinder should not apply before pipe signal arrives (delay=0.15 s), got {}",
        sys.cylinders[2].current_force_n
    );

    // Advance to 0.25 s total (25 ticks) — past the 0.15 s rear delay.
    for _ in 0..20 {
        sys.step(1.0, dt);
    }
    assert!(
        sys.cylinders[2].current_force_n > 0.0,
        "rear cylinder should be applying after 0.25 s (delay was 0.15 s), got {}",
        sys.cylinders[2].current_force_n
    );
}

#[test]
fn wagon_holds_brake_during_lap_release_until_driver_zero() {
    let vehicles: Vec<(f64, f64, bool)> = vec![(0.0, 50_000.0, true), (15.0, 40_000.0, false)];
    let mut sys = BrakeSystem::from_vehicles_with_options(&vehicles, 200.0, true);
    let dt = 0.1;

    // Service apply at 30% (≈9 PSI / 35 PSI cylinder scale).
    sys.precharge(0.30);
    assert!(sys.cylinders[1].current_force_n > 10_000.0);

    // Lap release: driver command drops but wagons stay latched.
    for _ in 0..20 {
        sys.step(0.05, dt);
    }
    assert!(
        sys.cylinders[1].current_force_n > 10_000.0,
        "wagon should hold latched brake during lap release, got {}",
        sys.cylinders[1].current_force_n
    );
    assert!(
        sys.cylinders[0].current_force_n < sys.cylinders[1].current_force_n,
        "EP loco should follow reduced command faster than wagons"
    );

    // Full release at command 0.
    for _ in 0..100 {
        sys.step(0.0, dt);
    }
    assert!(
        sys.cylinders[1].current_force_n < 1000.0,
        "wagon should release after driver hits 0, got {}",
        sys.cylinders[1].current_force_n
    );
}

#[test]
fn full_release_clears_all_cylinders() {
    let vehicles: Vec<(f64, f64, bool)> = vec![(0.0, 50_000.0, true), (30.0, 40_000.0, false)];
    let mut sys = BrakeSystem::from_vehicles(&vehicles, 200.0);

    // Fully apply.
    for _ in 0..100 {
        sys.step(1.0, 0.01);
    }
    assert!(
        sys.cylinders[1].current_force_n > 0.0,
        "rear should be applied after 1 s"
    );

    // Fully release (EP ~2.5 s, wagons ~8 s).
    for _ in 0..900 {
        sys.step(0.0, 0.01);
    }
    assert_eq!(
        sys.cylinders[0].current_force_n, 0.0,
        "front should be released"
    );
    assert_eq!(
        sys.cylinders[1].current_force_n, 0.0,
        "rear should be released"
    );
}

#[test]
fn ep_rear_locomotive_applies_without_pipe_delay() {
    // Rear power car at 140 m would wait ~0.7 s on air-only pipe; EP is instant.
    let vehicles: Vec<(f64, f64, bool)> = vec![
        (0.0, 50_000.0, true),
        (70.0, 40_000.0, false),
        (140.0, 50_000.0, true),
    ];
    let mut sys = BrakeSystem::from_vehicles(&vehicles, 200.0);
    sys.step(1.0, 0.05);
    assert!(
        sys.cylinders[2].current_force_n > 0.0,
        "rear EP loco should apply within 0.05 s, got {}",
        sys.cylinders[2].current_force_n
    );
    assert_eq!(
        sys.cylinders[1].current_force_n, 0.0,
        "middle wagon should still wait for pipe signal"
    );
}
