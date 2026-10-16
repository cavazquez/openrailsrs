//! Bounded dispatcher authority; it never authorizes entry into occupied track.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShuntAuthority {
    pub signal: String,
    pub start_m: f64,
    pub distance_m: f64,
    pub speed_mps: f64,
    pub issued_s: f64,
    pub expires_s: f64,
    pub reversed: bool,
}
impl ShuntAuthority {
    pub fn valid(&self) -> bool {
        !self.signal.is_empty()
            && self.signal.len() <= 128
            && [
                self.start_m,
                self.distance_m,
                self.speed_mps,
                self.issued_s,
                self.expires_s,
            ]
            .iter()
            .all(|v| v.is_finite())
            && self.start_m >= 0.
            && (10. ..=500.).contains(&self.distance_m)
            && (1. ..=15. / 3.6).contains(&self.speed_mps)
            && self.issued_s >= 0.
            && (0. ..=120.).contains(&(self.expires_s - self.issued_s))
    }
}
impl crate::LiveDriveSession {
    pub fn authorize_shunting(&mut self, signal: &str, distance_m: f64) -> Result<(), String> {
        if !distance_m.is_finite() || !(10. ..=500.).contains(&distance_m) {
            return Err("La autorización admite de 10 a 500 metros".into());
        }
        if self.velocity_mps() > 15. / 3.6 {
            return Err("Reducí la velocidad a 15 km/h para solicitar una maniobra".into());
        }
        let sig = self.graph.signal(signal).ok_or("Señal desconocida")?;
        let mut before = 0.;
        let mut ahead = None;
        for (index, edge) in self.state.path_edges.iter().enumerate() {
            let e = self.graph.edge(edge).ok_or("Vía inválida")?;
            if index >= self.state.edge_index && edge == &sig.edge_id {
                ahead = Some(before + sig.position_m - self.head_chainage_m());
                break;
            }
            before += e.length_m;
        }
        if ahead.is_none_or(|d| !(0. ..=100.).contains(&d)) {
            return Err(
                "Solicitá la autorización ante la señal del itinerario, a menos de 100 m".into(),
            );
        }
        if ahead.is_some_and(|d| distance_m <= d) {
            return Err("La distancia autorizada debe alcanzar más allá de la señal".into());
        }
        if self
            .distance_to_occupied_block_m()
            .is_some_and(|d| d <= distance_m)
            || self.shunt_has_parked_section(distance_m)
        {
            return Err(
                "No se puede autorizar una maniobra hacia otro tren o vehículos estacionados"
                    .into(),
            );
        }
        // Own wheels before the signal do not make the route unsafe. Generic
        // dispatcher overrides remain stricter; this authority checks the whole
        // requested corridor against other services and detached sections.
        self.signal_overrides
            .insert(signal.into(), openrailsrs_track::SignalAspect::Caution);
        self.signal_runtime
            .insert(signal.into(), openrailsrs_track::SignalAspect::Caution);
        self.native_signals.needs_refresh = true;
        self.state.shunt_authority = Some(ShuntAuthority {
            signal: signal.into(),
            start_m: self.head_chainage_m(),
            distance_m,
            speed_mps: 15. / 3.6,
            issued_s: self.time_s(),
            expires_s: self.time_s() + 120.,
            reversed: self.state.path_reversed,
        });
        Ok(())
    }
    pub(crate) fn update_shunt_authority(&mut self) {
        let Some(authority) = self.state.shunt_authority.as_ref() else {
            return;
        };
        let invalid = self.time_s() > authority.expires_s
            || self.velocity_mps() > authority.speed_mps * 1.05
            || self.head_chainage_m() - authority.start_m > authority.distance_m
            || self.head_chainage_m() + 1. < authority.start_m
            || self.state.path_reversed != authority.reversed
            || self.graph.signal(&authority.signal).is_none()
            || self.distance_to_occupied_block_m().is_some_and(|d| {
                d <= authority.distance_m - (self.head_chainage_m() - authority.start_m)
            })
            || self.shunt_has_parked_section(
                authority.distance_m - (self.head_chainage_m() - authority.start_m),
            );
        if invalid {
            let id = authority.signal.clone();
            self.state.shunt_authority = None;
            self.signal_overrides.remove(&id);
            self.native_signals.needs_refresh = true;
        }
    }
    fn shunt_has_parked_section(&self, distance_m: f64) -> bool {
        // Detached portions have immutable paths. A chainage from another PAT
        // cannot be compared directly with the current route's chainage.
        let parked: Vec<_> = self
            .own_track_occupancy()
            .into_iter()
            .filter(|span| span.owner != self.service_id)
            .collect();
        let head = self.head_chainage_m();
        let limit = head + distance_m.max(0.);
        let mut before = 0.;
        for (id, edge) in self.state.path_edges.iter().zip(&self.path_data.edges) {
            let base = id.strip_suffix("_r").unwrap_or(id);
            for span in parked.iter().filter(|p| p.edge == base) {
                let (a, b) = if id.ends_with("_r") {
                    (edge.length_m - span.end_m, edge.length_m - span.start_m)
                } else {
                    (span.start_m, span.end_m)
                };
                if before + b >= head && before + a <= limit {
                    return true;
                }
            }
            before += edge.length_m;
            if before > limit {
                break;
            }
        }
        false
    }
    pub fn shunting_authority_status(&self) -> Option<String> {
        self.state.shunt_authority.as_ref().map(|a| {
            format!(
                "Maniobra · {:.0} m restantes · límite {:.0} km/h · {:.0} s",
                (a.distance_m - (self.head_chainage_m() - a.start_m)).max(0.),
                a.speed_mps * 3.6,
                (a.expires_s - self.time_s()).max(0.)
            )
        })
    }
}

#[cfg(test)]
mod section_authority_tests {
    #[test]
    fn shunting_checks_each_parked_physical_track_instead_of_foreign_chainage() {
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/portion_operations");
        let definition =
            openrailsrs_scenarios::load_scenario(folder.join("scenario.toml")).unwrap();
        let mut s = crate::LiveDriveSession::from_scenario(&folder, &definition).unwrap();
        for car in &mut s.formation.cars[1..] {
            car.handbrake = true;
        }
        s.uncouple_after_named(2, "B").unwrap();
        // Prescribed parked position on the adjacent siding: its chainage is
        // numerically ahead but the current route never enters that track.
        s.formation.sections[0].path_edges = vec!["diverge".into()];
        s.rebuild_section_paths();
        s.state.pos_on_edge_m = 80.;
        assert!(!s.shunt_has_parked_section(100.));
        s.state.pos_on_edge_m = 200.;
        s.uncouple_after_named(0, "C").unwrap();
        s.state.pos_on_edge_m = 80.;
        assert!(s.shunt_has_parked_section(100.));
    }
}
