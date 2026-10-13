use openrailsrs_formats::typed::{
    CantProfile, CantSection, CantStandard, generate_cant_profiles_with_table,
};
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
    #[serde(default)]
    legacy_radius_cant: Vec<[f32; 2]>,
    #[serde(default)]
    use_legacy: bool,
}
#[test]
fn native_legacy_and_class_speed_branches_match_unchanged_or161() {
    let data: Oracle = serde_json::from_str(include_str!(
        "../../../oracles/cant-legacy-local-or-1.6.1.json"
    ))
    .unwrap();
    assert_eq!(data.commit, "d16e670da333d26d2edfc97d5631a19dadf49ce5");
    assert_eq!(data.cases.len(), 18);
    for case in data.cases {
        let mut standard = CantStandard::native_default(case.metric, case.high_speed);
        standard.use_legacy_calculation = case.use_legacy;
        if case.use_legacy && !case.legacy_radius_cant.is_empty() {
            standard.min_cant_m = case
                .legacy_radius_cant
                .iter()
                .map(|p| p[1])
                .fold(f32::MAX, f32::min);
            standard.max_cant_m = case
                .legacy_radius_cant
                .iter()
                .map(|p| p[1])
                .fold(0., f32::max);
            standard.min_speed_mps = 0.;
            standard.runoff_slope = 0.003;
            standard.runoff_speed_mps = if case.metric {
                if case.high_speed { 0.08 } else { 0.055 }
            } else if case.high_speed {
                0.1136 * 0.44704
            } else {
                0.0852 * 0.44704
            };
        }
        let actual = generate_cant_profiles_with_table(
            &case.sections,
            case.gauge_m,
            &[standard],
            case.direction,
            &case.legacy_radius_cant,
        );
        assert_eq!(actual.len(), case.profiles.len(), "{}", case.name);
        for (a, b) in actual.iter().zip(case.profiles) {
            assert_eq!(a.positions.len(), b.positions.len(), "{}", case.name);
            for (a, b) in a.positions.iter().zip(b.positions) {
                assert!((a - b).abs() < 1e-6, "{}: knot {a} != {b}", case.name);
            }
            for (a, b) in a.elevations_m.iter().zip(b.elevations_m) {
                assert!(
                    (a - b).abs() <= data.tolerance_m,
                    "{}: cant {a} != {b}",
                    case.name
                );
            }
            for (a, b) in a.angles_rad.iter().zip(b.angles_rad) {
                assert!(
                    (a - b).abs() <= data.tolerance_rad,
                    "{}: roll {a} != {b}",
                    case.name
                );
            }
        }
    }
}
