use openrailsrs_sim::LiveDriveSession;
use std::path::PathBuf;
fn session() -> (tempfile::TempDir, LiveDriveSession) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut scenario =
        openrailsrs_scenarios::load_scenario(root.join("examples/yard_training/scenario.toml"))
            .unwrap();
    scenario.gameplay.shunting.clear();
    scenario.route.stops.clear();
    scenario.route.start_offset_m = Some(160.);
    scenario.train.consist = root
        .join("examples/yard_training/consists/yard.con")
        .to_string_lossy()
        .into_owned();
    let temp = tempfile::tempdir().unwrap();
    let mut track =
        std::fs::read_to_string(root.join("examples/yard_training/route/track.toml")).unwrap();
    track +=
        "\n[[signals]]\nid='yard_signal'\nedge_id='approach'\nposition_m=170.0\naspect='stop'\n";
    std::fs::write(temp.path().join("track.toml"), track).unwrap();
    scenario.route.path = temp.path().to_string_lossy().into_owned();
    let s = LiveDriveSession::from_scenario(temp.path(), &scenario).unwrap();
    (temp, s)
}
#[test]
fn own_wheels_before_signal_do_not_block_a_bounded_authority_and_restore_preserves_it() {
    let (_root, mut s) = session();
    s.authorize_shunting("yard_signal", 100.).unwrap();
    assert!(s.shunting_authority_status().unwrap().contains("100 m"));
    let save = s.snapshot();
    let (_root2, mut resumed) = session();
    resumed.restore_snapshot(save).unwrap();
    resumed.step_realtime(1., |_| {});
    assert!(resumed.state.shunt_authority.is_some());
    assert!(resumed.effective_speed_limit_mps() <= 15. / 3.6);
    resumed.driver_direction = 1.;
    resumed.driver_throttle = 1.;
    resumed.driver_brake = 0.;
    resumed.traffic_brake_assistance = false;
    for _ in 0..1500 {
        resumed.step_realtime(0.05, |_| {});
    }
    assert!(
        resumed.head_chainage_m() <= 260.5,
        "head {}",
        resumed.head_chainage_m()
    );
    assert!(resumed.velocity_mps() < 0.1);
}
#[test]
fn another_train_invalid_distance_expiration_and_reversing_reject_or_revoke_authority() {
    let (_root, mut s) = session();
    for distance in [f64::NAN, 9., 501., 10.] {
        assert!(s.authorize_shunting("yard_signal", distance).is_err());
    }
    s.external_occupancy
        .insert("approach".into(), "another".into());
    assert!(s.authorize_shunting("yard_signal", 100.).is_err());
    s.external_occupancy.clear();
    s.authorize_shunting("yard_signal", 100.).unwrap();
    s.external_occupancy
        .insert("approach_r".into(), "another".into());
    s.step_realtime(0.1, |_| {});
    assert!(s.state.shunt_authority.is_none());
    s.external_occupancy.clear();
    s.authorize_shunting("yard_signal", 100.).unwrap();
    s.state.path_reversed = true;
    s.step_realtime(0.1, |_| {});
    assert!(s.state.shunt_authority.is_none());
    s.state.path_reversed = false;
    s.authorize_shunting("yard_signal", 100.).unwrap();
    s.step_realtime(121., |_| {});
    assert!(s.state.shunt_authority.is_none());
}
