use openrailsrs_route::load_track_graph_from_route_dir;

#[test]
fn load_route_missing_nodes_fails() {
    let dir = tempfile::tempdir().unwrap();
    let toml = r#"
[route]
id = "x"

[[edges]]
id = "e1"
from = "a"
to = "b"
length_m = 100.0
"#;
    std::fs::write(dir.path().join("track.toml"), toml).unwrap();

    let err = load_track_graph_from_route_dir(dir.path()).unwrap_err();
    assert!(err.to_string().contains("unknown node"));
}
