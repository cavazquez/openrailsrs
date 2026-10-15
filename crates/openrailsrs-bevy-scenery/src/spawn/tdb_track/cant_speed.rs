//! Resolve native class/facing speed signs before compiling immutable rail banks.
use super::TrackVectorPath;
use bevy::math::DVec3;
use openrailsrs_formats::typed::cant_speeds::CantSpeedLimit;
use openrailsrs_formats::{TSectionCatalog, TrItemKind, TrackDbFile, TrackNodeKind};
use std::collections::HashMap;

pub struct NativeCantSpeedIndex {
    signs: HashMap<u32, Vec<CantSpeedLimit>>,
    default_branches: HashMap<u32, usize>,
}
impl NativeCantSpeedIndex {
    pub fn new(tdb: &TrackDbFile, catalog: &TSectionCatalog) -> Self {
        let mut signs = HashMap::new();
        for node in &tdb.nodes {
            let TrackNodeKind::Vector { item_ids, .. } = &node.kind else {
                continue;
            };
            let Some(path) = TrackVectorPath::new(node, Some(catalog)) else {
                continue;
            };
            let mut local = vec![];
            for id in item_ids {
                let Some(item) = tdb.item_by_id(*id) else {
                    continue;
                };
                let TrItemKind::SpeedPost { speed_mph } = item.kind else {
                    continue;
                };
                let flags = item.speed_post_flags;
                if flags.is_some_and(|f| f & 2 == 0) {
                    continue;
                } // mileposts and advance warnings do not impose a limit
                let resume = flags.is_some_and(|f| f & 3 == 3);
                if !resume && (speed_mph <= 0. || !speed_mph.is_finite()) {
                    continue;
                }
                let passenger = flags.is_none_or(|f| f & ((1 << 5) | (1 << 7)) != 0);
                let freight = flags.is_none_or(|f| f & ((1 << 6) | (1 << 7)) != 0);
                let direction = item.speed_post_angle_rad.map_or(0, |angle| {
                    let pose = path.pose_in_frame(item.distance_m, DVec3::ZERO);
                    let heading = -pose.yaw_deg.to_radians();
                    if native_reverse(heading, angle as f32) {
                        -1
                    } else {
                        1
                    }
                });
                let speed = if resume {
                    f32::MAX
                } else {
                    (speed_mph * 0.44704) as f32
                };
                local.push(CantSpeedLimit {
                    position_m: item.distance_m,
                    direction,
                    passenger_mps: passenger.then_some(speed),
                    freight_mps: freight.then_some(speed),
                });
            }
            local.sort_by(|a, b| a.position_m.total_cmp(&b.position_m));
            signs.insert(node.id, local);
        }
        let default_branches = tdb
            .nodes
            .iter()
            .filter_map(|n| {
                let shape = catalog.shapes.get(&n.junction_shape_index?)?;
                Some((n.id, shape.main_route.unwrap_or(0) as usize))
            })
            .collect();
        Self {
            signs,
            default_branches,
        }
    }

    #[cfg(test)]
    fn backing_speed(
        &self,
        tdb: &TrackDbFile,
        first: u32,
        direction: i8,
        passenger: bool,
    ) -> Option<f32> {
        openrailsrs_formats::typed::cant_speeds::backing_speed(
            tdb,
            &self.signs,
            &self.default_branches,
            first,
            direction,
            passenger,
        )
    }
    pub fn limits_for_node(
        &self,
        tdb: &TrackDbFile,
        node: u32,
        length_m: f64,
    ) -> Vec<CantSpeedLimit> {
        openrailsrs_formats::typed::cant_speeds::limits_for_node(
            tdb,
            &self.signs,
            &self.default_branches,
            node,
            length_m,
        )
    }
}
fn native_reverse(heading: f32, angle: f32) -> bool {
    let wrap = |v: f32| {
        let mut v = v % std::f32::consts::TAU;
        if v <= -std::f32::consts::PI {
            v += std::f32::consts::TAU;
        } else if v > std::f32::consts::PI {
            v -= std::f32::consts::TAU;
        }
        v
    };
    let backward = wrap(heading + std::f32::consts::PI);
    let delta =
        wrap((f64::from(backward) - (std::f64::consts::FRAC_PI_2 - f64::from(angle))) as f32);
    f64::from(delta.abs()) < std::f64::consts::FRAC_PI_2
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::{TrPinRef, TrackDbNode};
    #[test]
    fn backing_speed_follows_authored_main_route_and_keeps_vehicle_class() {
        let vector = |id, pins| TrackDbNode {
            id,
            junction_shape_index: None,
            position: None,
            pin_refs: vec![],
            kind: TrackNodeKind::Vector {
                pins,
                length_m: 100.,
                speed_limit_mps: 40.,
                item_ids: vec![],
                sections: vec![],
                geometry: None,
            },
        };
        let junction = TrackDbNode {
            id: 2,
            junction_shape_index: Some(17),
            position: None,
            pin_refs: vec![],
            kind: TrackNodeKind::Junction {
                pins: [1, 3, 4]
                    .into_iter()
                    .enumerate()
                    .map(|(branch, node_id)| TrPinRef {
                        node_id,
                        branch_index: branch as u8,
                    })
                    .collect(),
            },
        };
        let end = TrackDbNode {
            id: 5,
            junction_shape_index: None,
            position: None,
            pin_refs: vec![],
            kind: TrackNodeKind::End,
        };
        let tdb = TrackDbFile {
            nodes: vec![
                vector(1, (2, 5)),
                junction,
                vector(3, (5, 2)),
                vector(4, (5, 2)),
                end,
            ],
            items: vec![],
        };
        let sign = |speed| CantSpeedLimit {
            position_m: 50.,
            direction: 1,
            passenger_mps: Some(speed),
            freight_mps: Some(speed * 0.5),
        };
        let index = NativeCantSpeedIndex {
            signs: HashMap::from([(3, vec![sign(20.)]), (4, vec![sign(40.)])]),
            default_branches: HashMap::from([(2, 1)]),
        };
        assert_eq!(index.backing_speed(&tdb, 1, 1, true), Some(40.));
        assert_eq!(index.backing_speed(&tdb, 1, 1, false), Some(20.));
        assert_eq!(index.backing_speed(&tdb, 1, -1, false), None);
    }
}
