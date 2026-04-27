use std::io::Write;

use openrailsrs_import::{OsmImportOptions, import_osm_str};

// ── helpers ───────────────────────────────────────────────────────────────────

fn default_opts(route_id: &str) -> OsmImportOptions {
    OsmImportOptions {
        route_id: route_id.into(),
        default_speed_kmh: 80.0,
        bidirectional: false,
    }
}

const SAMPLE_JSON: &str = include_str!("../../../examples/osm/overpass_sample.json");

// ── Structural tests ──────────────────────────────────────────────────────────

#[test]
fn sample_produces_three_nodes() {
    let toml = import_osm_str(SAMPLE_JSON, &default_opts("test")).expect("import");
    let count = toml.lines().filter(|l| *l == "[[nodes]]").count();
    assert_eq!(count, 3, "expected 3 nodes (Mödling, Guntramsdorf, Baden)");
}

#[test]
fn sample_produces_two_edges() {
    let toml = import_osm_str(SAMPLE_JSON, &default_opts("test")).expect("import");
    let count = toml.lines().filter(|l| *l == "[[edges]]").count();
    assert_eq!(count, 2, "expected 2 edges (one per way segment)");
}

#[test]
fn station_names_present() {
    let toml = import_osm_str(SAMPLE_JSON, &default_opts("test")).expect("import");
    assert!(toml.contains("Mödling"), "station Mödling should appear");
    assert!(
        toml.contains("Guntramsdorf"),
        "station Guntramsdorf should appear"
    );
    assert!(toml.contains("Baden"), "station Baden should appear");
}

#[test]
fn route_id_is_written() {
    let toml = import_osm_str(SAMPLE_JSON, &default_opts("my_route")).expect("import");
    assert!(
        toml.contains(r#"id = "my_route""#),
        "route id should be written"
    );
}

#[test]
fn speed_limit_from_tag() {
    let toml = import_osm_str(SAMPLE_JSON, &default_opts("test")).expect("import");
    // fixture has maxspeed = "80"
    assert!(
        toml.contains("speed_limit_kmh = 80.0"),
        "speed should be 80 km/h from tag"
    );
}

#[test]
fn edge_lengths_are_positive() {
    use openrailsrs_import::{OsmImportOptions as Opts, build_layout};
    let layout = build_layout(
        SAMPLE_JSON,
        &Opts {
            route_id: "t".into(),
            default_speed_kmh: 80.0,
            bidirectional: false,
        },
    )
    .expect("build");
    for edge in &layout.edges {
        assert!(
            edge.length_m > 0.0,
            "edge {} has non-positive length {}",
            edge.id,
            edge.length_m
        );
    }
}

#[test]
fn no_railway_ways_returns_error() {
    let empty = r#"{"elements": [{"type":"node","id":1,"lat":47.0,"lon":15.0,"tags":{}}]}"#;
    let err = import_osm_str(empty, &default_opts("x")).unwrap_err();
    assert!(
        err.to_string().contains("no railway ways"),
        "expected NoRailwayWays error, got: {err}"
    );
}

#[test]
fn invalid_json_returns_error() {
    let err = import_osm_str("not json at all", &default_opts("x")).unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("json"),
        "expected JSON error, got: {err}"
    );
}

// ── Round-trip test: generated TOML can be loaded by openrailsrs-route ────────

#[test]
fn generated_toml_loads_with_route_crate() {
    use openrailsrs_route::load_track_graph_from_route_dir;

    let toml_str = import_osm_str(SAMPLE_JSON, &default_opts("badner_bahn")).expect("import");

    // Write the TOML to a temp directory.
    let dir = tempfile::tempdir().expect("tempdir");
    let track_toml = dir.path().join("track.toml");
    let mut f = std::fs::File::create(&track_toml).unwrap();
    write!(f, "{}", toml_str).unwrap();

    // Load it with the standard route loader.
    let graph = load_track_graph_from_route_dir(dir.path())
        .expect("load_track_graph_from_route_dir should succeed");

    // Check that all expected nodes are present.
    assert!(
        graph.node("n100001").is_some(),
        "node n100001 (Mödling) should exist"
    );
    assert!(
        graph.node("n100003").is_some(),
        "node n100003 (Guntramsdorf) should exist"
    );
    assert!(
        graph.node("n100005").is_some(),
        "node n100005 (Baden) should exist"
    );

    // Check edges exist.
    assert!(
        graph.edge("w200001_0").is_some(),
        "edge w200001_0 should exist"
    );
    assert!(
        graph.edge("w200002_0").is_some(),
        "edge w200002_0 should exist"
    );

    // Station kind is preserved.
    use openrailsrs_track::NodeKind;
    let modling = graph.node("n100001").unwrap();
    assert!(
        matches!(&modling.kind, NodeKind::Station { name } if name == "Mödling"),
        "Mödling node should be a Station"
    );
}

// ── Light-rail ways are included ──────────────────────────────────────────────

#[test]
fn light_rail_ways_imported() {
    let json = r#"{
        "elements": [
            {"type": "node", "id": 1, "lat": 48.0, "lon": 16.0},
            {"type": "node", "id": 2, "lat": 48.1, "lon": 16.1},
            {"type": "way",  "id": 10, "nodes": [1, 2],
             "tags": {"railway": "light_rail", "maxspeed": "60"}}
        ]
    }"#;
    let toml = import_osm_str(json, &default_opts("lr")).expect("import");
    assert!(toml.contains("[[edges]]"));
    assert!(toml.contains("speed_limit_kmh = 60.0"));
}

// ── Default speed is used when maxspeed tag absent ────────────────────────────

#[test]
fn default_speed_used_when_no_maxspeed_tag() {
    let json = r#"{
        "elements": [
            {"type": "node", "id": 1, "lat": 48.0, "lon": 16.0},
            {"type": "node", "id": 2, "lat": 48.1, "lon": 16.1},
            {"type": "way",  "id": 10, "nodes": [1, 2],
             "tags": {"railway": "rail"}}
        ]
    }"#;
    let opts = OsmImportOptions {
        route_id: "x".into(),
        default_speed_kmh: 120.0,
        bidirectional: false,
    };
    let toml = import_osm_str(json, &opts).expect("import");
    assert!(
        toml.contains("speed_limit_kmh = 120.0"),
        "should use default 120 km/h"
    );
}
