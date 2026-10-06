//! Interactive traffic shares the player's physics, station service and clock.
//! Occupancy includes entire formations; presentation never determines signals.
use crate::{LiveDriveSession, SessionSnapshot};
use openrailsrs_scenarios::{ScenarioFile, TrainEntryDef};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub struct TrafficService {
    pub id: String,
    pub departure_s: f64,
    pub session: LiveDriveSession,
    pub departed: bool,
}

#[derive(Default)]
pub struct LiveTraffic {
    pub services: Vec<TrafficService>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrafficSnapshot {
    pub id: String,
    pub session: SessionSnapshot,
    pub departed: bool,
}

impl LiveTraffic {
    pub fn from_scenario(directory: &Path, scenario: &ScenarioFile) -> Result<Self, String> {
        let mut ids = HashSet::new();
        let mut services = vec![];
        for entry in &scenario.extra_trains {
            if entry.id.trim().is_empty()
                || entry.id == "Jugador"
                || !ids.insert(entry.id.clone())
                || !entry.start_time_s.is_finite()
                || entry.start_time_s < 0.0
                || !entry.start_offset_m.is_finite()
                || entry.start_offset_m < 0.0
            {
                return Err(format!("Servicio de tráfico inválido: {}", entry.id));
            }
            let ai = service_scenario(scenario, entry);
            let mut session = LiveDriveSession::from_scenario(directory, &ai)
                .map_err(|e| format!("{}: {e}", entry.id))?;
            session.service_id.clone_from(&entry.id);
            session.headlights = 2;
            services.push(TrafficService {
                id: entry.id.clone(),
                departure_s: entry.start_time_s,
                session,
                departed: false,
            });
        }
        Ok(Self { services })
    }

    pub fn snapshot(&self) -> Vec<TrafficSnapshot> {
        self.services
            .iter()
            .map(|s| TrafficSnapshot {
                id: s.id.clone(),
                session: s.session.snapshot(),
                departed: s.departed,
            })
            .collect()
    }

    pub fn snapshot_with_scripts(&mut self) -> Result<Vec<TrafficSnapshot>, String> {
        self.services
            .iter_mut()
            .map(|s| {
                Ok(TrafficSnapshot {
                    id: s.id.clone(),
                    session: s.session.snapshot_with_scripts()?,
                    departed: s.departed,
                })
            })
            .collect()
    }

    pub fn prepare_script_restore(
        &self,
        saved: &[TrafficSnapshot],
    ) -> Result<Vec<Option<crate::etcs::ScriptTcsHost>>, String> {
        self.validate_snapshot(saved)?;
        self.services
            .iter()
            .zip(saved)
            .map(|(s, saved)| s.session.prepare_script_restore(&saved.session))
            .collect()
    }

    /// Validate every service before modifying any of them, including the player.
    pub fn validate_snapshot(&self, saved: &[TrafficSnapshot]) -> Result<(), String> {
        if self.services.len() != saved.len() {
            return Err("El tráfico guardado no coincide con el escenario".into());
        }
        for (service, snapshot) in self.services.iter().zip(saved) {
            if service.id != snapshot.id {
                return Err("Identificador de tráfico incompatible".into());
            }
            service.session.validate_snapshot(&snapshot.session)?;
        }
        crate::SessionSnapshot::validate_shared_dispatcher(saved.iter().map(|s| &s.session))?;
        Ok(())
    }

    pub fn restore_snapshot(&mut self, saved: Vec<TrafficSnapshot>) -> Result<(), String> {
        let hosts = self.prepare_script_restore(&saved)?;
        self.restore_prepared_snapshot(saved, hosts)
    }

    pub fn restore_prepared_snapshot(
        &mut self,
        saved: Vec<TrafficSnapshot>,
        hosts: Vec<Option<crate::etcs::ScriptTcsHost>>,
    ) -> Result<(), String> {
        self.validate_snapshot(&saved)?;
        if hosts.len() != self.services.len() {
            return Err("Cantidad de estados C# inválida".into());
        }
        for ((service, saved), host) in self.services.iter_mut().zip(saved).zip(hosts) {
            service
                .session
                .restore_prepared_snapshot(saved.session, host)?;
            service.departed = saved.departed;
        }
        Ok(())
    }

