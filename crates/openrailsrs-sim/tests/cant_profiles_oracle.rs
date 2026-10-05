use openrailsrs_formats::typed::{CantProfile, CantSection, CantStandard, generate_cant_profiles};
use serde::Deserialize;
#[derive(Deserialize)]
struct Oracle {
    commit: String,
    tolerance_m: f32,
    tolerance_rad: f32,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    name: String,
    metric: bool,
    high_speed: bool,
    gauge_m: f32,
    direction: f32,
    sections: Vec<CantSection>,
    profiles: Vec<CantProfile>,
}
#[test]
fn generated_profiles_match_unmodified_pinned_or_knots() {
    let oracle: Oracle =
        serde_json::from_str(include_str!("../../../oracles/cant-profiles-or-1.6.1.json")).unwrap();
    assert_eq!(oracle.commit, "d16e670da333d26d2edfc97d5631a19dadf49ce5");
    assert_eq!(oracle.cases.len(), 10);
    let mut max_m = 0f32;
    let mut max_angle = 0f32;
    for case in oracle.cases {
        let actual = generate_cant_profiles(
            &case.sections,
            case.gauge_m,
            &[CantStandard::native_default(case.metric, case.high_speed)],
            case.direction,
        );
        assert_eq!(actual.len(), case.profiles.len());
        for (actual, expected) in actual.iter().zip(&case.profiles) {
            assert_eq!(
                actual.positions.len(),
                expected.positions.len(),
                "{}",
                case.name
            );
            for (a, b) in actual.positions.iter().zip(&expected.positions) {
                assert!((a - b).abs() < 1e-6, "{}: knot {a} != {b}", case.name);
            }
            for (a, b) in actual.elevations_m.iter().zip(&expected.elevations_m) {
                let e = (a - b).abs();
                max_m = max_m.max(e);
                assert!(e <= oracle.tolerance_m, "{}: cant {a} != {b}", case.name);
            }
            for (a, b) in actual.angles_rad.iter().zip(&expected.angles_rad) {
                let e = (a - b).abs();
                max_angle = max_angle.max(e);
                assert!(e <= oracle.tolerance_rad, "{}: roll {a} != {b}", case.name);
            }
        }
    }
    println!("OR cant oracle: max cant error {max_m} m; max angle error {max_angle} rad");
}
