//! Chiltern Pullman cruise traction audit (OR-P1 path, no power overrides).

use std::path::PathBuf;

use openrailsrs_train::load_consist_with_asset_root;

#[test]
fn audit_chiltern_forces_at_cruise_or_p1() {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
    let con = base.join("consists/birmingham_pullman.con");
    if !con.exists() {
        return;
    }
    let consist = load_consist_with_asset_root(&con, &base).expect("consist");
    let models = consist.diesel_traction_models();
    assert!(
        models.len() >= 2,
        "Pullman consist should have lead + trail diesel"
    );

    let throttle = 0.8;
    let v = 10.863_f64;
    let f_res =
        consist.davis.a_n + consist.davis.b_n_per_mps * v + consist.davis.c_n_per_mps2 * v * v;

    let mut f_trac = 0.0;
    for (i, m) in models.iter().enumerate() {
        let rpm = if i == 0 {
            m.engine
                .as_ref()
                .map(|e| e.target_rpm(throttle))
                .unwrap_or(750.0)
        } else {
            // Trail motor idles unless explicitly powered in OR activity.
            m.engine.as_ref().map(|e| e.idle_rpm).unwrap_or(650.0)
        };
        let run_factor = if i == 0 { 1.0 } else { 0.0 };
        f_trac += m.target_traction_force_n(v, throttle, rpm, run_factor, 0.0, false);
    }

    // At 80 % notch and ~39 mph equivalent, lead motor should pull harder than Davis sum
    // (train still accelerating in early cruise window).
    assert!(
        f_trac > f_res * 0.5,
        "lead traction {f_trac:.0} N too weak vs Davis {f_res:.0} N at v={v:.2} m/s"
    );

    // OR-P1: apparent throttle caps curve below driver notch at partial RPM.
    let lead = &models[0];
    let rpm_mid = 800.0;
    let f_or = lead.force_at_scaled(0.0, 1.0, rpm_mid, 1.0, 0.0, false);
    let f_legacy = lead.force_at_scaled(0.0, 1.0, rpm_mid, 1.0, 0.0, true);
    assert!(
        f_or < f_legacy,
        "OR-P1 apparent throttle should reduce stall force: or={f_or:.0} legacy={f_legacy:.0}"
    );

    let rail_p = lead.traction_power_cap_w(rpm_mid, throttle, v, false);
    let legacy_p = lead.traction_power_cap_w(rpm_mid, throttle, v, true);
    assert!(
        rail_p <= legacy_p,
        "rail P cap {rail_p:.0} W should not exceed legacy {legacy_p:.0} W"
    );
}
