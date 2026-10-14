//! Interactive traffic shares the player's physics, station service and clock.
//! Occupancy includes entire formations; presentation never determines signals.
use crate::{LiveDriveSession, SessionSnapshot};
use openrailsrs_scenarios::{ScenarioFile, TrainEntryDef};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

impl LiveDriveSession {
    fn shift_pre_run_clock(&mut self, offset: f64, start_clock: f64) {
        self.state.time.0 = (self.state.time.0 - offset).max(0.);
        self.service_clock_start_s = start_clock;
        self.gameplay.shift_pre_run_clock(offset);
        self.horn_pressed_until_s = (self.horn_pressed_until_s - offset).max(0.);
        if let Some(t) = &mut self.state.power_supply.last_energy_time_s {
            *t = (*t - offset).max(0.).min(self.state.time.0);
        }
        for car in &mut self.state.power_supply.cars {
            for timer in [
                &mut car.supply.main_started_s,
                &mut car.supply.auxiliary_started_s,
                &mut car.supply.relay_started_s,
            ]
            .into_iter()
            .flatten()
            {
                *timer -= offset;
            }
        }
        for record in &mut self.state.announcements.recent {
            record.time_s = (record.time_s - offset).max(0.);
        }
        self.state
            .driving_report
            .samples
            .retain(|sample| sample.time_s >= offset);
        for sample in &mut self.state.driving_report.samples {
            sample.time_s -= offset;
        }
        self.state.driving_report.last_sample_s = self
            .state
            .driving_report
            .last_sample_s
            .filter(|t| *t >= offset)
            .map(|t| t - offset);
        for timer in [
            &mut self.dispatcher.wait_since_s,
            &mut self.operational_wait_started,
        ]
        .into_iter()
        .flatten()
        {
            *timer = (*timer - offset).max(0.);
        }
        self.dispatcher.last_route_search_s =
            (self.dispatcher.last_route_search_s - offset).max(0.);
        self.native_signals.needs_refresh = true;
    }
}

pub struct TrafficService {
    pub parked: bool,
    pub id: String,
    pub departure_s: f64,
    pub session: LiveDriveSession,
    pub departed: bool,
}

