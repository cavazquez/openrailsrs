use openrailsrs_campaign::*;
use std::io::Write;
use tempfile::NamedTempFile;

const CAMPAIGN_TOML: &str = r#"
[campaign]
name = "Test Campaign"
description = "Unit test fixture"

[[missions]]
id = "m1"
name = "Mission 1"
scenario = "smoke/scenario.toml"
requires = []
min_pass_score = 60
bonus_threshold = 90

[[missions]]
id = "m2"
name = "Mission 2"
scenario = "mitre/scenario.toml"
requires = ["m1"]
min_pass_score = 60
bonus_threshold = 85
"#;

fn write_tmp(content: &str) -> NamedTempFile {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f
}

#[test]
fn load_campaign_parses_correctly() {
    let f = write_tmp(CAMPAIGN_TOML);
    let c = load_campaign(f.path()).unwrap();
    assert_eq!(c.campaign.name, "Test Campaign");
    assert_eq!(c.missions.len(), 2);
    assert_eq!(c.missions[0].id, "m1");
    assert_eq!(c.missions[1].requires, vec!["m1"]);
}

#[test]
fn empty_progress_gives_first_available_rest_locked() {
    let f = write_tmp(CAMPAIGN_TOML);
    let c = load_campaign(f.path()).unwrap();
    let p = Progress::default();
    let statuses = mission_statuses(&c, &p);

    assert_eq!(statuses[0].state, MissionState::Available);
    assert_eq!(statuses[1].state, MissionState::Locked);
}

#[test]
fn completing_m1_unlocks_m2() {
    let f = write_tmp(CAMPAIGN_TOML);
    let c = load_campaign(f.path()).unwrap();
    let mut p = Progress::default();
    record_result(&mut p, "m1", 75, 90);

    let statuses = mission_statuses(&c, &p);
    assert_eq!(statuses[0].state, MissionState::Completed);
    assert_eq!(statuses[1].state, MissionState::Available);
}

#[test]
fn below_min_score_keeps_m2_locked() {
    let f = write_tmp(CAMPAIGN_TOML);
    let c = load_campaign(f.path()).unwrap();
    let mut p = Progress::default();
    record_result(&mut p, "m1", 40, 90); // below min_pass_score=60

    let statuses = mission_statuses(&c, &p);
    assert_eq!(statuses[0].state, MissionState::Available); // still available, not completed
    assert_eq!(statuses[1].state, MissionState::Locked);
}

#[test]
fn bonus_flag_set_when_threshold_reached() {
    let f = write_tmp(CAMPAIGN_TOML);
    let c = load_campaign(f.path()).unwrap();
    let mut p = Progress::default();
    record_result(&mut p, "m1", 95, 90); // above bonus_threshold=90
    let statuses = mission_statuses(&c, &p);
    assert!(statuses[0].bonus);
}

#[test]
fn best_score_is_kept_on_retry() {
    let f = write_tmp(CAMPAIGN_TOML);
    let _ = load_campaign(f.path()).unwrap();
    let mut p = Progress::default();
    record_result(&mut p, "m1", 70, 90);
    record_result(&mut p, "m1", 50, 90); // worse — should not overwrite
    assert_eq!(p.completed["m1"].score, 70);

    record_result(&mut p, "m1", 85, 90); // better — should update
    assert_eq!(p.completed["m1"].score, 85);
}

#[test]
fn progress_round_trips_through_json() {
    let mut p = Progress::default();
    record_result(&mut p, "m1", 80, 90);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("progress.json");
    save_progress(&path, &p).unwrap();

    let loaded = load_progress(&path).unwrap();
    assert_eq!(loaded.completed["m1"].score, 80);
}

#[test]
fn missing_progress_file_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("progress.json");
    let p = load_progress(&path).unwrap();
    assert!(p.completed.is_empty());
}
