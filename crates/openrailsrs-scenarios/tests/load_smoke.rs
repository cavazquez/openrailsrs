use openrailsrs_scenarios::load_scenario;

#[test]
fn load_example_smoke_scenario() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/scenario.toml");
    let s = load_scenario(&path).expect("valid scenario");
    assert_eq!(s.route.start, "yard_a");
}
