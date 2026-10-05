//! Deterministic shared dispatcher: atomic block/point grants, tail release and
//! forward alternate routing. Complex OR timetable reversal remains separate.
use crate::{LiveDriveSession, live_traffic::TrafficService, native_signals::TrackOccupancy};
use openrailsrs_track::{NodeKind, SwitchPosition, TrackGraph};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwitchLock {
    pub node: String,
    pub position: SwitchPosition,
    pub owner: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DispatcherStatus {
    pub own_locks: Vec<SwitchLock>,
    pub other_locks: Vec<SwitchLock>,
    pub waiting_for: Vec<String>,
    pub protected_signal: Option<String>,
    pub wait_since_s: Option<f64>,
    pub last_route_search_s: f64,
    pub deadlock: bool,
    pub reroutes: u64,
}
pub(crate) fn overlaps(a: &TrackOccupancy, b: &TrackOccupancy) -> bool {
    a.edge == b.edge && a.end_m > b.start_m + 0.01 && a.start_m < b.end_m - 0.01
}
fn base(id: &str) -> &str {
    id.strip_suffix("_r").unwrap_or(id)
}
fn point_touched(
    graph: &TrackGraph,
    node: &str,
    owner: Option<&str>,
    footprint: &[TrackOccupancy],
) -> bool {
    footprint.iter().any(|p| {
        if owner.is_some_and(|o| p.owner != o && !p.owner.starts_with(&format!("{o} ·"))) {
            return false;
        }
        let (edge, reversed) = if let Some(edge) = graph.edge(&p.edge) {
            (edge, false)
        } else if let Some(edge) = graph.edge(&format!("{}_r", p.edge)) {
            (edge, true)
        } else {
            return false;
        };
        let point = if edge.from.0 == node {
            Some(if reversed { edge.length_m } else { 0. })
        } else if edge.to.0 == node {
            Some(if reversed { 0. } else { edge.length_m })
        } else {
            None
        };
        point.is_some_and(|at| p.start_m <= at + 5. && p.end_m >= at - 5.)
    })
}

fn point_ahead(session: &LiveDriveSession, node: &str) -> bool {
    let mut before = 0.;
    for (id, data) in session
        .state
        .path_edges
        .iter()
        .zip(&session.path_data.edges)
    {
        before += data.length_m;
        if session.graph.edge(id).is_some_and(|e| e.to.0 == node)
            && before >= session.head_chainage_m() - 0.01
        {
            return true;
        }
    }
    false
}

fn point_position(
    graph: &TrackGraph,
    node: &str,
    incoming: &str,
    outgoing: &str,
) -> Option<SwitchPosition> {
    let NodeKind::Switch {
        stem_edge,
        diverging_edge,
    } = &graph.node(node)?.kind
    else {
        return None;
    };
    if outgoing == stem_edge.0 || base(incoming) == base(&stem_edge.0) {
        Some(SwitchPosition::Straight)
    } else if outgoing == diverging_edge.0 || base(incoming) == base(&diverging_edge.0) {
        Some(SwitchPosition::Diverging)
    } else {
        None
    }
}
#[derive(Default)]
struct Request {
    owner: String,
    segments: Vec<TrackOccupancy>,
    locks: Vec<SwitchLock>,
    signal: Option<String>,
    retained: bool,
    fixed_blocks: bool,
    wait_since: f64,
}
impl LiveDriveSession {
    fn next_block_request(&self) -> Request {
        let mut request = Request {
            owner: self.service_id.clone(),
            wait_since: self.dispatcher.wait_since_s.unwrap_or(f64::MAX),
            ..Default::default()
        };
        if self.arrived {
            return request;
        }
        let mut heads = vec![];
        let mut total = 0.;
        for (i, edge) in self.state.path_edges.iter().enumerate() {
            for signal in self.graph.signals_on_edge(edge) {
                if signal
                    .script
                    .as_ref()
                    .and_then(|s| s.native.as_ref())
                    .map_or(self.native_signals.count() == 0, |n| {
                        n.function.eq_ignore_ascii_case("NORMAL")
                    })
                {
                    heads.push((total + signal.position_m, i, signal.id.clone()));
                }
            }
            total += self.path_data.edges[i].length_m;
        }
        heads.sort_by(|a, b| a.0.total_cmp(&b.0));
        request.fixed_blocks = !heads.is_empty();
        let head = self.head_chainage_m();
        let horizon = (100. + self.velocity_mps().powi(2) / 1.4).max(500.);
        let (start, end, end_index) =
            if let Some((start, _, signal)) = heads.iter().find(|h| h.0 >= head) {
                if start - head > horizon {
                    return request;
                }
                request.signal = Some(signal.clone());
                let end = heads.iter().find(|h| h.0 > *start + 0.1);
                (
                    *start,
                    end.map_or(total, |h| h.0),
                    end.map_or(self.state.path_edges.len() - 1, |h| h.1),
                )
            } else if self.native_signals.count() == 0 {
                // Graph examples without native heads still interlock junctions.
                let mut end_index = self.state.edge_index;
                let mut end = head - self.state.pos_on_edge_m;
                while end_index < self.state.path_edges.len() {
                    end += self.path_data.edges[end_index].length_m;
                    if end >= head + horizon || end_index + 1 == self.state.path_edges.len() {
                        break;
                    }
                    end_index += 1;
                }
                (head, end, end_index)
            } else {
                return request;
            };
        for i in self.state.edge_index..end_index.min(self.state.path_edges.len().saturating_sub(1))
        {
            let incoming = &self.state.path_edges[i];
            let outgoing = &self.state.path_edges[i + 1];
            let node = &self.graph.edge(incoming).unwrap().to.0;
            if let Some(position) = point_position(&self.graph, node, incoming, outgoing) {
                request.locks.push(SwitchLock {
                    node: node.clone(),
                    position,
                    owner: self.service_id.clone(),
                });
            }
        }
        let mut before = 0.;
        for (edge, data) in self.state.path_edges.iter().zip(&self.path_data.edges) {
            let mut a = (start - before).max(0.);
            let mut b = (end - before).min(data.length_m);
            if b > a + 0.01 {
                if edge.ends_with("_r") {
                    (a, b) = (data.length_m - b, data.length_m - a);
                }
                request.segments.push(TrackOccupancy {
                    edge: base(edge).into(),
                    start_m: a,
                    end_m: b,
                    owner: self.service_id.clone(),
                });
            }
            before += data.length_m;
        }
        request.retained =
            !request.segments.is_empty() && request.segments == self.own_track_reservations;
        request
    }

    /// Seek a forward diversion through every remaining station, avoiding other
    /// footprints and grants. Never reverse into the train's occupied prefix.
    fn alternate_route(
        &mut self,
        footprint: &[TrackOccupancy],
        reserved: &[TrackOccupancy],
        locks: &[SwitchLock],
    ) -> bool {
        if self.formation.parked_head_chainage_m.is_some() {
            return false;
        }
        let Some(current) = self.current_edge_id().and_then(|id| self.graph.edge(id)) else {
            return false;
        };
        let mut blocked: HashSet<String> = footprint
            .iter()
            .chain(reserved)
            .filter(|p| p.owner != self.service_id)
            .map(|p| p.edge.clone())
            .collect();
        blocked.extend(
            self.state.path_edges[..=self.state.edge_index]
                .iter()
                .map(|e| base(e).into()),
        );
        let mut path = self.state.path_edges[..=self.state.edge_index].to_vec();
        let mut node = current.to.0.clone();
        let mut targets: Vec<_> = self
            .gameplay
            .stop_targets
            .iter()
            .skip(self.gameplay.next_stop_idx)
            .map(|s| s.node_id.clone())
            .collect();
        if targets.last() != Some(&self.gameplay.destination_node) {
            targets.push(self.gameplay.destination_node.clone());
        }
        for target in targets {
            let Some(tail) = diversion_path(
                &self.graph,
                &node,
                &target,
                &blocked,
                locks,
                &self.service_id,
            ) else {
                return false;
            };
            for e in &tail {
                blocked.insert(base(e).into());
            }
            path.extend(tail);
            node = target;
        }
        if path == self.state.path_edges {
            return false;
        }
        if self.adopt_dispatch_path(path).is_err() {
            return false;
        }
        self.dispatcher.reroutes += 1;
        true
    }
}
fn diversion_path(
    graph: &TrackGraph,
    start: &str,
    target: &str,
    blocked: &HashSet<String>,
    locks: &[SwitchLock],
    owner: &str,
) -> Option<Vec<String>> {
    if start == target {
        return Some(vec![]);
    }
    let mut queue = VecDeque::from([start.to_owned()]);
    let mut parent = HashMap::from([(start.to_owned(), (String::new(), String::new()))]);
    while let Some(node) = queue.pop_front() {
        let mut outgoing = graph.outgoing_edges(&node).to_vec();
        outgoing.sort();
        for id in outgoing {
            if blocked.contains(base(&id)) {
                continue;
            }
            let edge = graph.edge(&id)?;
            if parent.contains_key(&edge.to.0) {
                continue;
            }
            if locks.iter().any(|l| {
                l.node == node
                    && l.owner != owner
                    && point_position(graph, &node, "", &id).is_some_and(|p| p != l.position)
            }) {
                continue;
            }
            parent.insert(edge.to.0.clone(), (node.clone(), id));
            if edge.to.0 == target {
                let mut at = target;
                let mut result = vec![];
                while at != start {
                    let (prev, edge) = parent.get(at)?;
                    result.push(edge.clone());
                    at = prev;
                }
                result.reverse();
                return Some(result);
            }
            queue.push_back(edge.to.0.clone());
        }
    }
    None
}
fn cycle_from(owner: &str, waits: &HashMap<String, Vec<String>>) -> bool {
    let mut queue = VecDeque::from(waits.get(owner).cloned().unwrap_or_default());
    let mut seen = HashSet::new();
    while let Some(next) = queue.pop_front() {
        if next == owner {
            return true;
        }
        if seen.insert(next.clone()) {
            queue.extend(waits.get(&next).into_iter().flatten().cloned());
        }
    }
    false
}

pub(crate) fn coordinate(
    player: &mut LiveDriveSession,
    services: &mut [TrafficService],
    footprint: &[TrackOccupancy],
) {
    let now = player.state.time_s();
    let mut requests = vec![player.next_block_request()];
    requests.extend(
        services
            .iter()
            .filter(|s| s.departed)
            .map(|s| s.session.next_block_request()),
    );
    requests.sort_by(|a, b| {
        b.retained
            .cmp(&a.retained)
            .then_with(|| a.wait_since.total_cmp(&b.wait_since))
            .then_with(|| a.owner.cmp(&b.owner))
    });
    let fixed_owners: HashSet<_> = requests
        .iter()
        .filter(|r| r.fixed_blocks)
        .map(|r| r.owner.clone())
        .collect();
    let mut granted = vec![];
    let mut locked = vec![];
    for session in
        std::iter::once(&*player).chain(services.iter().filter(|s| s.departed).map(|s| &s.session))
    {
        // A consumed grant/point remains protected until the complete tail (or
        // parked section) clears it. Approaching grants are reconsidered below.
        for lock in &session.dispatcher.own_locks {
            if point_touched(
                &session.graph,
                &lock.node,
                Some(&session.service_id),
                footprint,
            ) || point_ahead(session, &lock.node)
                && point_touched(
                    &session.graph,
                    &lock.node,
                    Some(&session.service_id),
                    &session.own_track_reservations,
                )
                && session.own_track_reservations.iter().any(|old| {
                    footprint
                        .iter()
                        .any(|p| p.owner == session.service_id && overlaps(p, old))
                })
            {
                locked.push(lock.clone());
            }
        }
        for old in &session.own_track_reservations {
            // Signal blocks remain protected beneath the tail. A moving
            // lookahead on graphs without signals is replaced each quantum;
            // retaining it would accumulate a new overlapping grant per tick.
            if fixed_owners.contains(&session.service_id)
                && footprint
                    .iter()
                    .any(|p| p.owner == session.service_id && overlaps(p, old))
            {
                granted.push(old.clone());
            }
        }
    }
    let mut waits = HashMap::new();
    let mut protected = HashMap::new();
    for request in requests {
        protected.insert(request.owner.clone(), request.signal);
        let mut conflicts = BTreeSet::new();
        for s in &request.segments {
            for other in footprint.iter().chain(&granted) {
                if other.owner != request.owner && overlaps(s, other) {
                    conflicts.insert(other.owner.clone());
                }
            }
        }
        for l in &request.locks {
            for other in &locked {
                if other.node == l.node && other.owner != request.owner {
                    conflicts.insert(other.owner.clone());
                }
            }
            if player
                .graph
                .switch_position(&l.node)
                .is_some_and(|p| p != l.position)
                && point_touched(&player.graph, &l.node, None, footprint)
            {
                conflicts.extend(
                    footprint
                        .iter()
                        .filter(|p| {
                            point_touched(&player.graph, &l.node, Some(&p.owner), footprint)
                        })
                        .map(|p| p.owner.clone()),
                );
            }
        }
        if !conflicts.is_empty() {
            waits.insert(request.owner, conflicts.into_iter().collect());
            continue;
        }
        for s in request.segments {
            if !granted.contains(&s) {
                granted.push(s);
            }
        }
        for l in request.locks {
            if !locked.contains(&l) {
                locked.push(l);
            }
        }
    }
    let apply = |session: &mut LiveDriveSession| {
        let was_waiting = !session.dispatcher.waiting_for.is_empty();
        let waiting = waits.get(&session.service_id).cloned().unwrap_or_default();
        session.dispatcher.deadlock = cycle_from(&session.service_id, &waits);
        session.dispatcher.wait_since_s = if waiting.is_empty() {
            None
        } else {
            Some(session.dispatcher.wait_since_s.unwrap_or(now))
        };
        session.dispatcher.waiting_for = waiting;
        session.dispatcher.protected_signal = protected.get(&session.service_id).cloned().flatten();
        session.dispatcher.own_locks = locked
            .iter()
            .filter(|l| l.owner == session.service_id)
            .cloned()
            .collect();
        session.dispatcher.other_locks = locked
            .iter()
            .filter(|l| l.owner != session.service_id)
            .cloned()
            .collect();
        for l in &locked {
            let _ = session.graph.set_switch(&l.node, l.position);
        }
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
        if own != session.own_track_reservations
            || external != session.external_track_reservations
            || was_waiting
            || !session.dispatcher.waiting_for.is_empty()
        {
            session.own_track_reservations = own;
            session.external_track_reservations = external;
            session.native_signals.needs_refresh = true;
        }
        if session
            .dispatcher
            .wait_since_s
            .is_some_and(|since| now - since >= 5.)
            && now - session.dispatcher.last_route_search_s >= 5.
        {
            session.dispatcher.last_route_search_s = now;
            if session.alternate_route(footprint, &granted, &locked) {
                session.native_signals.needs_refresh = true;
            }
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
    fn passing_loop() -> (LiveDriveSession, crate::LiveTraffic, tempfile::TempDir) {
        let project = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = tempfile::tempdir().unwrap();
        let mut text = String::from("[route]\nid='passing'\n");
        for (id, x, y) in [
            ("A", 0, 0),
            ("J", 200, 0),
            ("L", 700, 100),
            ("K", 1200, 0),
            ("B", 1400, 0),
        ] {
            text += &format!("[[nodes]]\nid='{id}'\nx_m={x}.0\ny_m={y}.0\n");
            if id == "J" {
                text += "kind={switch={stem_edge='main',diverging_edge='loop_a',default_position='straight'}}\n";
            }
        }
        for (id, from, to, length) in [
            ("approach", "A", "J", 200),
            ("main", "J", "K", 1000),
            ("main_r", "K", "J", 1000),
            ("loop_a", "J", "L", 510),
            ("loop_b", "L", "K", 510),
            ("exit", "K", "B", 200),
        ] {
            text += &format!(
                "[[edges]]\nid='{id}'\nfrom='{from}'\nto='{to}'\nlength_m={length}.0\nspeed_limit_kmh=36.0\n"
            );
        }
        for (id, edge, pos) in [("entry", "approach", 150), ("out", "exit", 100)] {
            text += &format!(
                "[[signals]]\nid='{id}'\nedge_id='{edge}'\nposition_m={pos}.0\naspect='clear'\n[signals.script.native]\nname='home'\nfunction='NORMAL'\nsource='if (!enabled || block_state()!=BLOCK_CLEAR || !route_set()) {{state=0;}} else {{state=7;}}'\n"
            );
        }
        std::fs::write(dir.path().join("track.toml"), text).unwrap();
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
        let mut player = LiveDriveSession::from_scenario(dir.path(), &scenario).unwrap();
        player.state.pos_on_edge_m = 100.;
        scenario.route.start = "K".into();
        scenario.route.destination = "J".into();
        let mut other = LiveDriveSession::from_scenario(dir.path(), &scenario).unwrap();
        other.service_id = "Opposite".into();
        other.state.pos_on_edge_m = 500.;
        let traffic = crate::LiveTraffic {
            services: vec![TrafficService {
                id: other.service_id.clone(),
                session: other,
                departure_s: 0.,
                departed: true,
            }],
        };
        (player, traffic, dir)
    }
    #[test]
    fn blocked_service_diverts_through_passing_loop_and_locks_points_atomically() {
        let (mut player, mut traffic, _dir) = passing_loop();
        traffic.synchronize_occupancy(&mut player);
        assert_eq!(player.native_signal_aspect("entry"), Some(0));
        assert_eq!(player.dispatcher.waiting_for, vec!["Opposite"]);
        player.state.time = openrailsrs_core::SimTime(6.);
        traffic.synchronize_occupancy(&mut player);
        assert_eq!(
            player.state.path_edges,
            vec!["approach", "loop_a", "loop_b", "exit"]
        );
        traffic.synchronize_occupancy(&mut player);
        assert_eq!(
            player.graph.switch_position("J"),
            Some(SwitchPosition::Diverging)
        );
        assert_eq!(
            traffic.services[0].session.graph.switch_position("J"),
            Some(SwitchPosition::Diverging)
        );
        assert_eq!(player.native_signal_aspect("entry"), Some(7));
        assert!(player.dispatcher.waiting_for.is_empty());
        assert_eq!(player.dispatcher.reroutes, 1);
        assert!(
            traffic
                .dispatch_switch(&mut player, "J")
                .unwrap_err()
                .contains("reservado")
        );
        let saved = player.snapshot();
        player.dispatcher.own_locks.clear();
        player.restore_snapshot(saved).unwrap();
        traffic.synchronize_occupancy(&mut player);
        assert_eq!(player.dispatcher.own_locks.len(), 1);
        // Front is past the point; a following car still spans its 5 m clearance.
        player.state.edge_index = 1;
        player.state.pos_on_edge_m = 5.;
        traffic.synchronize_occupancy(&mut player);
        assert_eq!(player.dispatcher.own_locks.len(), 1);
        player.state.pos_on_edge_m = player.formation.length_m() + 10.;
        traffic.synchronize_occupancy(&mut player);
        assert!(player.dispatcher.own_locks.is_empty());
    }
    #[test]
    fn dispatcher_reports_cycles_without_releasing_occupied_points() {
        let waits = HashMap::from([
            ("A".into(), vec!["B".into()]),
            ("B".into(), vec!["A".into()]),
            ("C".into(), vec!["A".into()]),
        ]);
        assert!(cycle_from("A", &waits));
        assert!(cycle_from("B", &waits));
        assert!(!cycle_from("C", &waits));
    }
    #[test]
    fn shared_restore_rejects_two_owners_of_one_grant_or_point() {
        let (mut player, mut traffic, _dir) = passing_loop();
        traffic.synchronize_occupancy(&mut player);
        player.state.time = openrailsrs_core::SimTime(6.);
        traffic.synchronize_occupancy(&mut player);
        traffic.synchronize_occupancy(&mut player);
        let saved = player.snapshot();
        let mut other = traffic.services[0].session.snapshot();
        crate::SessionSnapshot::validate_shared_dispatcher([&saved, &other]).unwrap();
        let original = other.clone();
        let mut stolen = saved.track_reservations[0].clone();
        stolen.owner = "Opposite".into();
        other.track_reservations.push(stolen);
        assert!(crate::SessionSnapshot::validate_shared_dispatcher([&saved, &other]).is_err());
        other = original;
        let mut stolen = saved.dispatcher.own_locks[0].clone();
        stolen.owner = "Opposite".into();
        other.dispatcher.own_locks.push(stolen);
        assert!(crate::SessionSnapshot::validate_shared_dispatcher([&saved, &other]).is_err());
        assert_eq!(
            player.dispatcher.own_locks[0].owner,
            saved.dispatcher.own_locks[0].owner
        );
    }
    #[test]
    fn stationary_train_far_from_points_does_not_lock_the_whole_adjacent_edge() {
        let (player, traffic, _dir) = passing_loop();
        let footprint = traffic.services[0].session.own_track_occupancy();
        assert!(!point_touched(&player.graph, "J", None, &footprint));
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
