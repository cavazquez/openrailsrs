//! OR-P2 golden tests for Open Rails auto-friction (no OR baseline required).

use openrailsrs_formats::{OrtsBearingType, OrtsFrictionFields, OrtsWagonType};
use openrailsrs_train::auto_friction::{
    auto_davis_coefficients, calc_davis_a_n, calc_davis_b_n_per_mps, calc_davis_c_n_per_mps2,
    default_drag_constant,
};
use openrailsrs_train::davis_est::resolve_davis_coefficients;
use openrailsrs_train::model::DavisCoefficients;

#[test]
fn heavy_freight_friction_bearing_a() {
    // 100 t, 8 axles → tons/axle > 5 (OR heavy formula).
    let a = calc_davis_a_n(OrtsBearingType::Friction, 100_000.0, 8);
    assert!((a - 1669.4).abs() < 5.0, "a={a}");
}

#[test]
fn light_vehicle_uses_sqrt_tons_formula() {
    let a = calc_davis_a_n(OrtsBearingType::Grease, 8_000.0, 2);
    assert!((a - 1206.2).abs() < 5.0, "a={a}");
}

#[test]
fn roller_bearing_lower_a_than_friction_same_mass() {
    let friction = calc_davis_a_n(OrtsBearingType::Friction, 34_000.0, 4);
    let roller = calc_davis_a_n(OrtsBearingType::Roller, 34_000.0, 4);
    assert!((roller - 570.3).abs() < 5.0, "roller a={roller}");
    assert!(roller < friction, "roller {roller} should beat friction {friction}");
}

#[test]
fn low_bearing_freight_b_between_roller_and_default() {
    let mass = 50_000.0;
    let axles = 4;
    let roller = calc_davis_b_n_per_mps(
        OrtsBearingType::Roller,
        mass,
        axles,
        OrtsWagonType::Freight,
    );
    let low = calc_davis_b_n_per_mps(OrtsBearingType::Low, mass, axles, OrtsWagonType::Freight);
    let default = calc_davis_b_n_per_mps(
        OrtsBearingType::Default,
        mass,
        axles,
        OrtsWagonType::Freight,
    );
    assert!(low < roller, "low b={low} roller b={roller}");
    assert!(low < default, "low b={low} default b={default}");
}

#[test]
fn passenger_vs_freight_b_at_heavy_tons_per_axle() {
    let mass = 80_000.0;
    let axles = 4;
    let pass = calc_davis_b_n_per_mps(
        OrtsBearingType::Friction,
        mass,
        axles,
        OrtsWagonType::Passenger,
    );
    let freight = calc_davis_b_n_per_mps(
        OrtsBearingType::Friction,
        mass,
        axles,
        OrtsWagonType::Freight,
    );
    assert!(pass < freight, "passenger b={pass} freight b={freight}");
}

#[test]
fn loco_total_axles_includes_drive_axles() {
    let meta = OrtsFrictionFields {
        wagon_type: OrtsWagonType::Engine,
        num_axles: Some(2),
        num_drive_axles: Some(3),
        ..Default::default()
    };
    assert_eq!(meta.total_axles(true), 6);
}

#[test]
fn c_from_frontal_area_and_drag_constant() {
    // 8 m² × 0.00034 (passenger default) → non-zero C.
    let c = calc_davis_c_n_per_mps2(8.0, default_drag_constant(OrtsWagonType::Passenger));
    assert!(c > 0.5 && c < 2.0, "c={c}");
}

#[test]
fn c_zero_without_frontal_area() {
    assert_eq!(calc_davis_c_n_per_mps2(0.0, 0.00034), 0.0);
}

#[test]
fn default_drag_constants_ordered_by_wagon_type() {
    let eng = default_drag_constant(OrtsWagonType::Engine);
    let pass = default_drag_constant(OrtsWagonType::Passenger);
    let freight = default_drag_constant(OrtsWagonType::Freight);
    assert!(eng > freight && freight > pass);
}

#[test]
fn partial_davis_fills_missing_components() {
    let parsed = DavisCoefficients {
        a_n: 500.0,
        b_n_per_mps: 0.0,
        c_n_per_mps2: 0.0,
    };
    let meta = OrtsFrictionFields {
        bearing_type: OrtsBearingType::Roller,
        wagon_type: OrtsWagonType::Passenger,
        num_axles: Some(4),
        frontal_area_m2: Some(10.0),
        drag_constant: Some(0.00034),
        ..Default::default()
    };
    let out = auto_davis_coefficients(parsed, 34_000.0, false, &meta);
    assert!((out.a_n - 500.0).abs() < 1e-6);
    assert!(out.b_n_per_mps > 0.0);
    assert!(out.c_n_per_mps2 > 0.0);
}

#[test]
fn resolve_davis_keeps_full_explicit_triple() {
    let parsed = DavisCoefficients {
        a_n: 371.0,
        b_n_per_mps: 20.0,
        c_n_per_mps2: 0.86,
    };
    let meta = OrtsFrictionFields::default();
    let out = resolve_davis_coefficients(parsed.clone(), 34_000.0, false, &meta);
    assert_eq!(out, parsed);
}

#[test]
fn chiltern_dmbsh_auto_a_when_assets_present() {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/chiltern/trains/RF_Blue_Pullman/RF_WP_DMBSH.eng");
    if !p.exists() {
        return;
    }
    let loco = openrailsrs_train::load_engine_from_path(&p).expect("dmbsh");
    // DMBSH has no ORTSDavis — auto-friction from mass + engine type.
    assert!(
        loco.davis.a_n > 400.0 && loco.davis.a_n < 700.0,
        "auto A={}",
        loco.davis.a_n
    );
    assert!(loco.davis.b_n_per_mps > 5.0);
}