#[derive(Default)]
pub struct LiveTraffic {
    pub control_history: Vec<String>,
    pub couplings: Vec<crate::independent_coupling::CouplingReceipt>,
    pub services: Vec<TrafficService>,
    pub(crate) workers: usize,
    pub(crate) pool: Option<rayon::ThreadPool>,
    /// Final shared point positions from the disposable stationary pre-run
    /// anchor. Applied once to the actual player before its first presentation.
    pub(crate) pre_run_switches: Option<HashMap<String, openrailsrs_track::SwitchPosition>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrafficSnapshot {
    #[serde(default)]
    pub parked: bool,
    pub id: String,
    pub session: SessionSnapshot,
    pub departed: bool,
}

impl LiveTraffic {
    pub fn from_scenario(directory: &Path, scenario: &ScenarioFile) -> Result<Self, String> {
        let warm = scenario
            .extra_trains
            .iter()
            .map(|e| e.pre_roll_s)
            .fold(0., f64::max);
        if !warm.is_finite() || !(0. ..=21600.).contains(&warm) {
            return Err("Preparación de tráfico fuera del límite de seis horas simuladas".into());
        }
        if warm > 0. && scenario.simulation.rail_operations != Default::default() {
            return Err("La preparación previa de AI original requiere una actividad sin planes de operación personalizados".into());
        }
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
                || !entry.pre_roll_s.is_finite()
                || entry.pre_roll_s < 0.
            {
                return Err(format!("Servicio de tráfico inválido: {}", entry.id));
            }
            let mut ai = service_scenario(scenario, entry);
            let departure = warm + entry.start_time_s - entry.pre_roll_s;
            for stop in &mut ai.route.stops {
                let offset = if entry.relative_stop_times {
                    departure
                } else {
                    warm
                };
                stop.arrive_s += offset;
                stop.depart_s += offset;
            }
            let offset = if entry.relative_stop_times {
                departure
            } else {
                warm
            };
            for leg in &mut ai.route.legs {
                for stop in &mut leg.stops {
                    stop.arrive_s += offset;
                    stop.depart_s += offset;
                }
                if let Some(time) = &mut leg.not_before_s {
                    *time += offset;
                }
            }
            if warm > 0. {
                ai.scenario.start_time_s =
                    Some((scenario.scenario.start_time_s.unwrap_or(0.) - warm).rem_euclid(86400.));
            }
            let mut session = LiveDriveSession::from_scenario(directory, &ai)
                .map_err(|e| format!("{}: {e}", entry.id))?;
            session.service_id.clone_from(&entry.id);
            session.headlights = 2;
            if entry.parked {
                session.driver_direction = 0.5;
                session.driver_throttle = 0.;
                session.driver_brake = 1.;
                for car in &mut session.formation.cars {
                    car.handbrake = true;
                    car.power_on = false;
                    car.battery_on = false;
                }
                for diesel in &mut session.state.diesel.cars {
                    diesel.command_running = false;
                    diesel.phase = crate::diesel_operation::EnginePhase::Stopped;
                    diesel.rpm = 0.;
                    diesel.demanded_rpm = 0.;
                }
                session.rebuild_formation_physics();
                session.sync_operating_brakes();
            }
            services.push(TrafficService {
                parked: entry.parked,
                id: entry.id.clone(),
                departure_s: departure,
                session,
                departed: entry.parked,
            });
        }
        let mut traffic = Self {
            services,
            ..Self::default()
        };
        let workers = std::env::var("OPENRAILSRS_TRAFFIC_THREADS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get().min(4)));
        if traffic.services.len() >= 4 {
            traffic.set_worker_count(workers)?;
        }
        if warm > 0. {
            // Pre-run the actual shared dispatcher, with the stationary player
            // occupying its initial track. No visual offset guesses or teleport.
            let mut anchor =
                LiveDriveSession::from_scenario(directory, scenario).map_err(|e| e.to_string())?;
            anchor.driver_brake = 1.;
            anchor.driver_throttle = 0.;
            anchor.driver_direction = 0.5;
            for car in &mut anchor.formation.cars {
                car.handbrake = true;
            }
            anchor.sync_operating_brakes();
            let mut remaining = warm;
            while remaining > 1e-8 {
                openrailsrs_formats::cancellation::check().map_err(|e| e.to_string())?;
                let step = remaining.min(0.5);
                traffic.advance(&mut anchor, step, None, |_| {});
                remaining -= step;
            }
            traffic.pre_run_switches = Some(
                anchor
                    .graph
                    .nodes_iter()
                    .filter_map(|(id, _)| {
                        anchor
                            .graph
                            .switch_position(id)
                            .map(|position| (id.to_owned(), position))
                    })
                    .collect(),
            );
            for (service, entry) in traffic.services.iter_mut().zip(&scenario.extra_trains) {
                service.departure_s = entry.start_time_s;
                service
                    .session
                    .shift_pre_run_clock(warm, scenario.scenario.start_time_s.unwrap_or(0.));
                service
                    .session
                    .validate_snapshot(&service.session.snapshot())?;
            }
        }
        Ok(traffic)
    }

    /// Complete a scene's shared pre-run world before any camera or save can
    /// observe it. The pre-run used the same stationary player's footprint;
    /// its final point positions must replace the fresh player's source defaults.
    /// Initial dispatch is resolved without advancing any simulation clock.
    pub fn initialize_pre_run_player(
        &mut self,
        player: &mut LiveDriveSession,
    ) -> Result<(), String> {
        let Some(switches) = self.pre_run_switches.as_ref() else {
            return Ok(());
        };
        // Validate the complete broadcast before changing any point.
        for id in switches.keys() {
            if player.graph.switch_position(id).is_none()
                || self
                    .services
                    .iter()
                    .any(|service| service.session.graph.switch_position(id).is_none())
            {
                return Err(format!("Cambio del tráfico preparado no disponible: {id}"));
            }
        }
        for (id, position) in switches {
            player
                .graph
                .set_switch(id, *position)
                .map_err(|error| error.to_string())?;
            for service in &mut self.services {
                service
                    .session
                    .graph
                    .set_switch(id, *position)
                    .map_err(|error| error.to_string())?;
            }
        }
        self.pre_run_switches = None;
        player.native_signals.needs_refresh = true;
        self.synchronize_occupancy(player);
        crate::SessionSnapshot::validate_shared_dispatcher(
            std::iter::once(&player.snapshot())
                .chain(self.snapshot().iter().map(|service| &service.session)),
        )?;
        Ok(())
    }

