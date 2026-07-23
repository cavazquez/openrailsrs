use openrailsrs_formats::{
    ConsistEntry, ConsistFile, EngineFile, MstsFile, RouteFile, WagonFile, kmh_to_mps, kn_to_n,
    kw_to_w, lb_to_kg, mph_to_mps, parse_from_first_paren, parse_msts_file,
};

fn read_fixture(name: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/{name}")).expect("fixture")
}

fn fixture_path(name: &str) -> String {
    format!("tests/fixtures/{name}")
}

#[test]
fn engine_file_from_ast_maps_fields() {
    let src = read_fixture("typed_minimal.eng");
    let ast = parse_from_first_paren(&src).expect("parse");
    let engine = EngineFile::from_ast(&ast).expect("typed parse");
    assert_eq!(engine.name, "typed_engine");
    assert!((engine.mass_kg - 81_000.0).abs() < 1e-9);
    assert!((engine.max_power_w - 2_100_000.0).abs() < 1e-9);
    assert!((engine.max_velocity_mps - 30.0).abs() < 1e-9);
}

#[test]
fn wagon_file_from_ast_maps_fields() {
    let src = read_fixture("typed_minimal.wag");
    let ast = parse_from_first_paren(&src).expect("parse");
    let wagon = WagonFile::from_ast(&ast).expect("typed parse");
    assert_eq!(wagon.name, "typed_wagon");
    assert!((wagon.mass_kg - 22_000.0).abs() < 1e-9);
    assert!((wagon.max_brake_force_n - 90_000.0).abs() < 1e-9);
}

#[test]
fn consist_file_from_ast_extracts_entries() {
    let src = read_fixture("typed_minimal.con");
    let ast = parse_from_first_paren(&src).expect("parse");
    let consist = ConsistFile::from_ast(&ast).expect("typed parse");
    assert_eq!(consist.entries.len(), 2);
    assert_eq!(
        consist.entries[0],
        ConsistEntry::Engine {
            path: "typed_minimal.eng".to_string(),
            uid: None,
            flipped: false,
        }
    );
    assert_eq!(
        consist.entries[1],
        ConsistEntry::Wagon {
            path: "typed_minimal.wag".to_string(),
            uid: None,
            flipped: false,
        }
    );
}

#[test]
fn consist_file_preserves_flip_and_uid() {
    let src = read_fixture("typed_flip_uid.con");
    let ast = parse_from_first_paren(&src).expect("parse");
    let consist = ConsistFile::from_ast(&ast).expect("typed parse");
    assert_eq!(consist.entries.len(), 3);
    assert_eq!(consist.entries[0].uid(), Some(1));
    assert!(consist.entries[0].flipped());
    assert!(consist.entries[0].path().contains("typed_minimal"));
    assert_eq!(consist.entries[1].uid(), Some(2));
    assert!(consist.entries[1].flipped());
    assert_eq!(consist.entries[2].uid(), Some(3));
    assert!(!consist.entries[2].flipped());
}

#[test]
fn route_file_from_ast_extracts_identifiers() {
    let src = read_fixture("typed_minimal.trk");
    let ast = parse_from_first_paren(&src).expect("parse");
    let route = RouteFile::from_ast(&ast).expect("typed parse");
    assert_eq!(route.route_id, "typed_route");
    assert_eq!(route.name, "Typed Route");
}

#[test]
fn parse_msts_file_dispatches_by_extension() {
    let eng = parse_msts_file(fixture_path("typed_minimal.eng")).expect("dispatch eng");
    let wag = parse_msts_file(fixture_path("typed_minimal.wag")).expect("dispatch wag");
    let con = parse_msts_file(fixture_path("typed_minimal.con")).expect("dispatch con");
    let trk = parse_msts_file(fixture_path("typed_minimal.trk")).expect("dispatch trk");

    assert!(matches!(eng, MstsFile::Engine(_)));
    assert!(matches!(wag, MstsFile::Wagon(_)));
    assert!(matches!(con, MstsFile::Consist(_)));
    assert!(matches!(trk, MstsFile::Route(_)));
}

#[test]
fn units_conversions_are_stable() {
    assert!((lb_to_kg(100.0) - 45.359_237).abs() < 1e-6);
    assert!((kw_to_w(2.5) - 2_500.0).abs() < 1e-9);
    assert!((mph_to_mps(60.0) - 26.8224).abs() < 1e-6);
    assert!((kn_to_n(12.0) - 12_000.0).abs() < 1e-9);
    assert!((kmh_to_mps(108.0) - 30.0).abs() < 1e-9);
}
