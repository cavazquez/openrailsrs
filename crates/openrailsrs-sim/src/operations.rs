//! Player operations and versioned snapshots, independent of the renderer.
use std::collections::HashMap;

use openrailsrs_track::{NodeKind, SignalAspect, SwitchPosition};
use openrailsrs_train::{Consist, TractiveCurve, Vehicle};
use serde::{Deserialize, Serialize};

use crate::path_data::PathData;
fn base_edge(id: &str) -> &str {
    id.strip_suffix("_r").unwrap_or(id)
}
use crate::{
    BrakeCylinder, CouplerState, LiveDriveSession, LiveGameplay, RollingStockExteriorState,
    TrainSimState,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CarOperationState {
    pub name: String,
    pub length_m: f64,
    pub mass_kg: f64,
    pub powered: bool,
    pub power_on: bool,
    pub battery_on: bool,
    #[serde(default)]
    pub train_supply_switch_on: bool,
    pub mu_connected: bool,
    pub handbrake: bool,
    /// Hose to the preceding car (index zero has no preceding car).
    pub hose_connected: bool,
    pub front_cock_open: bool,
    pub rear_cock_open: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FormationState {
    pub cars: Vec<CarOperationState>,
    pub coupled_count: usize,
    /// Parked section uses the original head chainage and original car offsets.
    pub parked_head_chainage_m: Option<f64>,
    pub parked_brakes: Vec<BrakeCylinder>,
    pub parked_vehicles: Vec<crate::VehicleState>,
    pub parked_couplers: Vec<CouplerState>,
}

impl FormationState {
    pub(crate) fn new(consist: &Consist) -> Self {
        let cars = consist
            .vehicles
            .iter()
            .map(|v| {
                let (name, length_m, mass_kg, powered) = match v {
                    Vehicle::Loco(l) => (l.name.clone(), l.length_m, l.mass_kg, true),
                    Vehicle::Wagon(w) => (w.name.clone(), w.length_m, w.mass_kg, false),
                };
                CarOperationState {
                    name,
                    length_m,
                    mass_kg,
                    powered,
                    power_on: powered,
                    battery_on: powered,
                    train_supply_switch_on: false,
                    mu_connected: powered,
                    handbrake: false,
                    hose_connected: true,
                    front_cock_open: true,
                    rear_cock_open: true,
                }
            })
            .collect::<Vec<_>>();
        Self {
            coupled_count: cars.len(),
            cars,
            parked_head_chainage_m: None,
            parked_brakes: vec![],
            parked_vehicles: vec![],
            parked_couplers: vec![],
        }
    }

    pub fn length_m(&self) -> f64 {
        self.cars[..self.coupled_count]
            .iter()
            .map(|c| c.length_m)
            .sum()
    }

    pub fn offset_m(&self, index: usize) -> f64 {
        -self
            .cars
            .iter()
            .take(index)
            .map(|c| c.length_m)
            .sum::<f64>()
    }

    /// Centre of a native vehicle; `offset_m` locates its leading end.
    pub fn center_offset_m(&self, index: usize) -> f64 {
        self.offset_m(index) - self.cars.get(index).map_or(0.0, |car| car.length_m * 0.5)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum CarOperation {
    Handbrake,
    Hose,
    FrontCock,
    RearCock,
    Power,
    Battery,
    TrainSupply,
    MultipleUnit,
}

fn default_traffic_brake_assistance() -> bool {
    true
}

/// The snapshot contains mutable state, never GPU assets or source content.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub version: u32,
    pub content_signature: String,
    pub state: TrainSimState,
    pub gameplay: LiveGameplay,
    pub formation: FormationState,
    pub exterior: RollingStockExteriorState,
    pub signal_runtime: HashMap<String, SignalAspect>,
    pub signal_overrides: HashMap<String, SignalAspect>,
    #[serde(default)]
    pub track_reservations: Vec<crate::native_signals::TrackOccupancy>,
    #[serde(default)]
    pub script_state: Option<crate::etcs::ScriptSnapshot>,
    #[serde(default)]
    pub dispatcher: crate::DispatcherStatus,
    pub switches: HashMap<String, SwitchPosition>,
    pub driver_throttle: f64,
    pub driver_brake: f64,
    pub driver_direction: f64,
    #[serde(default = "default_traffic_brake_assistance")]
    pub traffic_brake_assistance: bool,
    pub wiper_active: bool,
    #[serde(default)]
    pub headlights: u8,
    #[serde(default)]
    pub cab_light: bool,
    pub horn_pressed_until_s: f64,
    pub speed_mul: f64,
    pub sim_time_remainder: f64,
    pub previous_render_chainage_m: f64,
    pub signal_steps: u64,
    pub arrived: bool,
}

impl SessionSnapshot {
    /// Check the shared interlocking before any service is restored. Individual
    /// snapshots cannot establish that two trains do not own the same authority.
    pub fn validate_shared_dispatcher<'a>(
        sessions: impl IntoIterator<Item = &'a Self>,
    ) -> Result<(), String> {
        let sessions: Vec<_> = sessions.into_iter().collect();
        let mut points: HashMap<&str, &crate::SwitchLock> = HashMap::new();
        let mut grants: Vec<&crate::native_signals::TrackOccupancy> = vec![];
        for session in &sessions {
            for lock in &session.dispatcher.own_locks {
                if points
                    .insert(&lock.node, lock)
                    .is_some_and(|old| old.owner != lock.owner || old.position != lock.position)
                    || sessions
                        .iter()
                        .any(|s| s.switches.get(&lock.node) != Some(&lock.position))
                {
                    return Err(format!("Enclavamientos incompatibles en {}", lock.node));
                }
            }
            for grant in &session.track_reservations {
                if grants.iter().any(|other| {
                    other.owner != grant.owner && crate::track_reservations::overlaps(other, grant)
                }) {
                    return Err(format!("Reservas superpuestas en {}", grant.edge));
                }
                grants.push(grant);
            }
        }
        Ok(())
    }
}

impl LiveDriveSession {
    pub fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            version: 1,
            content_signature: self.content_signature.clone(),
            state: self.state.clone(),
            gameplay: self.gameplay.clone(),
            formation: self.formation.clone(),
            exterior: self.exterior.clone(),
            signal_runtime: self.signal_runtime.clone(),
            signal_overrides: self.signal_overrides.clone(),
            track_reservations: self.own_track_reservations.clone(),
            script_state: None,
            dispatcher: self.dispatcher.clone(),
            switches: self
                .graph
                .nodes_iter()
                .filter_map(|(id, _)| self.graph.switch_position(id).map(|p| (id.to_string(), p)))
                .collect(),
            driver_throttle: self.driver_throttle,
            driver_brake: self.driver_brake,
            driver_direction: self.driver_direction,
            traffic_brake_assistance: self.traffic_brake_assistance,
            wiper_active: self.wiper_active,
            headlights: self.headlights,
            cab_light: self.cab_light,
            horn_pressed_until_s: self.horn_pressed_until_s,
            speed_mul: self.speed_mul,
            sim_time_remainder: self.sim_time_remainder,
            previous_render_chainage_m: self.previous_render_chainage_m,
            signal_steps: self.signal_steps,
            arrived: self.arrived,
        }
    }

    /// Capture fallible script serialization without advancing physics or consuming inputs.
    pub fn snapshot_with_scripts(&mut self) -> Result<SessionSnapshot, String> {
        let context = self.script_context(0.0);
        let mut saved = self.snapshot();
        if let Some(host) = &mut self.script_tcs {
            saved.script_state = Some(host.snapshot(&context)?);
        }
        Ok(saved)
    }

    /// Prepare every external script before committing any mutable game state.
    pub fn prepare_script_restore(
        &self,
        saved: &SessionSnapshot,
    ) -> Result<Option<crate::etcs::ScriptTcsHost>, String> {
        self.validate_snapshot(saved)?;
        match (&self.script_tcs, &saved.script_state) {
            (Some(host), Some(state)) => host.prepare_restore(state).map(Some),
            _ => Ok(None),
        }
    }

    /// Validate completely before mutating the current game.
    pub fn validate_snapshot(&self, saved: &SessionSnapshot) -> Result<(), String> {
        match (&self.script_tcs, &saved.script_state) {
            (Some(host), Some(state)) => host.validate_snapshot(state)?,
            (None, None) => (),
            _ => return Err("La partida y la sesión deben usar el mismo TCS Rust o C#".into()),
        }
        if saved.version != 1 || saved.content_signature != self.content_signature {
            return Err(
                "La partida pertenece a otra versión de la ruta, servicio o formación".into(),
            );
        }
        let n = saved.formation.coupled_count;
        if saved.state.refill_used_kg.len() > 10000
            || saved
                .state
                .refill_used_kg
                .iter()
                .any(|(k, v)| k.len() > 128 || !v.is_finite() || *v < 0.)
        {
            return Err("La partida tiene reservas de abastecimiento inválidas".into());
        }
        if !saved.state.diesel.valid_for(&self.original_physics.diesel)
            || match (
                &saved.state.boiler_state,
                &self.original_physics.steam_params,
            ) {
                (Some(b), Some(p)) => !b.valid_for(p),
                (None, None) => false,
                _ => true,
            }
        {
            return Err("La partida tiene reservas o controles de tracción inválidos".into());
        }
        if !saved
            .state
            .electric
            .valid_for(&self.original_physics.electric)
        {
            return Err("La alimentación eléctrica guardada no corresponde a la formación".into());
        }
        if !saved
            .state
            .power_supply
            .valid_for(&self.original_physics.power_supply, saved.state.time.0)
        {
            return Err("La alimentación guardada no corresponde a la formación".into());
        }
        if let Some(rail) = &saved.state.rail_adhesion
            && self
                .original_physics
                .rail_adhesion
                .as_ref()
                .is_none_or(|c| !rail.valid_for(c))
        {
            return Err("La adherencia o las reservas de arena guardadas son inválidas".into());
        }
        if n == 0
            || n > self.consist.vehicles.len()
            || saved.formation.cars.len() != self.consist.vehicles.len()
            || saved.state.brake_system.cylinders.len() != n
            || saved.formation.parked_brakes.len() != self.consist.vehicles.len() - n
            || saved.formation.parked_head_chainage_m.is_some() != (n < self.consist.vehicles.len())
            || (!saved.state.vehicles.is_empty()
                && (saved.state.vehicles.len() != n
                    || saved.state.couplers.len() != n - 1
                    || saved.state.vehicle_masses.len() != n
                    || saved.formation.parked_vehicles.len() != self.consist.vehicles.len() - n
                    || saved.formation.parked_couplers.len() != self.consist.vehicles.len() - n))
            || saved.gameplay.next_stop_idx > saved.gameplay.stop_targets.len()
            || saved.gameplay.stop_targets.len() != self.gameplay.stop_targets.len()
        {
            return Err("La partida tiene una formación o un servicio inválidos".into());
        }
        if let Some(dynamics) = &saved.state.native_dynamics {
            let engines = self
                .original_physics
                .diesel_vehicle_indices
                .iter()
                .filter(|&&i| {
                    i < n && {
                        let car = &saved.formation.cars[i];
                        car.power_on && car.battery_on && (i == 0 || car.mu_connected)
                    }
                })
                .count();
            if self.original_physics.native.is_none()
                || dynamics.bearing_c.len() != n
                || dynamics.axles.len() != engines
                || dynamics
                    .bearing_c
                    .iter()
                    .any(|t| !t.is_finite() || !(-100.0..=500.0).contains(t))
                || dynamics.axles.iter().any(|a| !a.speed_mps.is_finite())
                || !dynamics.adhesion_factor.is_finite()
                || !(0.05..=2.5).contains(&dynamics.adhesion_factor)
            {
                return Err("La dinámica nativa guardada no corresponde a la formación".into());
            }
        }
        if saved.dispatcher.own_locks.len() > self.graph.nodes_iter().count()
            || saved.dispatcher.own_locks.iter().any(|l| {
                l.owner != self.service_id
                    || self.graph.switch_position(&l.node).is_none()
                    || saved.switches.get(&l.node) != Some(&l.position)
            })
            || saved
                .dispatcher
                .wait_since_s
                .is_some_and(|t| !t.is_finite() || t < 0.)
            || !saved.dispatcher.last_route_search_s.is_finite()
            || saved.dispatcher.last_route_search_s < 0.
        {
            return Err("Enclavamiento guardado inválido".into());
        }
        let mut graph = self.graph.clone();
        // A vector edge can contain several normal signals. Retained blocks
        // beneath the tail and the next block can therefore share that edge.
        if saved.track_reservations.len()
            > saved
                .state
                .path_edges
                .len()
                .saturating_add(self.graph.signals().count())
        {
            return Err("Cantidad de reservas de vía guardadas inválida".into());
        }
        for r in &saved.track_reservations {
            let edge = self
                .graph
                .edge(&r.edge)
                .or_else(|| self.graph.edge(&format!("{}_r", r.edge)));
            if r.owner != self.service_id
                || !r.start_m.is_finite()
                || !r.end_m.is_finite()
                || r.start_m < 0.
                || r.end_m <= r.start_m
                || edge.is_none_or(|e| r.end_m > e.length_m + 0.01)
            {
                return Err(format!(
                    "Reserva inválida: {} [{}, {}], dueño {} (esperado {}), longitud {:?}",
                    r.edge,
                    r.start_m,
                    r.end_m,
                    r.owner,
                    self.service_id,
                    edge.map(|e| e.length_m)
                ));
            }
        }
        for (id, pos) in &saved.switches {
            graph.set_switch(id, *pos).map_err(|e| e.to_string())?;
        }
        for id in saved
            .signal_overrides
            .keys()
            .chain(saved.signal_runtime.keys())
        {
            if graph.signal(id).is_none() {
                return Err(format!("Señal desconocida: {id}"));
            }
        }
        let mut previous_to = None;
        for id in &saved.state.path_edges {
            let edge = graph
                .edge(id)
                .ok_or_else(|| format!("Tramo desconocido: {id}"))?;
            if previous_to
                .as_ref()
                .is_some_and(|node| node != &edge.from.0)
            {
                return Err("La vía guardada no es continua".into());
            }
            previous_to = Some(edge.to.0.clone());
        }
        let pd = PathData::from_path(&saved.state.path_edges, &graph);
        if saved.state.path_edges.is_empty()
            || saved.state.edge_index > pd.edges.len()
            || (saved.state.edge_index == pd.edges.len() && !saved.arrived)
            || !saved.state.pos_on_edge_m.is_finite()
            || saved.state.pos_on_edge_m < 0.0
            || saved.state.pos_on_edge_m
                > pd.edges
                    .get(saved.state.edge_index)
                    .map_or(0.0, |e| e.length_m)
                    + 0.01
            || !saved.state.velocity_mps.is_finite()
            || !(0.0..=150.0).contains(&saved.state.velocity_mps)
            || !saved.state.time_s().is_finite()
            || saved.state.time_s() < 0.0
            || !saved.state.odometer_m.is_finite()
            || saved.state.odometer_m < 0.0
            || !saved.speed_mul.is_finite()
            || !(0.25..=16.0).contains(&saved.speed_mul)
            || ![
                saved.driver_brake,
                saved.driver_throttle,
                saved.driver_direction,
            ]
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            || !saved.sim_time_remainder.is_finite()
            || !(-1e-9..=self.realtime_physics_dt() + 1e-9).contains(&saved.sim_time_remainder)
            || !saved.previous_render_chainage_m.is_finite()
            || saved
                .formation
                .parked_head_chainage_m
                .is_some_and(|v| !v.is_finite() || v < 0.0)
        {
            return Err("La posición o los controles guardados no son válidos".into());
        }
        Ok(())
    }

    pub fn restore_snapshot(&mut self, saved: SessionSnapshot) -> Result<(), String> {
        let host = self.prepare_script_restore(&saved)?;
        self.restore_prepared_snapshot(saved, host)
    }

    pub fn restore_prepared_snapshot(
        &mut self,
        saved: SessionSnapshot,
        host: Option<crate::etcs::ScriptTcsHost>,
    ) -> Result<(), String> {
        self.validate_snapshot(&saved)?;
        match (&host, &saved.script_state) {
            (Some(host), Some(state)) => host.validate_snapshot(state)?,
            (None, None) => (),
            _ => return Err("Estado C# preparado incompatible".into()),
        }
        let mut graph = self.graph.clone();
        for (id, pos) in &saved.switches {
            graph.set_switch(id, *pos).map_err(|e| e.to_string())?;
        }
        let pd = PathData::from_path(&saved.state.path_edges, &graph);
        self.script_tcs = host;
        self.state = saved.state;
        // Reconnecting a saved train must not restart a hose without a WORLD match.
        self.refilling = None;
        self.gameplay = saved.gameplay;
        self.formation = saved.formation;
        self.exterior = saved.exterior;
        self.signal_runtime = saved.signal_runtime;
        self.signal_overrides = saved.signal_overrides;
        self.own_track_reservations = saved.track_reservations;
        self.dispatcher = saved.dispatcher;
        self.dispatcher.other_locks.clear();
        self.external_track_reservations.clear();
        self.graph = graph;
        self.path_data = pd;
        self.driver_throttle = saved.driver_throttle;
        self.driver_brake = saved.driver_brake;
        self.driver_direction = saved.driver_direction;
        self.traffic_brake_assistance = saved.traffic_brake_assistance;
        self.wiper_active = saved.wiper_active;
        self.headlights = saved.headlights.min(2);
        self.cab_light = saved.cab_light;
        self.horn_pressed_until_s = saved.horn_pressed_until_s;
        self.speed_mul = saved.speed_mul;
        self.sim_time_remainder = saved.sim_time_remainder.max(0.0);
        self.previous_render_chainage_m = saved.previous_render_chainage_m;
        self.signal_steps = saved.signal_steps;
        self.arrived = saved.arrived;
        self.rebuild_formation_physics();
        self.evaluate_native_signals();
        // Traffic footprints are restored separately; recompute after that sync,
        // including when the player remains paused on the loaded frame.
        self.native_signals.needs_refresh = true;
        Ok(())
    }

    pub fn operate_car(&mut self, index: usize, action: CarOperation) -> Result<(), String> {
        if self.velocity_mps() > 0.1 {
            return Err("Detené el tren antes de operar la formación".into());
        }
        let parked = index >= self.formation.coupled_count;
        let car = self
            .formation
            .cars
            .get_mut(index)
            .ok_or("Coche desconocido")?;
        match action {
            CarOperation::Handbrake => {
                if parked {
                    return Err(
                        "Acoplá la sección estacionada antes de soltar sus frenos de mano".into(),
                    );
                }
                car.handbrake = !car.handbrake;
            }
            CarOperation::Hose => {
                if index == 0 {
                    return Err(
                        "El primer coche no tiene manguera hacia otro coche delantero".into(),
                    );
                }
                if parked {
                    return Err("Acoplá primero la sección estacionada".into());
                }
                car.hose_connected = !car.hose_connected;
            }
            CarOperation::FrontCock => car.front_cock_open = !car.front_cock_open,
            CarOperation::RearCock => car.rear_cock_open = !car.rear_cock_open,
            CarOperation::Power
            | CarOperation::Battery
            | CarOperation::MultipleUnit
            | CarOperation::TrainSupply => {
                if !car.powered {
                    return Err("Este coche no tiene tracción".into());
                }
                match action {
                    CarOperation::Power => car.power_on = !car.power_on,
                    CarOperation::Battery => car.battery_on = !car.battery_on,
                    CarOperation::TrainSupply => {
                        if !self.original_physics.power_supply.cars.iter().any(|c| {
                            c.vehicle == index
                                && c.params.train_supply_fitted
                                && c.params.manual_train_supply
                        }) {
                            return Err(
                                "Este coche no tiene un interruptor de alimentación de pasajeros"
                                    .into(),
                            );
                        }
                        car.train_supply_switch_on = !car.train_supply_switch_on;
                    }
                    CarOperation::MultipleUnit => car.mu_connected = !car.mu_connected,
                    _ => unreachable!(),
                }
                self.rebuild_formation_physics();
            }
        }
        self.sync_operating_brakes();
        Ok(())
    }

    /// Park a secured trailing section; it remains on the track at its own position.
    pub fn uncouple_after(&mut self, index: usize) -> Result<(), String> {
        let keep = index + 1;
        if self.velocity_mps() > 0.1 {
            return Err("Detené el tren antes de desacoplar".into());
        }
        if self.formation.parked_head_chainage_m.is_some() {
            return Err("Acoplá la sección estacionada antes de hacer otro corte".into());
        }
        if keep >= self.formation.coupled_count {
            return Err("Seleccioná un coche que tenga otro detrás".into());
        }
        if self.formation.cars[keep..].iter().any(|c| !c.handbrake) {
            return Err("Aplicá los frenos de mano de todos los coches que vas a separar".into());
        }
        self.driver_throttle = 0.0;
        self.formation.parked_head_chainage_m = Some(self.head_chainage_m());
        self.formation.coupled_count = keep;
        self.formation.parked_brakes = self.state.brake_system.cylinders.split_off(keep);
        if !self.state.vehicles.is_empty() {
            self.formation.parked_vehicles = self.state.vehicles.split_off(keep);
            self.formation.parked_couplers = self.state.couplers.split_off(keep - 1);
            self.state.vehicle_masses.truncate(keep);
        }
        self.formation.cars[index].rear_cock_open = false;
        self.formation.cars[keep].front_cock_open = false;
        self.formation.cars[keep].hose_connected = false;
        self.rebuild_formation_physics();
        self.sync_operating_brakes();
        Ok(())
    }

    pub fn recouple(&mut self) -> Result<(), String> {
        let parked = self
            .formation
            .parked_head_chainage_m
            .ok_or("No hay una sección separada")?;
        if self.velocity_mps() > 0.1 || (self.head_chainage_m() - parked).abs() > 1.0 {
            return Err("Acercá el enganche a menos de 1 m y detené el tren para acoplar".into());
        }
        self.state
            .brake_system
            .cylinders
            .append(&mut self.formation.parked_brakes);
        if !self.state.vehicles.is_empty() {
            for vehicle in &mut self.formation.parked_vehicles {
                vehicle.velocity_mps = 0.0;
                vehicle.position_m = self.state.odometer_m;
            }
            for coupler in &mut self.formation.parked_couplers {
                coupler.extension_m = 0.0;
            }
            self.state
                .vehicles
                .append(&mut self.formation.parked_vehicles);
            self.state
                .couplers
                .append(&mut self.formation.parked_couplers);
            self.state.vehicle_masses = self.formation.cars.iter().map(|c| c.mass_kg).collect();
        }
        self.formation.coupled_count = self.formation.cars.len();
        self.formation.parked_head_chainage_m = None;
        self.rebuild_formation_physics();
        self.sync_operating_brakes();
        Ok(())
    }

    pub(crate) fn rebuild_formation_physics(&mut self) {
        let n = self.formation.coupled_count;
        let mut consist = self.consist.clone();
        consist.vehicles.truncate(n);
        for (index, (v, op)) in consist
            .vehicles
            .iter_mut()
            .zip(&self.formation.cars)
            .enumerate()
        {
            if let Vehicle::Loco(l) = v
                && !(op.power_on
                    && (l.steam.is_some() || op.battery_on)
                    && (index == 0 || op.mu_connected))
            {
                l.max_power_w = 0.0;
                l.max_tractive_effort_n = 0.0;
                l.tractive_curve = Some(TractiveCurve::default());
                l.diesel_traction = None;
                l.steam = None;
            }
        }
        let mut p = self.original_physics.clone();
        for car in &mut p.power_supply.cars {
            if let Some(op) = self.formation.cars.get(car.vehicle) {
                car.enabled =
                    car.vehicle < n && op.power_on && (car.vehicle == 0 || op.mu_connected);
                car.battery_on = op.battery_on;
                car.master_key_on = op.power_on;
                car.train_supply_switch_on = op.train_supply_switch_on;
            }
        }
        if let Some(rail) = &mut p.rail_adhesion {
            for (i, v) in rail.vehicles.iter_mut().enumerate() {
                v.enabled = v.powered
                    && i < n
                    && self.formation.cars.get(i).is_some_and(|c| {
                        c.power_on && (v.steam || c.battery_on) && (i == 0 || c.mu_connected)
                    });
            }
        }
        for car in &mut p.diesel.cars {
            if let Some(op) = self.formation.cars.get(car.vehicle) {
                car.connected = car.vehicle < n
                    && op.power_on
                    && op.battery_on
                    && (car.vehicle == 0 || op.mu_connected);
                car.battery = op.battery_on;
            }
        }
        p.electric.fallback_cars = crate::electric::ElectricTrainConfig::fallback_curves(&consist);
        for car in &mut p.electric.cars {
            car.enabled = car.vehicle < n
                && self.formation.cars.get(car.vehicle).is_some_and(|op| {
                    op.power_on && op.battery_on && (car.vehicle == 0 || op.mu_connected)
                });
        }
        p.mass_kg = consist.total_mass_kg();
        p.max_power_w = consist.total_max_power_w();
        p.max_tractive_effort_n = consist.total_max_tractive_effort_n();
        p.max_brake_n = consist.total_max_brake_n();
        p.vehicle_davis.truncate(n);
        p.vehicle_lengths_m = consist.vehicle_lengths_m();
        p.diesel_vehicle_indices = consist.diesel_vehicle_indices();
        if let Some(native) = &mut p.native {
            native.vehicles.truncate(n);
            native.vehicle_masses_kg.truncate(n);
        }
        p.davis = p.vehicle_davis.iter().fold(
            openrailsrs_train::DavisCoefficients {
                a_n: 0.0,
                b_n_per_mps: 0.0,
                c_n_per_mps2: 0.0,
            },
            |sum, d| sum.sum(d),
        );
        // Preserve authored aggregate overrides when no cars were removed. Some
        // legacy assets have no per-car Davis data, so a zero sum is not a new
        // resistance model for a resumed game.
        if n == self.consist.vehicles.len() {
            p.davis = self.original_physics.davis.clone();
        } else if p.davis.a_n == 0.0 && p.davis.b_n_per_mps == 0.0 && p.davis.c_n_per_mps2 == 0.0 {
            let fraction = p.mass_kg / self.original_physics.mass_kg;
            p.davis = openrailsrs_train::DavisCoefficients {
                a_n: self.original_physics.davis.a_n * fraction,
                b_n_per_mps: self.original_physics.davis.b_n_per_mps * fraction,
                c_n_per_mps2: self.original_physics.davis.c_n_per_mps2 * fraction,
            };
        }
        p.diesel_engines = consist.diesel_traction_models();
        p.steam_params = consist.aggregate_steam_params();
        p.tractive = consist.aggregate_tractive_curve();
        if p.tractive.points.is_empty() {
            p.tractive =
                TractiveCurve::from_power_and_effort(p.max_power_w, p.max_tractive_effort_n);
        }
        if let (Some(native), Some(dynamics)) = (&p.native, &mut self.state.native_dynamics) {
            dynamics.bearing_c.resize(n, native.environment.ambient_c);
            dynamics.axles = p
                .diesel_vehicle_indices
                .iter()
                .map(|i| {
                    self.physics
                        .diesel_vehicle_indices
                        .iter()
                        .position(|old| old == i)
                        .and_then(|index| dynamics.axles.get(index).cloned())
                        .unwrap_or(crate::native_dynamics::NativeAxleState {
                            speed_mps: self.state.velocity_mps,
                        })
                })
                .collect();
        }
        self.physics = p;
        self.state.electric.pantograph_command_up = self.exterior.pantograph_command_up;
        crate::electric::advance(&mut self.state, &self.path_data, &self.physics.electric, 0.);
    }

    pub(crate) fn sync_operating_brakes(&mut self) {
        for cylinder in &mut self.state.brake_system.cylinders {
            cylinder.air_vented = false;
        }
        let mut isolated = false;
        let mut vented = false;
        for i in 0..self.formation.coupled_count {
            let car = &self.formation.cars[i];
            if i > 0 {
                let previous = &self.formation.cars[i - 1];
                if !previous.rear_cock_open || !car.front_cock_open {
                    isolated = true;
                }
                if !car.hose_connected {
                    // A disconnected open hose vents the connected front section too.
                    if previous.rear_cock_open {
                        for cyl in &mut self.state.brake_system.cylinders[..i] {
                            cyl.air_vented = true;
                        }
                    }
                    vented = car.front_cock_open;
                }
            }
            let cyl = &mut self.state.brake_system.cylinders[i];
            cyl.handbrake_force_n = if car.handbrake {
                cyl.max_force_n * 0.65
            } else {
                0.0
            };
            cyl.air_isolated = isolated;
            cyl.air_vented = vented;
        }
    }

    /// Used by body/bogie rendering: the parked section has an independent head.
    pub fn presentation_chainage_at_offset(&self, offset: f64, remainder: f64) -> f64 {
        self.presentation_chainage_for_car(offset, offset, remainder)
    }

    pub fn presentation_chainage_for_car(
        &self,
        offset: f64,
        car_offset: f64,
        remainder: f64,
    ) -> f64 {
        let first_parked = self.formation.offset_m(self.formation.coupled_count);
        if car_offset <= first_parked + 0.001
            && let Some(head) = self.formation.parked_head_chainage_m
        {
            head + offset
        } else {
            self.render_head_chainage_m(remainder) + offset
        }
    }

    pub fn occupied_edges(&self) -> HashMap<String, String> {
        let mut occupied = self.external_occupancy.clone();
        occupied.extend(self.own_occupied_edges());
        occupied
    }

    pub fn own_occupied_edges(&self) -> HashMap<String, String> {
        let mut occupied = HashMap::new();
        for (i, car) in self.formation.cars.iter().enumerate() {
            let head = if i < self.formation.coupled_count {
                self.head_chainage_m()
            } else {
                self.formation
                    .parked_head_chainage_m
                    .unwrap_or(self.head_chainage_m())
            };
            let center = head + self.formation.center_offset_m(i);
            let rear = (center - car.length_m * 0.5).max(0.0);
            let front = center + car.length_m * 0.5;
            let mut start = 0.0;
            for (edge, data) in self.state.path_edges.iter().zip(&self.path_data.edges) {
                let end = start + data.length_m;
                // Intersect the full vehicle interval. Three point samples can
                // skip a short crossover underneath the middle of a vehicle.
                if end >= rear && start <= front {
                    let owner = if i < self.formation.coupled_count {
                        self.service_id.clone()
                    } else {
                        format!("{} · estacionado", self.service_id)
                    };
                    let base = edge.strip_suffix("_r").unwrap_or(edge);
                    occupied.insert(base.to_string(), owner.clone());
                    occupied.insert(format!("{base}_r"), owner);
                }
                start = end;
            }
        }
        occupied
    }

    /// Authority ends at the first section occupied by another service, even
    /// on imported routes with incomplete signal scripts. Never ignore the tail.
    pub fn distance_to_occupied_block_m(&self) -> Option<f64> {
        let mut distance = -self.pos_on_edge_m();
        let mut result = None;
        for (index, id) in self
            .state
            .path_edges
            .iter()
            .enumerate()
            .skip(self.state.edge_index)
        {
            let data = self.path_data.edges.get(index)?;
            if self.edge_is_occupied(id, &self.external_occupancy) {
                result = Some(distance.max(0.));
                break;
            }
            for r in &self.external_track_reservations {
                if base_edge(id) == r.edge {
                    let start = if id.ends_with("_r") {
                        data.length_m - r.end_m
                    } else {
                        r.start_m
                    };
                    let end = if id.ends_with("_r") {
                        data.length_m - r.start_m
                    } else {
                        r.end_m
                    };
                    if distance + end > 0. {
                        let at = (distance + start).max(0.);
                        result = Some(result.map_or(at, |old: f64| old.min(at)));
                    }
                }
            }
            distance += data.length_m;
        }
        if self.dispatcher.protected_signal.is_none() && !self.dispatcher.waiting_for.is_empty() {
            let at = (self.path_data.edges.get(self.state.edge_index)?.length_m
                - self.pos_on_edge_m()
                - 2.)
                .max(0.);
            result = Some(result.map_or(at, |old| old.min(at)));
        }
        result
    }

    /// Reverse graph edges refer to the same physical track section.
    fn edge_is_occupied(&self, id: &str, occupied: &HashMap<String, String>) -> bool {
        let base = id.strip_suffix("_r").unwrap_or(id);
        occupied.contains_key(base) || occupied.contains_key(&format!("{base}_r"))
    }

    pub fn dispatch_signal(
        &mut self,
        id: &str,
        aspect: Option<SignalAspect>,
    ) -> Result<(), String> {
        let sig = self.graph.signal(id).ok_or("Señal desconocida")?;
        if matches!(aspect, Some(SignalAspect::Clear | SignalAspect::Caution))
            && self.edge_is_occupied(&sig.edge_id, &self.occupied_edges())
        {
            return Err("No se puede autorizar una señal en un tramo ocupado".into());
        }
        if let Some(aspect) = aspect {
            self.signal_overrides.insert(id.into(), aspect);
            self.signal_runtime.insert(id.into(), aspect);
        } else {
            self.signal_overrides.remove(id);
            self.signal_runtime.insert(id.into(), sig.aspect);
        }
        Ok(())
    }

    pub(crate) fn adopt_dispatch_path(&mut self, path: Vec<String>) -> Result<(), String> {
        let mut before = 0.;
        let mut chainages = HashMap::new();
        for edge in &path {
            let e = self.graph.edge(edge).ok_or("Tramo desconocido")?;
            before += e.length_m;
            chainages.insert(e.to.0.clone(), before);
        }
        let mut targets = self.gameplay.stop_targets.clone();
        for target in targets.iter_mut().skip(self.gameplay.next_stop_idx) {
            let position = chainages
                .get(&target.node_id)
                .ok_or("El itinerario omite una estación")?;
            target.cum_dist_m = position
                + self
                    .stop_offsets
                    .get(&target.node_id)
                    .copied()
                    .unwrap_or(0.);
            if target.cum_dist_m < self.head_chainage_m() {
                return Err("Parada detrás del tren".into());
            }
        }
        self.gameplay.stop_targets = targets;
        self.path_data = PathData::from_path(&path, &self.graph);
        self.state.path_edges = path;
        Ok(())
    }

    pub fn dispatch_switch(&mut self, id: &str) -> Result<(), String> {
        if let Some(lock) = self
            .dispatcher
            .own_locks
            .iter()
            .chain(&self.dispatcher.other_locks)
            .find(|l| l.node == id)
        {
            return Err(format!(
                "Cambio reservado por {}; se libera cuando pase la cola",
                lock.owner
            ));
        }
        let node = self.graph.node(id).ok_or("Cambio desconocido")?;
        if !matches!(node.kind, NodeKind::Switch { .. }) {
            return Err("Este nodo no es un cambio".into());
        }
        let occupied = self.occupied_edges();
        if self.graph.edges_iter().any(|(eid, e)| {
            (e.from.0 == id || e.to.0 == id) && self.edge_is_occupied(eid, &occupied)
        }) {
            return Err("No se puede mover un cambio ocupado por la formación".into());
        }
        let previous = self.graph.switch_position(id).unwrap_or_default();
        let next = if previous == SwitchPosition::Straight {
            SwitchPosition::Diverging
        } else {
            SwitchPosition::Straight
        };
        let mut graph = self.graph.clone();
        graph.set_switch(id, next).map_err(|e| e.to_string())?;
        // Re-route only if the switch belongs to the remaining player's path.
        let edge = self
            .current_edge_id()
            .and_then(|eid| graph.edge(eid))
            .ok_or("Sin posición en vía")?;
        let on_future = self.state.path_edges[self.state.edge_index..]
            .iter()
            .any(|eid| graph.edge(eid).is_some_and(|e| e.from.0 == id));
        if on_future {
            if self.formation.parked_head_chainage_m.is_some() {
                return Err("Acoplá la formación antes de cambiar su itinerario".into());
            }
            let tail = crate::path::edge_path(&graph, &edge.to.0, &self.gameplay.destination_node)
                .map_err(|_| "El cambio no permite llegar al destino del servicio")?;
            let mut path = self.state.path_edges[..=self.state.edge_index].to_vec();
            path.extend(tail);
            let mut cumulative = 0.0;
            let mut chainages = HashMap::new();
            for eid in &path {
                let e = graph.edge(eid).ok_or("Tramo de itinerario desconocido")?;
                cumulative += e.length_m;
                chainages.insert(e.to.0.clone(), cumulative);
            }
            let mut targets = self.gameplay.stop_targets.clone();
            for target in targets.iter_mut().skip(self.gameplay.next_stop_idx) {
                let distance = chainages
                    .get(&target.node_id)
                    .ok_or("El cambio deja una estación fuera del itinerario")?;
                target.cum_dist_m = distance
                    + self
                        .stop_offsets
                        .get(&target.node_id)
                        .copied()
                        .unwrap_or(0.0);
                if target.cum_dist_m < self.head_chainage_m() {
                    return Err("El cambio dejaría una parada detrás del tren".into());
                }
            }
            self.gameplay.stop_targets = targets;
            self.path_data = PathData::from_path(&path, &graph);
            self.state.path_edges = path;
        }
        self.graph = graph;
        Ok(())
    }
}

