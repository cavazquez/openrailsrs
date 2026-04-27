use std::path::PathBuf;

use openrailsrs_game::play_headless_from_scenario_file;

#[test]
fn impossible_time_limit_creates_late_penalty() {
    let temp = tempfile::tempdir().unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke");
    std::fs::create_dir_all(temp.path().join("routes/test")).unwrap();
    std::fs::create_dir_all(temp.path().join("consists")).unwrap();
    std::fs::create_dir_all(temp.path().join("vehicles")).unwrap();
    std::fs::copy(
        root.join("routes/test/track.toml"),
        temp.path().join("routes/test/track.toml"),
    )
    .unwrap();
    std::fs::copy(
        root.join("consists/freight.con"),
        temp.path().join("consists/freight.con"),
    )
    .unwrap();
    std::fs::copy(
        root.join("vehicles/minimal.eng"),
        temp.path().join("vehicles/minimal.eng"),
    )
    .unwrap();
    std::fs::copy(
        root.join("vehicles/minimal.wag"),
        temp.path().join("vehicles/minimal.wag"),
    )
    .unwrap();
    let scenario = temp.path().join("scenario.toml");

    let s = "[scenario]\nname = \"strict\"\n\n[route]\npath = \"routes/test\"\nstart = \"yard_a\"\ndestination = \"yard_b\"\n\n[train]\nconsist = \"consists/freight.con\"\n\n[gameplay]\nobjective = \"arrive_on_time\"\ntime_limit_seconds = 1\ndifficulty = \"normal\"\n\n[simulation]\nduration = 800.0\ntime_step = 0.1\nseed = 42\n\n[output]\ncsv = \"run.csv\"\nmetadata = \"run.toml\"\n";
    std::fs::write(&scenario, s).unwrap();

    let out = play_headless_from_scenario_file(&scenario).unwrap();
    assert!(!out.success);
    assert!(out.penalties.iter().any(|p| p.contains("late_arrival")));
}
