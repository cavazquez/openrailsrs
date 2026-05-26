//! Debug Chiltern path — run with: cargo test -p openrailsrs-sim chiltern_path_debug -- --nocapture

use openrailsrs_route::load_track_graph_from_route_dir;
use openrailsrs_sim::path::edge_path;
use openrailsrs_track::SwitchPosition;

#[test]
fn chiltern_path_debug() {
    let route_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
    if !route_dir.join("track.toml").exists() {
        return;
    }
    let mut g = load_track_graph_from_route_dir(&route_dir).unwrap();
    g.set_switch("n10770", SwitchPosition::Diverging).unwrap();
    g.set_switch("n10780", SwitchPosition::Straight).unwrap();
    let path = edge_path(&g, "n3", "n10770").expect("path");
    eprintln!("path len={}", path.len());
    let mut cum = 0.0;
    for (i, e) in path.iter().enumerate() {
        let edge = g.edge(e).unwrap();
        cum += edge.length_m;
        eprintln!(
            "{i:3} {e} {} -> {} len={:.1} cum={:.1}",
            edge.from.0, edge.to.0, edge.length_m, cum
        );
        for s in g.signals_on_edge(e) {
            eprintln!("      sig {} pos={:.1} asp={:?}", s.id, s.position_m, s.aspect);
        }
    }
}
