use std::path::Path;

use openrailsrs_formats::{ActivityFile, PathFile, TrItemKind, TrackDbFile};
use openrailsrs_msts::{import_activity, import_route, import_route_with_activity};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn fixtures_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

// ── Parser unit tests ─────────────────────────────────────────────────────────

#[test]
fn parse_minimal_tdb() {
    let tdb =
        TrackDbFile::from_path(fixtures_dir().join("minimal.tdb")).expect("parse minimal.tdb");

    // 3 TrackNode entries: node 1 (End), 2 (Vector), 3 (End).
    assert_eq!(
        tdb.nodes.len(),
        3,
        "expected 3 nodes, got {}",
        tdb.nodes.len()
    );

    let ids: Vec<u32> = tdb.nodes.iter().map(|n| n.id).collect();
    assert!(ids.contains(&1), "node 1 missing");
    assert!(ids.contains(&2), "node 2 missing");
    assert!(ids.contains(&3), "node 3 missing");

    // Node 2 should be a Vector with length 1000 m.
    let n2 = tdb.nodes.iter().find(|n| n.id == 2).unwrap();
    match &n2.kind {
        openrailsrs_formats::TrackNodeKind::Vector { length_m, .. } => {
            assert!(
                (*length_m - 1000.0).abs() < 1.0,
                "expected ~1000 m, got {length_m}"
            );
        }
        other => panic!("node 2 should be Vector, got {other:?}"),
    }
}

#[test]
fn parse_minimal_pat() {
    let pat = PathFile::from_path(fixtures_dir().join("minimal.pat")).expect("parse minimal.pat");

    assert_eq!(pat.name, "TestPath");
    assert_eq!(pat.pdps.len(), 2);
    assert_eq!(pat.start_node(), Some(1));
    assert_eq!(pat.end_node(), Some(3));
}

// ── Import tests ──────────────────────────────────────────────────────────────

#[test]
fn import_route_produces_valid_toml() {
    let toml_str = import_route(&fixtures_dir()).expect("import route");

    let value: toml::Value = toml::from_str(&toml_str).expect("generated TOML is not valid");

    assert!(
        value.get("route").and_then(|r| r.get("id")).is_some(),
        "[route].id missing"
    );

    let nodes = value
        .get("nodes")
        .and_then(|v| v.as_array())
        .expect("nodes array missing");
    let edges = value
        .get("edges")
        .and_then(|v| v.as_array())
        .expect("edges array missing");

    assert!(!nodes.is_empty(), "nodes array is empty");
    assert_eq!(edges.len(), 1, "expected 1 edge, got {}", edges.len());

    let length = edges[0]
        .get("length_m")
        .and_then(|v| v.as_float())
        .expect("edge.length_m missing");
    assert!(
        (length - 1000.0).abs() < 1.0,
        "edge length should be ~1000 m, got {length}"
    );

    let speed_kmh = edges[0]
        .get("speed_limit_kmh")
        .and_then(|v| v.as_float())
        .expect("edge.speed_limit_kmh missing");
    assert!(
        speed_kmh > 0.0,
        "speed_limit_kmh should be positive, got {speed_kmh}"
    );
}

#[test]
fn import_route_toml_loads_with_route_crate() {
    let toml_str = import_route(&fixtures_dir()).expect("import route");
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("track.toml"), &toml_str).expect("write track.toml");
    let graph = openrailsrs_route::load_track_graph_from_route_dir(dir.path())
        .expect("load imported track.toml");
    assert!(
        graph.edges_iter().next().is_some(),
        "graph should have edges"
    );
}

#[test]
fn import_activity_produces_scenario() {
    let act_path = fixtures_dir().join("minimal.act");
    let scenario_toml = import_activity(&fixtures_dir(), &act_path).expect("import activity");

    // Must be valid TOML.
    let value: toml::Value =
        toml::from_str(&scenario_toml).expect("generated scenario TOML is not valid");

    // Must have [scenario], [route], [train] sections.
    assert!(
        value.get("scenario").is_some(),
        "[scenario] section missing"
    );
    assert!(value.get("route").is_some(), "[route] section missing");
    assert!(value.get("train").is_some(), "[train] section missing");

    // Check name was propagated.
    let name = value["scenario"]["name"].as_str().unwrap_or_default();
    assert_eq!(name, "Minimal Test Activity");

    // Duration from activity (1h30m = 5400s).
    let duration = value["simulation"]["duration"].as_float().unwrap_or(0.0);
    assert!(
        (duration - 5400.0).abs() < 1.0,
        "expected 5400 s, got {duration}"
    );
}

