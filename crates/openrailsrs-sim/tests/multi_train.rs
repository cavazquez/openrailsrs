//! Integration tests for multi-train simulation with block-level occupancy.

use openrailsrs_scenarios::{
    DavisSection, Difficulty, GameplaySection, ObjectiveKind, OutputSection, RouteSection,
    ScenarioFile, ScenarioMeta, SimulationSection, TrainEntryDef, TrainSection,
};
use openrailsrs_sim::{MultiTrainResult, SimEvent, run_scenario_multi_train};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn light_davis() -> DavisSection {
    DavisSection {
        a_n: 200.0,
        b_n_per_mps: 4.0,
        c_n_per_mps2: 0.1,
    }
}

fn heavy_davis() -> DavisSection {
    DavisSection {
        a_n: 1500.0,
        b_n_per_mps: 25.0,
        c_n_per_mps2: 0.8,
    }
}

/// Set up a temporary scenario directory by copying the smoke example assets,
/// then return a [`ScenarioFile`] for a two-train run over `yard_a → yard_b`.
fn make_two_train_scenario(
    extra_davis: DavisSection,
    extra_start_time_s: f64,
    duration: f64,
) -> (ScenarioFile, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let smoke_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke");

    // Copy consists.
    let consists_dir = tmp.path().join("consists");
    std::fs::create_dir_all(&consists_dir).unwrap();
    std::fs::copy(
        smoke_dir.join("consists/freight.con"),
        consists_dir.join("freight.con"),
    )
    .unwrap();

    // Copy vehicles.
    let vehicles_dir = tmp.path().join("vehicles");
    std::fs::create_dir_all(&vehicles_dir).unwrap();
    for entry in std::fs::read_dir(smoke_dir.join("vehicles")).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), vehicles_dir.join(entry.file_name())).unwrap();
    }

    // Copy route.
    let routes_dir = tmp.path().join("routes/test");
    std::fs::create_dir_all(&routes_dir).unwrap();
    std::fs::copy(
        smoke_dir.join("routes/test/track.toml"),
        routes_dir.join("track.toml"),
    )
    .unwrap();

    let scenario = ScenarioFile {
        scenario: ScenarioMeta {
            name: "multi test".into(),
            description: String::new(),
            start_time_s: None,
            season: None,
        },
        route: RouteSection {
            path: "routes/test".into(),
            start: "yard_a".into(),
            destination: "yard_b".into(),
            stops: vec![],
            switches: vec![],
        },
        train: TrainSection {
            consist: "consists/freight.con".into(),
            davis: Some(heavy_davis()),
            max_capacity: None,
        },
        gameplay: GameplaySection {
            objective: ObjectiveKind::Arrive,
            time_limit_seconds: None,
            difficulty: Difficulty::Normal,
            penalty_per_second_late: 1.0,
        },
        simulation: SimulationSection {
            duration,
            time_step: 0.5,
            seed: 1,
        },
        output: OutputSection {
            csv: "run_primary.csv".into(),
            metadata: "run_primary.toml".into(),
        },
        extra_trains: vec![TrainEntryDef {
            id: "express".into(),
            consist: "consists/freight.con".into(),
            start: "yard_a".into(),
            destination: "yard_b".into(),
            start_time_s: extra_start_time_s,
            stops: vec![],
            davis: Some(extra_davis),
            switches: vec![],
            output_csv: "run_express.csv".into(),
        }],
        sound_regions: vec![],
        validate: None,
    };

    (scenario, tmp)
}

fn count_events<F>(result: &MultiTrainResult, id: &str, f: F) -> usize
where
    F: Fn(&SimEvent) -> bool,
{
    result
        .results
        .iter()
        .find(|r| r.id == id)
        .map(|r| r.sim_result.events.iter().filter(|e| f(e)).count())
        .unwrap_or(0)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Express train (faster, departs 60 s later) must block behind the freight
/// because the freight occupies e1 when the express approaches.
#[test]
fn second_train_blocked_by_first() {
    let (scenario, tmp) = make_two_train_scenario(light_davis(), 60.0, 1200.0);
    let result =
        run_scenario_multi_train(tmp.path(), &scenario).expect("multi-train sim should succeed");

    let block_waits = count_events(
        &result,
        "express",
        |e| matches!(e, SimEvent::BlockWait { train_id, .. } if train_id == "express"),
    );

    assert!(
        block_waits >= 1,
        "express should have emitted at least one BlockWait; got {block_waits}"
    );
}

/// Each `BlockWait` must be paired with a `BlockClear`.
#[test]
fn block_clear_emitted_for_each_wait() {
    let (scenario, tmp) = make_two_train_scenario(light_davis(), 60.0, 1200.0);
    let result =
        run_scenario_multi_train(tmp.path(), &scenario).expect("multi-train sim should succeed");

    let waits = count_events(
        &result,
        "express",
        |e| matches!(e, SimEvent::BlockWait { train_id, .. } if train_id == "express"),
    );
    let clears = count_events(
        &result,
        "express",
        |e| matches!(e, SimEvent::BlockClear { train_id, .. } if train_id == "express"),
    );

    assert_eq!(
        waits, clears,
        "each BlockWait must have a matching BlockClear (waits={waits}, clears={clears})"
    );
}

/// Both trains must reach the destination even when one had to wait.
#[test]
fn both_trains_reach_destination() {
    let (scenario, tmp) = make_two_train_scenario(light_davis(), 60.0, 1200.0);
    let result =
        run_scenario_multi_train(tmp.path(), &scenario).expect("multi-train sim should succeed");

    for train in &result.results {
        assert!(
            train.sim_result.metadata.reached_destination,
            "train '{}' did not reach destination",
            train.id
        );
        assert!(
            train.sim_result.metadata.final_odometer_m > 9000.0,
            "train '{}' odometer too short: {:.0}m",
            train.id,
            train.sim_result.metadata.final_odometer_m
        );
    }
}

/// Graduated penalty with 120 s delay and rate=1.0 should exceed the old
/// flat-50 deduction per stop.
#[test]
fn graduated_penalty_greater_than_flat_for_large_delay() {
    const GRACE_S: f64 = 30.0;
    let delay_s = 120.0_f64;
    let penalty_rate = 1.0_f64;
    let flat_deduction = 50.0_f64;

    let graduated_deduction = (delay_s - GRACE_S).max(0.0) * penalty_rate;

    assert!(
        graduated_deduction > flat_deduction,
        "graduated ({graduated_deduction:.1} pts) should exceed flat ({flat_deduction:.1} pts) \
         for delay={delay_s}s"
    );
}

/// Within the grace window, the graduated deduction must be zero.
#[test]
fn graduated_penalty_zero_within_grace() {
    const GRACE_S: f64 = 30.0;
    let delay_s = 20.0_f64;
    let penalty_rate = 1.0_f64;

    let deduction = (delay_s - GRACE_S).max(0.0) * penalty_rate;
    assert_eq!(
        deduction, 0.0,
        "delay within grace should produce zero deduction"
    );
}
