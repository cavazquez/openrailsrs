use openrailsrs_scenarios::load_timetable;
use std::path::Path;

#[test]
fn load_minimal_timetable_two_trains() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/minimal_timetable.toml");
    let tt = load_timetable(&path).expect("load timetable");
    assert_eq!(tt.trains.len(), 2);
    assert_eq!(tt.trains[0].id, "T-1");
    assert_eq!(tt.trains[1].id, "T-2");
    assert_eq!(tt.trains[0].depart_s, 0.0);
    assert_eq!(tt.trains[1].depart_s, 60.0);
    assert_eq!(tt.timetable.name, "Minimal test timetable");
}