// ── Sub-phase A: TrItemTable signals ─────────────────────────────────────────

#[test]
fn parse_tritem_table_extracts_signal() {
    let tdb = TrackDbFile::from_path(fixtures_dir().join("with_signals/route.tdb"))
        .expect("parse with_signals/route.tdb");

    assert_eq!(tdb.items.len(), 1, "expected one TrItem");
    let item = &tdb.items[0];
    assert_eq!(item.id, 1, "TrItemId mismatch");
    assert!(
        (item.distance_m - 250.0).abs() < 1e-6,
        "expected 250 m, got {}",
        item.distance_m
    );
    assert!(
        matches!(item.kind, TrItemKind::Signal { .. }),
        "expected SignalItem, got {:?}",
        item.kind
    );

    // The vector node 2 must reference TrItemId 1 via TrItemRefs.
    let n2 = tdb
        .nodes
        .iter()
        .find(|n| n.id == 2)
        .expect("node 2 missing");
    match &n2.kind {
        openrailsrs_formats::TrackNodeKind::Vector { item_ids, .. } => {
            assert_eq!(item_ids.as_slice(), &[1u32], "TrItemRefs not parsed");
        }
        other => panic!("expected Vector, got {other:?}"),
    }
}

#[test]
fn import_route_emits_signals_section() {
    let dir = fixtures_dir().join("with_signals");
    let toml_str = import_route(&dir).expect("import route with signals");

    let value: toml::Value = toml::from_str(&toml_str).expect("generated TOML must be valid");

    let signals = value
        .get("signals")
        .and_then(|v| v.as_array())
        .expect("[[signals]] section missing");

    assert_eq!(signals.len(), 1, "expected exactly one signal");
    let sig = &signals[0];
    assert_eq!(
        sig.get("id").and_then(|v| v.as_str()),
        Some("sig1"),
        "signal id should be 'sig1'"
    );
    assert_eq!(
        sig.get("edge_id").and_then(|v| v.as_str()),
        Some("e2"),
        "signal must be projected onto edge e2"
    );
    let pos = sig
        .get("position_m")
        .and_then(|v| v.as_float())
        .unwrap_or_default();
    assert!(
        (pos - 250.0).abs() < 1e-6,
        "expected position_m=250, got {pos}"
    );
    assert_eq!(
        sig.get("aspect").and_then(|v| v.as_str()),
        Some("stop"),
        "default initial aspect must be 'stop'"
    );
}

#[test]
fn import_route_without_signals_omits_section() {
    // The minimal.tdb fixture has no TrItemTable; the generated TOML must not
    // include a `[[signals]]` array (skip_serializing_if = "Vec::is_empty").
    let toml_str = import_route(&fixtures_dir()).expect("import minimal route");
    let value: toml::Value = toml::from_str(&toml_str).expect("valid TOML");
    assert!(
        value.get("signals").is_none(),
        "minimal route must not emit a signals section, got: {toml_str}"
    );
}

// ── Sub-phase B: TrafficService → extra_trains ──────────────────────────────

fn as_f64(value: &toml::Value) -> Option<f64> {
    value
        .as_float()
        .or_else(|| value.as_integer().map(|i| i as f64))
}

#[test]
fn parse_activity_collects_traffic_services() {
    let act = ActivityFile::from_path(fixtures_dir().join("with_traffic/traffic.act"))
        .expect("parse traffic activity");

    assert_eq!(act.services.len(), 2, "expected 2 services");
    assert_eq!(act.services[0].name, "freight_north");
    assert_eq!(act.services[0].path_file, "service1.pat");
    assert!((act.services[0].start_time_s - 1800.0).abs() < 1e-6);
    assert_eq!(act.services[1].name, "express");
    assert_eq!(act.services[1].path_file, "service2.pat");
    assert!((act.services[1].start_time_s - 600.0).abs() < 1e-6);
    assert_eq!(act.season.as_deref(), Some("Summer"));
}