    pub fn dispatch_switch(
        &mut self,
        player: &mut LiveDriveSession,
        id: &str,
    ) -> Result<(), String> {
        self.synchronize_occupancy(player);
        player.dispatch_switch(id)?;
        let position = player
            .graph
            .switch_position(id)
            .ok_or("Cambio desconocido")?;
        for service in &mut self.services {
            service
                .session
                .graph
                .set_switch(id, position)
                .map_err(|e| e.to_string())?;
            service.session.native_signals.needs_refresh = true;
        }
        player.native_signals.needs_refresh = true;
        self.synchronize_occupancy(player);
        Ok(())
    }

    pub fn synchronize_occupancy(&mut self, player: &mut LiveDriveSession) {
        let mut footprints = player.own_track_occupancy();
        let mut blocks = player.own_occupied_edges();
        for service in &self.services {
            if service.departed {
                blocks.extend(service.session.own_occupied_edges());
                footprints.extend(service.session.own_track_occupancy());
            }
        }
        let other_blocks = |id: &str| -> HashMap<_, _> {
            blocks
                .iter()
                .filter(|(_, owner)| owner.as_str() != id && !owner.starts_with(&format!("{id} ·")))
                .map(|(edge, owner)| (edge.clone(), owner.clone()))
                .collect()
        };
        crate::track_reservations::coordinate(player, &mut self.services, &footprints);
        let others = other_blocks(&player.service_id);
        let positions: Vec<_> = footprints
            .iter()
            .filter(|i| i.owner != player.service_id)
            .cloned()
            .collect();
        player.external_track_occupancy = positions;
        if player.external_occupancy != others || player.native_signals.needs_refresh {
            player.external_occupancy = others;
            player.refresh_traffic_signals();
        }
        for service in &mut self.services {
            let others = other_blocks(&service.id);
            let positions: Vec<_> = footprints
                .iter()
                .filter(|i| i.owner != service.id)
                .cloned()
                .collect();
            service.session.external_track_occupancy = positions;
            if service.session.external_occupancy != others
                || service.session.native_signals.needs_refresh
            {
                service.session.external_occupancy = others;
                service.session.refresh_traffic_signals();
            }
        }
    }

