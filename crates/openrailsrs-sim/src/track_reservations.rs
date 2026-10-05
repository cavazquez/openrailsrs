//! Exclusive reservations of the next native signal block on a fixed route.
//! This is not OR's general junction/interlocking/deadlock dispatcher.
use crate::{LiveDriveSession, live_traffic::TrafficService, native_signals::TrackOccupancy};

pub(crate) fn overlaps(a: &TrackOccupancy, b: &TrackOccupancy) -> bool {
    a.edge == b.edge && a.end_m > b.start_m + 0.01 && a.start_m < b.end_m - 0.01
}
impl LiveDriveSession {
    fn next_block_request(&self) -> Vec<TrackOccupancy> {
        if self.arrived || self.native_signals.count() == 0 {
            return vec![];
        }
        let mut heads = vec![];
        let mut before = 0.;
        for (index, edge) in self.state.path_edges.iter().enumerate() {
            for signal in self.graph.signals_on_edge(edge) {
                if signal
                    .script
                    .as_ref()
                    .and_then(|s| s.native.as_ref())
                    .is_some_and(|n| n.function.eq_ignore_ascii_case("NORMAL"))
                {
                    heads.push((before + signal.position_m, index));
                }
            }
            before += self.path_data.edges[index].length_m;
        }
        heads.sort_by(|a, b| a.0.total_cmp(&b.0));
        let head = self.head_chainage_m();
        let Some((start, index)) = heads
            .iter()
            .copied()
            .find(|(position, _)| *position >= head)
        else {
            return vec![];
        };
        let horizon = (100. + self.velocity_mps().powi(2) / 1.4).max(500.);
        if start - head > horizon {
            return vec![];
        }
        let (end, end_index) = heads
            .iter()
            .copied()
            .find(|(p, _)| *p > start + 0.1)
            .unwrap_or((before, self.state.path_edges.len() - 1));
        // A reservation does not change or lock an authored turnout.
        for i in index..end_index {
            let edge = self.graph.edge(&self.state.path_edges[i]).unwrap();
            if let Some(openrailsrs_track::NodeKind::Switch {
                stem_edge,
                diverging_edge,
            }) = self.graph.node(&edge.to.0).map(|n| &n.kind)
            {
                let desired = match self.graph.switch_position(&edge.to.0).unwrap_or_default() {
                    openrailsrs_track::SwitchPosition::Straight => stem_edge,
                    openrailsrs_track::SwitchPosition::Diverging => diverging_edge,
                };
                if self.graph.outgoing_edges(&edge.to.0).contains(&desired.0)
                    && desired.0 != self.state.path_edges[i + 1]
                {
                    return vec![];
                }
            }
        }
        let mut result = vec![];
        before = 0.;
        for (edge, data) in self.state.path_edges.iter().zip(&self.path_data.edges) {
            let mut a = (start - before).max(0.);
            let mut b = (end - before).min(data.length_m);
            if b > a + 0.01 {
                if edge.ends_with("_r") {
                    (a, b) = (data.length_m - b, data.length_m - a);
                }
                result.push(TrackOccupancy {
                    edge: edge.strip_suffix("_r").unwrap_or(edge).into(),
                    start_m: a,
                    end_m: b,
                    owner: self.service_id.clone(),
                });
            }
            before += data.length_m;
        }
        result
    }
}

