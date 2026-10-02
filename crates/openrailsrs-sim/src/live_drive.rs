//! Real-time single-train session for interactive viewers (`openrailsrs-viewer3d --live`).

use std::collections::HashMap;
use std::path::Path;

use openrailsrs_route::load_track_graph_from_route_dir;
use openrailsrs_scenarios::{RegionTracker, RegionTransition, ScenarioFile};
use openrailsrs_track::{NodeKind, SignalAspect, TrackGraph};
use openrailsrs_train::{DavisCoefficients, TractiveCurve, load_consist_with_asset_root};

use crate::SimError;
use crate::brake::BrakeSystem;
use crate::coupler::CouplerKind;
use crate::exterior::RollingStockExteriorState;
use crate::path::resolve_route_edges;
use crate::path_data::PathData;
use crate::physics::{TrainPhysics, max_partial_throttle_run_up_time_s, step};
use crate::runner::consist_root;
pub use crate::service::{LiveGameplay, LiveStopTarget};
use crate::service::{STOP_POSITION_TOLERANCE_M, STOP_SPEED_TOLERANCE_MPS, ServicePhase};
use crate::state::TrainSimState;

const BRAKE_PIPE_SPEED_MPS: f64 = 200.0;
/// Caution signals halve the effective speed limit on the signalled edge (same as headless runner).
const CAUTION_SPEED_FACTOR: f64 = 0.5;

fn build_brake_system(
    consist: &openrailsrs_train::Consist,
    train_air_lap_hold: bool,
    train_air_full_release_s: f64,
    brake_shoe_speed_factor: bool,
    brake_skid_limit: bool,
) -> BrakeSystem {
    let specs = crate::brake::vehicle_specs_from_consist(
        consist,
        brake_shoe_speed_factor,
        brake_skid_limit,
    );
    BrakeSystem::from_vehicle_specs(
        &specs,
        BRAKE_PIPE_SPEED_MPS,
        train_air_lap_hold,
        train_air_full_release_s,
    )
}

fn apply_start_offset(state: &mut TrainSimState, path_data: &PathData, offset_m: f64) {
    let mut remaining = offset_m.max(0.0);
    state.pos_on_edge_m = 0.0;
    state.odometer_m = 0.0;
    state.edge_index = 0;
    while remaining > 0.0 && state.edge_index < state.path_edges.len() {
        let Some(edge) = path_data.get(state.edge_index) else {
            break;
        };
        if edge.length_m <= 0.0 {
            state.edge_index += 1;
            continue;
        }
        if remaining <= edge.length_m {
            state.pos_on_edge_m = remaining;
            return;
        }
        remaining -= edge.length_m;
        state.edge_index += 1;
    }
}

fn init_signal_runtime(
    graph: &TrackGraph,
    assume_signals_clear: bool,
) -> HashMap<String, SignalAspect> {
    if assume_signals_clear {
        graph
            .signals()
            .map(|s| (s.id.clone(), SignalAspect::Clear))
            .collect()
    } else {
        graph.signals().map(|s| (s.id.clone(), s.aspect)).collect()
    }
}

