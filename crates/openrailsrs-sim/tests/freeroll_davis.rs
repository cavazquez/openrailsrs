//! Coast-down (free roll) sanity check with parsed Pullman Davis coefficients.

use openrailsrs_sim::path_data::{PathData, PathEdgeData};
use openrailsrs_sim::physics::{TrainPhysics, step};
use openrailsrs_sim::state::TrainSimState;
use openrailsrs_train::{TractiveCurve, load_consist_with_asset_root};

#[test]
fn chiltern_pullman_freeroll_deceleration() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
    let con = base.join("consists/birmingham_pullman.con");
    if !con.exists() {
        return;
    }
    let consist = load_consist_with_asset_root(&con, &base).expect("consist");
    let davis = consist.davis.clone();
    assert!(
        davis.a_n > 2600.0,
        "expected parsed Davis from assets, got {}",
        davis.a_n
    );

    let train = TrainPhysics {
        mass_kg: consist.total_mass_kg(),
        max_power_w: consist.total_max_power_w(),
        max_tractive_effort_n: consist.total_max_tractive_effort_n(),
        max_brake_n: consist.total_max_brake_n(),
        davis,
        tractive: TractiveCurve::default(),
        diesel_engines: consist.diesel_traction_models(),
        regen_factor: 0.0,
        diesel_sfc_g_per_kwh: None,
        steam_params: None,
        brake_mapping: Default::default(),
        legacy_power_cap: true,
        brake_skid_limit: false,
    };

    let path_data = PathData {
        edges: vec![PathEdgeData {
            length_m: 10_000.0,
            speed_limit_mps: 50.0,
            grade_percent: 0.0,
        }],
    };

    let mut state = TrainSimState::new(vec!["e1".into()]);
    state.velocity_mps = 15.0;
    state.throttle = 0.0;
    state.brake = 0.0;

    let v0 = state.velocity_mps;
    for _ in 0..30 {
        step(&mut state, &path_data, &train, 1.0);
    }
    let decel = (v0 - state.velocity_mps) / 30.0;
    // Parsed Pullman Davis at 15 m/s: F ≈ 3463 + 221*15 + 16.5*225 ≈ 10 kN on ~440 t → ~0.02 m/s²
    assert!(
        (0.005..=0.08).contains(&decel),
        "unexpected coast-down decel {decel} m/s² at v0={v0}"
    );
}
