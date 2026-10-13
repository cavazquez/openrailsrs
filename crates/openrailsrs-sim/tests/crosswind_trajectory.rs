use openrailsrs_scenarios::vehicle_systems::{CrosswindDef, VehicleSystemsDef};
use openrailsrs_sim::LiveDriveSession;
fn session(enabled: bool) -> LiveDriveSession {
    let dir =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/player_lab");
    let mut scenario = openrailsrs_scenarios::load_scenario(dir.join("scenario.toml")).unwrap();
    if enabled {
        scenario.train.systems = vec![VehicleSystemsDef {
            vehicle: 0,
            crosswind: Some(CrosswindDef {
                area_m2: 60.,
                lateral_coefficient: 1.2,
                center_height_m: 2.,
                roll_stiffness_nm_rad: 2e6,
                roll_damping_nm_s: 200000.,
                roll_inertia_kg_m2: 80000.,
            }),
            ..Default::default()
        }];
    }
    let mut s = LiveDriveSession::from_scenario(&dir, &scenario).unwrap();
    s.driver_direction = 1.;
    s.driver_throttle = 0.5;
    s.state.surface_conditions = Some(openrailsrs_sim::surface_conditions::SurfaceConditions::new(
        openrailsrs_sim::surface_conditions::SurfaceWeather {
            wind_mps: [0., 25.],
            ..Default::default()
        },
    ));
    s
}
#[test]
fn lateral_wind_rotates_an_equipped_body_without_spurious_longitudinal_braking_and_restores() {
    let mut wind = session(true);
    let mut reference = session(false);
    for _ in 0..600 {
        wind.step_realtime(0.05, |_| {});
        reference.step_realtime(0.05, |_| {});
    }
    assert_eq!(wind.state.velocity_mps, reference.state.velocity_mps);
    assert_eq!(wind.state.odometer_m, reference.state.odometer_m);
    let roll = &wind.state.vehicle_systems[0].crosswind;
    assert!(roll.angle_rad > 0.01 && roll.valid());
    let mut restored = session(true);
    restored.restore_snapshot(wind.snapshot()).unwrap();
    for _ in 0..100 {
        wind.step_realtime(0.05, |_| {});
        restored.step_realtime(0.05, |_| {});
    }
    assert_eq!(wind.state.vehicle_systems, restored.state.vehicle_systems);
    assert_eq!(wind.state.velocity_mps, restored.state.velocity_mps);
}