#[test]
fn import_activity_emits_extra_trains_from_traffic() {
    let dir = fixtures_dir().join("with_traffic");
    let act_path = dir.join("traffic.act");
    let toml_str = import_activity(&dir, &act_path).expect("import activity with traffic");

    let value: toml::Value = toml::from_str(&toml_str).expect("valid TOML");
    let extras = value
        .get("extra_trains")
        .and_then(|v| v.as_array())
        .expect("[[extra_trains]] missing");
    assert_eq!(extras.len(), 2, "expected 2 extra trains");

    let svc1 = &extras[0];
    assert_eq!(
        svc1.get("id").and_then(|v| v.as_str()),
        Some("freight_north")
    );
    let t1 = as_f64(svc1.get("start_time_s").expect("start_time_s")).unwrap_or_default();
    assert!((t1 - 1800.0).abs() < 1e-6, "expected 1800s, got {t1}");
    assert_eq!(svc1.get("start").and_then(|v| v.as_str()), Some("n3"));
    assert_eq!(svc1.get("destination").and_then(|v| v.as_str()), Some("n1"));
    assert_eq!(
        svc1.get("output_csv").and_then(|v| v.as_str()),
        Some("run_freight_north.csv")
    );

    let svc2 = &extras[1];
    assert_eq!(svc2.get("id").and_then(|v| v.as_str()), Some("express"));
    let t2 = as_f64(svc2.get("start_time_s").expect("start_time_s")).unwrap_or_default();
    assert!((t2 - 600.0).abs() < 1e-6, "expected 600s, got {t2}");
}

#[test]
fn import_activity_without_traffic_keeps_extra_trains_empty() {
    // The minimal.act fixture has no Traffic_Definition → extra_trains must be absent.
    let act_path = fixtures_dir().join("minimal.act");
    let toml_str = import_activity(&fixtures_dir(), &act_path).expect("import minimal activity");
    let value: toml::Value = toml::from_str(&toml_str).expect("valid TOML");
    let extras = value
        .get("extra_trains")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    assert_eq!(extras, 0, "minimal activity should not emit extra_trains");
}

// ── Sub-phase C: Activity events and restrictions ──────────────────────────

#[test]
fn parse_activity_collects_events_and_restrictions() {
    let act = ActivityFile::from_path(fixtures_dir().join("with_events/events.act"))
        .expect("parse events activity");

    assert_eq!(act.failed_signals, vec![1u32]);
    assert_eq!(act.restricted_zones.len(), 1);
    let z = &act.restricted_zones[0];
    assert_eq!(z.item_id_start, 1);
    assert_eq!(z.item_id_end, 2);
    assert!((z.max_speed_mps - 10.0).abs() < 1e-6);

    assert_eq!(act.activity_objects.len(), 1);
    let obj = &act.activity_objects[0];
    assert_eq!(obj.item_id, 2);
    assert_eq!(obj.workers, 50);
    assert_eq!(obj.kind, "PickupWagon");
}

#[test]
fn import_route_with_activity_applies_failed_signals_and_restrictions() {
    let dir = fixtures_dir().join("with_events");
    let act_path = dir.join("events.act");
    let toml_str = import_route_with_activity(&dir, &act_path).expect("import route w/ activity");
    let value: toml::Value = toml::from_str(&toml_str).expect("valid TOML");

    // Failed signal: signal "sig1" must be forced to "stop".
    let signals = value
        .get("signals")
        .and_then(|v| v.as_array())
        .expect("signals missing");
    let sig1 = signals
        .iter()
        .find(|s| s.get("id").and_then(|v| v.as_str()) == Some("sig1"))
        .expect("sig1 missing");
    assert_eq!(
        sig1.get("aspect").and_then(|v| v.as_str()),
        Some("stop"),
        "failed signal must be aspect=stop"
    );

    // Restricted zone touches edge e2: speed_limit_kmh must equal 36.0 (10 m/s).
    let edges = value
        .get("edges")
        .and_then(|v| v.as_array())
        .expect("edges missing");
    let e2 = edges
        .iter()
        .find(|e| e.get("id").and_then(|v| v.as_str()) == Some("e2"))
        .expect("e2 missing");
    let lim = as_f64(e2.get("speed_limit_kmh").expect("speed_limit_kmh")).unwrap_or_default();
    assert!(
        (lim - 36.0).abs() < 1e-6,
        "expected speed_limit_kmh=36, got {lim}"
    );
}

