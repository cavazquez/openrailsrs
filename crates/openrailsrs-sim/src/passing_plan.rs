//! Authored passing plans reserve an actual siding before a meet/overtake.
//! Release is driven by the complete other train's footprint, never a timer.
use crate::{LiveDriveSession, live_traffic::TrafficService, native_signals::TrackOccupancy};
fn base(id: &str) -> &str {
    id.strip_suffix("_r").unwrap_or(id)
}
impl LiveDriveSession {
    pub(crate) fn passing_hold_chainage(&self) -> Option<f64> {
        self.state
            .rail_operations
            .definition
            .passing_plans
            .iter()
            .filter(|p| {
                p.waiting_service == self.service_id
                    && self.state.rail_operations.passing_started.contains(&p.id)
                    && !self.state.rail_operations.passing_complete.contains(&p.id)
            })
            .filter_map(|p| {
                let mut at = 0.;
                for (id, e) in self.state.path_edges.iter().zip(&self.path_data.edges) {
                    if *id == p.siding_edge {
                        return Some(at + p.position_m);
                    }
                    at += e.length_m;
                }
                None
            })
            .min_by(f64::total_cmp)
    }
    pub fn passing_status(&self) -> Option<String> {
        let p = self
            .state
            .rail_operations
            .definition
            .passing_plans
            .iter()
            .find(|p| {
                p.waiting_service == self.service_id
                    && !self.state.rail_operations.passing_complete.contains(&p.id)
            })?;
        Some(format!(
            "{} · esperar en {} hasta que la cola de {} despeje {}",
            p.id, p.siding_edge, p.passing_service, p.clear_edge
        ))
    }
}
pub(crate) fn plan(
    player: &mut LiveDriveSession,
    services: &mut [TrafficService],
    footprints: &[TrackOccupancy],
) {
    let participants =
        std::iter::once((&player.service_id, &player.state.path_edges, player.arrived))
            .chain(
                services
                    .iter()
                    .filter(|s| s.departed)
                    .map(|s| (&s.id, &s.session.state.path_edges, s.session.arrived)),
            )
            .map(|(id, path, arrived)| (id.clone(), (path.clone(), arrived)))
            .collect::<std::collections::HashMap<_, _>>();
    let apply = |session: &mut LiveDriveSession| {
        let definitions = session
            .state
            .rail_operations
            .definition
            .passing_plans
            .clone();
        let waiting_id = session.service_id.clone();
        for p in definitions
            .iter()
            .filter(|p| p.waiting_service == waiting_id)
        {
            if session
                .state
                .rail_operations
                .passing_complete
                .contains(&p.id)
            {
                continue;
            }
            let Some((other_path, arrived)) = participants.get(&p.passing_service) else {
                continue;
            };
            let occupied = footprints
                .iter()
                .any(|f| f.owner == p.passing_service && f.edge == base(&p.clear_edge));
            if occupied {
                session
                    .state
                    .rail_operations
                    .passing_seen
                    .insert(p.id.clone());
            }
            if !occupied && (session.state.rail_operations.passing_seen.contains(&p.id) || *arrived)
            {
                session
                    .state
                    .rail_operations
                    .passing_complete
                    .insert(p.id.clone());
                continue;
            }
            if session
                .state
                .rail_operations
                .passing_started
                .contains(&p.id)
            {
                continue;
            }
            // Detect shared future corridor before either train blocks the meet.
            if !session
                .state
                .path_edges
                .iter()
                .skip(session.state.edge_index)
                .any(|e| other_path.iter().any(|other| base(e) == base(other)))
            {
                continue;
            }
            let Some(siding) = session.graph.edge(&p.siding_edge) else {
                session.state.rail_operations.notice = format!("{}: apartadero inexistente", p.id);
                continue;
            };
            if p.position_m < session.formation.length_m() + 5.
                || p.position_m > siding.length_m - 5.
            {
                session.state.rail_operations.notice = format!(
                    "{}: apartadero demasiado corto para {:.0} m",
                    p.id,
                    session.formation.length_m()
                );
                continue;
            }
            let mut accepted = false;
            for alternative in &p.alternatives {
                if !alternative.contains(&p.siding_edge) {
                    continue;
                }
                // The other train may occupy the common exit beyond our
                // stopping point. Only the route into the protected siding
                // needs to be free now; the exit is granted after tail release.
                let approach = alternative
                    .iter()
                    .take_while(|e| *e != &p.siding_edge)
                    .chain(std::iter::once(&p.siding_edge));
                if approach.into_iter().any(|e| {
                    footprints
                        .iter()
                        .any(|f| f.owner != session.service_id && f.edge == base(e))
                }) {
                    continue;
                }
                if session.adopt_dispatch_path(alternative.clone()).is_ok() {
                    session
                        .state
                        .rail_operations
                        .passing_started
                        .insert(p.id.clone());
                    session.dispatcher.reroutes += 1;
                    session.state.rail_operations.notice = format!(
                        "{}: vía {} reservada; espera por {}",
                        p.id, p.siding_edge, p.passing_service
                    );
                    accepted = true;
                    break;
                }
            }
            if !accepted {
                session.state.rail_operations.notice = format!(
                    "{}: ninguna alternativa libre conserva las paradas y la posición; mantener señal de protección",
                    p.id
                );
            }
        }
    };
    apply(player);
    for service in services.iter_mut().filter(|s| s.departed) {
        apply(&mut service.session);
    }
}
