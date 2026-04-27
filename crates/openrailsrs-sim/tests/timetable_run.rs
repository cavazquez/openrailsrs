use std::path::{Path, PathBuf};

use openrailsrs_sim::{LiveMultiSim, TrainStatus};

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR = crates/openrailsrs-sim
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent() // crates/
        .unwrap()
        .parent() // workspace root
        .unwrap()
        .to_path_buf()
}

#[test]
fn two_trains_both_arrive() {
    let ws_root = workspace_root();

    // Route path is absolute so it resolves regardless of timetable location.
    // Consist path uses "../../consists/freight.con" relative to route_dir
    // (routes/test → smoke/consists).  consist_root() = smoke/ where vehicles/ live.
    let route_abs = ws_root.join("examples/smoke/routes/test");
    let patched = ws_root.join("target/smoke_timetable_test.toml");
    let contents = format!(
        "[timetable]\nname = \"Smoke timetable\"\nroute = \"{route}\"\nduration_s = 700.0\ntime_step_s = 1.0\n\n[[trains]]\nid = \"F-1\"\nconsist = \"../../consists/freight.con\"\nstart = \"yard_a\"\ndestination = \"yard_b\"\ndepart_s = 0.0\n\n[[trains]]\nid = \"F-2\"\nconsist = \"../../consists/freight.con\"\nstart = \"yard_a\"\ndestination = \"yard_b\"\ndepart_s = 60.0\n",
        route = route_abs.display(),
    );
    std::fs::create_dir_all(patched.parent().unwrap()).unwrap();
    std::fs::write(&patched, contents).unwrap();

    let mut sim = LiveMultiSim::from_timetable(&patched).expect("load timetable");

    let mut iterations = 0;
    while !sim.all_arrived() && sim.sim_time() < sim.duration() {
        sim.step_frame(50);
        iterations += 1;
        assert!(iterations < 1_000_000, "simulation did not converge");
    }

    let snapshots = sim.step_frame(0);
    assert_eq!(snapshots.len(), 2);
    for snap in &snapshots {
        assert_eq!(
            snap.status,
            TrainStatus::Arrived,
            "train {} did not arrive (status={:?})",
            snap.id,
            snap.status
        );
    }
}
