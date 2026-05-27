//! SCE Class 47 cruise traction audit vs OR equilibrium (~14 mph @ 27% throttle).

use std::path::PathBuf;

use openrailsrs_train::load_consist_with_asset_root;

#[test]
fn sce_class47_cruise_force_balance() {
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
    let target_rpm = m
        .engine
        .as_ref()
        .map(|e| e.target_rpm(throttle))
        .unwrap_or(0.0);

    let mass = consist.total_mass_kg();
    let davis = consist.davis.clone();

    eprintln!("\n=== SCE Class 47 cruise audit ===");
    eprintln!(
        "mass={mass:.0} kg effort_scale={:.3} curtius=({:.2},{:.1},{:.3}) adhesion_mass={:.0}",
        m.effort_scale, m.curtius_a, m.curtius_b, m.curtius_c, m.adhesion_mass_kg
    );
    eprintln!("target_rpm@27%={target_rpm:.1}");

    for v_mph in [12.0, 13.0, 14.0, 15.0, 16.0] {
        let v = v_mph / 2.237;
        let f_curve = m.force_at_scaled(v, throttle, target_rpm, 1.0, 0.0, true);
        let p = m.traction_power_cap_w(target_rpm, throttle, v, true);
        let f_pv = if v > 0.5 { p / v } else { 0.0 };
        let f_trac = f_curve.min(f_pv).min(m.adhesion_limit_n(v));
        let f_res = davis.a_n + davis.b_n_per_mps * v + davis.c_n_per_mps2 * v * v;
        let margin = f_trac - f_res;
        eprintln!(
            "v={v_mph:4.1} mph F={f_trac:6.0}N R={f_res:6.0}N margin={margin:+6.0}N (curve={f_curve:.0} P/v={f_pv:.0})"
        );
    }

    // OR cruise ~14 mph implies near-zero force margin at that speed.
    let v_or = 14.0 / 2.237;
    let f_res_or = davis.a_n + davis.b_n_per_mps * v_or + davis.c_n_per_mps2 * v_or * v_or;
    let f_sim = m
        .force_at_scaled(v_or, throttle, target_rpm, 1.0, 0.0, true)
        .min(m.traction_power_cap_w(target_rpm, throttle, v_or, true) / v_or);

    eprintln!(
        "OR equilibrium hint: need F≈{f_res_or:.0}N at 14 mph; sim delivers {f_sim:.0}N (+{:.0}N)",
        f_sim - f_res_or
    );
}
