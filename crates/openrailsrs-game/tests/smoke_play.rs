use std::path::PathBuf;

use openrailsrs_game::play_headless_from_scenario_file;

#[test]
fn smoke_play_headless() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/scenario.toml");
    let o = play_headless_from_scenario_file(&path).expect("play");
    assert!(o.reached_destination);
}