pub(crate) fn coordinate(
    player: &mut LiveDriveSession,
    services: &mut [TrafficService],
    footprint: &[TrackOccupancy],
) {
    let mut requests = vec![(
        player.service_id.clone(),
        player.next_block_request(),
        player.own_track_reservations.clone(),
    )];
    requests.extend(services.iter().filter(|s| s.departed).map(|s| {
        (
            s.id.clone(),
            s.session.next_block_request(),
            s.session.own_track_reservations.clone(),
        )
    }));
    // Retain an existing grant before considering a new request. New requests
    // use stable service IDs, independent of frame rate and iteration order.
    requests.sort_by(|a, b| (a.1 != a.2).cmp(&(b.1 != b.2)).then_with(|| a.0.cmp(&b.0)));
    let mut granted: Vec<TrackOccupancy> = vec![];
    for (owner, request, _) in requests {
        if request.iter().any(|segment| {
            footprint
                .iter()
                .chain(&granted)
                .any(|other| other.owner != owner && overlaps(segment, other))
        }) {
            continue;
        }
        granted.extend(request);
    }
    let apply = |session: &mut LiveDriveSession| {
        let own: Vec<_> = granted
            .iter()
            .filter(|r| r.owner == session.service_id)
            .cloned()
            .collect();
        let external: Vec<_> = granted
            .iter()
            .filter(|r| r.owner != session.service_id)
            .cloned()
            .collect();
        if own != session.own_track_reservations || external != session.external_track_reservations
        {
            session.own_track_reservations = own;
            session.external_track_reservations = external;
            session.native_signals.needs_refresh = true;
        }
    };
    apply(player);
    for service in services {
        apply(&mut service.session);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn single_track() -> (LiveDriveSession, crate::LiveTraffic, tempfile::TempDir) {
        let project = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let directory = tempfile::tempdir().unwrap();
        let mut track = "[route]\nid='single'\n[[nodes]]\nid='A'\nx_m=0.0\ny_m=0.0\n[[nodes]]\nid='B'\nx_m=1000.0\ny_m=0.0\n".to_string();
        for (id, from, to) in [("e1", "A", "B"), ("e1_r", "B", "A")] {
            track += &format!(
                "[[edges]]\nid='{id}'\nfrom='{from}'\nto='{to}'\nlength_m=1000.0\nspeed_limit_kmh=36.0\n"
            );
        }
        for (id, edge, position) in [
            ("f_entry", "e1", 100),
            ("f_exit", "e1", 900),
            ("r_entry", "e1_r", 100),
            ("r_exit", "e1_r", 900),
        ] {
            track += &format!(
                "[[signals]]\nid='{id}'\nedge_id='{edge}'\nposition_m={position}.0\naspect='clear'\n[signals.script.native]\nname='home'\nfunction='NORMAL'\nsource='if (!enabled || block_state()!=BLOCK_CLEAR || !route_set()) {{state=0;}} else {{state=7;}}'\n"
            );
        }
        std::fs::write(directory.path().join("track.toml"), track).unwrap();
        let mut scenario =
            openrailsrs_scenarios::load_scenario(project.join("examples/smoke/scenario.toml"))
                .unwrap();
        scenario.route.path = ".".into();
        scenario.route.start = "A".into();
        scenario.route.destination = "B".into();
        scenario.route.stops.clear();
        scenario.route.waypoints.clear();
        scenario.route.switches.clear();
        scenario.sound_regions.clear();
        scenario.extra_trains.clear();
        scenario.train.consist = project
            .join("examples/smoke/consists/freight.con")
            .to_string_lossy()
            .into();
        let player = LiveDriveSession::from_scenario(directory.path(), &scenario).unwrap();
        scenario.route.start = "B".into();
        scenario.route.destination = "A".into();
        let mut other = LiveDriveSession::from_scenario(directory.path(), &scenario).unwrap();
        other.service_id = "Z_opposite".into();
        let traffic = crate::LiveTraffic {
            services: vec![TrafficService {
                id: other.service_id.clone(),
                departure_s: 0.,
                session: other,
                departed: true,
            }],
        };
        (player, traffic, directory)
    }
    #[test]
    fn opposite_trains_cannot_reserve_the_same_clear_block_and_grant_survives_restore() {
        let (mut player, mut traffic, _directory) = single_track();
        traffic.synchronize_occupancy(&mut player);
        assert_eq!(player.own_track_reservations.len(), 1);
        assert_eq!(player.native_signal_aspect("f_entry"), Some(7));
        assert!(
            traffic.services[0]
                .session
                .own_track_reservations
                .is_empty()
        );
        assert_eq!(
            traffic.services[0].session.native_signal_aspect("r_entry"),
            Some(0)
        );
        let player_save = player.snapshot();
        let traffic_save = traffic.snapshot();
        player.own_track_reservations.clear();
        traffic.services[0]
            .session
            .external_track_reservations
            .clear();
        player.restore_snapshot(player_save).unwrap();
        traffic.restore_snapshot(traffic_save).unwrap();
        traffic.synchronize_occupancy(&mut player);
        assert_eq!(player.native_signal_aspect("f_entry"), Some(7));
        assert_eq!(
            traffic.services[0].session.native_signal_aspect("r_entry"),
            Some(0)
        );
        // Move the complete formation beyond the protected block: it releases
        // the grant and the opposite service may now claim that same interval.
        player.state.pos_on_edge_m = 1000.;
        traffic.synchronize_occupancy(&mut player);
        assert!(player.own_track_reservations.is_empty());
        assert_eq!(traffic.services[0].session.own_track_reservations.len(), 1);
        assert_eq!(
            traffic.services[0].session.native_signal_aspect("r_entry"),
            Some(7)
        );
    }
    #[test]
    fn corrupt_saved_reservation_is_rejected_before_state_changes() {
        let (mut player, mut traffic, _directory) = single_track();
        traffic.synchronize_occupancy(&mut player);
        let mut saved = player.snapshot();
        saved.track_reservations[0].end_m = f64::NAN;
        assert!(player.restore_snapshot(saved).is_err());
        assert_eq!(player.own_track_reservations[0].end_m, 900.);
    }
    #[test]
    fn canonical_intervals_conflict_in_both_directions_but_not_adjacent_blocks() {
        let a = TrackOccupancy {
            edge: "e7".into(),
            start_m: 100.,
            end_m: 200.,
            owner: "A".into(),
        };
        let mut b = a.clone();
        b.owner = "B".into();
        b.start_m = 150.;
        b.end_m = 250.;
        assert!(overlaps(&a, &b) && overlaps(&b, &a));
        b.start_m = 200.;
        assert!(!overlaps(&a, &b));
        b.edge = "e8".into();
        b.start_m = 100.;
        assert!(!overlaps(&a, &b));
    }
}
