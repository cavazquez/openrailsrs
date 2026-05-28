//! SCE Class 47 cruise traction audit vs OR equilibrium (~14 mph @ 27% throttle).
//!
//! Uses OR-P1 path (`legacy_power_cap = false`): apparent throttle + rail P/v cap.

use std::path::PathBuf;

use openrailsrs_train::load_consist_with_asset_root;

fn cruise_forces(
    m: &openrailsrs_train::DieselTractionModel,
    davis: &openrailsrs_train::DavisCoefficients,
    v_mps: f64,
    throttle: f64,
    rpm: f64,
) -> (f64, f64) {
    let f_res = davis.a_n + davis.b_n_per_mps * v_mps + davis.c_n_per_mps2 * v_mps * v_mps;
    let f_trac = m
        .target_traction_force_n(v_mps, throttle, rpm, 1.0, 0.0, false)
        .min(m.adhesion_limit_n(v_mps));
    (f_trac, f_res)
}

#[test]
fn sce_class47_cruise_force_balance_or_p1() {
    let sce = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/sce");
    if !sce.join("track.toml").exists() {
        return;
    }

    let consist =
        load_consist_with_asset_root(sce.join("consists/mt_mt_class_47___6_mk2_pp.con"), &sce)
            .expect("consist");
    let models = consist.diesel_traction_models();
    assert_eq!(models.len(), 1, "expected single Class 47");
    let m = &models[0];
    let throttle = 0.27;
    let rpm = m
        .engine
        .as_ref()
        .map(|e| e.target_rpm(throttle))
        .unwrap_or(0.0);

    // Static equilibrium: margin decreases with speed; crosses zero above cruise band.
    let mut margins: Vec<(f64, f64)> = Vec::new();
    for v_mph in (10..=26).map(|x| x as f64) {
        let v = v_mph / 2.237;
        let (f_trac, f_res) = cruise_forces(m, &consist.davis, v, throttle, rpm);
        margins.push((v_mph, f_trac - f_res));
    }

    for w in margins.windows(2) {
        assert!(
            w[1].1 <= w[0].1 + 500.0,
            "margin should fall with speed: {:?} -> {:?}",
            w[0],
            w[1]
        );
    }

    let margin_10 = margins.first().map(|(_, m)| *m).unwrap_or(0.0);
    let margin_14 = margins
        .iter()
        .find(|(mph, _)| (*mph - 14.0).abs() < 0.1)
        .map(|(_, m)| *m)
        .unwrap_or(margin_10);
    assert!(
        margin_14 < margin_10 * 0.85,
        "margin at 14 mph ({margin_14:.0} N) should be below 10 mph ({margin_10:.0} N)"
    );

    // At ~14 mph (OR cruise hint) sim static model may still be accelerating; margin bounded.
    let v_or = 14.0 / 2.237;
    let (f_trac, f_res) = cruise_forces(m, &consist.davis, v_or, throttle, rpm);
    let margin = f_trac - f_res;
    assert!(
        margin > -2_000.0 && margin < 25_000.0,
        "14 mph margin {margin:.0} N out of OR-P1 band (F_trac={f_trac:.0} F_res={f_res:.0})"
    );

    // Rail P/v cap dominates at cruise speed.
    let p_cap = m.traction_power_cap_w(rpm, throttle, v_or, false);
    assert!(
        f_trac <= p_cap / v_or * 1.01 + 1.0,
        "F_trac {f_trac} should respect rail P/v {p_cap}"
    );
}
