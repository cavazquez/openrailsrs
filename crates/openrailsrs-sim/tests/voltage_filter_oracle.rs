use serde::Deserialize;
#[derive(Deserialize)]
struct Oracle {
    commit: String,
    tolerance_v: f64,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    name: String,
    samples: Vec<Sample>,
}
#[derive(Deserialize)]
struct Sample {
    input_v: f64,
    dt_s: f64,
    output_v: f64,
}
#[test]
fn native_filter_steps_loss_and_recovery_match_the_pinned_unmodified_class() {
    let data: Oracle = serde_json::from_str(include_str!(
        "../../../oracles/voltage-filter-or-1.6.1.json"
    ))
    .unwrap();
    assert_eq!(data.commit, "d16e670da333d26d2edfc97d5631a19dadf49ce5");
    assert_eq!(data.cases.len(), 4);
    let mut count = 0;
    for case in data.cases {
        let mut filter = openrailsrs_sim::voltage_filter::VoltageFilter::default();
        for (i, sample) in case.samples.into_iter().enumerate() {
            let output = filter.filter(sample.input_v, sample.dt_s);
            assert!(
                (output - sample.output_v).abs() <= data.tolerance_v,
                "{} sample {i}: {output} != {}",
                case.name,
                sample.output_v
            );
            assert!(filter.valid());
            count += 1;
            if i == 70 {
                let restored =
                    serde_json::from_str(&serde_json::to_string(&filter).unwrap()).unwrap();
                assert_eq!(filter, restored);
                filter = restored;
            }
        }
    }
    assert_eq!(count, 728);
}
