//! Crossing demand follows directed itineraries and complete train footprints.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CrossingLane {
    pub edge: String,
    pub position_m: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CrossingDef {
    pub id: String,
    pub lanes: Vec<CrossingLane>,
    pub warning_s: f64,
    pub minimum_m: f64,
    pub animation_s: f64,
}
impl CrossingDef {
    pub fn valid(&self) -> bool {
        !self.id.is_empty()
            && self.id.len() <= 128
            && self.lanes.len() <= 16
            && !self.lanes.is_empty()
            && self.lanes.iter().all(|l| {
                !l.edge.is_empty()
                    && l.edge.len() <= 128
                    && l.position_m.is_finite()
                    && l.position_m >= 0.
            })
            && [self.warning_s, self.minimum_m, self.animation_s]
                .iter()
                .all(|v| v.is_finite() && *v >= 0.)
            && self.warning_s <= 120.
            && self.minimum_m <= 2000.
            && (0.1..=120.).contains(&self.animation_s)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CrossingState {
    pub definition: CrossingDef,
    pub fraction: f64,
    pub road_closed: bool,
    pub demand: bool,
    #[serde(default)]
    pub elapsed_closed_s: f64,
}
impl CrossingState {
    pub fn new(definition: CrossingDef) -> Self {
        Self {
            definition,
            fraction: 0.,
            road_closed: false,
            demand: false,
            elapsed_closed_s: 0.,
        }
    }
    pub fn advance(&mut self, dt: f64, demand: bool) {
        self.demand = demand;
        let delta = dt.max(0.) / self.definition.animation_s;
        self.elapsed_closed_s = if demand {
            self.elapsed_closed_s + dt.max(0.)
        } else {
            0.
        };
        self.fraction = (self.fraction + if demand { delta } else { -delta }).clamp(0., 1.);
        self.road_closed = demand || self.fraction > 0.001;
    }
    pub fn valid(&self) -> bool {
        self.elapsed_closed_s.is_finite()
            && self.elapsed_closed_s >= 0.
            && self.definition.valid()
            && self.fraction.is_finite()
            && (0. ..=1.).contains(&self.fraction)
            && self.road_closed == (self.demand || self.fraction > 0.001)
    }
}
fn demand(session: &crate::LiveDriveSession, crossing: &CrossingDef) -> bool {
    let occupied = session.own_track_occupancy();
    let velocity = session.velocity_mps();
    // Physics stores speed as a magnitude. This is the same directed-route
    // choice made by LiveDriveSession when applying that speed to chainage.
    let backwards = (session.driver_direction <= 0.25) != session.state.path_reversed;
    let direction = if backwards { -1. } else { 1. };
    let head = session.head_chainage_m();
    // Route chainage decreases during a manual backward move. The leading end
    // then belongs to the rear attached car, not the nominal head traveller.
    // Operational body reversal changes car order within this same head/rear
    // interval. Detached cars are handled separately by occupied intervals.
    let leading = if direction < 0. {
        head - session.formation.length_m()
    } else {
        head
    };
    crossing.lanes.iter().any(|lane| {
        if occupied.iter().any(|o| {
            o.edge == lane.edge
                && o.start_m <= lane.position_m + crossing.minimum_m
                && o.end_m >= lane.position_m - crossing.minimum_m
        }) {
            return true;
        }
        if velocity.abs() < 0.1 {
            return false;
        }
        let mut chainage = 0.;
        for edge in &session.state.path_edges {
            let Some(e) = session.graph.edge(edge) else {
                continue;
            };
            if edge.strip_suffix("_r").unwrap_or(edge) == lane.edge {
                let at = chainage
                    + if edge.ends_with("_r") {
                        e.length_m - lane.position_m
                    } else {
                        lane.position_m
                    };
                let distance = (at - leading) * direction;
                if (0. ..=(velocity.abs() * crossing.warning_s + crossing.minimum_m).min(2000.))
                    .contains(&distance)
                {
                    return true;
                }
            }
            chainage += e.length_m;
        }
        false
    })
}
impl crate::LiveTraffic {
    pub fn update_crossings(&self, player: &mut crate::LiveDriveSession, dt: f64) {
        let decisions = player
            .state
            .crossings
            .iter()
            .map(|(id, state)| {
                let close = demand(player, &state.definition)
                    || self
                        .services
                        .iter()
                        .filter(|s| s.departed)
                        .any(|s| demand(&s.session, &state.definition));
                (id.clone(), close)
            })
            .collect::<Vec<_>>();
        for (id, close) in decisions {
            if let Some(c) = player.state.crossings.get_mut(&id) {
                c.advance(dt, close);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn demand_closes_road_before_gates_move_and_keeps_it_closed_during_opening() {
        let mut c = CrossingState::new(CrossingDef {
            id: "cross".into(),
            lanes: vec![CrossingLane {
                edge: "e1".into(),
                position_m: 100.,
            }],
            warning_s: 20.,
            minimum_m: 10.,
            animation_s: 4.,
        });
        c.advance(0., true);
        assert!(c.road_closed);
        c.advance(4., true);
        assert_eq!(c.fraction, 1.);
        c.advance(1., false);
        assert!(c.road_closed);
        c.advance(3., false);
        assert!(!c.road_closed);
        assert!(c.valid());
    }
}
