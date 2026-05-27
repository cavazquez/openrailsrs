//! Inspect diesel RPM / traction at cruise for Chiltern Birmingham calibration.

use std::path::PathBuf;

use openrailsrs_scenarios::load_scenario;
use openrailsrs_sim::{ScriptedDriver, run_scenario_headless_with_driver};
use openrailsrs_train::load_consist_with_asset_root;

#[test]
fn chiltern_cruise_state_at_61s() {
    let chiltern = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
    if !chiltern.join("track.toml").exists() {
        return;
    }

    let mut scenario = load_scenario(chiltern.join("scenario.toml")).expect("scenario");
    scenario.simulation.duration = 61.0;

    let mut driver = ScriptedDriver::from_csv(chiltern.join("driver_or.csv")).expect("driver");
    let result = run_scenario_headless_with_driver(&chiltern, &scenario, &mut driver).expect("sim");

    let state = &result.final_state;
    let consist =
        load_consist_with_asset_root(chiltern.join("consists/birmingham_pullman.con"), &chiltern)
            .expect("consist");
    let models = consist.diesel_traction_models();
    let v = state.velocity_mps;
    let t = state.throttle;

    eprintln!(
        "t={:.1} v={:.3} throttle={:.2} rpm={:?} heat={:?}",
        state.time.0, v, t, state.diesel_rpm, state.diesel_motor_heat
    );

    let mut f_sum = 0.0;
    let mut p_sum = 0.0;
    for (i, m) in models.iter().enumerate() {
        let rpm = state.diesel_rpm.get(i).copied().unwrap_or(0.0);
        let heat = state.diesel_motor_heat.get(i).copied().unwrap_or(0.0);
        let run_up = state.diesel_run_up.get(i).copied().unwrap_or(0.0);
        let run_factor = if m.legacy_run_up_time_s().is_some() {
            run_up
        } else {
            1.0
        };
        let pr = openrailsrs_train::DieselTractionModel::power_reduction_from_heat(heat);
        let mut f = m.force_at_scaled(v, t, rpm, run_factor, pr, true);
        let p = m.traction_power_cap_w(rpm, t, v, true) * run_factor * (1.0 - pr.clamp(0.0, 0.95));
        if v > 0.5 && p > 0.0 {
            f = f.min(p / v);
        }
        eprintln!(
            "eng[{i}] rpm={rpm:.0} heat={heat:.3} pr={pr:.3} run_up={run_up:.3} F={f:.0} P={p:.0} P/v={:.0}",
            if v > 0.5 { p / v } else { 0.0 }
        );
        f_sum += f;
        p_sum += p;
    }
    let f_res =
        consist.davis.a_n + consist.davis.b_n_per_mps * v + consist.davis.c_n_per_mps2 * v * v;
    eprintln!(
        "F_trac_sum={f_sum:.0} P_sum={p_sum:.0} global_P/v={:.0} F_res={f_res:.0}",
        if v > 0.5 { p_sum / v } else { 0.0 }
    );
}
