#[test]
fn six_original_injector_tables_match_unmodified_or_interpolation() {
    let o: serde_json::Value = serde_json::from_str(include_str!(
        "../../../oracles/steam-injectors-or-1.6.1.json"
    ))
    .unwrap();
    assert_eq!(o["commit"], "d16e670da333d26d2edfc97d5631a19dadf49ce5");
    for c in o["cases"].as_array().unwrap() {
        let actual = openrailsrs_sim::steam_injectors::flow_kg_s(
            c["size_mm"].as_u64().unwrap() as u8,
            c["pressure_psi"].as_f64().unwrap() * 0.0689475729,
        );
        assert!(
            (actual - c["flow_kg_s"].as_f64().unwrap()).abs()
                <= o["flow_tolerance_kg_s"].as_f64().unwrap()
        );
    }
}
