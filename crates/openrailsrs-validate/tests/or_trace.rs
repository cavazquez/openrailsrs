use openrailsrs_validate::{
    OrColumnMap, ValidationConfig, compare_or_dump_with_run, parse_openrailsrs_run_csv,
};

#[test]
fn compare_or_and_run_aligned_passes() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let rep = compare_or_dump_with_run(
        &dir.join("or_dump_minimal.csv"),
        &dir.join("ors_run_aligned.csv"),
        &OrColumnMap::default(),
        &ValidationConfig::strict(),
        0.1,
    )
    .expect("compare");
    assert!(rep.pass, "aligned fixtures should match");
}

#[test]
fn parse_diverging_run_after_header_fix() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/smoke/run_diverging.csv");
    let trace = parse_openrailsrs_run_csv(&path).expect("parse diverging");
    assert!(trace.samples.len() > 100, "expected many samples");
}