#[test]
fn import_activity_with_objects_emits_route_stops() {
    let dir = fixtures_dir().join("with_events");
    let act_path = dir.join("events.act");
    let toml_str = import_activity(&dir, &act_path).expect("import activity with events");
    let value: toml::Value = toml::from_str(&toml_str).expect("valid TOML");

    let route = value.get("route").expect("[route] missing");
    let stops = route
        .get("stops")
        .and_then(|v| v.as_array())
        .expect("[[route.stops]] missing");
    assert_eq!(stops.len(), 1, "expected 1 stop, got {}", stops.len());
    let stop = &stops[0];

    // TrItem 2 sits at distance 800m on a 1000m vector node going n2→n3,
    // so it should map to n3 (closer endpoint).
    assert_eq!(stop.get("node").and_then(|v| v.as_str()), Some("n3"));
    let on = stop
        .get("passengers_on")
        .and_then(|v| v.as_integer())
        .unwrap_or(0);
    assert_eq!(on, 50, "PickupWagon must populate passengers_on");
}

// ── Sub-phase D: ScenarioMeta start_time_s / season ─────────────────────────

#[test]
fn import_activity_propagates_start_time_and_season() {
    let dir = fixtures_dir().join("with_traffic");
    let act_path = dir.join("traffic.act");
    let toml_str = import_activity(&dir, &act_path).expect("import activity");
    let value: toml::Value = toml::from_str(&toml_str).expect("valid TOML");

    let scenario = value.get("scenario").expect("[scenario] missing");
    // StartTime 8 0 0 → 28800 seconds.
    let st = as_f64(scenario.get("start_time_s").expect("start_time_s missing"))
        .expect("start_time_s should be numeric");
    assert!((st - 28800.0).abs() < 1e-6, "expected 28800s, got {st}");
    assert_eq!(
        scenario.get("season").and_then(|v| v.as_str()),
        Some("summer"),
        "season must be lowercased"
    );
}

#[test]
fn import_activity_without_season_omits_field() {
    // The minimal.act fixture declares no Season → `season` must be omitted
    // (skip_serializing_if = "Option::is_none").
    let act_path = fixtures_dir().join("minimal.act");
    let toml_str = import_activity(&fixtures_dir(), &act_path).expect("import minimal activity");
    let value: toml::Value = toml::from_str(&toml_str).expect("valid TOML");
    let scenario = value.get("scenario").expect("[scenario] missing");
    assert!(
        scenario.get("season").is_none(),
        "minimal activity must omit season"
    );
}

// ── SoundRegions: TDB + activity overrides → scenario.toml ──────────────────

