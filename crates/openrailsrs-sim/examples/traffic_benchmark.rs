//! Serial/parallel traffic measurement with an identical authored formation.
//! JSON preserves initialization, driving, worker count and final-state digest.
use openrailsrs_sim::{LiveDriveSession, LiveTraffic};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, time::Instant};

fn main() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = tempfile::tempdir().unwrap();
    let lanes: usize = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "32".into())
        .parse()
        .unwrap();
    assert!((4..=128).contains(&lanes));
    let mut graph = "[route]\nid=\"traffic-performance\"\n".to_string();
    for lane in 0..=lanes {
        graph += &format!(
            "\n[[nodes]]\nid=\"a{lane}\"\nx_m=0.0\ny_m={y}.0\n[[nodes]]\nid=\"b{lane}\"\nx_m=50000.0\ny_m={y}.0\n[[edges]]\nid=\"lane{lane}\"\nfrom=\"a{lane}\"\nto=\"b{lane}\"\nlength_m=50000.0\nspeed_limit_kmh=90.0\ngrade_percent=0.0\n",
            y = lane * 100
        );
    }
    std::fs::write(root.path().join("track.toml"), &graph).unwrap();
    let mut scenario =
        openrailsrs_scenarios::load_scenario(repo.join("examples/player_lab/scenario.toml"))
            .unwrap();
    scenario.route.path = root.path().to_string_lossy().into_owned();
    scenario.route.start = "a0".into();
    scenario.route.destination = "b0".into();
    scenario.route.stops.clear();
    scenario.route.waypoints.clear();
    scenario.train.consist = repo
        .join("examples/player_lab/consists/lab.con")
        .to_string_lossy()
        .into_owned();
    scenario.extra_trains.clear();
    for lane in 1..=lanes {
        scenario.extra_trains.push(toml::from_str(&format!("id=\"train-{lane}\"\nconsist={}\nstart=\"a{lane}\"\ndestination=\"b{lane}\"\nstart_offset_m=80.0\noutput_csv=\"train-{lane}.csv\"",serde_json::to_string(&scenario.train.consist).unwrap())).unwrap());
    }
    let runs = measure(root.path(), &scenario);
    let chiltern_path = repo.join("examples/chiltern_extended/scenario.toml");
    let chiltern = openrailsrs_scenarios::load_scenario(&chiltern_path).unwrap();
    let chiltern_runs = measure(chiltern_path.parent().unwrap(), &chiltern);
    println!(
        "{}",
        serde_json::json!({"schema":2,"graph_sha256":format!("{:x}",Sha256::digest(graph)),"formation_sha256":format!("{:x}",Sha256::digest(std::fs::read(&scenario.train.consist).unwrap())),"reference":"same binary, serial worker as algorithmic reference","camera":"headless","seed":42,"runs":runs,
            "corridor":{"scenario":"examples/chiltern_extended/scenario.toml","scenario_sha256":format!("{:x}",Sha256::digest(std::fs::read(&chiltern_path).unwrap())),"runs":chiltern_runs,"scope":"repository corridor physics, switches and signalling; native rendering/content audit remains separate"}})
    );
}

fn measure(
    directory: &std::path::Path,
    scenario: &openrailsrs_scenarios::ScenarioFile,
) -> Vec<serde_json::Value> {
    let mut rows = vec![];
    let mut digest = None;
    for workers in [1, 2, 4, 8] {
        let started = Instant::now();
        let mut player = LiveDriveSession::from_scenario(directory, scenario).unwrap();
        let mut traffic = LiveTraffic::from_scenario(directory, scenario).unwrap();
        traffic.set_worker_count(workers).unwrap();
        let initial_ms = started.elapsed().as_secs_f64() * 1000.;
        let started = Instant::now();
        for _ in 0..600 {
            traffic.advance(&mut player, 0.1, Some(0.5), |_| {});
        }
        let driving_ms = started.elapsed().as_secs_f64() * 1000.;
        let mut final_value =
            serde_json::to_value((player.snapshot(), traffic.snapshot())).unwrap();
        final_value.sort_all_objects();
        let final_state = serde_json::to_vec(&final_value).unwrap();
        let hash = format!("{:x}", Sha256::digest(final_state));
        assert_eq!(
            digest.get_or_insert_with(|| hash.clone()),
            &hash,
            "Worker count changed the simulation"
        );
        rows.push(serde_json::json!({"workers":workers,"initial_load_ms":initial_ms,"driving_ms":driving_ms,"simulated_seconds":60,"state_sha256":hash,"services":traffic.services.len()}));
    }
    rows
}
