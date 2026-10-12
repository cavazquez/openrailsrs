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
    /// The detached portion retains the path and orientation on which it stopped.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parked_path_edges: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parked_path_reversed: Option<bool>,
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
            parked_path_edges: vec![],
            parked_path_reversed: None,
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
    #[serde(default)]
    pub operational_operation: usize,
    #[serde(default)]
    pub operational_leg: usize,
    #[serde(default)]
    pub operational_prefix_m: f64,
    #[serde(default)]
    pub operational_start_m: f64,
    #[serde(default)]
    pub operational_wait_started: Option<f64>,
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
    pub wiper_mode: crate::wiper::WiperMode,
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
    pub fn reset_emergency(&mut self) -> Result<(), String> {
        if self.velocity_mps() > 0.1 || self.driver_throttle > 0.0 {
            return Err("Detené el tren y llevá el regulador a cero antes de rearmar".into());
        }
        if self
            .state
            .brake_system
            .cylinders
            .iter()
            .any(|c| c.air_vented)
        {
            return Err(
                "Conectá las mangueras o cerrá las llaves abiertas antes de rearmar".into(),
            );
        }
        self.state.brake_system.emergency = false;
        Ok(())
    }

    pub fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            operational_operation: self.operational_operation,
            operational_leg: self.operational_leg,
            operational_prefix_m: self.operational_prefix_m,
            operational_start_m: self.operational_start_m,
            operational_wait_started: self.operational_wait_started,
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
            wiper_mode: self.selected_wiper_mode(),
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
        if saved.state.cab_equipment.iter().any(|c| !c.valid())
            || (!saved.state.cab_equipment.is_empty()
                && saved
                    .state
                    .cab_equipment
                    .iter()
                    .map(|c| &c.config)
                    .collect::<Vec<_>>()
                    != self.original_cab_equipment.iter().collect::<Vec<_>>())
            || saved
                .state
                .weather_track
                .as_ref()
                .is_some_and(|p| !p.valid())
            || !saved.state.driving_report.valid()
            || saved.state.learning.as_ref().is_some_and(|s| !s.valid())
        {
            return Err("Informe de conducción o progreso de práctica inválido".into());
        }
        if !saved.state.ambient_temperature_c.is_finite()
            || !(-100.0..=65.0).contains(&saved.state.ambient_temperature_c)
        {
            return Err("Temperatura ambiente guardada inválida".into());
        }
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
        if (!self.operational_legs.is_empty()
            && (saved.operational_leg >= self.operational_legs.len()
                || saved.state.path_reversed
                    != self.operational_legs[saved.operational_leg].reversed
                || saved.operational_operation
                    > self.operational_legs[saved.operational_leg]
                        .operations
                        .len()))
            || (self.operational_legs.is_empty() && saved.operational_operation != 0)
            || ![saved.operational_prefix_m, saved.operational_start_m]
                .iter()
                .all(|n| n.is_finite() && *n >= 0.0)
            || saved
                .operational_wait_started
                .is_some_and(|n| !n.is_finite() || n < 0.0 || n > saved.state.time.0)
        {
            return Err("Tramo operativo guardado inválido".into());
        }
        let n = saved.formation.coupled_count;
        if saved.gameplay.shunting_steps != self.gameplay.shunting_steps
            || saved.gameplay.shunting_completed_s.len() > saved.gameplay.shunting_steps.len()
            || saved
                .gameplay
                .shunting_completed_s
                .iter()
                .any(|t| !t.is_finite() || *t < 0.0 || *t > saved.state.time.0)
            || saved
                .gameplay
                .shunting_completed_s
                .windows(2)
                .any(|pair| pair[0] > pair[1])
        {
            return Err("Objetivos de maniobras guardados inválidos".into());
        }
        if !saved.state.brake_system.independent_command.is_finite()
            || !(0.0..=1.0).contains(&saved.state.brake_system.independent_command)
            || saved
                .state
                .brake_system
                .cylinders
                .iter()
                .any(|c| !c.independent_state_valid())
            || saved.state.brake_system.circuits.iter().any(|c| {
                !c.valid()
                    || self
                        .original_physics
                        .power_supply
                        .cars
                        .iter()
                        .all(|p| p.vehicle != c.vehicle)
            })
        {
            return Err("Circuito de frenos guardado inválido".into());
        }
        let circuits = &saved.state.brake_system.circuits;
        let expected: Vec<_> = self
            .consist
            .vehicles
            .iter()
            .enumerate()
            .filter_map(|(i, v)| {
                let profile = match v {
                    Vehicle::Loco(l) => &l.brake_profile,
                    Vehicle::Wagon(w) => &w.brake_profile,
                };
                profile.air_circuit.as_ref().map(|p| (i, p))
            })
            .collect();
        if !circuits.is_empty()
            && (circuits.len() != expected.len()
                || circuits
                    .iter()
                    .zip(&expected)
                    .any(|(c, (i, p))| c.vehicle != *i || &c.params != *p))
        {
            return Err("El circuito guardado no coincide con el equipo original".into());
        }
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
        if !(-5..=5).contains(&saved.dispatcher.priority)
            || saved.dispatcher.grant_reason.len() > 4096
            || saved.dispatcher.own_locks.len() > self.graph.nodes_iter().count()
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
        if saved.formation.parked_path_edges.len() > 50000
            || (saved.formation.parked_head_chainage_m.is_none()
                && (!saved.formation.parked_path_edges.is_empty()
                    || saved.formation.parked_path_reversed.is_some()))
        {
            return Err("Itinerario de la sección estacionada inválido".into());
        }
        let mut previous = None;
        for id in &saved.formation.parked_path_edges {
            let edge = graph.edge(id).ok_or("Vía estacionada desconocida")?;
            if previous.is_some_and(|node: &str| node != edge.from.0) {
                return Err("La vía estacionada no es continua".into());
            }
            previous = Some(edge.to.0.as_str());
        }
        if !saved.formation.parked_path_edges.is_empty() {
            let parked = PathData::from_path(&saved.formation.parked_path_edges, &graph);
            if saved
                .formation
                .parked_head_chainage_m
                .is_none_or(|head| head > parked.total_length_m())
            {
                return Err("La sección estacionada queda fuera de su vía".into());
            }
        }
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
        self.operational_leg = saved.operational_leg;
        self.operational_operation = saved.operational_operation;
        self.operational_prefix_m = saved.operational_prefix_m;
        self.operational_start_m = saved.operational_start_m;
        self.operational_wait_started = saved.operational_wait_started;
        self.state = saved.state;
        if self.state.cab_equipment.is_empty() {
            self.state.cab_equipment = self
                .original_cab_equipment
                .iter()
                .cloned()
                .map(crate::cab_equipment::CabEquipmentState::new)
                .collect();
        }
        if self.state.brake_system.circuits.is_empty() {
            self.state.brake_system.circuits = self
                .consist
                .vehicles
                .iter()
                .enumerate()
                .filter_map(|(i, v)| {
                    let profile = match v {
                        Vehicle::Loco(l) => &l.brake_profile,
                        Vehicle::Wagon(w) => &w.brake_profile,
                    };
                    profile
                        .air_circuit
                        .clone()
                        .map(|p| crate::brake_circuit::AirCircuit::new(i, p))
                })
                .collect();
        }
        // Reconnecting a saved train must not restart a hose without a WORLD match.
        self.refilling = None;
        self.gameplay = saved.gameplay;
        self.formation = saved.formation;
        if self.formation.parked_head_chainage_m.is_some() {
            if self.formation.parked_path_edges.is_empty() {
                self.formation
                    .parked_path_edges
                    .clone_from(&self.state.path_edges);
            }
            self.parked_path_data = Some(PathData::from_path(
                &self.formation.parked_path_edges,
                &graph,
            ));
        } else {
            self.parked_path_data = None;
        }
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
        self.wiper_mode = saved.wiper_mode;
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
        self.update_shunting();
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
        self.formation
            .parked_path_edges
            .clone_from(&self.state.path_edges);
        self.formation.parked_path_reversed = Some(self.state.path_reversed);
        self.parked_path_data = Some(PathData::from_path(&self.state.path_edges, &self.graph));
        self.formation.coupled_count = keep;
        if self.state.path_reversed {
            let anchor = self.head_chainage_m()
                - self.formation.cars[keep..]
                    .iter()
                    .map(|car| car.length_m)
                    .sum::<f64>();
            let odometer = self.state.odometer_m;
            crate::live_drive::apply_start_offset(&mut self.state, &self.path_data, anchor);
            self.state.odometer_m = odometer;
            self.previous_render_chainage_m = anchor;
        }
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
        self.update_shunting();
        Ok(())
    }

    pub fn recouple(&mut self) -> Result<(), String> {
        let parked = self
            .formation
            .parked_head_chainage_m
            .ok_or("No hay una sección separada")?;
        let prefix = self.formation.length_m();
        let full_length = self.formation.cars.iter().map(|c| c.length_m).sum::<f64>();
        let at = self.position_at_head_offset(-prefix);
        let (edges, pd) = self.presentation_path_for_car(-prefix);
        let target = PathData::position_at_odometer(
            edges,
            &pd.edges,
            parked + self.parked_car_offset(-prefix),
        );
        let normalise = |(id, position): (String, f64)| {
            let edge = self.graph.edge(&id).unwrap();
            (
                base_edge(&id).to_string(),
                if id.ends_with("_r") {
                    edge.length_m - position
                } else {
                    position
                },
            )
        };
        if self.velocity_mps() > 0.1
            || !at.zip(target).is_some_and(|(a, b)| {
                let a = normalise(a);
                let b = normalise(b);
                a.0 == b.0 && (a.1 - b.1).abs() <= 1.0
            })
        {
            return Err("Acercá el enganche a menos de 1 m y detené el tren para acoplar".into());
        }
        let anchor = self.head_chainage_m()
            + if self.state.path_reversed {
                full_length - prefix
            } else {
                0.0
            };
        for i in self.formation.coupled_count..self.formation.cars.len() {
            let offset = self.formation.center_offset_m(i);
            let old = self.presentation_position_for_car(offset, offset, 0.0);
            let next = PathData::position_at_odometer(
                &self.state.path_edges,
                &self.path_data.edges,
                anchor
                    + if self.state.path_reversed {
                        -full_length - offset
                    } else {
                        offset
                    },
            );
            if !old.zip(next).is_some_and(|(a, b)| {
                let a = normalise(a);
                let b = normalise(b);
                a.0 == b.0 && (a.1 - b.1).abs() <= 1.0
            }) {
                return Err(
                    "La vía actual no conserva la posición de la sección estacionada".into(),
                );
            }
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
        self.formation.parked_path_edges.clear();
        self.formation.parked_path_reversed = None;
        self.parked_path_data = None;
        if self.state.path_reversed {
            let odometer = self.state.odometer_m;
            crate::live_drive::apply_start_offset(&mut self.state, &self.path_data, anchor);
            self.state.odometer_m = odometer;
            self.previous_render_chainage_m = anchor;
        }
        self.rebuild_formation_physics();
        self.sync_operating_brakes();
        self.update_shunting();
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
        p.power_supply.active_fraction = n as f64 / self.consist.vehicles.len().max(1) as f64;
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
        p.electric.length_m = self.formation.length_m();
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
            head + self.parked_car_offset(offset)
        } else {
            self.render_head_chainage_m(remainder) + self.oriented_car_offset(offset)
        }
    }

    pub fn presentation_path_for_car(&self, car_offset: f64) -> (&[String], &PathData) {
        if self.formation.parked_head_chainage_m.is_some()
            && car_offset <= self.formation.offset_m(self.formation.coupled_count) + 0.001
            && let Some(pd) = &self.parked_path_data
        {
            (&self.formation.parked_path_edges, pd)
        } else {
            (&self.state.path_edges, &self.path_data)
        }
    }

    pub fn presentation_position_for_car(
        &self,
        offset: f64,
        car_offset: f64,
        remainder: f64,
    ) -> Option<(String, f64)> {
        let chainage = self.presentation_chainage_for_car(offset, car_offset, remainder);
        let (edges, pd) = self.presentation_path_for_car(car_offset);
        PathData::position_at_odometer(edges, &pd.edges, chainage.max(0.0))
    }

    pub fn occupied_edges(&self) -> HashMap<String, String> {
        let mut occupied = self.external_occupancy.clone();
        occupied.extend(self.own_occupied_edges());
        occupied
    }

    pub fn own_occupied_edges(&self) -> HashMap<String, String> {
        let mut occupied = HashMap::new();
        for interval in self.own_track_occupancy() {
            occupied.insert(format!("{}_r", interval.edge), interval.owner.clone());
            occupied.insert(interval.edge, interval.owner);
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
            // The parked section must remain on the immutable path prefix.
            // Re-routing track ahead of both sections is safe and enables yard work.
            let unchanged_end: f64 = self.path_data.edges[..=self.state.edge_index]
                .iter()
                .map(|e| e.length_m)
                .sum();
            if self
                .formation
                .parked_head_chainage_m
                .is_some_and(|p| p > unchanged_end)
            {
                return Err("La sección estacionada queda fuera del itinerario nuevo".into());
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
        self.update_shunting();
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
    #[test]
    fn timetable_operations_require_station_alignment_and_survive_save() {
        use openrailsrs_scenarios::model::{RouteLeg, TimetableAction, TimetableOperation};
        let mut s = session("smoke");
        s.state.path_edges = vec!["e1".into()];
        s.path_data = PathData::from_path(&s.state.path_edges, &s.graph);
        s.state.pos_on_edge_m = 2000.0;
        s.previous_render_chainage_m = 2000.0;
        let count = s.formation.coupled_count;
        assert!(count > 1);
        s.operational_legs = vec![RouteLeg {
            service_id: None,
            not_before_s: None,
            start: "yard_a".into(),
            destination: "mid".into(),
            edges: vec!["e1".into()],
            start_offset_m: 2000.0,
            end_chainage_m: 4000.0,
            reversed: false,
            wait_s: 0.0,
            stops: vec![],
            switches: vec![],
            operations: vec![
                TimetableOperation {
                    chainage_m: 2000.0,
                    section: "tail".into(),
                    manual: true,
                    action: TimetableAction::Detach { keep: 1 },
                },
                TimetableOperation {
                    chainage_m: 2000.0,
                    section: "tail".into(),
                    manual: false,
                    action: TimetableAction::PickUp { count },
                },
            ],
        }];
        s.tick_timetable_operation(false);
        assert_eq!(
            s.formation.coupled_count, count,
            "manual work must wait for F9"
        );
        for car in &mut s.formation.cars[1..] {
            car.handbrake = true;
        }
        s.uncouple_after(0).unwrap();
        s.state.pos_on_edge_m += 100.0;
        s.tick_timetable_operation(false);
        assert_eq!(
            s.operational_operation, 0,
            "a cut made elsewhere cannot satisfy the station task"
        );
        s.state.pos_on_edge_m -= 100.0;
        s.tick_timetable_operation(false);
        assert_eq!(s.operational_operation, 1);
        let saved = s.snapshot();
        s.restore_snapshot(saved).unwrap();
        assert_eq!(s.operational_operation, 1);
        s.state.pos_on_edge_m += 100.0;
        s.tick_timetable_operation(true);
        assert_eq!(
            s.formation.coupled_count, 1,
            "automatic pickup cannot teleport the tail"
        );
        s.state.pos_on_edge_m -= 100.0;
        s.tick_timetable_operation(false);
        assert_eq!(s.operational_operation, 2);
        assert_eq!(s.formation.coupled_count, count);
        assert!(
            s.formation
                .cars
                .iter()
                .all(|c| !c.handbrake && c.hose_connected)
        );
    }

    #[test]
    fn detached_tail_retains_its_track_during_reversal_save_and_rejoin() {
        let mut s = session("smoke");
        let mut reverse = s.graph.edge("e1").unwrap().clone();
        let edge_length = reverse.length_m;
        reverse.id = openrailsrs_core::EdgeId("e1_r".into());
        std::mem::swap(&mut reverse.from, &mut reverse.to);
        s.graph.insert_edge(reverse).unwrap();
        s.state.path_edges = vec!["e1".into()];
        s.path_data = PathData::from_path(&s.state.path_edges, &s.graph);
        s.state.pos_on_edge_m = 2000.0;
        s.previous_render_chainage_m = 2000.0;
        let full = s.formation.length_m();
        let poses = (0..s.formation.cars.len())
            .map(|i| {
                let offset = s.formation.center_offset_m(i);
                s.presentation_position_for_car(offset, offset, 0.0)
                    .unwrap()
                    .1
            })
            .collect::<Vec<_>>();
        for car in &mut s.formation.cars[1..] {
            car.handbrake = true;
        }
        s.uncouple_after(0).unwrap();
        let make =
            |edges: Vec<String>, reversed, start, end| openrailsrs_scenarios::model::RouteLeg {
                operations: vec![],
                service_id: None,
                not_before_s: None,
                start: "yard_a".into(),
                destination: "mid".into(),
                edges,
                start_offset_m: start,
                end_chainage_m: end,
                reversed,
                wait_s: 0.0,
                stops: vec![],
                switches: vec![],
            };
        s.operational_legs = vec![
            make(vec!["e1".into()], false, full, 2000.0),
            make(
                vec!["e1_r".into()],
                true,
                edge_length - 2000.0 + s.formation.length_m(),
                edge_length - 500.0,
            ),
        ];
        assert!(!s.tick_operational_path(true));
        assert_eq!(s.operational_leg, 1);
        assert_eq!(s.formation.parked_path_edges, ["e1"]);
        let tail = s.formation.center_offset_m(1);
        assert_eq!(
            s.presentation_position_for_car(tail, tail, 0.0).unwrap().1,
            poses[1]
        );
        assert!(
            s.own_track_occupancy()
                .iter()
                .any(|o| o.owner.ends_with("estacionado")
                    && o.start_m <= poses[1]
                    && o.end_m >= poses[1])
        );
        let saved = s.snapshot();
        s.restore_snapshot(saved).unwrap();
        assert_eq!(
            s.presentation_position_for_car(tail, tail, 0.0).unwrap().1,
            poses[1]
        );
        s.recouple().unwrap();
        assert_eq!(s.formation.length_m(), full);
        for (i, old) in poses.iter().enumerate() {
            let offset = s.formation.center_offset_m(i);
            let (_, pos) = s
                .presentation_position_for_car(offset, offset, 0.0)
                .unwrap();
            assert!((edge_length - pos - old).abs() < 1e-6);
        }
        assert!(s.parked_path_data.is_none());
    }

    #[test]
    fn reversal_preserves_car_locations_fuel_brakes_odometer_and_save() {
        let mut s = session("smoke");
        let mut reverse = s.graph.edge("e1").unwrap().clone();
        reverse.id = openrailsrs_core::EdgeId("e1_r".into());
        std::mem::swap(&mut reverse.from, &mut reverse.to);
        reverse.grade_percent = -reverse.grade_percent;
        s.graph.insert_edge(reverse).unwrap();
        let length = s.formation.length_m();
        let make = |start: &str, dest: &str, edge: &str, offset, end, reversed| {
            openrailsrs_scenarios::model::RouteLeg {
                operations: vec![],
                service_id: None,
                not_before_s: None,
                start: start.into(),
                destination: dest.into(),
                edges: vec![edge.into()],
                start_offset_m: offset,
                end_chainage_m: end,
                reversed,
                wait_s: 2.0,
                stops: vec![],
                switches: vec![],
            }
        };
        s.operational_legs = vec![
            make("yard_a", "mid", "e1", length, 2000.0, false),
            make("mid", "yard_a", "e1_r", 3000.0 + length, 4900.0, true),
        ];
        s.state.path_edges = vec!["e1".into()];
        s.path_data = PathData::from_path(&s.state.path_edges, &s.graph);
        s.operational_start_m = length;
        s.state.pos_on_edge_m = 1996.0;
        s.state.odometer_m = 1996.0 - length;
        s.state.time.0 = 20.0;
        s.state.fuel_consumption_g = 1234.0;
        s.driver_direction = 1.0;
        let original_cylinders = format!("{:?}", s.state.brake_system.cylinders);
        let positions = (0..s.formation.cars.len())
            .map(|i| {
                s.position_at_head_offset(s.formation.center_offset_m(i))
                    .unwrap()
                    .1
            })
            .collect::<Vec<_>>();
        let before = s.state.odometer_m;
        assert!(!s.tick_operational_path(false));
        s.state.time.0 = 22.0;
        assert!(!s.tick_operational_path(false));
        assert_eq!(s.operational_leg, 0);
        s.driver_direction = 0.0;
        assert!(!s.tick_operational_path(false));
        assert_eq!(s.operational_leg, 1);
        assert!(s.state.path_reversed);
        for (i, old) in positions.iter().enumerate() {
            let (_, pos) = s
                .position_at_head_offset(s.formation.center_offset_m(i))
                .unwrap();
            assert!((5000.0 - pos - old).abs() < 1e-7);
        }
        assert_eq!(s.state.odometer_m, before);
        assert_eq!(s.state.fuel_consumption_g, 1234.0);
        assert_eq!(
            format!("{:?}", s.state.brake_system.cylinders),
            original_cylinders
        );
        let snapshot = s.snapshot();
        s.validate_snapshot(&snapshot).unwrap();
        s.restore_snapshot(snapshot).unwrap();
        assert_eq!(s.operational_leg, 1);
        assert!((s.gameplay_chainage_m() - before).abs() < 1e-7);
    }
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

    #[test]
    fn plan30_wiper_cadence_power_and_temperature_restore_without_phase_drift() {
        let mut s = session("chiltern_extended");
        s.wiper_mode = crate::wiper::WiperMode::Fast;
        s.wiper_active = true;
        s.state.ambient_temperature_c = -8.;
        s.state.time.0 = 0.45;
        let selected = s.selected_wiper_mode();
        assert_eq!(selected, crate::wiper::WiperMode::Fast);
        assert!(s.cab_telemetry().wiper_active);
        let snapshot = s.snapshot();
        let phase = selected.phase(s.time_s());
        s.wiper_mode = crate::wiper::WiperMode::Off;
        s.wiper_active = false;
        s.state.ambient_temperature_c = 20.;
        s.restore_snapshot(snapshot).unwrap();
        assert_eq!(s.state.ambient_temperature_c, -8.);
        assert_eq!(s.selected_wiper_mode().phase(s.time_s()), phase);
        let cab = s.cab_vehicle();
        s.formation.cars[cab].battery_on = false;
        for c in &mut s.state.power_supply.cars {
            if c.vehicle == cab {
                c.supply.low_voltage = false;
            }
        }
        assert!(!s.cab_telemetry().wiper_active);
        assert_eq!(s.selected_wiper_mode(), crate::wiper::WiperMode::Fast);
    }
    #[test]
    fn plan30_traction_diagnosis_uses_active_vehicle_and_recovers_from_physical_faults() {
        let mut s = session("chiltern_extended");
        s.driver_direction = 1.;
        s.driver_throttle = 0.7;
        s.exterior.door = crate::DoorState::Closed;
        let cab = s.cab_vehicle();
        s.state.brake_system.emergency = true;
        assert!(s.traction_diagnosis().contains("Emergencia"));
        s.state.brake_system.emergency = false;
        s.driver_direction = 0.5;
        assert!(s.traction_diagnosis().contains("neutro"));
        s.driver_direction = 1.;
        s.state
            .diesel
            .cars
            .iter_mut()
            .find(|c| c.vehicle == cab)
            .unwrap()
            .fuel_l = 0.;
        assert!(s.traction_diagnosis().contains("sin combustible"));
        s.state
            .diesel
            .cars
            .iter_mut()
            .find(|c| c.vehicle == cab)
            .unwrap()
            .fuel_l = 100.;
        s.state
            .diesel
            .cars
            .iter_mut()
            .find(|c| c.vehicle == cab)
            .unwrap()
            .phase = crate::diesel_operation::EnginePhase::Stopped;
        assert!(s.traction_diagnosis().contains("motor detenido"));
        s.state
            .diesel
            .cars
            .iter_mut()
            .find(|c| c.vehicle == cab)
            .unwrap()
            .phase = crate::diesel_operation::EnginePhase::Running;
        s.formation.cars[cab].battery_on = false;
        assert!(s.traction_diagnosis().contains("batería"));
        s.formation.cars[cab].battery_on = true;
        s.state.brake_system.independent_command = 0.4;
        assert!(s.traction_diagnosis().contains("independiente"));
        s.state.brake_system.independent_command = 0.;
        s.driver_throttle = 0.;
        s.driver_brake = 0.;
        s.step_realtime(30., |_| {});
        s.driver_throttle = 0.7;
        for car in &mut s.state.power_supply.cars {
            car.supply.main = true;
        }
        if let Some(adhesion) = &mut s.state.rail_adhesion {
            for car in &mut adhesion.cars {
                car.slipping = false;
            }
        }
        s.state.velocity_mps = 40.;
        let before = serde_json::to_string(&s.state).unwrap();
        assert!(
            s.traction_diagnosis().contains("límite de potencia"),
            "{}",
            s.traction_diagnosis()
        );
        assert_eq!(serde_json::to_string(&s.state).unwrap(), before);
    }
    #[test]
    fn plan30_ambient_temperature_affects_compatible_diesel_heat_without_changing_fuel_or_phase() {
        let s = session("chiltern_extended");
        let mut cold = s.state.clone();
        for car in &mut cold.diesel.cars {
            car.temperature_c = 90.;
        }
        let mut hot = cold.clone();
        cold.ambient_temperature_c = -10.;
        hot.ambient_temperature_c = 35.;
        let motors = s
            .physics
            .diesel
            .cars
            .iter()
            .map(|c| c.vehicle)
            .collect::<Vec<_>>();
        crate::diesel_operation::advance_temperature(&mut cold, &s.physics.diesel, &motors, 30.);
        crate::diesel_operation::advance_temperature(&mut hot, &s.physics.diesel, &motors, 30.);
        for (c, h) in cold.diesel.cars.iter().zip(&hot.diesel.cars) {
            assert!(c.temperature_c < h.temperature_c);
            assert_eq!(c.fuel_l, h.fuel_l);
            assert_eq!(c.rpm, h.rpm);
            assert_eq!(c.phase, h.phase);
        }
        let before = cold.diesel.clone();
        crate::diesel_operation::advance_temperature(&mut cold, &s.physics.diesel, &motors, 0.);
        assert_eq!(cold.diesel, before);
    }
}