#[test]
fn parse_tdb_collects_sound_source_items() {
    let tdb = TrackDbFile::from_path(fixtures_dir().join("with_sound_regions").join("route.tdb"))
        .expect("parse tdb");

    let kinds: Vec<&TrItemKind> = tdb.items.iter().map(|i| &i.kind).collect();
    let sound_count = kinds
        .iter()
        .filter(|k| matches!(k, TrItemKind::SoundSource { .. }))
        .count();
    assert_eq!(sound_count, 2, "expected 2 SoundSourceItem entries");

    let with_files = tdb
        .items
        .iter()
        .filter_map(|i| match &i.kind {
            TrItemKind::SoundSource { sms_file: Some(f) } => Some(f.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(with_files.len(), 2, "both items should reference an .sms");
    assert!(with_files.iter().any(|f| f.ends_with("depot.sms")));
    assert!(with_files.iter().any(|f| f.ends_with("tunnel.sms")));
}

#[test]
fn parse_activity_collects_sound_region_overrides() {
    let act = ActivityFile::from_path(fixtures_dir().join("with_sound_regions").join("sound.act"))
        .expect("parse act");

    assert_eq!(
        act.sound_regions.len(),
        2,
        "expected 2 SoundRegion overrides"
    );

    let tunnel = act
        .sound_regions
        .iter()
        .find(|r| r.tr_item_id == 2)
        .expect("override for TrItemId 2");
    assert_eq!(tunnel.kind, "tunnel");
    assert!((tunnel.volume - 0.7).abs() < 1e-6);
    assert_eq!(tunnel.radius_m, Some(80.0));

    let depot = act
        .sound_regions
        .iter()
        .find(|r| r.tr_item_id == 1)
        .expect("override for TrItemId 1");
    assert_eq!(depot.kind, "depot");
    assert!((depot.volume - 0.5).abs() < 1e-6);
    assert!(depot.radius_m.is_none(), "depot has no explicit radius");
}

#[test]
fn import_activity_emits_sound_regions_section() {
    let dir = fixtures_dir().join("with_sound_regions");
    let act_path = dir.join("sound.act");
    let toml_str = import_activity(&dir, &act_path).expect("import activity");
    let value: toml::Value = toml::from_str(&toml_str).expect("valid TOML");

    let regions = value
        .get("sound_regions")
        .and_then(|v| v.as_array())
        .expect("[[sound_regions]] missing");
    assert_eq!(regions.len(), 2, "expected 2 emitted regions");

    let ids: Vec<&str> = regions
        .iter()
        .filter_map(|r| r.get("id").and_then(|v| v.as_str()))
        .collect();
    assert!(ids.contains(&"sr1"), "missing region sr1");
    assert!(ids.contains(&"sr2"), "missing region sr2");

    for r in regions {
        let edge = r.get("edge_id").and_then(|v| v.as_str()).unwrap_or("");
        assert_eq!(edge, "e2", "vector node 2 produces edge e2");
    }
}

#[test]
fn import_activity_applies_sound_region_overrides() {
    let dir = fixtures_dir().join("with_sound_regions");
    let act_path = dir.join("sound.act");
    let toml_str = import_activity(&dir, &act_path).expect("import activity");
    let value: toml::Value = toml::from_str(&toml_str).expect("valid TOML");

    let regions = value
        .get("sound_regions")
        .and_then(|v| v.as_array())
        .expect("[[sound_regions]] missing");

    let by_id = |id: &str| -> &toml::Value {
        regions
            .iter()
            .find(|r| r.get("id").and_then(|v| v.as_str()) == Some(id))
            .unwrap_or_else(|| panic!("region {id} not found"))
    };

    let tunnel = by_id("sr2");
    assert_eq!(tunnel.get("kind").and_then(|v| v.as_str()), Some("tunnel"));
    let radius =
        as_f64(tunnel.get("radius_m").expect("radius_m")).expect("radius_m must be numeric");
    assert!(
        (radius - 80.0).abs() < 1e-6,
        "tunnel override radius_m must be 80 m, got {radius}"
    );
    let vol = as_f64(tunnel.get("base_volume").expect("base_volume"))
        .expect("base_volume must be numeric");
    assert!(
        (vol - 0.7).abs() < 1e-3,
        "tunnel override base_volume must be 0.7, got {vol}"
    );

    let depot = by_id("sr1");
    assert_eq!(depot.get("kind").and_then(|v| v.as_str()), Some("depot"));
    let depot_radius =
        as_f64(depot.get("radius_m").expect("radius_m")).expect("radius_m must be numeric");
    assert!(
        (depot_radius - 50.0).abs() < 1e-6,
        "depot keeps default 50 m radius (no override), got {depot_radius}"
    );
}

// ── Engine traction curve propagation ────────────────────────────────────────

#[test]
fn engine_traction_curve_parsed() {
    use openrailsrs_formats::EngineFile;
    use openrailsrs_formats::parse_from_first_paren;

    let eng_src = r#"
( Engine
    ( MassKG 80000 )
    ( MaxPower 3000000 )
    ( MaxVelocity 120 )
    ( MaxTractiveEffortCurves
        ( CurveEntry 0.0 350000 )
        ( CurveEntry 10.0 280000 )
        ( CurveEntry 30.0 120000 )
    )
)
"#;
    let ast = parse_from_first_paren(eng_src).unwrap();
    let eng = EngineFile::from_ast(&ast).unwrap();

    assert_eq!(eng.traction_curve.len(), 3, "expected 3 curve points");
    // First point: 0.0 km/h → 0.0 m/s, force 350 000 N.
    let (v0, f0) = eng.traction_curve[0];
    assert!(v0.abs() < 1e-6, "first velocity should be 0 m/s");
    assert!(
        (f0 - 350_000.0).abs() < 1.0,
        "first force should be 350 000 N"
    );
}
