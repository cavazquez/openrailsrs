//! CPU-only regression against genuine OR1.6.1 subsystem captures02/05.
//! No Wine, installed ENG/PAT, downloaded route, or measured evolving state is
//! an input. The programme explicitly prescribes speed and external pipe, so
//! this target does not claim a closed-loop train or universal steam parity.
use openrailsrs_formats::{NativeSteamBrakeKind, NativeSteamParams};
use openrailsrs_sim::native_steam::{NativeSteamInput, NativeSteamState, tables};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const REFERENCE: &[u8] = include_bytes!(
    "../../../docs/fixtures/compatibility/twenty-20261015/steam-cycle161-subsystems/reference.json"
);
const PROVENANCE: &[u8] = include_bytes!(
    "../../../docs/fixtures/compatibility/twenty-20261015/steam-cycle161-subsystems/provenance.json"
);
const PROTOCOL: &[u8] = include_bytes!(
    "../../../docs/fixtures/compatibility/twenty-20261015/steam-cycle161-subsystems/protocol.json"
);
const EXTRACTOR: &[u8] = include_bytes!(
    "../../../docs/fixtures/compatibility/twenty-20261015/steam-cycle161-subsystems/extract-fixture.py"
);
const HOT_MANIFEST: &[u8] = include_bytes!(
    "../../../docs/fixtures/compatibility/twenty-20261015/steam-cycle161-subsystems/raw-manifests/hot_subsystem.json"
);
const COLD_MANIFEST: &[u8] = include_bytes!(
    "../../../docs/fixtures/compatibility/twenty-20261015/steam-cycle161-subsystems/raw-manifests/cold_subsystem.json"
);
const COMMIT: &str = "d16e670da333d26d2edfc97d5631a19dadf49ce5";
const PROTOCOL_SHA: &str = "c62fc972d0799c3fa7d32db8e6bcd19fde852b986b8ebe652bbb96de0e7e73cb";
const HOT_SHA: &str = "abd64cb3088dc54a0266df0c275d3953dfe8a4589fca92166d09dac8a4683e40";
const COLD_SHA: &str = "69c3e00601db4b00e79b77f07cd18de4c30e547481f8635588e5b02fdd999915";
const KG_PER_LB: f64 = 0.45359237;
const COLUMNS: [&str; 17] = [
    "tick",
    "pressure_psi",
    "boiler_mass_lb",
    "boiler_heat_btu",
    "water_fraction",
    "evaporation_lb_s",
    "steam_usage_lb_s",
    "water_consumption_lb_s",
    "fire_burn_kg_s",
    "tender_coal_kg",
    "combined_tender_water_ukg",
    "water_lb_per_ukg",
    "motive_force_n",
    "injector1",
    "injector2",
    "injector1_fraction",
    "injector2_fraction",
];
const CHANNELS: [&str; 11] = [
    "pressure_bar",
    "boiler_mass_kg",
    "boiler_enthalpy_j",
    "water_fraction",
    "production_kg_s",
    "steam_consumption_kg_s",
    "injection_kg_s",
    "coal_burn_kg_s",
    "tender_coal_kg",
    "tender_water_kg",
    "motive_force_n",
];

