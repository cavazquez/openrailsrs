//! Resolve activity station records onto their original paired TDB platforms.

use std::collections::HashMap;

use openrailsrs_formats::{ActivityFile, TrItemKind, TrackDbFile, TrackNodeKind};
use openrailsrs_route::path::{edge_path_ignoring_switches, edge_path_via_waypoints};
use openrailsrs_scenarios::model::{RouteSection, StopDef, SwitchDef, SwitchPositionDef};
use openrailsrs_track::{NodeKind, SwitchPosition, TrackGraph};

use crate::MstsError;

pub(crate) fn apply(
    activity: &ActivityFile,
    tdb: &TrackDbFile,
    graph: &TrackGraph,
    route: &mut RouteSection,
) -> Result<(), MstsError> {
    let mut graph = graph.clone();
    for switch in &route.switches {
        let position = match switch.position {
            SwitchPositionDef::Straight => SwitchPosition::Straight,
            SwitchPositionDef::Diverging => SwitchPosition::Diverging,
        };
        graph
            .set_switch(&switch.node, position)
            .map_err(|e| MstsError::msg(e.to_string()))?;
    }
    let mut edges = if route.waypoints.len() >= 2 {
        edge_path_via_waypoints(&graph, &route.waypoints)
    } else {
        edge_path_ignoring_switches(&graph, &route.start, &route.destination)
    }
    .map_err(|e| MstsError::msg(e.to_string()))?;
    let items: HashMap<_, _> = tdb.items.iter().map(|item| (item.id, item)).collect();
    let mut hosts = HashMap::new();
    for node in &tdb.nodes {
        if let TrackNodeKind::Vector { item_ids, .. } = &node.kind {
            for id in item_ids {
                if hosts.insert(*id, node.id).is_some() {
                    return Err(MstsError::msg(format!(
                        "Ambiguous native platform host for item {id}"
                    )));
                }
            }
        }
    }
    let mut stops = Vec::new();
    let mut boarded = 0u32;
    let mut previous_chainage = -1.0;
    let start_chainage = route.start_offset_m.unwrap_or(0.0);
    for (index, record) in activity.player_stops.iter().enumerate() {
        let id = record.platform_start_id;
        let item = items
            .get(&id)
            .ok_or_else(|| MstsError::msg(format!("Missing native platform {id}")))?;
        let TrItemKind::Platform {
            pair_id,
            station,
            name,
            passengers_waiting,
        } = &item.kind
        else {
            return Err(MstsError::msg(format!(
                "Timetable item {id} is not a native platform; use the activity and track database from the same route edition"
            )));
        };
        let pair = items
            .get(pair_id)
            .ok_or_else(|| MstsError::msg(format!("Missing native platform pair {pair_id}")))?;
        let Some(host) = hosts.get(&id) else {
            return Err(MstsError::msg(format!(
                "Native platform {id} has no host vector"
            )));
        };
        if !matches!(&pair.kind, TrItemKind::Platform { pair_id: back, station: other, name: other_name, .. }
            if *back == id && station == other && name == other_name)
            || station.is_empty()
            || hosts.get(pair_id) != Some(host)
        {
            return Err(MstsError::msg(format!(
                "Invalid native platform pair {id}/{pair_id}"
            )));
        }
        let forward = format!("e{host}");
        let reverse = format!("e{host}_r");
        if !edges
            .iter()
            .any(|edge| edge == &forward || edge == &reverse)
        {
            // A PAT ending inside a long station vector can snap to its near
            // junction. Extend only that final, directly adjacent native vector,
            // never find an unrelated path to a timetable station.
            let candidate = [&forward, &reverse]
                .into_iter()
                .filter_map(|id| graph.edge(id))
                .find(|edge| edge.from.0 == route.destination);
            let Some(edge) = candidate.filter(|_| index + 1 == activity.player_stops.len()) else {
                return Err(MstsError::msg(format!(
                    "Native station {station} is outside the player path"
                )));
            };
            let before: f64 = edges
                .iter()
                .filter_map(|id| graph.edge(id))
                .map(|edge| edge.length_m)
                .sum();
            let position = if edge.id.0.ends_with("_r") {
                edge.length_m - item.distance_m.min(pair.distance_m)
            } else {
                item.distance_m.max(pair.distance_m)
            };
            if (before + position - record.distance_down_path_m).abs() > 12.0 {
                return Err(MstsError::msg(format!(
                    "Native station {station} does not match the PAT distance"
                )));
            }
            if route.waypoints.is_empty() {
                route.waypoints.push(route.start.clone());
                for id in &edges {
                    route.waypoints.push(graph.edge(id).unwrap().to.0.clone());
                }
            }
            route.destination.clone_from(&edge.to.0);
            route.waypoints.push(edge.to.0.clone());
            edges.push(edge.id.0.clone());
        }
        let edge_index = edges
            .iter()
            .position(|edge| edge == &forward || edge == &reverse)
            .unwrap();
        let edge = graph.edge(&edges[edge_index]).unwrap();
        if [item.distance_m, pair.distance_m]
            .iter()
            .any(|distance| !distance.is_finite() || *distance < 0.0 || *distance > edge.length_m)
        {
            return Err(MstsError::msg(format!(
                "Native platform {id} lies outside its track vector"
            )));
        }
        let before: f64 = edges[..edge_index]
            .iter()
            .filter_map(|id| graph.edge(id))
            .map(|edge| edge.length_m)
            .sum();
        let (entry, exit) = if edge.id.0.ends_with("_r") {
            (
                edge.length_m - item.distance_m.max(pair.distance_m),
                edge.length_m - item.distance_m.min(pair.distance_m),
            )
        } else {
            (
                item.distance_m.min(pair.distance_m),
                item.distance_m.max(pair.distance_m),
            )
        };
        let mut chainage = before + exit;
        // Native activities may begin with boarding already in progress. Keep
        // that exact parked head while it is inside the original platform.
        if index == 0
            && record.arrival_time_s <= activity.start_time_s
            && start_chainage >= before + entry
            && start_chainage <= chainage
        {
            chainage = start_chainage;
        }
        if chainage <= previous_chainage || chainage < start_chainage {
            return Err(MstsError::msg(format!(
                "Native station {station} is out of player path order"
            )));
        }
        let arrive_s = elapsed_time(record.arrival_time_s, activity.start_time_s);
        let mut depart_s = elapsed_time(record.departure_time_s, activity.start_time_s);
        if depart_s < arrive_s {
            depart_s += 86400.0;
        }
        let terminal = index + 1 == activity.player_stops.len() && edge.to.0 == route.destination;
        let passengers_on = if terminal { 0 } else { *passengers_waiting };
        stops.push(StopDef {
            node: edge.to.0.clone(),
            name: Some(station.clone()),
            offset_m: chainage - before - edge.length_m,
            arrive_s,
            depart_s,
            dwell_s: depart_s - arrive_s,
            passengers_on,
            passengers_off: if terminal { boarded } else { 0 },
        });
        boarded = boarded.saturating_add(passengers_on);
        previous_chainage = chainage;
    }
    // Native TrackPDP world coordinates define this corridor. Align both
    // facing and trailing traversals to it instead of an unrelated BFS route.
    for pair in edges.windows(2) {
        let incoming = graph.edge(&pair[0]).unwrap();
        let Some(node) = graph.node(&incoming.to.0) else {
            continue;
        };
        let NodeKind::Switch {
            stem_edge,
            diverging_edge,
        } = &node.kind
        else {
            continue;
        };
        let reverse = if let Some(base) = pair[0].strip_suffix("_r") {
            base.to_owned()
        } else {
            format!("{}_r", pair[0])
        };
        let position = if pair[1] == stem_edge.0 || reverse == stem_edge.0 {
            SwitchPositionDef::Straight
        } else if pair[1] == diverging_edge.0 || reverse == diverging_edge.0 {
            SwitchPositionDef::Diverging
        } else {
            return Err(MstsError::msg(format!(
                "Native path has no connected branch at {}",
                node.id.0
            )));
        };
        route.switches.retain(|switch| switch.node != node.id.0);
        route.switches.push(SwitchDef {
            node: node.id.0.clone(),
            position,
        });
    }
    route.stops = stops;
    Ok(())
}

fn elapsed_time(time: f64, start: f64) -> f64 {
    let delta = time - start;
    if delta < -43200.0 {
        delta + 86400.0
    } else {
        delta.max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midnight_and_initial_boarding_use_activity_clock() {
        assert_eq!(elapsed_time(60.0, 86340.0), 120.0);
        assert_eq!(elapsed_time(33420.0, 34140.0), 0.0);
        assert_eq!(elapsed_time(34200.0, 34140.0), 60.0);
    }
}
