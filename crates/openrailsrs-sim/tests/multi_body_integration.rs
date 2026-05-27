//! End-to-end: `physics::step()` with OR-P4 multi-body enabled on a 2-vehicle consist.

use openrailsrs_sim::path_data::{PathData, PathEdgeData};
use openrailsrs_sim::physics::{TrainPhysics, step};
use openrailsrs_sim::state::TrainSimState;
use openrailsrs_train::{TractiveCurve, load_consist_with_asset_root};

fn smoke_freight_consist() -> Option<(openrailsrs_train::Consist, std::path::PathBuf)> {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke");
    let con = base.join("consists/freight.con");
    if !con.exists() {
        return None;
    }
    let consist = load_consist_with_asset_root(&con, &base).ok()?;
    Some((consist, base))
}

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
fn physics_step_wagon_lags_locomotive_with_multi_body() {
    let (consist, _) = match smoke_freight_consist() {
        Some(x) => x,
        None => return,
    };
    assert!(
        consist.vehicles.len() >= 2,
        "smoke freight consist must have loco + wagon"
    );

    let train = TrainPhysics {
        mass_kg: consist.total_mass_kg(),
        max_power_w: consist.total_max_power_w(),
        max_tractive_effort_n: consist.total_max_tractive_effort_n(),
        max_brake_n: consist.total_max_brake_n(),
        davis: consist.davis.clone(),
        tractive: TractiveCurve::default(),
        diesel_engines: consist.diesel_traction_models(),
        regen_factor: 0.0,
        diesel_sfc_g_per_kwh: None,
        steam_params: None,
        brake_mapping: Default::default(),
        legacy_power_cap: true,
        brake_skid_limit: false,
    };

    let path_data = flat_path();
    let mut state = TrainSimState::new(vec!["e1".into()]);
    state.init_multi_body_if_enabled(&consist, true);
    assert_eq!(state.vehicles.len(), consist.vehicles.len());
    assert_eq!(state.couplers.len(), consist.vehicles.len() - 1);

    state.throttle = 1.0;
    state.brake = 0.0;

    step(&mut state, &path_data, &train, 0.01);
    assert!(
        state.vehicles[0].velocity_mps > 0.0,
        "loco should move on first tick"
    );
    assert_eq!(
        state.vehicles[1].velocity_mps, 0.0,
        "wagon should wait for coupler slack"
    );

    for _ in 0..99 {
        step(&mut state, &path_data, &train, 0.01);
    }
    assert!(
        state.vehicles[1].velocity_mps > 0.0,
        "wagon should move after ~1 s"
    );
    assert!(
        state.velocity_mps > 0.0,
        "mean train velocity should be positive"
    );
}

#[test]
fn init_multi_body_disabled_leaves_single_mass_path() {
    let (consist, _) = match smoke_freight_consist() {
        Some(x) => x,
        None => return,
    };
    let mut state = TrainSimState::new(vec!["e1".into()]);
    state.init_multi_body_if_enabled(&consist, false);
    assert!(state.vehicles.is_empty());
    assert!(state.couplers.is_empty());
}