    /// The viewer calls this at its fixed cadence. AI and player decisions run
    /// at the same small physics quanta even when time is accelerated.
    pub fn advance<F>(
        &mut self,
        player: &mut LiveDriveSession,
        real_dt: f64,
        automatic: Option<f64>,
        mut transition: F,
    ) where
        F: FnMut(&openrailsrs_scenarios::sound_regions::RegionTransition),
    {
        if self.services.is_empty() {
            if let Some(notch) = automatic {
                player.step_autodrive(real_dt, notch, transition);
            } else {
                player.step_realtime(real_dt, transition);
            }
            return;
        }
        if player.arrived || !real_dt.is_finite() || real_dt <= 0.0 {
            return;
        }
        let speed = player.speed_mul;
        let dt = player.realtime_physics_dt();
        let mut budget = player.sim_time_remainder + real_dt * speed;
        player.speed_mul = 1.0;
        player.sim_time_remainder = 0.0;
        while budget + 1e-12 >= dt && !player.arrived {
            let clock = player.time_s();
            let mut occupied = player.own_occupied_edges();
            for service in &self.services {
                if service.departed {
                    occupied.extend(service.session.own_occupied_edges());
                }
            }
            for service in &mut self.services {
                // A scheduled train appears when it departs, avoiding two
                // formations superimposed at a shared spawn point before then.
                if !service.departed {
                    service.session.state.time.0 = clock;
                    let footprint = service.session.own_occupied_edges();
                    if clock + 1e-9 >= service.departure_s
                        && footprint.keys().all(|edge| !occupied.contains_key(edge))
                    {
                        service.departed = true;
                        occupied.extend(footprint);
                    }
                }
            }
            self.synchronize_occupancy(player);
            if let Some(notch) = automatic {
                player.step_autodrive(dt, notch, &mut transition);
            } else {
                player.step_realtime(dt, &mut transition);
            }
            for service in &mut self.services {
                if service.departed && !service.session.arrived {
                    service.session.speed_mul = 1.0;
                    service.session.step_autodrive(dt, 0.75, |_| {});
                }
            }
            budget -= dt;
        }
        player.speed_mul = speed;
        player.sim_time_remainder = if player.arrived { 0.0 } else { budget };
        self.synchronize_occupancy(player);
    }
}

fn service_scenario(player: &ScenarioFile, entry: &TrainEntryDef) -> ScenarioFile {
    let mut ai = player.clone();
    ai.scenario.name.clone_from(&entry.id);
    ai.extra_trains.clear();
    ai.route.start.clone_from(&entry.start);
    ai.route.destination.clone_from(&entry.destination);
    ai.route.start_offset_m = Some(entry.start_offset_m);
    ai.route.waypoints.clone_from(&entry.waypoints);
    ai.route.stops.clone_from(&entry.stops);
    ai.route.switches.clone_from(&entry.switches);
    ai.train.consist.clone_from(&entry.consist);
    ai.train
        .electric_pickups
        .clone_from(&entry.electric_pickups);
    ai.train.davis.clone_from(&entry.davis);
    ai
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn occupancy_covers_short_crossovers_between_vehicle_endpoints() {
        let directory =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern_traffic");
        let scenario =
            openrailsrs_scenarios::load_scenario(directory.join("scenario.toml")).unwrap();
        let mut player = LiveDriveSession::from_scenario(&directory, &scenario).unwrap();
        player.formation.cars.truncate(1);
        player.formation.coupled_count = 1;
        player.formation.cars[0].length_m = 22.0;
        player.state.path_edges = vec!["before".into(), "cross".into(), "after".into()];
        player.path_data.edges.truncate(3);
        for (edge, length) in player.path_data.edges.iter_mut().zip([10.0, 1.0, 30.0]) {
            edge.length_m = length;
        }
        player.state.edge_index = 2;
        player.state.pos_on_edge_m = 9.0;
        let occupied = player.own_occupied_edges();
        assert!(occupied.contains_key("cross"));
        assert!(occupied.contains_key("cross_r"));
    }
    #[test]
    fn chiltern_services_stop_clear_signals_and_restore_together() {
        let directory =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern_traffic");
        let scenario =
            openrailsrs_scenarios::load_scenario(directory.join("scenario.toml")).unwrap();
        let mut player = LiveDriveSession::from_scenario(&directory, &scenario).unwrap();
        let mut traffic = LiveTraffic::from_scenario(&directory, &scenario).unwrap();
        // A manual clear issued before traffic starts cannot leave a green
        // signal protecting the subsequently occupied block.
        player
            .signal_overrides
            .insert("sig688".into(), openrailsrs_track::SignalAspect::Clear);
        traffic.advance(&mut player, 0.05, Some(0.75), |_| {});
        assert_eq!(
            player.signal_aspect("sig688"),
            Some(openrailsrs_track::SignalAspect::Stop)
        );
        assert!(traffic.services[0].departed);
        assert!(!traffic.services[1].departed);
        player.dispatch_signal("sig688", None).unwrap();
        let mut red_cleared = false;
        let mut clock_equal = false;
        for _ in 0..1600 {
            traffic.advance(&mut player, 1.0, Some(0.75), |_| {});
            red_cleared |=
                player.signal_aspect("sig688") == Some(openrailsrs_track::SignalAspect::Clear);
            if player.time_s() >= 800.0 && !clock_equal {
                assert!(traffic.services[1].departed);
                assert!((player.time_s() - traffic.services[1].session.time_s()).abs() < 0.1);
                let saved_player = player.snapshot();
                let saved_traffic = traffic.snapshot();
                let mut restored_player =
                    LiveDriveSession::from_scenario(&directory, &scenario).unwrap();
                let mut restored_traffic =
                    LiveTraffic::from_scenario(&directory, &scenario).unwrap();
                restored_player.restore_snapshot(saved_player).unwrap();
                restored_traffic.restore_snapshot(saved_traffic).unwrap();
                restored_traffic.synchronize_occupancy(&mut restored_player);
                traffic.advance(&mut player, 3.0, Some(0.75), |_| {});
                restored_traffic.advance(&mut restored_player, 3.0, Some(0.75), |_| {});
                assert!(
                    (player.head_chainage_m() - restored_player.head_chainage_m()).abs() < 1e-8
                );
                for (a, b) in traffic.services.iter().zip(restored_traffic.services) {
                    assert!(
                        (a.session.head_chainage_m() - b.session.head_chainage_m()).abs() < 1e-8
                    );
                }
                clock_equal = true;
            }
            if player.arrived {
                break;
            }
        }
        assert!(red_cleared, "lead service must release the first signal");
        assert!(clock_equal);
        assert!(
            player.arrived,
            "player at {}m, {:?}, {:?}",
            player.head_chainage_m(),
            player.gameplay.phase,
            player.gameplay.failure
        );
        assert_eq!(player.gameplay.phase, crate::ServicePhase::Completed);
        assert_eq!(player.gameplay.stop_results.len(), 3);
        assert_eq!(traffic.services[0].session.gameplay.stop_results.len(), 2);
        assert_eq!(traffic.services[1].session.gameplay.stop_results.len(), 1);
    }
}
