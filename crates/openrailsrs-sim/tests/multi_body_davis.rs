//! OR-P4/P5: per-vehicle Davis resistance in multi-body `physics::step()`.

use openrailsrs_sim::path_data::{PathData, PathEdgeData};
use openrailsrs_sim::physics::{TrainPhysics, step};
use openrailsrs_sim::state::TrainSimState;
use openrailsrs_train::model::{DavisCoefficients, Vehicle, Wagon};
use openrailsrs_train::{Consist, TractiveCurve, load_consist_with_asset_root};

fn flat_path() -> PathData {
    PathData {
        edges: vec![PathEdgeData {
            length_m: 10_000.0,
            speed_limit_mps: 50.0,
            grade_percent: 0.0,
        }],
    }
}

#[test]
fn chiltern_per_vehicle_davis_sums_to_aggregate() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
    let con = base.join("consists/birmingham_pullman.con");
    if !con.exists() {
        return;
    }
    let consist = load_consist_with_asset_root(&con, &base).expect("consist");
    let per_vehicle = consist.per_vehicle_davis(None);
    assert_eq!(per_vehicle.len(), consist.vehicles.len());

    for v in [0.0, 8.0, 15.0, 25.0] {
        let sum: f64 = per_vehicle.iter().map(|d| d.resistance_n(v)).sum();
        let agg = consist.davis.resistance_n(v);
        assert!(
            (sum - agg).abs() <= agg.abs().max(1.0) * 0.01,
            "v={v}: per-vehicle sum {sum:.1} vs aggregate {agg:.1}"
        );
    }
}

#[test]
fn per_vehicle_davis_uses_each_vehicle_speed() {
    let consist = Consist {
        vehicles: vec![
            Vehicle::Wagon(Wagon {
                name: "front".into(),
                mass_kg: 50_000.0,
                max_brake_force_n: 50_000.0,
                length_m: 20.0,
                davis: DavisCoefficients {
                    a_n: 500.0,
                    b_n_per_mps: 40.0,
                    c_n_per_mps2: 1.0,
                },
                wagon_shape: None,
                brake_shoe_type: Default::default(),
                brake_shoe_friction: None,
                flipped: false,
            }),
            Vehicle::Wagon(Wagon {
                name: "rear".into(),
                mass_kg: 30_000.0,
                max_brake_force_n: 30_000.0,
                length_m: 20.0,
                davis: DavisCoefficients {
                    a_n: 300.0,
                    b_n_per_mps: 10.0,
                    c_n_per_mps2: 0.5,
                },
                wagon_shape: None,
                brake_shoe_type: Default::default(),
                brake_shoe_friction: None,
                flipped: false,
            }),
        ],
        davis: DavisCoefficients::default(),
    };
    let vehicle_davis = consist.per_vehicle_davis(None);
    let train = TrainPhysics {
        mass_kg: consist.total_mass_kg(),
        max_power_w: 0.0,
        max_tractive_effort_n: 0.0,
        max_brake_n: 80_000.0,
        davis: consist.aggregate_davis(),
        vehicle_davis,
        tractive: TractiveCurve::default(),
        diesel_engines: Vec::new(),
        regen_factor: 0.0,
        diesel_sfc_g_per_kwh: None,
        steam_params: None,
        brake_mapping: Default::default(),
        legacy_power_cap: true,
        brake_skid_limit: false,
        multi_body_scalar_coast_below_v_mps: None,
        partial_throttle_run_up_time_s: None,
        orts_inherit_partial_run_up: false,
    };

    let path = flat_path();
    let mut state = TrainSimState::new(vec!["e1".into()]);
    state.init_multi_body_if_enabled(&consist, true, openrailsrs_sim::CouplerKind::Freight);
    state.vehicles[0].velocity_mps = 10.0;
    state.vehicles[1].velocity_mps = 10.0;
    state.velocity_mps = 10.0;
    state.throttle = 0.0;
    state.brake = 0.0;

    let v0_front = state.vehicles[0].velocity_mps;
    let v0_rear = state.vehicles[1].velocity_mps;
    step(&mut state, &path, &train, 0.1);

    let dv_front = state.vehicles[0].velocity_mps - v0_front;
    let dv_rear = state.vehicles[1].velocity_mps - v0_rear;
    let expected_front = -train.vehicle_davis[0].resistance_n(10.0) / 50_000.0 * 0.1;
    let expected_rear = -train.vehicle_davis[1].resistance_n(10.0) / 30_000.0 * 0.1;
    assert!(
        (dv_front - expected_front).abs() < 0.0005,
        "front dv={dv_front} expected={expected_front}"
    );
    assert!(
        (dv_rear - expected_rear).abs() < 0.0005,
        "rear dv={dv_rear} expected={expected_rear}"
    );
    assert!(
        expected_front.abs() > expected_rear.abs() * 1.2,
        "heavier Davis on front wagon should decelerate it faster per step"
    );
}
