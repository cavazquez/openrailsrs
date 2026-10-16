//! Native WorldSounds.GetTType boundary policy, evaluated per vehicle on its
//! actual directed path. `None` is OR's int.MaxValue: retain the previous type.
use crate::TrackGraph;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrackSoundBoundary {
    pub position_m: f64,
    pub track_type: u32,
    pub faces_forward: bool,
    pub track_item_id: u32,
}

/// The native neutral gap is zero, except for a gap shorter than ten metres
/// ahead, where retaining the previous type avoids a spurious short transition.
pub fn select_track_sound_type(
    previous: Option<(f64, u32)>,
    next: Option<(f64, u32)>,
) -> Option<u32> {
    match (previous, next) {
        (Some((_, a)), Some((_, b))) if a == b => Some(a),
        (Some(_), Some((distance, _))) if distance < 10. => None,
        (Some(_), Some(_)) => Some(0),
        (Some((_, a)), None) | (None, Some((_, a))) => Some(a),
        (None, None) => None,
    }
}

impl TrackGraph {
    pub fn track_sound_type_on_path(
        &self,
        path: &[String],
        edge: &str,
        position_m: f64,
        reverse: bool,
    ) -> Option<u32> {
        let index = path.iter().position(|id| id == edge)?;
        let length = self.edge(edge)?.length_m;
        if !position_m.is_finite() || !(0. ..=length).contains(&position_m) {
            return None;
        }
        let mut origin = position_m;
        for id in &path[..index] {
            origin += self.edge(id)?.length_m;
        }
        self.track_sound_type_at_chainage(path, origin, reverse)
    }
    /// A chainage selects the correct occurrence on paths that revisit a vector.
    pub fn track_sound_type_at_chainage(
        &self,
        path: &[String],
        origin: f64,
        reverse: bool,
    ) -> Option<u32> {
        if !origin.is_finite() || origin < 0. {
            return None;
        }
        let mut start = 0.;
        let mut previous: Option<(f64, u32, bool)> = None;
        let mut next: Option<(f64, u32, bool)> = None;
        for id in path {
            let length = self.edge(id)?.length_m;
            if start > origin + 8192. {
                break;
            }
            if start + length < origin - 8192. {
                start += length;
                continue;
            }
            if let Some(profile) = self.physics_profile(id) {
                for boundary in &profile.track_sound_regions {
                    let delta =
                        (start + boundary.position_m - origin) * if reverse { -1. } else { 1. };
                    let distance = delta.abs();
                    if distance > 8192. {
                        continue;
                    }
                    let ahead = delta >= 0.;
                    let faces_with_travel = boundary.faces_forward != reverse;
                    let applicable = if ahead {
                        !faces_with_travel
                    } else {
                        faces_with_travel
                    };
                    let target = if ahead { &mut next } else { &mut previous };
                    if target.is_none_or(|(d, _, old_applicable)| {
                        distance < d || (distance == d && applicable && !old_applicable)
                    }) {
                        *target = Some((distance, boundary.track_type, applicable));
                    }
                }
            }
            start += length;
        }
        select_track_sound_type(
            previous.map(|(d, t, _)| (d, t)),
            next.map(|(d, t, _)| (d, t)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Edge, EdgePhysicsProfile, Node, NodeKind};
    use openrailsrs_core::{EdgeId, NodeId};
    fn graph() -> (TrackGraph, Vec<String>) {
        let mut g = TrackGraph::new();
        for name in ["a", "b", "c"] {
            g.insert_node(Node {
                id: NodeId(name.into()),
                kind: NodeKind::Plain,
                x_m: 0.,
                y_m: 0.,
            })
            .unwrap();
        }
        for (id, from, to) in [("e1", "a", "b"), ("e2", "b", "c")] {
            g.insert_edge(Edge {
                id: EdgeId(id.into()),
                from: NodeId(from.into()),
                to: NodeId(to.into()),
                length_m: 100.,
                speed_limit_mps: 30.,
                grade_percent: 0.,
            })
            .unwrap();
        }
        for (id, positions, t) in [("e1", vec![20., 80.], 1), ("e2", vec![20., 80.], 2)] {
            g.set_physics_profile(
                id,
                EdgePhysicsProfile {
                    track_sound_regions: positions
                        .into_iter()
                        .enumerate()
                        .map(|(i, p)| TrackSoundBoundary {
                            position_m: p,
                            track_type: t,
                            faces_forward: i == 0,
                            track_item_id: i as u32,
                        })
                        .collect(),
                    ..Default::default()
                },
            );
        }
        (g, vec!["e1".into(), "e2".into()])
    }
    #[test]
    fn each_car_changes_at_its_own_boundary_in_both_directions() {
        let (g, path) = graph();
        assert_eq!(g.track_sound_type_on_path(&path, "e1", 50., false), Some(1));
        assert_eq!(g.track_sound_type_on_path(&path, "e2", 50., false), Some(2));
        assert_eq!(g.track_sound_type_on_path(&path, "e1", 99., false), Some(0));
        assert_eq!(g.track_sound_type_on_path(&path, "e2", 15., false), None);
        assert_eq!(g.track_sound_type_on_path(&path, "e2", 50., true), Some(2));
        assert_eq!(g.track_sound_type_on_path(&path, "e1", 85., true), None);
        assert_eq!(g.track_sound_type_on_path(&path, "e2", 1., true), Some(0));
    }
    #[test]
    fn absence_holds_and_a_single_boundary_extends_native_region() {
        assert_eq!(select_track_sound_type(None, None), None);
        assert_eq!(select_track_sound_type(None, Some((12., 3))), Some(3));
        assert_eq!(select_track_sound_type(Some((12., 3)), None), Some(3));
        assert_eq!(
            select_track_sound_type(Some((2., 3)), Some((10., 4))),
            Some(0)
        );
    }
    #[test]
    fn service_overlay_keeps_native_audio_boundaries() {
        let (g, path) = graph();
        let over = g.with_service_overlay(&TrackGraph::new()).unwrap();
        assert_eq!(
            over.track_sound_type_on_path(&path, "e1", 50., false),
            Some(1)
        );
    }
}
