//! Pre-computed physical edge data for a specific train path.
//!
//! Building a [`PathData`] once from the path and the graph replaces repeated
//! `HashMap::get` calls inside the hot simulation loop with direct `Vec` indexing,
//! avoiding string hashing on every tick.

use openrailsrs_track::TrackGraph;

/// Physical data for a single edge in the path — everything `physics::step` needs.
#[derive(Clone, Debug)]
pub struct PathEdgeData {
    pub length_m: f64,
    pub speed_limit_mps: f64,
    pub grade_percent: f64,
}

/// All edge data for the route a particular train will travel.
///
/// Built once before the simulation loop; indexed by `state.edge_index`.
pub struct PathData {
    pub edges: Vec<PathEdgeData>,
}

impl PathData {
    /// Pre-compute edge data for `path_edges` by looking each edge up in `graph`.
    /// Missing edges get conservative defaults (0 grade, 55 km/h speed limit).
    pub fn from_path(path_edges: &[String], graph: &TrackGraph) -> Self {
        let edges = path_edges
            .iter()
            .map(|eid| {
                graph
                    .edge(eid)
                    .map(|e| PathEdgeData {
                        length_m: e.length_m,
                        speed_limit_mps: e.speed_limit_mps,
                        grade_percent: e.grade_percent,
                    })
                    .unwrap_or(PathEdgeData {
                        length_m: 0.0,
                        speed_limit_mps: 55.0 / 3.6,
                        grade_percent: 0.0,
                    })
            })
            .collect();
        Self { edges }
    }

    /// Get data for the edge at `idx` (the current `state.edge_index`).
    #[inline]
    pub fn get(&self, idx: usize) -> Option<&PathEdgeData> {
        self.edges.get(idx)
    }

    /// Total path length (m).
    pub fn total_length_m(&self) -> f64 {
        self.edges.iter().map(|e| e.length_m).sum()
    }

    /// Map a distance along the path to `(edge_id, pos_on_edge_m)`.
    pub fn position_at_odometer(
        path_edges: &[String],
        edges: &[PathEdgeData],
        odometer_m: f64,
    ) -> Option<(String, f64)> {
        let mut cum = 0.0;
        for (i, eid) in path_edges.iter().enumerate() {
            let len = edges.get(i).map(|e| e.length_m).unwrap_or(0.0);
            let end = cum + len;
            if odometer_m <= end + 1e-6 || i + 1 == path_edges.len() {
                let pos = (odometer_m - cum).clamp(0.0, len.max(0.0));
                return Some((eid.clone(), pos));
            }
            cum = end;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_core::{EdgeId, NodeId};
    use openrailsrs_track::{Edge, Node, NodeKind, TrackGraph};

    fn two_edge_graph() -> TrackGraph {
        let mut g = TrackGraph::new();
        for (id, x) in [("a", 0.0), ("b", 100.0), ("c", 250.0)] {
            g.insert_node(Node {
                id: NodeId(id.into()),
                kind: NodeKind::Plain,
                x_m: x,
                y_m: 0.0,
            })
            .unwrap();
        }
        g.insert_edge(Edge {
            id: EdgeId("e1".into()),
            from: NodeId("a".into()),
            to: NodeId("b".into()),
            length_m: 100.0,
            speed_limit_mps: 30.0,
            grade_percent: 0.0,
        })
        .unwrap();
        g.insert_edge(Edge {
            id: EdgeId("e2".into()),
            from: NodeId("b".into()),
            to: NodeId("c".into()),
            length_m: 150.0,
            speed_limit_mps: 30.0,
            grade_percent: 0.0,
        })
        .unwrap();
        g
    }

    #[test]
    fn position_at_odometer_maps_to_edge_and_offset() {
        let g = two_edge_graph();
        let path = vec!["e1".into(), "e2".into()];
        let pd = PathData::from_path(&path, &g);
        let (eid, pos) = PathData::position_at_odometer(&path, &pd.edges, 120.0).unwrap();
        assert_eq!(eid, "e2");
        assert!((pos - 20.0).abs() < 1e-6);
    }
}