    pub fn set_worker_count(&mut self, workers: usize) -> Result<(), String> {
        if !(1..=8).contains(&workers) {
            return Err("El tráfico admite de 1 a 8 hilos".into());
        }
        self.pool = if workers > 1 {
            Some(
                rayon::ThreadPoolBuilder::new()
                    .num_threads(workers)
                    .thread_name(|i| format!("rail-traffic-{i}"))
                    .build()
                    .map_err(|e| e.to_string())?,
            )
        } else {
            None
        };
        self.workers = workers;
        Ok(())
    }
    pub fn worker_count(&self) -> usize {
        self.workers.max(1)
    }
    pub fn take_control(
        &mut self,
        player: &mut LiveDriveSession,
        index: usize,
    ) -> Result<(), String> {
        self.validate_control(player, index)?;
        let id = self.services[index].session.service_id.clone();
        self.restore_control(player, index)?;
        self.control_history.push(id);
        player.driver_throttle = 0.;
        player.driver_direction = 0.5;
        self.synchronize_occupancy(player);
        Ok(())
    }
    pub fn validate_control(&self, player: &LiveDriveSession, index: usize) -> Result<(), String> {
        if self.control_history.len() >= 128 || !self.couplings.is_empty() {
            return Err("Completá la maniobra actual antes de otro cambio de conductor".into());
        }
        let service = self.services.get(index).ok_or("Servicio desconocido")?;
        if !service.departed || service.session.arrived {
            return Err("El servicio tiene que estar activo".into());
        }
        if player.velocity_mps() > 0.1
            || service.session.velocity_mps() > 0.1
            || !(0.25..0.75).contains(&player.driver_direction)
        {
            return Err(
                "Detené ambos servicios y poné tu inversor en neutro antes de cambiar de conductor"
                    .into(),
            );
        }
        if !service
            .session
            .formation
            .cars
            .iter()
            .take(service.session.formation.coupled_count)
            .any(|c| c.powered)
        {
            return Err("El servicio no tiene una locomotora operativa".into());
        }
        Ok(())
    }
    /// Reconstruct the same actor ownership before validating a saved world.
    pub fn restore_control(
        &mut self,
        player: &mut LiveDriveSession,
        index: usize,
    ) -> Result<(), String> {
        let service = self
            .services
            .get_mut(index)
            .ok_or("Servicio guardado desconocido")?;
        let old = player.service_id.clone();
        std::mem::swap(player, &mut service.session);
        service.id = old;
        service.departed = true;
        service.parked = false;
        Ok(())
    }

    fn advance_services(&mut self, dt: f64) {
        let step = |service: &mut TrafficService| {
            if service.departed && !service.parked && !service.session.arrived {
                service.session.speed_mul = 1.;
                service.session.step_autodrive(dt, 0.75, |_| {});
            }
        };
        // Grants and occupancy are already frozen by the serial coordinator.
        // Only disjoint train states run concurrently, and publication retains
        // their original order; no worker mutates another service or dispatcher.
        if let Some(pool) = &self.pool {
            pool.install(|| self.services.par_iter_mut().for_each(step));
        } else {
            self.services.iter_mut().for_each(step);
        }
    }

