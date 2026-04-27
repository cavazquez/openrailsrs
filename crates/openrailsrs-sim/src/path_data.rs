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
}
