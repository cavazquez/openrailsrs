use openrailsrs_track::TrackGraph;

/// Graphviz DOT directed graph for debugging topology.
pub fn track_graph_to_dot(graph: &TrackGraph) -> String {
    let mut s = String::from("digraph track {\n  rankdir=LR;\n");
    for (_, n) in graph.nodes_iter() {
        let label = n.id.0.replace('"', "'");
        s.push_str(&format!("  \"{}\" [label=\"{}\"];\n", n.id.0, label));
    }
    for (_, e) in graph.edges_iter() {
        s.push_str(&format!(
            "  \"{}\" -> \"{}\" [label=\"{} {:.0}m {:.1}m/s\"];\n",
            e.from.0, e.to.0, e.id.0, e.length_m, e.speed_limit_mps
        ));
    }
    s.push_str("}\n");
    s
}
