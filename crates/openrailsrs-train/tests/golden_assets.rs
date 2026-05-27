//! Golden Davis / diesel tables from repo assets (formula-derived or explicit ORTSDavis).

use std::path::{Path, PathBuf};

use openrailsrs_formats::{OrtsBearingType, OrtsFrictionFields, OrtsWagonType};
use openrailsrs_train::auto_friction::{auto_davis_coefficients, calc_davis_a_n, calc_davis_b_n_per_mps};
use openrailsrs_train::model::DavisCoefficients;
use openrailsrs_train::{load_consist_with_asset_root, load_engine_from_path, load_wagon_from_path};

fn repo_examples(sub: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(sub)
}

fn load_diesel_model(path: &Path) -> openrailsrs_train::DieselTractionModel {
    *load_engine_from_path(path)
        .expect("load .eng")
        .diesel_traction
        .expect("diesel")
}

fn assert_near(label: &str, got: f64, expected: f64, tol: f64) {
    assert!(
        (got - expected).abs() <= tol,
        "{label}: got {got:.3} expected {expected:.3} (±{tol})"
    );
}

// ── OR-P2: explicit ORTSDavis from Chiltern Pullman assets ─────────────────

#[test]
fn golden_pullman_wagon_explicit_davis() {
    let wag = repo_examples("chiltern/trains/RF_Blue_Pullman/RF_WP_PSG.wag");
    if !wag.exists() {
        return;
    }
    let w = load_wagon_from_path(&wag).expect("psg wag");
    assert_near("Pullman PSG A", w.davis.a_n, 371.0, 0.5);
    assert_near("Pullman PSG B", w.davis.b_n_per_mps, 20.0, 0.5);
    assert_near("Pullman PSG C", w.davis.c_n_per_mps2, 0.86, 0.05);
}

#[test]
fn golden_dmbsa_explicit_davis() {
    let eng = repo_examples("chiltern/trains/RF_Blue_Pullman/RF_WP_DMBSA.eng");
    if !eng.exists() {
        return;
    }
    let l = load_engine_from_path(&eng).expect("dmbsa");
    assert_near("DMBSA A", l.davis.a_n, 433.0, 0.5);
    assert_near("DMBSA B", l.davis.b_n_per_mps, 27.2, 0.5);
    assert_near("DMBSA C", l.davis.c_n_per_mps2, 3.50, 0.05);
}

#[test]
fn golden_sce_mk2_auto_friction_formula() {
    let wag = repo_examples("sce/trains/MT_DB_MKII_BlueGrey/MT_DB_MK2_TSO_SC5134.wag");
    if !wag.exists() {
        return;
    }
    let w = load_wagon_from_path(&wag).expect("mk2");
    let meta = OrtsFrictionFields {
        bearing_type: OrtsBearingType::Friction,
        wagon_type: OrtsWagonType::Passenger,
        num_axles: Some(4),
        ..Default::default()
    };
    let expected_a = calc_davis_a_n(OrtsBearingType::Friction, 34_000.0, 4);
    let expected_b = calc_davis_b_n_per_mps(
        OrtsBearingType::Friction,
        34_000.0,
        4,
        OrtsWagonType::Passenger,
    );
    assert_near("MK2 A", w.davis.a_n, expected_a, 5.0);
    assert_near("MK2 B", w.davis.b_n_per_mps, expected_b, 1.0);
    assert_eq!(w.davis.c_n_per_mps2, 0.0, "no frontal area in minimal .wag");
    let _ = auto_davis_coefficients(
        DavisCoefficients::default(),
        34_000.0,
        false,
        &meta,
    );
}

#[test]
fn golden_class47_auto_davis_from_engine_mass() {
    let eng = repo_examples("sce/trains/MT_DD_CLASS_47_47706/MT_DD_CLASS_47_47706.eng");
    if !eng.exists() {
        return;
    }
    let l = load_engine_from_path(&eng).expect("class 47");
    assert!((l.mass_kg - 118_674.0).abs() < 1.0);
    // No ORTSDavis / axle fields → auto-friction with engine defaults (1 axle fallback in calc).
    assert_near("Class47 auto A", l.davis.a_n, 885.5, 10.0);
    assert_near("Class47 auto B", l.davis.b_n_per_mps, 39.1, 2.0);
}

#[test]
fn golden_dmbsh_auto_davis_from_engine_mass() {
    let eng = repo_examples("chiltern/trains/RF_Blue_Pullman/RF_WP_DMBSH.eng");
    if !eng.exists() {
        return;
    }
    let l = load_engine_from_path(&eng).expect("dmbsh");
    assert_near("DMBSH auto A", l.davis.a_n, 556.1, 10.0);
    assert_near("DMBSH auto B", l.davis.b_n_per_mps, 22.0, 2.0);
}

