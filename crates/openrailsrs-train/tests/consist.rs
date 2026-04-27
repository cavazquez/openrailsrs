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
