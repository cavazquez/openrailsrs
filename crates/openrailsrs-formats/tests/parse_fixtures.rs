use openrailsrs_formats::parse_from_first_paren;

fn read_fixture(name: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/{name}")).expect("fixture")
}

#[test]
fn parse_minimal_trk() {
    let src = read_fixture("minimal.trk");
    let ast = parse_from_first_paren(&src).expect("parse");
    let s = format!("{ast}");
    assert!(s.contains("Tr_RouteFile"));
    assert!(s.contains("test_route"));
}

#[test]
fn parse_minimal_eng() {
    let src = read_fixture("minimal.eng");
    let ast = parse_from_first_paren(&src).expect("parse");
    assert!(format!("{ast}").contains("Engine"));
}

#[test]
fn parse_minimal_wag() {
    let src = read_fixture("minimal.wag");
    let ast = parse_from_first_paren(&src).expect("parse");
    assert!(format!("{ast}").contains("Wagon"));
}

#[test]
fn parse_minimal_con() {
    let src = read_fixture("minimal.con");
    let ast = parse_from_first_paren(&src).expect("parse");
    assert!(format!("{ast}").contains("Train"));
}
