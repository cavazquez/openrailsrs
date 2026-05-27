use std::io::Write;
use std::path::PathBuf;

use openrailsrs_train::load_consist_with_asset_root;

#[test]
fn load_consist_from_fixture_dir() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    std::fs::create_dir_all(base.join("vehicles")).unwrap();
    let mut eng = std::fs::File::create(base.join("vehicles/minimal.eng")).unwrap();
    eng.write_all(b"(Engine (Mass 80000) (MaxPower 2000000) (MaxVelocity 120))")
        .unwrap();
    let mut wag = std::fs::File::create(base.join("vehicles/minimal.wag")).unwrap();
    wag.write_all(b"(Wagon (Type \"x\") (Mass 20000))").unwrap();
    let p: PathBuf = base.join("consists/test.con");
    std::fs::create_dir_all(base.join("consists")).unwrap();
    let mut con = std::fs::File::create(&p).unwrap();
    con.write_all(b"(Train (Engine \"vehicles/minimal.eng\") (Wagon \"vehicles/minimal.wag\"))")
        .unwrap();
    let c = load_consist_with_asset_root(&p, base).expect("consist");
    assert_eq!(c.vehicles.len(), 2);
    assert!(c.total_mass_kg() > 0.0);
}

#[test]
fn load_chiltern_pullman_engine_if_present() {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/chiltern/trains/RF_Blue_Pullman/RF_WP_DMBSA.eng");
    if !p.exists() {
        return;
    }
    let loco = openrailsrs_train::load_engine_from_path(&p).expect("pullman eng");
    assert!(loco.mass_kg > 60_000.0);
    // Stub .eng uses sync_chiltern_assets power scale (×0.1 vs OR diesel table).
    assert!(loco.max_power_w > 50_000.0);
    let diesel = loco
        .diesel_traction
        .as_ref()
        .expect("Pullman stub should include ORTS notch curves");
    assert!(diesel.notch_curves.len() >= 5, "expected multiple notches");
    let f = diesel.force_at(0.0, 0.8);
    assert!(f > 50_000.0, "80% notch stall force too low: {f}");
    assert!(
        diesel.engine.is_some(),
        "DMBSA stub should include DieselPowerTab/ThrottleRPMTab"
    );
    assert!(loco.davis.a_n > 400.0, "expected ORTSDavis_A on DMBSA");
}

#[test]
fn chiltern_pullman_davis_from_assets() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
    let con = base.join("consists/birmingham_pullman.con");
    if !con.exists() {
        return;
    }
    let consist = load_consist_with_asset_root(&con, &base).expect("pullman consist");
    // DMBSA (433 explicit) + 6× wagon (371 explicit) + DMBSH auto-friction (~556 A).
    assert!(
        consist.davis.a_n > 3100.0 && consist.davis.a_n < 3350.0,
        "aggregated Davis A: {}",
        consist.davis.a_n
    );
    assert!(consist.davis.b_n_per_mps > 140.0);
    assert!(consist.davis.c_n_per_mps2 > 8.0);
}

#[test]
fn chiltern_dmbsh_legacy_diesel() {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/chiltern/trains/RF_Blue_Pullman/RF_WP_DMBSH.eng");
    if !p.exists() {
        return;
    }
    let loco = openrailsrs_train::load_engine_from_path(&p).expect("dmbsh");
    let diesel = loco.diesel_traction.as_ref().expect("legacy diesel model");
    assert!(
        diesel.engine.is_none(),
        "DMBSH OR source has legacy P/v only (no ORTS tables)"
    );
    assert_eq!(
        diesel.legacy_run_up_time_s,
        Some(30.0),
        "OR/MSTS RunUpTimeToMaxForce on trail motor"
    );
}

#[test]
fn chiltern_pullman_two_engines_aggregate() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
    let con = base.join("consists/birmingham_pullman.con");
    if !con.exists() {
        return;
    }
    let consist = load_consist_with_asset_root(&con, &base).expect("pullman consist");
    let models = consist.diesel_traction_models();
    assert_eq!(
        models.len(),
        2,
        "expected two diesel engines in Blue Pullman consist"
    );
    let f_dmbsa = models[0].force_at(0.0, 0.8);
    let f_combined: f64 = models.iter().map(|m| m.force_at(0.0, 0.8)).sum();
    assert!(
        f_combined > f_dmbsa * 1.3,
        "combined stall should exceed lead engine: {f_combined} vs {f_dmbsa}"
    );
    // DMBSH legacy run-up limits early contribution vs instant full stall.
    let rpm_dmbsh = models[1]
        .engine
        .as_ref()
        .map(|e| e.target_rpm(0.8))
        .unwrap_or(0.0);
    let f_dmbsh = models[1].force_at_scaled(0.0, 0.8, rpm_dmbsh, 0.5, 0.0, true);
    assert!(f_dmbsh < models[1].force_at(0.0, 0.8) * 0.6);
}
