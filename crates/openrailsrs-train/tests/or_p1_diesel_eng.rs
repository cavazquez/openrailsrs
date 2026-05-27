//! OR-P1 unit tests using parsed `.eng` diesel tables (no OR Wine baseline).

use std::path::Path;

use openrailsrs_train::{DieselTractionModel, load_engine_from_path};

fn load_diesel(path: &Path) -> DieselTractionModel {
    *load_engine_from_path(path)
        .expect("load .eng")
        .diesel_traction
        .expect("diesel traction model")
}

#[test]
fn sce_class47_builds_reverse_throttle_tab() {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/sce/trains/MT_DD_CLASS_47_47706/MT_DD_CLASS_47_47706.eng");
    if !p.exists() {
        return;
    }
    let m = load_diesel(&p);
    let e = m.engine.as_ref().expect("DieselEngineParams");
    assert!(
        !e.reverse_throttle_rpm_tab.is_empty(),
        "ReverseThrottleRPMTab should be built from ThrottleRPMTab"
    );
    assert!(e.apparent_throttle_fraction(325.0) < 0.05);
    assert!((e.apparent_throttle_fraction(750.0) - 1.0).abs() < 0.05);
    let mid = e.apparent_throttle_fraction(450.0);
    assert!(
        mid > 0.35 && mid < 0.45,
        "450 RPM ≈ 40 % notch, got {mid}"
    );
}

#[test]
fn sce_class47_apparent_throttle_caps_curve_at_low_rpm() {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/sce/trains/MT_DD_CLASS_47_47706/MT_DD_CLASS_47_47706.eng");
    if !p.exists() {
        return;
    }
    let m = load_diesel(&p);
    let rpm = 400.0;
    let f_legacy = m.force_at_scaled(0.0, 1.0, rpm, 1.0, 0.0, true);
    let f_or = m.force_at_scaled(0.0, 1.0, rpm, 1.0, 0.0, false);
    assert!(f_legacy > 200_000.0, "legacy stall {f_legacy}");
    assert!(
        f_or < f_legacy * 0.55,
        "OR-P1 should cap curve throttle at low RPM: legacy={f_legacy} or={f_or}"
    );
    assert_eq!(
        m.effective_traction_throttle(1.0, rpm),
        m.engine.as_ref().unwrap().apparent_throttle_fraction(rpm)
    );
}

#[test]
fn sce_class47_rail_power_cap_from_max_power() {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/sce/trains/MT_DD_CLASS_47_47706/MT_DD_CLASS_47_47706.eng");
    if !p.exists() {
        return;
    }
    let m = load_diesel(&p);
    assert!((m.max_rail_output_power_w - 1_924_651.0).abs() < 1.0);
    let p27 = m.traction_power_cap_w(750.0, 0.27, 14.0, false);
    assert!(
        (p27 - 1_924_651.0 * 0.27).abs() < 50_000.0,
        "p27={p27}"
    );
}

#[test]
fn sce_class47_target_force_respects_p_over_v_cap() {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/sce/trains/MT_DD_CLASS_47_47706/MT_DD_CLASS_47_47706.eng");
    if !p.exists() {
        return;
    }
    let m = load_diesel(&p);
    let rpm = 750.0;
    let v = 44.0;
    let target = m.target_traction_force_n(v, 1.0, rpm, 1.0, 0.0, false);
    let p_cap = m.traction_power_cap_w(rpm, 1.0, v, false);
    assert!(
        target <= p_cap / v * 1.01 + 1.0,
        "target={target} cap={} v={v}",
        p_cap / v
    );
    assert!(target > 5_000.0, "target={target}");
}

#[test]
fn sce_class47_legacy_path_uses_driver_throttle_on_curve() {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/sce/trains/MT_DD_CLASS_47_47706/MT_DD_CLASS_47_47706.eng");
    if !p.exists() {
        return;
    }
    let m = load_diesel(&p);
    let rpm = 400.0;
    let f_legacy = m.force_at_scaled(0.0, 1.0, rpm, 1.0, 0.0, true);
    let f_or = m.force_at_scaled(0.0, 1.0, rpm, 1.0, 0.0, false);
    assert!(f_legacy > f_or, "legacy={f_legacy} or={f_or}");
    // At full RPM both cap paths align on max shaft/rail power.
    let p_legacy = m.traction_power_cap_w(750.0, 1.0, 10.0, true);
    let p_or = m.traction_power_cap_w(750.0, 1.0, 10.0, false);
    assert!(
        (p_legacy - p_or).abs() < 50_000.0,
        "legacy={p_legacy} or={p_or}"
    );
}

#[test]
fn chiltern_dmbsa_apparent_throttle_mid_rpm() {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/chiltern/trains/RF_Blue_Pullman/RF_WP_DMBSA.eng");
    if !p.exists() {
        return;
    }
    let m = load_diesel(&p);
    let e = m.engine.as_ref().expect("engine");
    assert!((m.max_rail_output_power_w - 745_513.0).abs() < 1.0);
    let at_800 = e.apparent_throttle_fraction(800.0);
    assert!(
        at_800 > 0.45 && at_800 < 0.55,
        "800 RPM ≈ 50 % notch on DMBSA, got {at_800}"
    );
    let f = m.force_at_scaled(0.0, 1.0, 800.0, 1.0, 0.0, false);
    let f_full_curve = m.force_at_scaled(0.0, 1.0, 800.0, 1.0, 0.0, true);
    assert!(f < f_full_curve, "f={f} full={f_full_curve}");
}

#[test]
fn chiltern_dmbsa_or_rpm_dynamics_present() {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/chiltern/trains/RF_Blue_Pullman/RF_WP_DMBSA.eng");
    if !p.exists() {
        return;
    }
    let m = load_diesel(&p);
    let e = m.engine.as_ref().expect("engine");
    assert!(e.rate_of_change_up_rpm_pss > 0.0);
    assert!(e.change_up_rpm_ps > 0.0);
    let rpm = e.advance_rpm(650.0, 1.0, 1.0);
    assert!(rpm > 650.0 && rpm <= 1500.0, "rpm={rpm}");
}