#[test]
fn golden_birmingham_pullman_consist_aggregate() {
    let base = repo_examples("chiltern");
    let con = base.join("consists/birmingham_pullman.con");
    if !con.exists() {
        return;
    }
    let c = load_consist_with_asset_root(&con, &base).expect("consist");
    // DMBSA 433+27.2+3.5 + 6×371+20+0.86 + DMBSH auto ~556+22 + C on locos/wagons
    assert!(
        c.davis.a_n > 3_100.0 && c.davis.a_n < 3_350.0,
        "aggregate A={}",
        c.davis.a_n
    );
    assert!(
        c.davis.b_n_per_mps > 140.0 && c.davis.b_n_per_mps < 175.0,
        "aggregate B={}",
        c.davis.b_n_per_mps
    );
    assert!(c.davis.c_n_per_mps2 > 8.0 && c.davis.c_n_per_mps2 < 12.0);
}

#[test]
fn golden_sce_consist_aggregate_davis() {
    let base = repo_examples("sce");
    let con = base.join("consists/mt_mt_class_47___6_mk2_pp.con");
    if !con.exists() {
        return;
    }
    let c = load_consist_with_asset_root(&con, &base).expect("sce consist");
    assert!(
        c.davis.a_n > 4_000.0 && c.davis.a_n < 6_500.0,
        "SCE aggregate A={}",
        c.davis.a_n
    );
    assert!(
        c.davis.b_n_per_mps > 50.0 && c.davis.b_n_per_mps < 115.0,
        "SCE aggregate B={}",
        c.davis.b_n_per_mps
    );
}

// ── OR-P1: diesel table golden points from SCE Class 47 .eng ────────────────

#[test]
fn golden_class47_diesel_power_tab_points() {
    let eng = repo_examples("sce/trains/MT_DD_CLASS_47_47706/MT_DD_CLASS_47_47706.eng");
    if !eng.exists() {
        return;
    }
    let m = load_diesel_model(&eng);
    let e = m.engine.as_ref().expect("engine params");
    assert_near("power@325", e.power_at_rpm(325.0), 111_119.0, 1.0);
    assert_near("power@750", e.power_at_rpm(750.0), 1_924_651.0, 1.0);
    assert_near("target rpm@0", e.target_rpm(0.0), 325.0, 1.0);
    assert_near("target rpm@1", e.target_rpm(1.0), 750.0, 1.0);
}

#[test]
fn golden_class47_traction_curve_stall_notches() {
    let eng = repo_examples("sce/trains/MT_DD_CLASS_47_47706/MT_DD_CLASS_47_47706.eng");
    if !eng.exists() {
        return;
    }
    let m = load_diesel_model(&eng);
    let f10 = m.force_at(0.0, 0.10);
    let f100 = m.force_at(0.0, 1.0);
    // 10% notch: below adhesion cap → raw ORTS stall from .eng.
    assert_near("stall 10%", f10, 44_482.0, 50.0);
    // 100% notch: capped by wheel-rail adhesion (~μ×mass×g), not raw ORTS peak.
    assert_near("stall 100%", f100, 350_951.0, 200.0);
    assert!(f100 > f10 * 5.0);
}

#[test]
fn golden_class47_reverse_throttle_rpm_golden_points() {
    let eng = repo_examples("sce/trains/MT_DD_CLASS_47_47706/MT_DD_CLASS_47_47706.eng");
    if !eng.exists() {
        return;
    }
    let m = load_diesel_model(&eng);
    let e = m.engine.as_ref().expect("engine");
    assert_near("apparent@325", e.apparent_throttle_fraction(325.0), 0.0, 0.02);
    assert_near("apparent@450", e.apparent_throttle_fraction(450.0), 0.40, 0.02);
    assert_near("apparent@750", e.apparent_throttle_fraction(750.0), 1.0, 0.02);
}

#[test]
fn golden_dmbsa_traction_stall_scaled() {
    let eng = repo_examples("chiltern/trains/RF_Blue_Pullman/RF_WP_DMBSA.eng");
    if !eng.exists() {
        return;
    }
    let m = load_diesel_model(&eng);
    let stall100 = m.force_at(0.0, 1.0);
    assert!(
        stall100 > 80_000.0 && stall100 < 95_000.0,
        "DMBSA stall scaled {stall100}"
    );
    assert_near("rail power W", m.max_rail_output_power_w, 745_513.0, 1.0);
}
