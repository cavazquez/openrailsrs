//! Integration tests for OR auto-friction on real SCE assets.

use openrailsrs_train::{load_consist_with_asset_root, load_wagon_from_path};

#[test]
fn sce_mk2_wagon_uses_or_auto_friction() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/sce");
    let wag = base.join("trains/MT_DB_MKII_BlueGrey/MT_DB_MK2_TSO_SC5134.wag");
    let wagon = load_wagon_from_path(&wag).expect("mk2 wag");
    assert!(
        (wagon.davis.a_n - 732.7).abs() < 10.0,
        "a={}",
        wagon.davis.a_n
    );
    assert!(
        (wagon.davis.b_n_per_mps - 11.2).abs() < 2.0,
        "b={}",
        wagon.davis.b_n_per_mps
    );
}

#[test]
fn sce_consist_aggregate_davis_without_manual_override() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/sce");
    let con = base.join("consists/mt_mt_class_47___6_mk2_pp.con");
    let consist = load_consist_with_asset_root(&con, &base).expect("sce consist");
    let d = consist.davis;
    // 1× Class 47 (auto) + 5× MK2 — total A well above legacy 502.8×6 estimate.
    assert!(d.a_n > 4000.0, "aggregate a={}", d.a_n);
    assert!(d.b_n_per_mps > 50.0, "aggregate b={}", d.b_n_per_mps);
}
