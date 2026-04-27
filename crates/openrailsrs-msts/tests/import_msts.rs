use std::path::Path;

use openrailsrs_formats::{PathFile, TrackDbFile};
use openrailsrs_msts::{import_activity, import_route};

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

    // The generated TOML must be parseable as a plain TOML document.
    let value: toml::Value = toml::from_str(&toml_str).expect("generated TOML is not valid");

    // Must contain a `nodes` array and an `edges` array.
    let nodes = value
        .get("nodes")
        .and_then(|v| v.as_array())
        .expect("nodes array missing");
    let edges = value
        .get("edges")
        .and_then(|v| v.as_array())
        .expect("edges array missing");

    // From the minimal .tdb: 2 End nodes + 2 anonymous endpoint nodes → ≥2 nodes; 1 edge.
    assert!(!nodes.is_empty(), "nodes array is empty");
    assert_eq!(edges.len(), 1, "expected 1 edge, got {}", edges.len());

    // The edge must have length_m close to 1000.
    let length = edges[0]
        .get("length_m")
        .and_then(|v| v.as_float())
        .expect("edge.length_m missing");
    assert!(
        (length - 1000.0).abs() < 1.0,
        "edge length should be ~1000 m, got {length}"
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
