use openrailsrs_track::sigscript::{SignalContext, SignalProgram, linked_route_set};

#[test]
fn block_states_and_linked_routes_match_original_or161_methods_exactly() {
    let data: serde_json::Value = serde_json::from_str(include_str!(
        "../../../docs/fixtures/compatibility/operational-signals-or161/reference.json"
    ))
    .unwrap();
    let p = SignalProgram::compile("state=BLOCK_STATE();").unwrap();
    for row in data["blocks"].as_array().unwrap() {
        let name = row["internal_state"].as_str().unwrap();
        let c = SignalContext {
            block_clear: matches!(name, "Reserved" | "Reservable"),
            junction_obstructed: !matches!(
                name,
                "Reserved" | "Reservable" | "OccupiedSameDirection"
            ),
            ..Default::default()
        };
        assert_eq!(
            u64::from(p.evaluate(c).unwrap().aspect),
            row["result"].as_u64().unwrap(),
            "{name}"
        );
    }
    for row in data["links"].as_array().unwrap() {
        let required = row["required_vector"].as_u64().unwrap();
        let vector = row["route_vector"].as_u64().unwrap();
        let required = format!("e{required}");
        let path = if vector > 0 {
            vec![format!("e{vector}")]
        } else {
            vec![]
        };
        assert_eq!(
            linked_route_set(
                (row["required_vector"] != 0).then_some(required.as_str()),
                &path,
                row["enabled"].as_bool().unwrap()
            ),
            row["result"] == 1,
            "{row}"
        );
    }
    for row in data["bindings"].as_array().unwrap() {
        assert_eq!(
            row["required_vector"].as_u64().unwrap(),
            2 + row["junction_path"].as_u64().unwrap()
        );
    }
}
