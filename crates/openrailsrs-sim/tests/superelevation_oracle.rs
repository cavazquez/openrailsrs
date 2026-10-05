//! Isolated original OR formula; this is not complete curve/derailment parity.
#[test]
fn comfort_speed_matches_frozen_original_csharp_float_results() {
    let oracle: serde_json::Value = serde_json::from_str(include_str!(
        "../../../oracles/superelevation-or-1.6.1.json"
    ))
    .unwrap();
    assert_eq!(
        oracle["reference_commit"],
        "d16e670da333d26d2edfc97d5631a19dadf49ce5"
    );
    let tolerance = oracle["tolerance_mps"].as_f64().unwrap();
    assert!(tolerance > 0. && tolerance <= 0.00002);
    let cases = oracle["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 10);
    for case in cases {
        let value = |name: &str| case[name].as_f64().unwrap();
        let p = openrailsrs_formats::VehicleCurveParameters {
            track_gauge_m: Some(value("gauge_m")),
            max_unbalanced_m: value("max_unbalanced_m"),
        };
        let actual = p
            .evaluate(value("radius_m"), value("cant_m"), 0., value("gauge_m"))
            .unwrap();
        let error = (actual.comfortable_speed_mps - value("comfortable_speed_mps")).abs();
        assert!(error <= tolerance, "{}: {error} m/s", case["name"]);
    }
}
