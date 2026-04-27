use openrailsrs_track::TrackGraph;
use serde_json::{Value, json};

/// GeoJSON `FeatureCollection` of edge centerlines using node `x_m`/`y_m` as coordinates.
pub fn track_graph_to_geojson(graph: &TrackGraph) -> Value {
    let mut features = Vec::new();
    for (_, e) in graph.edges_iter() {
        let from = graph.node(&e.from.0);
        let to = graph.node(&e.to.0);
        if let (Some(a), Some(b)) = (from, to) {
            features.push(json!({
                "type": "Feature",
                "properties": {
                    "edge_id": e.id.0,
                    "length_m": e.length_m,
                    "speed_limit_mps": e.speed_limit_mps,
                },
                "geometry": {
                    "type": "LineString",
                    "coordinates": [[a.x_m, a.y_m], [b.x_m, b.y_m]],
                }
            }));
        }
    }
    json!({
        "type": "FeatureCollection",
        "features": features,
    })
}