    pub fn snapshot(&self) -> Vec<TrafficSnapshot> {
        self.services
            .iter()
            .map(|s| TrafficSnapshot {
                parked: s.parked,
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
                    parked: s.parked,
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

    /// Restore the complete validated player/traffic aggregate. Standalone
    /// SessionSnapshot restoration cannot know foreign authorities; this API
    /// reconnects them without rerunning dispatch or consuming a physics tick.
    pub fn restore_shared_snapshots(
        &mut self,
        player: &mut LiveDriveSession,
        player_saved: SessionSnapshot,
        traffic_saved: Vec<TrafficSnapshot>,
    ) -> Result<(), String> {
        player.validate_snapshot(&player_saved)?;
        self.validate_snapshot(&traffic_saved)?;
        SessionSnapshot::validate_shared_dispatcher(
            std::iter::once(&player_saved)
                .chain(traffic_saved.iter().map(|service| &service.session)),
        )?;
        let owned_locks: Vec<_> = std::iter::once(&player_saved)
            .chain(traffic_saved.iter().map(|service| &service.session))
            .flat_map(|session| session.dispatcher.own_locks.iter())
            .collect();
        for (id, saved) in std::iter::once((&player.service_id, &player_saved)).chain(
            self.services
                .iter()
                .zip(&traffic_saved)
                .map(|(service, saved)| (&service.session.service_id, &saved.session)),
        ) {
            let mut remaining: Vec<_> = owned_locks
                .iter()
                .copied()
                .filter(|lock| &lock.owner != id)
                .collect();
            for foreign in &saved.dispatcher.other_locks {
                let Some(index) = remaining.iter().position(|lock| *lock == foreign) else {
                    return Err("Enclavamiento ajeno guardado inválido".into());
                };
                remaining.swap_remove(index);
            }
            if !remaining.is_empty() {
                return Err("Faltan enclavamientos ajenos en la partida guardada".into());
            }
        }
        let player_host = player.prepare_script_restore(&player_saved)?;
        let traffic_hosts = self.prepare_script_restore(&traffic_saved)?;
        let player_has_cache = player_saved.native_signal_cache.is_some();
        let player_foreign_locks = player_saved.dispatcher.other_locks.clone();
        let traffic_metadata: Vec<_> = traffic_saved
            .iter()
            .map(|service| {
                (
                    service.session.native_signal_cache.is_some(),
                    service.session.dispatcher.other_locks.clone(),
                )
            })
            .collect();
        player.restore_prepared_snapshot(player_saved, player_host)?;
        self.restore_prepared_snapshot(traffic_saved, traffic_hosts)?;
        self.reconnect_restored_occupancy(player);
        if !player_has_cache {
            player.rebuild_legacy_native_signal_cache();
        }
        player.dispatcher.other_locks = player_foreign_locks;
        for (service, (has_cache, foreign_locks)) in self.services.iter_mut().zip(traffic_metadata)
        {
            if !has_cache {
                service.session.rebuild_legacy_native_signal_cache();
            }
            service.session.dispatcher.other_locks = foreign_locks;
        }
        self.pre_run_switches = None;
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
            service.parked = saved.parked;
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

    fn shared_occupancy(
        &self,
        player: &mut LiveDriveSession,
    ) -> (
        HashMap<String, String>,
        Vec<crate::native_signals::TrackOccupancy>,
    ) {
        let mut footprints = player.own_track_occupancy();
        let mut blocks = player.own_occupied_edges();
        for service in &self.services {
            if service.departed {
                blocks.extend(service.session.own_occupied_edges());
                footprints.extend(service.session.own_track_occupancy());
            }
        }
        let works = player.closed_work_occupancy(&footprints);
        blocks.extend(works.iter().map(|w| (w.edge.clone(), w.owner.clone())));
        footprints.extend(works);
        (blocks, footprints)
    }

    /// Reconnect a validated saved world without running the dispatcher or
    /// changing the saved signal cadence. Foreign authorities come exclusively
    /// from each train's validated own grants and locks, never foreign entries
    /// supplied by a save file. The next ordinary physics tick makes new
    /// dispatch decisions at the same time as an uninterrupted session.
    pub fn reconnect_restored_occupancy(&mut self, player: &mut LiveDriveSession) {
        // The ordinary collector updates remembered work windows. Rebinding
        // their cached footprints is not a simulation tick: retain the exact
        // validated window memory until the next ordinary traffic advance.
        let closed_works = player.state.rail_operations.closed_works.clone();
        let (blocks, footprints) = self.shared_occupancy(player);
        player.state.rail_operations.closed_works = closed_works;
        let grants: Vec<_> = std::iter::once(&*player)
            .chain(self.services.iter().map(|service| &service.session))
            .flat_map(|session| session.own_track_reservations.iter().cloned())
            .collect();
        let locks: Vec<_> = std::iter::once(&*player)
            .chain(self.services.iter().map(|service| &service.session))
            .flat_map(|session| session.dispatcher.own_locks.iter().cloned())
            .collect();
        let reconnect = |session: &mut LiveDriveSession| {
            session.external_occupancy = blocks
                .iter()
                .filter(|(_, owner)| {
                    owner.as_str() != session.service_id
                        && !owner.starts_with(&format!("{} ·", session.service_id))
                })
                .map(|(edge, owner)| (edge.clone(), owner.clone()))
                .collect();
            session.external_track_occupancy = footprints
                .iter()
                .filter(|footprint| footprint.owner != session.service_id)
                .cloned()
                .collect();
            session.external_track_reservations = grants
                .iter()
                .filter(|grant| grant.owner != session.service_id)
                .cloned()
                .collect();
            session.dispatcher.other_locks = locks
                .iter()
                .filter(|lock| lock.owner != session.service_id)
                .cloned()
                .collect();
        };
        reconnect(player);
        for service in &mut self.services {
            reconnect(&mut service.session);
        }
    }

    pub fn synchronize_occupancy(&mut self, player: &mut LiveDriveSession) {
        let (blocks, footprints) = self.shared_occupancy(player);
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
        crate::rail_operations::coordinate(player, self);
        let timed_closures = player
            .state
            .rail_operations
            .definition
            .works
            .iter()
            .any(|w| w.closed);
        if self.services.is_empty() && !timed_closures {
            let before = player.time_s();
            if let Some(notch) = automatic {
                player.step_autodrive(real_dt, notch, transition);
            } else {
                player.step_realtime(real_dt, transition);
            }
            self.update_crossings(player, (player.time_s() - before).max(0.));
            return;
        }
        if !real_dt.is_finite() || real_dt <= 0.0 {
            return;
        }
        let speed = player.speed_mul;
        let dt = player.realtime_physics_dt();
        let mut budget = player.sim_time_remainder + real_dt * speed;
        player.speed_mul = 1.0;
        player.sim_time_remainder = 0.0;
        while budget + 1e-12 >= dt {
            let clock = player.time_s();
            let mut occupied = player.own_occupied_edges();
            for service in &self.services {
                if service.departed {
                    occupied.extend(service.session.own_occupied_edges());
                }
            }
            for service in &mut self.services {
                if service.parked {
                    service.session.state.time.0 = clock;
                }
                // A scheduled train appears when it departs, avoiding two
                // formations superimposed at a shared spawn point before then.
                if !service.departed {
                    service.session.state.time.0 = clock;
                    crate::surface_conditions::update_program_input(&mut service.session.state);
                    if let Some(surface) = &mut service.session.state.surface_conditions {
                        surface.advance(dt);
                    }
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
            self.update_crossings(player, dt);
            crate::rail_operations::coordinate(player, self);
            if player.arrived {
                // Finishing the player's service does not freeze other trains.
                player.state.time.0 += dt;
            } else if let Some(notch) = automatic {
                player.step_autodrive(dt, notch, &mut transition);
            } else {
                player.step_realtime(dt, &mut transition);
            }
            self.advance_services(dt);
            budget -= dt;
        }
        player.speed_mul = speed;
        player.sim_time_remainder = budget;
        self.synchronize_occupancy(player);
    }
}

fn service_scenario(player: &ScenarioFile, entry: &TrainEntryDef) -> ScenarioFile {
    let mut ai = player.clone();
    ai.gameplay.shunting.clear();
    ai.scenario.name.clone_from(&entry.id);
    ai.extra_trains.clear();
    ai.simulation.rail_operations.connections.clear();
    ai.simulation.rail_operations.incidents.clear();
    ai.train
        .announcements
        .events
        .retain(|e| e.service.as_deref() == Some(entry.id.as_str()));
    ai.route.start.clone_from(&entry.start);
    ai.route.legs.clone_from(&entry.legs);
    ai.route.destination.clone_from(&entry.destination);
    ai.route.start_offset_m = Some(entry.start_offset_m);
    ai.route.waypoints.clone_from(&entry.waypoints);
    ai.route.stops.clone_from(&entry.stops);
    ai.route.switches.clone_from(&entry.switches);
    ai.train.consist.clone_from(&entry.consist);
    ai.train.systems.clone_from(&entry.systems);
    ai.train.freight.clone_from(&entry.freight);
    ai.train.platform_sides.clear();
    ai.train.cab_equipment.clone_from(&entry.cab_equipment);
    ai.train.presentation.clone_from(&entry.presentation);
    ai.train.vehicle_profiles.clear();
    ai.train.power_load = Default::default();
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