/// Stable signature across processes; changes in geometry, timetable or rolling stock invalidate saves.
pub(crate) fn content_signature(
    graph: &openrailsrs_track::TrackGraph,
    scenario: &openrailsrs_scenarios::ScenarioFile,
    consist: &Consist,
    electric: &crate::electric::ElectricTrainConfig,
) -> String {
    let mut h = 0xcbf29ce484222325_u64;
    let mut feed = |s: &str| {
        for b in s.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x100000001b3);
        }
    };
    let mut route = scenario.route.clone();
    route.path.clear(); // Absolute launch copies and source-relative scenarios describe the same content.
    feed(&format!(
        "{:?}{:?}{:?}",
        scenario.scenario, route, scenario.simulation
    ));
    for (_, e) in graph.edges_iter() {
        feed(&format!("{e:?}"));
    }
    for (_, n) in graph.nodes_iter() {
        feed(&format!("{n:?}"));
    }
    for signal in graph.signals() {
        feed(&format!("{signal:?}"));
    }
    feed(&format!("{consist:?}{electric:?}"));
    feed(&format!("{:?}", scenario.train.electric_pickups));
    format!("{h:016x}")
}

impl LiveDriveSession {
    pub fn expand_dispatch_network(
        &mut self,
        full: &openrailsrs_track::TrackGraph,
        scenario: &openrailsrs_scenarios::ScenarioFile,
    ) -> Result<(), String> {
        self.graph = full
            .with_service_overlay(&self.graph)
            .map_err(|e| e.to_string())?;
        for signal in self.graph.signals() {
            self.signal_runtime
                .entry(signal.id.clone())
                .or_insert(signal.aspect);
        }
        self.content_signature = content_signature(
            &self.graph,
            scenario,
            &self.consist,
            &self.original_physics.electric,
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    fn session(name: &str) -> LiveDriveSession {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples")
            .join(name)
            .join("scenario.toml");
        let scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
        LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap()
    }
    #[test]
    fn save_restores_physics_and_next_steps_without_divergence() {
        let mut original = session("smoke");
        original.traffic_brake_assistance = false;
        original.driver_direction = 1.0;
        original.driver_throttle = 0.65;
        original.step_realtime(13.0, |_| {});
        original.driver_brake = 0.37;
        original.step_realtime(0.25, |_| {});
        let serialized = serde_json::to_vec(&original.snapshot()).unwrap();
        let mut resumed = session("smoke");
        resumed
            .restore_snapshot(serde_json::from_slice(&serialized).unwrap())
            .unwrap();
        assert!(!resumed.traffic_brake_assistance);
        original.step_realtime(4.0, |_| {});
        resumed.step_realtime(4.0, |_| {});
        assert!((original.velocity_mps() - resumed.velocity_mps()).abs() < 1e-9);
        assert!((original.state.odometer_m - resumed.state.odometer_m).abs() < 1e-9);
        assert_eq!(original.state.diesel_rpm, resumed.state.diesel_rpm);
        assert_eq!(original.exterior, resumed.exterior);
        assert!(
            (original.state.brake_system.total_force_n(10.0)
                - resumed.state.brake_system.total_force_n(10.0))
            .abs()
                < 1e-6
        );
    }
    #[test]
    fn invalid_save_is_rejected_without_mutating_running_train() {
        let mut s = session("smoke");
        s.driver_throttle = 0.7;
        let mut invalid = s.snapshot();
        invalid.state.edge_index = usize::MAX;
        assert!(s.restore_snapshot(invalid).is_err());
        assert_eq!(s.driver_throttle, 0.7);
        assert_eq!(s.time_s(), 0.0);
        let mut invalid = s.snapshot();
        invalid.formation.coupled_count = 0;
        assert!(s.restore_snapshot(invalid).is_err());
        assert_eq!(s.formation.coupled_count, s.consist.vehicles.len());
    }
    #[test]
    fn handbrake_has_real_force_and_reduces_acceleration() {
        let mut free = session("smoke");
        let mut secured = session("smoke");
        secured.operate_car(0, CarOperation::Handbrake).unwrap();
        for s in [&mut free, &mut secured] {
            s.driver_direction = 1.0;
            s.driver_throttle = 1.0;
            s.step_realtime(10.0, |_| {});
        }
        assert!(secured.velocity_mps() < free.velocity_mps());
        assert!(secured.state.brake_system.cylinders[0].handbrake_force_n > 0.0);
    }
    #[test]
    fn open_hose_vents_train_but_closed_cocks_isolate_it() {
        let mut s = session("chiltern_local");
        s.operate_car(1, CarOperation::Hose).unwrap();
        s.step_realtime(0.1, |_| {});
        assert!(s.state.brake_system.cylinders.iter().all(|c| c.air_vented));
        assert!(s.state.brake_system.total_force_n(0.0) > 0.0);
        s.operate_car(1, CarOperation::Hose).unwrap();
        s.operate_car(0, CarOperation::RearCock).unwrap();
        s.operate_car(1, CarOperation::FrontCock).unwrap();
        s.operate_car(1, CarOperation::Hose).unwrap();
        assert!(!s.state.brake_system.cylinders[0].air_vented);
        assert!(s.state.brake_system.cylinders[1].air_isolated);
        assert!(!s.state.brake_system.cylinders[1].air_vented);
    }
    #[test]
    fn split_changes_mass_and_parks_body_bogies_and_wheels_on_same_clock() {
        let mut s = session("chiltern_local");
        let mass = s.physics.mass_kg;
        let n = s.formation.coupled_count;
        assert!(s.uncouple_after(2).is_err());
        for i in 3..n {
            s.operate_car(i, CarOperation::Handbrake).unwrap();
        }
        let parked = s.head_chainage_m();
        s.uncouple_after(2).unwrap();
        assert_eq!(s.formation.coupled_count, 3);
        assert!(s.physics.mass_kg < mass);
        let car = s.formation.offset_m(3);
        s.state.pos_on_edge_m += 40.0;
        s.previous_render_chainage_m = s.head_chainage_m();
        assert!(
            (s.presentation_chainage_for_car(car + 7.0, car, 0.0) - (parked + car + 7.0)).abs()
                < 1e-9
        );
        assert!((s.presentation_chainage_for_car(0.0, car, 0.0) - parked).abs() < 1e-9);
        assert!(s.recouple().is_err());
        let saved = s.snapshot();
        let mut resumed = session("chiltern_local");
        resumed.restore_snapshot(saved).unwrap();
        assert_eq!(resumed.formation.coupled_count, 3);
        assert_eq!(resumed.formation.parked_head_chainage_m, Some(parked));
        s.state.pos_on_edge_m -= 40.0;
        s.recouple().unwrap();
        assert_eq!(s.formation.coupled_count, n);
        assert!((s.physics.mass_kg - mass).abs() < 0.01);
        assert_eq!(s.state.brake_system.cylinders.len(), n);
    }
    #[test]
    fn reverse_moves_backward_and_cannot_be_selected_while_moving() {
        let mut s = session("smoke");
        s.driver_direction = 1.0;
        s.driver_throttle = 1.0;
        s.step_realtime(12.0, |_| {});
        assert!(s.set_direction(0.0).is_err());
        s.state.velocity_mps = 0.0;
        for v in &mut s.state.vehicles {
            v.velocity_mps = 0.0;
        }
        let head = s.head_chainage_m();
        let odometer = s.state.odometer_m;
        s.set_direction(0.0).unwrap();
        s.step_realtime(2.0, |_| {});
        assert!(s.head_chainage_m() < head);
        assert!(s.state.odometer_m > odometer);
    }
    #[test]
    fn dispatch_orders_persist_and_occupied_switches_are_locked() {
        let mut s = session("chiltern_local");
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chiltern_local/scenario.toml");
        let scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
        let graph = openrailsrs_route::load_track_graph_from_route_dir(
            path.parent().unwrap().parent().unwrap().join("chiltern"),
        )
        .unwrap();
        s.expand_dispatch_network(&graph, &scenario).unwrap();
        assert!(s.dispatch_switch("n95").is_err());
        let id = s
            .graph
            .signals()
            .find(|sig| sig.edge_id == s.current_edge_id().unwrap())
            .unwrap()
            .id
            .clone();
        assert!(s.dispatch_signal(&id, Some(SignalAspect::Clear)).is_err());
        s.dispatch_signal(&id, Some(SignalAspect::Stop)).unwrap();
        s.step_realtime(2.0, |_| {});
        assert_eq!(s.signal_aspect(&id), Some(SignalAspect::Stop));
        let path_before = s.state.path_edges.clone();
        let free = s
            .graph
            .nodes_iter()
            .filter(|(_, n)| matches!(n.kind, NodeKind::Switch { .. }))
            .map(|(id, _)| id.to_string())
            .find(|id| {
                !path_before.iter().any(|e| {
                    s.graph
                        .edge(e)
                        .is_some_and(|e| e.from.0 == *id || e.to.0 == *id)
                })
            })
            .unwrap();
        let previous = s.graph.switch_position(&free).unwrap_or_default();
        s.dispatch_switch(&free).unwrap();
        assert_ne!(s.graph.switch_position(&free).unwrap(), previous);
        assert_eq!(s.state.path_edges, path_before);
    }
    #[test]
    fn cutting_every_powered_unit_removes_traction() {
        let mut s = session("chiltern_local");
        for i in 0..s.formation.cars.len() {
            if s.formation.cars[i].powered {
                s.operate_car(i, CarOperation::Power).unwrap();
            }
        }
        assert_eq!(s.physics.max_power_w, 0.0);
        assert_eq!(s.physics.max_tractive_effort_n, 0.0);
        assert!(s.physics.diesel_engines.is_empty());
    }
}