fn build_live_gameplay(
    scenario: &ScenarioFile,
    graph: &TrackGraph,
    path_edges: &[String],
) -> Result<LiveGameplay, SimError> {
    let stops = &scenario.route.stops;
    let mut stop_targets = Vec::new();
    let mut cum = 0.0;
    let mut node_chainages = vec![(scenario.route.start.as_str(), 0.0)];
    for eid in path_edges {
        if let Some(edge) = graph.edge(eid) {
            cum += edge.length_m;
            node_chainages.push((edge.to.0.as_str(), cum));
        }
    }
    for stop in stops {
        let to_id = &stop.node;
        if let Some((_, distance)) = node_chainages.iter().find(|(n, _)| *n == to_id) {
            let target_distance = distance + stop.offset_m;
            if target_distance < 0.0 || target_distance > cum {
                return Err(SimError::Msg(format!(
                    "stop {to_id} lies outside the route"
                )));
            }
            let name = graph
                .node(to_id)
                .and_then(|n| {
                    if let NodeKind::Station { name } = &n.kind {
                        Some(name.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| to_id.clone());
            stop_targets.push(LiveStopTarget {
                node_id: to_id.clone(),
                cum_dist_m: target_distance,
                arrive_s: stop.arrive_s,
                depart_s: stop.depart_s,
                dwell_s: stop.dwell_s,
                name: stop.name.clone().unwrap_or(name),
                is_terminal: to_id == &scenario.route.destination,
                passengers_on: stop.passengers_on,
                passengers_off: stop.passengers_off,
            });
        } else {
            return Err(SimError::Msg(format!(
                "scheduled stop {to_id} is not on the resolved path"
            )));
        }
    }
    for pair in stop_targets.windows(2) {
        if pair[0].cum_dist_m >= pair[1].cum_dist_m || pair[0].is_terminal {
            return Err(SimError::Msg(
                "stops must follow route order; the terminal stop must be last".into(),
            ));
        }
    }
    Ok(LiveGameplay::new(
        scenario.route.destination.clone(),
        scenario.gameplay.penalty_per_second_late,
        stop_targets,
    ))
}

/// Interactive session: same physics as headless `sim` / `cab`, stepped from a real-time loop.
pub struct LiveDriveSession {
    pub scenario_name: String,
    pub state: TrainSimState,
    pub physics: TrainPhysics,
    pub path_data: PathData,
    pub graph: TrackGraph,
    pub dt: f64,
    pub assume_signals_clear: bool,
    /// Runtime signal aspects (updated each step; used by 3D markers).
    pub signal_runtime: HashMap<String, SignalAspect>,
    pub gameplay: LiveGameplay,
    pub region_tracker: RegionTracker,
    /// Driver notch [0, 1] (not yet written to `state` until step).
    pub driver_throttle: f64,
    pub driver_brake: f64,
    /// Reverser: 0 = REV, 0.5 = neutral, 1 = FWD (cab CVF / HUD).
    pub driver_direction: f64,
    /// Door / pantograph presentation for exterior shape keys (#81).
    pub exterior: RollingStockExteriorState,
    /// Sim time until which horn button appears pressed (cab M5).
    horn_pressed_until_s: f64,
    /// Wiper switch (cab CVF TWO_STATE / EXTERNALWIPERS).
    pub wiper_active: bool,
    pub speed_mul: f64,
    sim_time_remainder: f64,
    signal_steps: u64,
    pub arrived: bool,
    pub start_chainage_m: f64,
}

impl LiveDriveSession {
    pub fn from_scenario(scenario_dir: &Path, scenario: &ScenarioFile) -> Result<Self, SimError> {
        let route_dir = scenario_dir.join(&scenario.route.path);
        let mut graph = load_track_graph_from_route_dir(&route_dir)?;
        crate::path::apply_route_switches(&mut graph, &scenario.route)?;
        for cap in &scenario.route.edge_speed_limits {
            graph.cap_edge_speed_limit_kmh(&cap.edge, cap.speed_limit_kmh);
        }

        let path_edges = resolve_route_edges(&graph, &scenario.route)?;
        let consist_path = scenario_dir.join(&scenario.train.consist);
        let consist = load_consist_with_asset_root(&consist_path, consist_root(&consist_path))?;
        let davis_override = scenario.train.davis.as_ref().map(|d| DavisCoefficients {
            a_n: d.a_n,
            b_n_per_mps: d.b_n_per_mps,
            c_n_per_mps2: d.c_n_per_mps2,
        });
        let davis = davis_override
            .clone()
            .unwrap_or_else(|| consist.davis.clone());
        let vehicle_davis = consist.per_vehicle_davis(davis_override.as_ref());
        let diesel_engines = consist.diesel_traction_models();
        let raw_curve = consist.aggregate_tractive_curve();
        let tractive = if !diesel_engines.is_empty() {
            TractiveCurve::default()
        } else if raw_curve.points.is_empty() {
            TractiveCurve::from_power_and_effort(
                consist.total_max_power_w(),
                consist.total_max_tractive_effort_n(),
            )
        } else {
            raw_curve
        };
        let partial_throttle_run_up_time_s = max_partial_throttle_run_up_time_s(&diesel_engines);
        let physics = TrainPhysics {
            mass_kg: consist.total_mass_kg(),
            max_power_w: consist.total_max_power_w(),
            max_tractive_effort_n: consist.total_max_tractive_effort_n(),
            max_brake_n: consist.total_max_brake_n(),
            davis,
            vehicle_davis,
            tractive,
            diesel_engines,
            regen_factor: consist.regen_factor(),
            diesel_sfc_g_per_kwh: consist.diesel_sfc_g_per_kwh(),
            steam_params: consist.aggregate_steam_params(),
            brake_mapping: scenario.brake_mapping(),
            legacy_power_cap: scenario.simulation.legacy_power_cap,
            brake_skid_limit: scenario.simulation.brake_skid_limit,
            multi_body_scalar_coast_below_v_mps: scenario
                .simulation
                .multi_body_scalar_coast_below_v_mps,
            partial_throttle_run_up_time_s,
            orts_inherit_partial_run_up: scenario.simulation.orts_inherit_partial_run_up,
        };

        let path_data = PathData::from_path(&path_edges, &graph);
        let mut state = TrainSimState::new(path_edges.clone());
        if let Some(offset) = scenario.route.start_offset_m {
            apply_start_offset(&mut state, &path_data, offset);
        }
        state.brake_system = build_brake_system(
            &consist,
            scenario.simulation.train_air_lap_hold,
            scenario.simulation.train_air_full_release_s,
            scenario.simulation.brake_shoe_speed_factor,
            scenario.simulation.brake_skid_limit,
        );
        state.boiler_state = consist
            .aggregate_steam_params()
            .map(|p| crate::steam::BoilerState::from_params(&p));
        if !physics.diesel_engines.is_empty() {
            state.diesel_rpm = physics
                .diesel_engines
                .iter()
                .map(|e| e.idle_rpm())
                .collect();
            let n = physics.diesel_engines.len();
            state.diesel_run_up = vec![0.0; n];
            state.diesel_motor_heat = vec![0.0; n];
            state.diesel_traction_force_n = vec![0.0; n];
            state.diesel_average_force_n = vec![0.0; n];
            state.diesel_apparent_throttle = vec![0.0; n];
        }
        state.init_multi_body_if_enabled(
            &consist,
            scenario.simulation.multi_body,
            CouplerKind::parse(&scenario.simulation.coupler_kind),
        );

        let assume_signals_clear = scenario.route.assume_signals_clear;
        let signal_runtime = init_signal_runtime(&graph, assume_signals_clear);
        let gameplay = build_live_gameplay(scenario, &graph, &path_edges)?;
        let start_chainage_m =
            path_data.chainage_at_edge_position(state.edge_index, state.pos_on_edge_m);
        let region_tracker = RegionTracker::new(scenario.sound_regions.clone());

        Ok(Self {
            scenario_name: scenario.scenario.name.clone(),
            state,
            physics,
            path_data,
            graph,
            dt: scenario.simulation.time_step,
            assume_signals_clear,
            signal_runtime,
            gameplay,
            region_tracker,
            driver_throttle: 0.0,
            driver_brake: 0.0,
            driver_direction: 0.5,
            exterior: RollingStockExteriorState::new(),
            horn_pressed_until_s: 0.0,
            wiper_active: false,
            speed_mul: 1.0,
            sim_time_remainder: 0.0,
            signal_steps: 0,
            arrived: false,
            start_chainage_m,
        })
    }

    pub fn trigger_horn(&mut self, hold_s: f64) {
        self.horn_pressed_until_s = self.time_s() + hold_s.max(0.05);
    }

    pub fn toggle_wiper(&mut self) {
        self.wiper_active = !self.wiper_active;
    }

    pub fn time_s(&self) -> f64 {
        self.state.time_s()
    }

    pub fn velocity_mps(&self) -> f64 {
        self.state.velocity_mps
    }

    pub fn current_edge_id(&self) -> Option<&str> {
        self.state.current_edge()
    }

    pub fn pos_on_edge_m(&self) -> f64 {
        self.state.pos_on_edge_m
    }

    /// Graph position of a point `offset_along_path_m` ahead of (or behind) the head.
    ///
    /// Used by rolling-stock bogie articulation (#69): car/bogie longitudinal
    /// offsets are metres along the path from the consist head.
    pub fn position_at_head_offset(&self, offset_along_path_m: f64) -> Option<(String, f64)> {
        let head_chainage_m = self
            .path_data
            .chainage_at_edge_position(self.state.edge_index, self.state.pos_on_edge_m);
        PathData::position_at_odometer(
            &self.state.path_edges,
            &self.path_data.edges,
            (head_chainage_m + offset_along_path_m).max(0.0),
        )
    }

    pub fn speed_limit_mps(&self) -> f64 {
        self.path_data
            .get(self.state.edge_index)
            .map(|e| e.speed_limit_mps)
            .unwrap_or(f64::INFINITY)
    }

    /// Effective limit including caution signals on the current edge.
    pub fn effective_speed_limit_mps(&self) -> f64 {
        let base = self.speed_limit_mps();
        let Some(edge) = self.current_edge_id() else {
            return base;
        };
        let has_caution = self.graph.signals_on_edge(edge).any(|s| {
            self.signal_runtime.get(&s.id).copied().unwrap_or(s.aspect) == SignalAspect::Caution
        });
        if has_caution {
            base * CAUTION_SPEED_FACTOR
        } else {
            base
        }
    }

    pub fn signal_aspect(&self, signal_id: &str) -> Option<SignalAspect> {
        self.signal_runtime.get(signal_id).copied()
    }

    pub fn next_signal_ahead(&self) -> Option<(f64, SignalAspect)> {
        let mut before = -self.state.pos_on_edge_m;
        for (index, edge_id) in self
            .state
            .path_edges
            .iter()
            .enumerate()
            .skip(self.state.edge_index)
        {
            let nearest = self
                .graph
                .signals_on_edge(edge_id)
                .filter(|signal| before + signal.position_m >= 0.0)
                .min_by(|a, b| a.position_m.total_cmp(&b.position_m));
            if let Some(signal) = nearest {
                return Some((
                    before + signal.position_m,
                    self.signal_runtime
                        .get(&signal.id)
                        .copied()
                        .unwrap_or(signal.aspect),
                ));
            }
            before += self.path_data.edges[index].length_m;
        }
        None
    }

    fn distance_to_red_signal_m(&self) -> Option<f64> {
        let mut before = -self.state.pos_on_edge_m;
        for (index, edge_id) in self
            .state
            .path_edges
            .iter()
            .enumerate()
            .skip(self.state.edge_index)
        {
            let nearest = self
                .graph
                .signals_on_edge(edge_id)
                .filter(|signal| {
                    before + signal.position_m >= 0.0
                        && self
                            .signal_runtime
                            .get(&signal.id)
                            .copied()
                            .unwrap_or(signal.aspect)
                            == SignalAspect::Stop
                })
                .min_by(|a, b| a.position_m.total_cmp(&b.position_m));
            if let Some(signal) = nearest {
                return Some(before + signal.position_m);
            }
            before += self.path_data.edges[index].length_m;
        }
        None
    }

    pub fn next_stop_label(&self) -> Option<&str> {
        self.gameplay
            .stop_targets
            .get(self.gameplay.next_stop_idx)
            .map(|s| s.name.as_str())
    }

    /// Remaining distance to the next scheduled stop (m), if any remain.
    pub fn distance_to_next_stop_m(&self) -> Option<f64> {
        self.gameplay
            .stop_targets
            .get(self.gameplay.next_stop_idx)
            .map(|t| (t.cum_dist_m - self.head_chainage_m()).max(0.0))
    }

    pub fn head_chainage_m(&self) -> f64 {
        self.path_data
            .chainage_at_edge_position(self.state.edge_index, self.state.pos_on_edge_m)
    }

    pub fn toggle_doors(&mut self) {
        if self.velocity_mps().abs() <= STOP_SPEED_TOLERANCE_MPS {
            self.exterior.toggle_door();
        }
    }

    /// Fraction of route distance travelled [0, 1].
    pub fn route_progress(&self) -> f64 {
        let end = self
            .gameplay
            .stop_targets
            .last()
            .filter(|s| s.is_terminal)
            .map(|s| s.cum_dist_m)
            .unwrap_or_else(|| self.path_data.total_length_m());
        let total = end - self.start_chainage_m;
        if total > 0.0 {
            (self.state.odometer_m / total).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// ETCS status from the built-in Rust TCS (#163).
    pub fn etcs_status(&self) -> crate::etcs::EtcsTcsStatus {
        use crate::etcs::{BasicEtcsTcs, EtcsTcs};
        BasicEtcsTcs::default().compute(self)
    }

    /// Snapshot for the live cab panel (Fase C3).
    pub fn cab_telemetry(&self) -> CabTelemetry {
        let speed_kmh = self.state.velocity_mps * 3.6;
        let limit_kmh = self.effective_speed_limit_mps() * 3.6;
        let brake_force_kn = self
            .state
            .brake_system
            .total_force_n(self.state.velocity_mps)
            / 1000.0;
        let diesel_rpm = if self
            .physics
            .diesel_engines
            .iter()
            .any(|e| e.engine.is_some())
            && !self.state.diesel_rpm.is_empty()
        {
            Some(self.state.diesel_rpm.iter().sum::<f64>() / self.state.diesel_rpm.len() as f64)
        } else {
            None
        };
        let boiler_bar = self.state.boiler_state.as_ref().map(|b| b.pressure_bar);
        let main_res_bar = boiler_bar.unwrap_or(8.0 - self.driver_brake * 2.0);
        let brake_pipe_bar = (5.0 - self.driver_brake * 3.5).max(0.0);
        let brake_cyl_bar = (self.driver_brake * 4.5).min(5.0);
        CabTelemetry {
            speed_kmh,
            limit_kmh,
            throttle_pct: self.driver_throttle * 100.0,
            brake_pct: self.driver_brake * 100.0,
            direction: self.driver_direction.clamp(0.0, 1.0),
            horn_active: self.time_s() < self.horn_pressed_until_s,
            wiper_active: self.wiper_active,
            main_res_bar,
            brake_pipe_bar,
            brake_cyl_bar,
            brake_force_kn,
            diesel_rpm,
            boiler_bar,
            overspeed: self.gameplay.overspeed_active,
        }
    }
}

/// Driver-facing gauges for the 3D cab panel.
#[derive(Clone, Debug, PartialEq)]
pub struct CabTelemetry {
    pub speed_kmh: f64,
    pub limit_kmh: f64,
    pub throttle_pct: f64,
    pub brake_pct: f64,
    /// Reverser position 0–1 (0 = REV, 0.5 = neutral, 1 = FWD).
    pub direction: f64,
    pub horn_active: bool,
    pub wiper_active: bool,
    pub main_res_bar: f64,
    pub brake_pipe_bar: f64,
    pub brake_cyl_bar: f64,
    pub brake_force_kn: f64,
    pub diesel_rpm: Option<f64>,
    pub boiler_bar: Option<f64>,
    pub overspeed: bool,
}

/// Max physics quantum when stepping from wall-clock (viewer / interactive).
///
/// Headless Chiltern scenarios use `time_step = 1.0` for OR compare; without a
/// cap the live train sits still for ~1 s then teleports — feels like jumps.
const LIVE_REALTIME_MAX_DT_S: f64 = 0.05;

impl LiveDriveSession {
    /// Physics dt used by [`Self::step_realtime`] (scenario dt, capped for smoothness).
    pub fn realtime_physics_dt(&self) -> f64 {
        self.dt.clamp(1e-4, LIVE_REALTIME_MAX_DT_S)
    }

    /// Advance simulation by `real_dt` seconds of wall-clock time (scaled by `speed_mul`).
    ///
    /// `on_region_transition` is invoked for each sound-region enter/leave (e.g. audio engine).
    pub fn step_realtime<F>(&mut self, real_dt: f64, mut on_region_transition: F)
    where
        F: FnMut(&RegionTransition),
    {
        self.step_with_controller(real_dt, None, &mut on_region_transition);
    }

    /// Deterministic demonstration driver; decisions run at each physics quantum.
    pub fn step_autodrive<F>(&mut self, real_dt: f64, throttle: f64, mut on_region_transition: F)
    where
        F: FnMut(&RegionTransition),
    {
        self.step_with_controller(
            real_dt,
            Some(throttle.clamp(0.0, 1.0)),
            &mut on_region_transition,
        );
    }

    fn step_with_controller<F>(
        &mut self,
        real_dt: f64,
        automatic: Option<f64>,
        on_region_transition: &mut F,
    ) where
        F: FnMut(&RegionTransition),
    {
        if self.arrived
            || !real_dt.is_finite()
            || real_dt <= 0.0
            || !self.speed_mul.is_finite()
            || self.speed_mul <= 0.0
        {
            return;
        }
        let mut budget = self.sim_time_remainder + real_dt * self.speed_mul;
        let dt = self.realtime_physics_dt();
        while budget + 1e-12 >= dt {
            if let Some(throttle) = automatic {
                self.autodrive_inputs(throttle);
            }
            self.state.throttle = if self.driver_direction >= 0.75
                && self.exterior.door == crate::exterior::DoorState::Closed
                && !matches!(
                    self.gameplay.phase,
                    ServicePhase::Boarding | ServicePhase::ReadyToDepart
                ) {
                self.driver_throttle
            } else {
                0.0
            };
            self.state.brake = self.driver_brake;
            let red_distance = self.distance_to_red_signal_m();
            let previous_odometer = self.state.odometer_m;
            let res = step(&mut self.state, &self.path_data, &self.physics, dt);
            if red_distance
                .is_some_and(|distance| self.state.odometer_m - previous_odometer > distance + 0.01)
            {
                self.gameplay.fail("Señal de parada rebasada");
            }
            if res.arrived && self.gameplay.next_stop_idx < self.gameplay.stop_targets.len() {
                self.gameplay
                    .fail("Fin de vía alcanzado sin completar las paradas");
            }
            self.tick_after_physics_step(dt, on_region_transition);
            budget -= dt;
            if self.gameplay.is_finished() {
                self.arrived = true;
                break;
            }
            if res.arrived {
                self.gameplay.phase = ServicePhase::Completed;
                self.arrived = true;
                break;
            }
        }
        self.sim_time_remainder = if self.arrived { 0.0 } else { budget };
    }

    fn autodrive_inputs(&mut self, notch: f64) {
        use crate::exterior::DoorState;
        self.driver_direction = 1.0;
        let v = self.velocity_mps();
        match self.gameplay.phase {
            ServicePhase::Boarding | ServicePhase::ReadyToDepart => {
                self.driver_throttle = 0.0;
                self.driver_brake = 1.0;
                if self.gameplay.phase == ServicePhase::ReadyToDepart {
                    if matches!(self.exterior.door, DoorState::Open | DoorState::Opening) {
                        self.exterior.set_door(DoorState::Closing);
                    }
                } else if self.exterior.door == DoorState::Closed {
                    self.exterior.set_door(DoorState::Opening);
                }
                return;
            }
            ServicePhase::Completed | ServicePhase::Failed => return,
            ServicePhase::Approaching => {}
        }
        // Conservative service-brake approach with a propagation/release margin.
        // The curve tightens to zero at the authored stop, without snapping physics.
        let stop_cap = self
            .distance_to_next_stop_m()
            .map(|distance| (2.0 * 0.22 * (distance - 4.0).max(0.0)).sqrt())
            .unwrap_or(f64::INFINITY);
        let route_cap = self.effective_speed_limit_mps() * 0.9;
        let signal_cap = self
            .distance_to_red_signal_m()
            .map(|distance| (2.0 * 0.22 * (distance - 4.0).max(0.0)).sqrt())
            .unwrap_or(f64::INFINITY);
        let cap = route_cap.min(stop_cap).min(signal_cap);
        if self
            .distance_to_next_stop_m()
            .is_some_and(|d| d <= STOP_POSITION_TOLERANCE_M)
            && v <= STOP_SPEED_TOLERANCE_MPS
        {
            self.driver_throttle = 0.0;
            self.driver_brake = 1.0;
        } else if v > cap + 0.15 {
            self.driver_throttle = 0.0;
            self.driver_brake = if cap < 0.5 { 1.0 } else { 0.45 };
        } else if v < cap - 0.3 {
            self.driver_brake = 0.0;
            self.driver_throttle = notch;
        } else {
            self.driver_throttle = 0.0;
            self.driver_brake = 0.0;
        }
    }

    fn tick_after_physics_step<F>(&mut self, step_dt: f64, on_region_transition: &mut F)
    where
        F: FnMut(&RegionTransition),
    {
        self.exterior.tick(step_dt);
        self.tick_signals(step_dt);
        self.tick_gameplay(step_dt);

        if let Some(edge_id) = self.state.current_edge() {
            let transitions = self.region_tracker.step(edge_id, self.state.pos_on_edge_m);
            for t in &transitions {
                on_region_transition(t);
            }
        }
    }

    fn tick_signals(&mut self, step_dt: f64) {
        self.signal_steps += 1;
        let every = (1.0 / step_dt).round().max(1.0) as u64;
        if every > 0 && self.signal_steps.is_multiple_of(every) {
            let mut block_map = HashMap::new();
            if let Some(eid) = self.state.current_edge() {
                block_map.insert(eid.to_string(), "player".to_string());
            }
            self.graph.evaluate_signals(&block_map);
            for sig in self.graph.signals() {
                if sig
                    .clear_after_s
                    .is_some_and(|clear_t| self.state.time_s() >= clear_t)
                {
                    self.signal_runtime
                        .insert(sig.id.clone(), SignalAspect::Clear);
                    continue;
                }
                if self.assume_signals_clear && sig.script.is_none() {
                    continue;
                }
                self.signal_runtime.insert(sig.id.clone(), sig.aspect);
            }
        }
    }

    fn tick_gameplay(&mut self, step_dt: f64) {
        let limit = self.effective_speed_limit_mps();
        self.gameplay.overspeed_active =
            limit.is_finite() && self.state.velocity_mps > limit * 1.05;

        if let Some((off, on)) = self.gameplay.tick(
            self.time_s(),
            self.head_chainage_m(),
            self.velocity_mps(),
            self.exterior.door,
            step_dt,
        ) {
            self.state.passengers = self.state.passengers.saturating_sub(off).saturating_add(on);
            self.state.extra_mass_kg =
                self.state.passengers as f64 * crate::runner::KG_PER_PASSENGER;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_scenarios::load_scenario;
    use std::path::PathBuf;

    #[test]
    fn live_session_advances_time_on_smoke_scenario() {
        let scenario_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/scenario.toml");
        if !scenario_path.exists() {
            return;
        }
        let scenario_dir = scenario_path.parent().unwrap();
        let scenario = load_scenario(&scenario_path).expect("scenario");
        let mut session =
            LiveDriveSession::from_scenario(scenario_dir, &scenario).expect("live session");
        session.driver_throttle = 1.0;
        session.driver_direction = 1.0;
        assert_eq!(session.time_s(), 0.0);
        session.step_realtime(5.0, |_| {});
        assert!(session.time_s() > 0.0);
    }

    #[test]
    fn live_realtime_caps_large_scenario_dt() {
        // Chiltern headless uses time_step=1.0; interactive must still advance every frame.
        let scenario_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chiltern/scenario_brake_coast.toml");
        if !scenario_path.exists() {
            return;
        }
        let scenario_dir = scenario_path.parent().unwrap();
        let Ok(scenario) = load_scenario(&scenario_path) else {
            return;
        };
        let Ok(mut session) = LiveDriveSession::from_scenario(scenario_dir, &scenario) else {
            return;
        };
        assert!(
            (session.dt - 1.0).abs() < 1e-9,
            "fixture should keep headless dt=1; got {}",
            session.dt
        );
        assert!(
            (session.realtime_physics_dt() - LIVE_REALTIME_MAX_DT_S).abs() < 1e-12,
            "realtime dt must be capped"
        );
        session.driver_throttle = 1.0;
        session.driver_direction = 1.0;
        session.step_realtime(0.2, |_| {});
        assert!(
            session.time_s() >= 0.15,
            "0.2s wall-clock must advance sim (~0.2s), not wait for a 1s quantum; got {}",
            session.time_s()
        );
    }

    #[test]
    fn live_session_has_stop_target_for_smoke_mid() {
        let scenario_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/scenario.toml");
        if !scenario_path.exists() {
            return;
        }
        let scenario_dir = scenario_path.parent().unwrap();
        let scenario = load_scenario(&scenario_path).expect("scenario");
        let session =
            LiveDriveSession::from_scenario(scenario_dir, &scenario).expect("live session");
        assert!(
            session
                .gameplay
                .stop_targets
                .iter()
                .any(|s| s.name == "mid"),
            "smoke scenario should schedule stop at node mid"
        );
    }

    #[test]
    fn consist_offsets_are_relative_to_head_after_scenario_start_offset() {
        let scenario_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern/scenario.toml");
        if !scenario_path.exists() {
            return;
        }
        let scenario_dir = scenario_path.parent().unwrap();
        let scenario = load_scenario(&scenario_path).expect("scenario");
        let session =
            LiveDriveSession::from_scenario(scenario_dir, &scenario).expect("live session");
        let head_edge = session.current_edge_id().expect("head edge");
        let head_pos = session.pos_on_edge_m();
        let (same_edge, same_pos) = session.position_at_head_offset(0.0).expect("head pose");
        assert_eq!(same_edge, head_edge);
        assert!((same_pos - head_pos).abs() < 1e-6);

        let (rear_edge, rear_pos) = session.position_at_head_offset(-100.0).expect("rear pose");
        assert_eq!(
            rear_edge, head_edge,
            "Paddington start has enough chainage behind the head"
        );
        assert!(
            (rear_pos - (head_pos - 100.0)).abs() < 1e-6,
            "rear must be 100 m behind the head, not clamped to path origin: \
             head={head_pos:.3}, rear={rear_pos:.3}"
        );
    }

    #[test]
    fn live_session_reaches_chiltern_destination_with_throttle() {
        // Short corridor (brake-coast), not the full PAT in scenario.toml (~4000 km waypoints).
        let scenario_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chiltern/scenario_brake_coast.toml");
        if !scenario_path.exists() {
            return;
        }
        let scenario_dir = scenario_path.parent().unwrap();
        let Ok(mut scenario) = load_scenario(&scenario_path) else {
            return;
        };
        // This regression exercises propulsion and path arrival, without signals.
        scenario.route.assume_signals_clear = true;
        let Ok(mut session) = LiveDriveSession::from_scenario(scenario_dir, &scenario) else {
            return;
        };
        let start_odo = session.state.odometer_m;
        let path_m = session.path_data.total_length_m();
        // n3 → n10770 corridor is a few tens of km; 30 min at speed is enough to arrive.
        for _ in 0..1800 {
            session.driver_throttle = 1.0;
            session.driver_direction = 1.0;
            session.driver_brake = 0.0;
            session.step_realtime(1.0, |_| {});
            if session.arrived {
                break;
            }
        }
        assert!(
            session.arrived,
            "train should reach destination {} under full throttle (odo={:.0}m of {:.0}m)",
            scenario.route.destination, session.state.odometer_m, path_m,
        );
        assert!(
            session.state.odometer_m > start_odo,
            "odometer should advance toward the station"
        );
    }

    #[test]
    fn live_caution_signal_halves_effective_limit_on_e1() {
        let scenario_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/scenario.toml");
        if !scenario_path.exists() {
            return;
        }
        let scenario_dir = scenario_path.parent().unwrap();
        let scenario = load_scenario(&scenario_path).expect("scenario");
        let session =
            LiveDriveSession::from_scenario(scenario_dir, &scenario).expect("live session");
        assert_eq!(session.current_edge_id(), Some("e1"));
        let base = session.speed_limit_mps();
        let effective = session.effective_speed_limit_mps();
        assert!(
            (effective - base * CAUTION_SPEED_FACTOR).abs() < 1e-6,
            "caution on e1 should halve limit: base={base} effective={effective}"
        );
    }
}