#[derive(Deserialize)]
struct Fixture {
    schema: u32,
    reference_version: String,
    reference_commit: String,
    protocol_sha256: String,
    columns: Vec<String>,
    limits: Value,
    table_limits: Value,
    tables: TableCalls,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct TableCalls {
    points: Vec<Point>,
    grids: Vec<GridPoint>,
}
#[derive(Deserialize)]
struct Point {
    factory: String,
    x: f32,
    value: f64,
}
#[derive(Deserialize)]
struct GridPoint {
    factory: String,
    rpm: f32,
    cutoff: f32,
    value: f64,
}
#[derive(Deserialize)]
struct Case {
    id: String,
    hot_start: bool,
    profile: NativeSteamParams,
    ticks: usize,
    dt_s: f32,
    programme: Vec<Command>,
    environment: Environment,
    expected: Vec<[f64; 17]>,
}
#[derive(Deserialize)]
struct Command {
    tick: usize,
    phase: String,
    throttle: f32,
    brake: f32,
    cutoff: f32,
    damper: f32,
    firing: f32,
    injector1: bool,
    injector2: bool,
    speed_mps: f32,
    pipe_absolute_psi: f32,
}
#[derive(Deserialize)]
struct Environment {
    world_position: WorldPosition,
    height_above_sea_level_m: f32,
    season: String,
    weather: String,
}
#[derive(Deserialize)]
struct WorldPosition {
    tile_x: i32,
    tile_z: i32,
    location_x: f64,
    location_z: f64,
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn number(v: &Value, name: &str) -> f64 {
    v[name].as_f64().filter(|x| x.is_finite()).expect(name)
}
fn fixture() -> Fixture {
    let f: Fixture = serde_json::from_slice(REFERENCE).unwrap();
    let proof: Value = serde_json::from_slice(PROVENANCE).unwrap();
    let protocol: Value = serde_json::from_slice(PROTOCOL).unwrap();
    assert_eq!(f.schema, 1);
    assert_eq!(f.reference_version, "1.6.1");
    assert_eq!(f.reference_commit, COMMIT);
    assert_eq!(f.protocol_sha256, PROTOCOL_SHA);
    assert_eq!(digest(PROTOCOL), PROTOCOL_SHA);
    assert_eq!(f.columns, COLUMNS);
    assert_eq!(f.limits, protocol["native_limits"]);
    assert_eq!(f.table_limits, protocol["new_table_limits"]);
    assert_eq!(protocol["limits_declared_before_capture"], true);
    assert_eq!(f.limits["injector_state"], "exact");
    assert_eq!(proof["original_resources_bundled"], false);
    assert_eq!(proof["post_start_observed_model_state_prescribed"], false);
    assert_eq!(proof["native_limits_changed"], false);
    let files = proof["files_sha256"].as_object().unwrap();
    assert_eq!(files.len(), 5);
    for (name, bytes) in [
        ("reference.json", REFERENCE),
        ("protocol.json", PROTOCOL),
        ("extract-fixture.py", EXTRACTOR),
        ("raw-manifests/hot_subsystem.json", HOT_MANIFEST),
        ("raw-manifests/cold_subsystem.json", COLD_MANIFEST),
    ] {
        assert_eq!(
            files[name],
            digest(bytes),
            "derived evidence changed: {name}"
        );
    }
    assert_eq!(digest(HOT_MANIFEST), HOT_SHA);
    assert_eq!(digest(COLD_MANIFEST), COLD_SHA);
    for (id, bytes, expected_hash) in [
        ("hot_subsystem", HOT_MANIFEST, HOT_SHA),
        ("cold_subsystem", COLD_MANIFEST, COLD_SHA),
    ] {
        let manifest: Value = serde_json::from_slice(bytes).unwrap();
        let provenance = &proof["cases"][id];
        assert_eq!(provenance["capture_manifest_sha256"], expected_hash);
        assert_eq!(manifest["status"], "captured_not_compared");
        assert_eq!(manifest["rust_acceptance"], "not_run");
        assert_eq!(manifest["method"], "MSTSSteamLocomotive.Update");
        assert_eq!(manifest["reference_commit"], COMMIT);
        assert_eq!(manifest["protocol_sha256"], PROTOCOL_SHA);
        assert_eq!(
            manifest["files"]["native/reference.json"],
            provenance["raw_reference_sha256"]
        );
        assert_eq!(
            manifest["files"]["native/initial.json"],
            provenance["raw_initial_sha256"]
        );
        assert_eq!(
            manifest["files"]["native/tables.json"],
            provenance["raw_tables_sha256"]
        );
        for (name, hash) in protocol["assemblies_sha256"].as_object().unwrap() {
            assert_eq!(manifest["assemblies"][name], *hash);
        }
        for (name, hash) in protocol["source_sha256"].as_object().unwrap() {
            assert_eq!(manifest["source_files"][name], *hash);
        }
        assert_eq!(
            provenance["original_assets_sha256"]["BR_3FT_47629.eng"],
            "3cb8a216ac9f3c43859fbef8a5bc1b355382e92474eeb2b35134975a77b25fa5"
        );
        assert_eq!(
            provenance["profile_derivation"]["post_start_native_model_state_used_as_hardware"],
            false
        );
    }
    assert_eq!(f.cases.len(), 2);
    assert_eq!(f.cases[0].id, "hot_subsystem");
    assert_eq!(f.cases[1].id, "cold_subsystem");
    f
}

#[test]
fn all310_original_dll_steam_table_calls_respect_the_prior_limits() {
    let f = fixture();
    assert_eq!(f.tables.points.len(), 250);
    assert_eq!(f.tables.grids.len(), 60);
    assert_eq!(number(&f.table_limits, "abs"), 0.0002);
    assert_eq!(number(&f.table_limits, "relative"), 0.000002);
    let limit = |expected: f64| {
        number(&f.table_limits, "abs") + number(&f.table_limits, "relative") * expected.abs()
    };
    let mut factories = BTreeMap::<&str, usize>::new();
    for point in &f.tables.points {
        *factories.entry(&point.factory).or_default() += 1;
        let actual = tables::by_factory(&point.factory)
            .expect("original factory must be implemented")
            .at(point.x) as f64;
        assert!(
            (actual - point.value).abs() <= limit(point.value),
            "{}({}): {actual} versus {}",
            point.factory,
            point.x,
            point.value
        );
    }
    assert_eq!(factories.len(), 50);
    assert!(factories.values().all(|&count| count == 5));
    let mut grids = BTreeMap::<&str, BTreeSet<(u32, u32)>>::new();
    for point in &f.tables.grids {
        assert!(
            grids
                .entry(&point.factory)
                .or_default()
                .insert((point.rpm.to_bits(), point.cutoff.to_bits()))
        );
        let upper = match point.factory.as_str() {
            "CutoffInitialPressureUpper" => true,
            "CutoffInitialPressureLower" => false,
            name => panic!("Unsupported original grid {name}"),
        };
        let actual = tables::cutoff_initial_pressure(upper, point.rpm, point.cutoff) as f64;
        assert!(
            (actual - point.value).abs() <= limit(point.value),
            "{}({},{}): {actual} versus {}",
            point.factory,
            point.rpm,
            point.cutoff,
            point.value
        );
    }
    assert_eq!(grids.len(), 2);
    assert!(grids.values().all(|points| points.len() == 30));
}

fn outside_temperature(environment: &Environment) -> f32 {
    assert_eq!(environment.season, "summer");
    assert_eq!(environment.weather, "clear");
    let w = &environment.world_position;
    let geo = openrailsrs_formats::geography::geographic_position(
        w.tile_x,
        w.tile_z,
        w.location_x,
        w.location_z,
    )
    .expect("registered static native position must project");
    let latitude = geo.latitude as f32 * 57.29578;
    let summer = tables::interpolate(
        &[
            -50., -40., -30., -20., -10., 0., 10., 20., 30., 40., 50., 60.,
        ],
        &[
            13.4, 18.3, 22.8, 24.3, 24.4, 25., 25.2, 22.5, 26.6, 24.8, 19.4, 14.3,
        ],
        latitude,
    );
    // TrainCar.CarHeightAboveSeaLevelM is a separate native field. Location.Y
    // is not used in its place, and no observed outside temperature is read.
    summer - (environment.height_above_sea_level_m / 1000. * 9.8).clamp(0., 30.)
}
fn expected_sample(row: &[f64; 17]) -> [f64; 11] {
    [
        row[1] * 0.06894757293168,
        row[2] * KG_PER_LB,
        row[3] * 1055.05585262,
        row[4],
        row[5] * KG_PER_LB,
        row[6] * KG_PER_LB,
        row[7] * KG_PER_LB,
        row[8],
        row[9],
        row[10] * row[11] * KG_PER_LB,
        row[12],
    ]
}
fn sample(n: &NativeSteamState) -> [f64; 11] {
    [
        n.BoilerPressurePSI as f64 * 0.06894757293168,
        n.BoilerMassLB as f64 * KG_PER_LB,
        n.BoilerHeatBTU as f64 * 1055.05585262,
        n.WaterFraction as f64,
        n.EvaporationLBpS as f64 * KG_PER_LB,
        n.PreviousTotalSteamUsageLBpS as f64 * KG_PER_LB,
        n.WaterConsumptionLbpS as f64 * KG_PER_LB,
        n.FuelBurnRateSmoothedKGpS as f64,
        n.tender_coal_kg() as f64,
        n.tender_water_ukg() as f64 * 10.021 * KG_PER_LB,
        n.engine.RealTractiveForceN as f64,
    ]
}
fn compare_case(f: &Fixture, case: &Case) {
    assert_eq!(case.ticks, 1600);
    assert_eq!(case.expected.len(), 1601);
    assert_eq!(case.dt_s, 0.05);
    assert!(case.profile.valid());
    assert_eq!(case.profile.hot_start, case.hot_start);
    assert_eq!(
        case.programme.iter().map(|c| c.tick).collect::<Vec<_>>(),
        [0, 200, 800, 1100, 1400]
    );
    assert_eq!(
        case.programme
            .iter()
            .map(|c| c.phase.as_str())
            .collect::<Vec<_>>(),
        ["idle_fire", "steam_load", "injection", "coast", "braking"]
    );
    assert_eq!(
        case.programme
            .iter()
            .map(|c| c.speed_mps)
            .collect::<Vec<_>>(),
        [0., 8., 16., 12., 4.]
    );
    assert_eq!(
        case.programme
            .iter()
            .map(|c| c.pipe_absolute_psi)
            .collect::<Vec<_>>(),
        [14.503774_f32, 4.2, 4.2, 4.2, 14.503774]
    );
    // This constructor receives authored hardware only. None of expected[]
    // supplies pressure, mass, heat, flow, force, filter, or initial model state.
    let mut n = NativeSteamState::new(&case.profile, case.hot_start).unwrap();
    let outside = outside_temperature(&case.environment);
    let mut command = 0;
    let mut injectors_on = [false, false];
    let mut maximum = [0_f64; 11];
    let mut worst_tick = [0; 11];
    for (tick, expected) in case.expected.iter().enumerate() {
        assert_eq!(expected[0], tick as f64);
        assert!(expected.iter().all(|x| x.is_finite()));
        if tick > 0 {
            while command + 1 < case.programme.len() && case.programme[command + 1].tick < tick {
                command += 1;
            }
            let c = &case.programme[command];
            injectors_on = [c.injector1, c.injector2];
            let brake_release = matches!(
                n.brake_kind(c.brake, false),
                NativeSteamBrakeKind::Release | NativeSteamBrakeKind::FullQuickRelease
            );
            n.advance(
                NativeSteamInput {
                    regulator: c.throttle,
                    cutoff: c.cutoff,
                    speed_mps: c.speed_mps.abs(),
                    signed_speed_mps: c.speed_mps,
                    damper: c.damper,
                    firing: c.firing,
                    blower: case.profile.initial_blower,
                    injectors_on,
                    injectors: [
                        if c.injector1 { 1. } else { 0. },
                        if c.injector2 { 1. } else { 0. },
                    ],
                    outside_c: outside,
                    brake_release,
                    brake_pipe_absolute_psi: c.pipe_absolute_psi,
                    has_tender_coupled: !case.profile.tender_required,
                    generator_steam_effects: case.profile.generator_fitted,
                    ..Default::default()
                },
                case.dt_s,
            );
        }
        assert!(
            n.valid_for(&case.profile),
            "{} invalid evolved state at{tick}",
            case.id
        );
        let native = expected_sample(expected);
        let actual = sample(&n);
        for (index, (native, actual)) in native.into_iter().zip(actual).enumerate() {
            let error = (native - actual).abs();
            assert!(error.is_finite());
            if error > maximum[index] {
                maximum[index] = error;
                worst_tick[index] = tick;
            }
        }
        // Native boolean controls are checked against the registered command;
        // the kernel's own delayed InjectorFraction state is checked exactly
        // as well, including after the injector has been switched off.
        for (index, &on) in injectors_on.iter().enumerate() {
            assert!(expected[13 + index] == 0. || expected[13 + index] == 1.);
            assert_eq!(
                on,
                expected[13 + index] == 1.,
                "{} tick{tick} injector{index}",
                case.id
            );
            assert_eq!(
                n.InjectorFraction[index] as f64,
                expected[15 + index],
                "{} tick{tick} injector fraction{index}",
                case.id
            );
        }
    }
    for (index, field) in CHANNELS.iter().enumerate() {
        let limit = number(&f.limits, &format!("{field}_abs"));
        assert!(
            maximum[index] <= limit,
            "{} {field}: max error{} at tick{} exceeds prior limit{limit}",
            case.id,
            maximum[index],
            worst_tick[index]
        );
    }
}
#[test]
fn original_hot_subsystem1601_rows_evolve_from_authored_hardware() {
    let f = fixture();
    compare_case(&f, &f.cases[0]);
}
#[test]
fn original_cold_subsystem1601_rows_evolve_from_authored_hardware() {
    let f = fixture();
    compare_case(&f, &f.cases[1]);
}
