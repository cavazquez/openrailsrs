//! Pre-computed physical edge data for a specific train path.
//!
//! Building a [`PathData`] once from the path and the graph replaces repeated
//! `HashMap::get` calls inside the hot simulation loop with direct `Vec` indexing,
//! avoiding string hashing on every tick.

use openrailsrs_track::TrackGraph;

/// Physical data for a single edge in the path — everything `physics::step` needs.
#[derive(Clone, Debug, Default)]
pub struct PathEdgeData {
    pub profile: openrailsrs_track::EdgePhysicsProfile,
    pub length_m: f64,
    pub speed_limit_mps: f64,
    pub grade_percent: f64,
}
impl PathEdgeData {
    pub fn speed_limit_at(&self, position: f64) -> f64 {
        let index = self
            .profile
            .speed_posts
            .partition_point(|p| p.position_m <= position);
        if index == 0 {
            self.speed_limit_mps
        } else {
            self.profile.speed_posts[index - 1].speed_limit_kmh / 3.6
        }
    }
    pub fn grade_at(&self, position: f64) -> f64 {
        let index = self
            .profile
            .grades
            .partition_point(|p| p.position_m <= position);
        if index == 0 {
            self.grade_percent
        } else {
            self.profile.grades[index - 1].grade_percent
        }
    }
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
        let mut inherited: Option<f64> = None;
        let edges = path_edges
            .iter()
            .map(|eid| {
                graph
                    .edge(eid)
                    .map(|e| {
                        let mut profile = graph.physics_profile(eid).cloned().unwrap_or_default();
                        let next_inherited =
                            profile.speed_posts.last().map(|p| p.speed_limit_kmh / 3.6);
                        for post in &mut profile.speed_posts {
                            post.speed_limit_kmh =
                                post.speed_limit_kmh.min(e.speed_limit_mps * 3.6);
                        }
                        let initial = inherited
                            .map_or(e.speed_limit_mps, |limit| limit.min(e.speed_limit_mps));
                        if let Some(limit) = next_inherited {
                            inherited = Some(limit);
                        }
                        PathEdgeData {
                            profile,
                            length_m: e.length_m,
                            speed_limit_mps: initial,
                            grade_percent: e.grade_percent,
                        }
                    })
                    .unwrap_or(PathEdgeData {
                        profile: Default::default(),
                        length_m: 0.0,
                        speed_limit_mps: 55.0 / 3.6,
                        grade_percent: 0.0,
                    })
            })
            .collect();
        Self { edges }
    }

    /// Increases apply only after the complete train has cleared the old limit.
    pub fn minimum_speed_limit_between(&self, start: f64, end: f64) -> f64 {
        let mut before = 0.0;
        let mut limit = f64::INFINITY;
        for edge in &self.edges {
            let lo = (start - before).max(0.0);
            let hi = (end - before).min(edge.length_m);
            if hi >= lo && end >= before && start <= before + edge.length_m {
                limit = limit.min(edge.speed_limit_at(lo));
                for post in &edge.profile.speed_posts {
                    if (lo..=hi).contains(&post.position_m) {
                        limit = limit.min(post.speed_limit_kmh / 3.6);
                    }
                }
            }
            before += edge.length_m;
        }
        limit
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

    /// Absolute path chainage for an edge index and an offset on that edge.
    ///
    /// Unlike the train odometer, this includes any scenario `start_offset_m`.
    pub fn chainage_at_edge_position(&self, edge_index: usize, pos_on_edge_m: f64) -> f64 {
        let before: f64 = self
            .edges
            .iter()
            .take(edge_index)
            .map(|edge| edge.length_m.max(0.0))
            .sum();
        let on_edge = self
            .edges
            .get(edge_index)
            .map(|edge| pos_on_edge_m.clamp(0.0, edge.length_m.max(0.0)))
            .unwrap_or(0.0);
        before + on_edge
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

    #[test]
    fn chainage_at_edge_position_includes_prior_edges() {
        let g = two_edge_graph();
        let path = vec!["e1".into(), "e2".into()];
        let pd = PathData::from_path(&path, &g);
        assert!((pd.chainage_at_edge_position(1, 20.0) - 120.0).abs() < 1e-6);
    }
    #[test]
    fn speed_posts_apply_at_position_and_increases_wait_for_the_tail() {
        use openrailsrs_track::{EdgePhysicsProfile, PositionGrade, PositionSpeedLimit};
        let mut g = two_edge_graph();
        g.set_physics_profile(
            "e1",
            EdgePhysicsProfile {
                speed_posts: vec![
                    PositionSpeedLimit {
                        position_m: 30.0,
                        speed_limit_kmh: 36.0,
                    },
                    PositionSpeedLimit {
                        position_m: 70.0,
                        speed_limit_kmh: 72.0,
                    },
                ],
                grades: vec![
                    PositionGrade {
                        position_m: 0.0,
                        grade_percent: 1.0,
                    },
                    PositionGrade {
                        position_m: 50.0,
                        grade_percent: -0.5,
                    },
                ],
            },
        );
        let pd = PathData::from_path(&["e1".into(), "e2".into()], &g);
        assert_eq!(pd.edges[0].speed_limit_at(29.0), 30.0);
        assert_eq!(pd.edges[0].speed_limit_at(30.0), 10.0);
        assert_eq!(pd.edges[1].speed_limit_at(0.0), 20.0);
        assert_eq!(pd.minimum_speed_limit_between(60.0, 110.0), 10.0);
        assert_eq!(pd.minimum_speed_limit_between(70.0, 120.0), 20.0);
        assert_eq!(pd.edges[0].grade_at(49.0), 1.0);
        assert_eq!(pd.edges[0].grade_at(50.0), -0.5);
    }

    #[test]
    fn temporary_edge_cap_does_not_leak_into_inherited_speed_post() {
        use openrailsrs_track::{EdgePhysicsProfile, PositionSpeedLimit};
        let mut g = two_edge_graph();
        g.cap_edge_speed_limit_kmh("e1", 36.0);
        g.set_physics_profile(
            "e1",
            EdgePhysicsProfile {
                speed_posts: vec![PositionSpeedLimit {
                    position_m: 0.0,
                    speed_limit_kmh: 72.0,
                }],
                ..Default::default()
            },
        );
        let pd = PathData::from_path(&["e1".into(), "e2".into()], &g);
        assert_eq!(pd.edges[0].speed_limit_at(1.0), 10.0);
        assert_eq!(pd.edges[1].speed_limit_at(1.0), 20.0);
    }
}
