use openrailsrs_sim::run_from_scenario_file;

#[test]
fn smoke_run_example_scenario_reaches_destination() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/scenario.toml");
    let r = run_from_scenario_file(&path).expect("sim run");
    assert!(
        r.metadata.reached_destination,
        "expected arrival within duration, odometer={}",
        r.metadata.final_odometer_m
    );
}
