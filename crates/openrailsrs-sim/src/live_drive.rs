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
    pub formation: crate::FormationState,
    pub curve_parameters: Vec<openrailsrs_formats::VehicleCurveParameters>,
    pub signal_overrides: HashMap<String, SignalAspect>,
    pub service_id: String,
    pub native_signals: crate::native_signals::NativeSignalRuntime,
    pub external_track_occupancy: Vec<crate::native_signals::TrackOccupancy>,
    pub dispatcher: crate::DispatcherStatus,
    pub own_track_reservations: Vec<crate::native_signals::TrackOccupancy>,
    pub external_track_reservations: Vec<crate::native_signals::TrackOccupancy>,
    /// Rebuilt by the live traffic coordinator at every physics quantum.
    pub external_occupancy: HashMap<String, String>,
    pub(crate) consist: openrailsrs_train::Consist,
    pub(crate) original_physics: TrainPhysics,
    pub(crate) content_signature: String,
    pub(crate) stop_offsets: HashMap<String, f64>,
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
    pub(crate) horn_pressed_until_s: f64,
    /// Wiper switch (cab CVF TWO_STATE / EXTERNALWIPERS).
    pub wiper_active: bool,
    /// OR headlight positions: 0 off, 1 dim, 2 bright.
    pub headlights: u8,
    pub cab_light: bool,
    pub speed_mul: f64,
    pub(crate) sim_time_remainder: f64,
    pub(crate) previous_render_chainage_m: f64,
    pub(crate) signal_steps: u64,
    pub arrived: bool,
    pub start_chainage_m: f64,
    pub(crate) script_tcs: Option<crate::etcs::ScriptTcsHost>,
}

impl LiveDriveSession {
    pub fn toggle_pantograph(&mut self) -> Result<(), String> {
        if !self
            .physics
            .electric
            .cars
            .iter()
            .any(|c| c.params.pickup == openrailsrs_core::electrification::ElectricPickup::Overhead)
        {
            return Err("Esta formación no usa pantógrafo".into());
        }
        self.exterior.toggle_pantograph();
        self.state.electric.pantograph_command_up = self.exterior.pantograph_command_up;
        crate::electric::advance(&mut self.state, &self.path_data, &self.physics.electric, 0.);
        Ok(())
    }

    pub fn toggle_circuit_breaker(&mut self) -> Result<(), String> {
        if self.physics.electric.cars.is_empty() {
            return Err("Esta formación no tiene tracción eléctrica".into());
        }
        self.state.electric.breaker_command_closed = !self.state.electric.breaker_command_closed;
        crate::electric::advance(&mut self.state, &self.path_data, &self.physics.electric, 0.);
        Ok(())
    }

    pub fn electric_status(&self) -> Option<String> {
        self.state
            .electric
            .cars
            .first()
            .map(|c| format!("{:.0} V · {}", c.contact_voltage_v, c.loss.label()))
    }

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
        let curve_parameters = openrailsrs_train::load_consist_curve_parameters(
            &consist_path,
            consist_root(&consist_path),
        )?;
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
            electric: crate::electric::ElectricTrainConfig::load(
                &route_dir,
                scenario.route.electric_supply.as_ref(),
                &scenario.train.electric_pickups,
                &consist,
                &graph,
            )?,
            native: crate::native_dynamics::NativeTrainPhysics::load(
                &consist_path,
                consist_root(&consist_path),
                &consist,
                scenario.simulation.native_physics.as_ref(),
            )?,
            mass_kg: consist.total_mass_kg(),
            max_power_w: consist.total_max_power_w(),
            max_tractive_effort_n: consist.total_max_tractive_effort_n(),
            max_brake_n: consist.total_max_brake_n(),
            davis,
            vehicle_davis,
            vehicle_lengths_m: consist.vehicle_lengths_m(),
            diesel_vehicle_indices: consist.diesel_vehicle_indices(),
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
        crate::electric::advance(&mut state, &path_data, &physics.electric, 0.);
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
        let native_signals = crate::native_signals::NativeSignalRuntime::from_graph(&graph)
            .map_err(crate::SimError::Msg)?;
        let gameplay = build_live_gameplay(scenario, &graph, &path_edges)?;
        let start_chainage_m =
            path_data.chainage_at_edge_position(state.edge_index, state.pos_on_edge_m);
        let at_station = gameplay.stop_targets.first().is_some_and(|stop| {
            (stop.cum_dist_m - start_chainage_m).abs() <= crate::service::STOP_POSITION_TOLERANCE_M
        });
        let initial_brake = if at_station { 1.0 } else { 0.0 };
        if at_station {
            // New native gradients must not let a parked consist roll away and
            // accidentally complete its first station before the player acts.
            state.brake = initial_brake;
            state
                .brake_system
                .precharge(physics.brake_mapping.command_to_sim_fraction(initial_brake));
        }
        let region_tracker = RegionTracker::new(scenario.sound_regions.clone());

        let mut exterior = RollingStockExteriorState::new();
        exterior.set_pantograph_up(physics.electric.cars.iter().any(|c| {
            c.params.pickup == openrailsrs_core::electrification::ElectricPickup::Overhead
        }));
        let mut session = Self {
            scenario_name: scenario.scenario.name.clone(),
            formation: crate::FormationState::new(&consist),
            signal_overrides: HashMap::new(),
            service_id: "Jugador".into(),
            external_occupancy: HashMap::new(),
            curve_parameters,
            dispatcher: crate::DispatcherStatus::default(),
            own_track_reservations: vec![],
            external_track_reservations: vec![],
            original_physics: physics.clone(),
            content_signature: crate::operations::content_signature(
                &graph,
                scenario,
                &consist,
                &physics.electric,
            ),
            consist,
            stop_offsets: scenario
                .route
                .stops
                .iter()
                .map(|s| (s.node.clone(), s.offset_m))
                .collect(),
            state,
            physics,
            path_data,
            graph,
            dt: scenario.simulation.time_step,
            assume_signals_clear,
            signal_runtime,
            native_signals,
            external_track_occupancy: Vec::new(),
            gameplay,
            region_tracker,
            driver_throttle: 0.0,
            driver_brake: initial_brake,
            driver_direction: 0.5,
            exterior,
            horn_pressed_until_s: 0.0,
            wiper_active: false,
            headlights: 1,
            cab_light: false,
            speed_mul: 1.0,
            sim_time_remainder: 0.0,
            previous_render_chainage_m: start_chainage_m,
            signal_steps: 0,
            arrived: false,
            start_chainage_m,
            script_tcs: None,
        };
        session.evaluate_native_signals();
        // A static cab/capture needs real aspects at t=0. The traffic layer
        // supplies its positional footprints before the first physics step.
        session.native_signals.needs_refresh = true;
        Ok(session)
    }

    pub fn set_direction(&mut self, direction: f64) -> Result<(), String> {
        if (direction - self.driver_direction).abs() > 0.01
            && self.velocity_mps() > 0.1
            && direction != 0.5
        {
            return Err("Detené el tren antes de invertir el sentido".into());
        }
        self.driver_direction = direction.clamp(0.0, 1.0);
        Ok(())
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

    /// Original vehicle data remains available after disabling or separating
    /// a motor; consumers cannot mutate the simulator's consist through it.
    pub fn vehicle_definition(&self, index: usize) -> Option<&openrailsrs_train::Vehicle> {
        self.consist.vehicles.get(index)
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

    /// Presentation position between completed physics steps, along the same
    /// routed centreline as every car. This does not advance simulation state.
    /// `frame_remainder_s` is time left over by the host's fixed-step scheduler.
    pub fn render_position_at_head_offset(
        &self,
        offset_along_path_m: f64,
        frame_remainder_s: f64,
    ) -> Option<(String, f64)> {
        let interpolated =
            self.presentation_chainage_at_offset(offset_along_path_m, frame_remainder_s);
        PathData::position_at_odometer(
            &self.state.path_edges,
            &self.path_data.edges,
            interpolated.max(0.0),
        )
    }

    /// Signed routed distance on the same presentation clock used by the bodies,
    /// cameras and wheels. Stable during pause; resetting the session resets it.
    pub fn render_head_chainage_m(&self, frame_remainder_s: f64) -> f64 {
        let current = self.head_chainage_m();
        let fraction = if self.arrived {
            1.0
        } else {
            ((self.sim_time_remainder + frame_remainder_s.max(0.0) * self.speed_mul)
                / self.realtime_physics_dt())
            .clamp(0.0, 1.0)
        };
        self.previous_render_chainage_m + (current - self.previous_render_chainage_m) * fraction
    }

    pub fn speed_limit_mps(&self) -> f64 {
        let head = self.head_chainage_m();
        let length = self.formation.length_m();
        self.path_data
            .minimum_speed_limit_between((head - length).max(0.0), head)
    }

    /// Effective limit including caution signals on the current edge.
    pub fn effective_speed_limit_mps(&self) -> f64 {
        let base = self.speed_limit_mps();
        let Some(edge) = self.current_edge_id() else {
            return base;
        };
        let nearest = self
            .graph
            .signals_on_edge(edge)
            .filter(|signal| signal.position_m >= self.state.pos_on_edge_m)
            .filter(|signal| {
                signal
                    .script
                    .as_ref()
                    .and_then(|s| s.native.as_ref())
                    .is_none_or(|native| native.function.eq_ignore_ascii_case("NORMAL"))
            })
            .min_by(|a, b| a.position_m.total_cmp(&b.position_m));
        let has_caution = nearest.is_some_and(|signal| {
            self.signal_runtime
                .get(&signal.id)
                .copied()
                .unwrap_or(signal.aspect)
                == SignalAspect::Caution
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

    /// Cached TCS status. Reading this never performs IPC or executes a script.
    pub fn etcs_status(&self) -> crate::etcs::EtcsTcsStatus {
        use crate::etcs::{BasicEtcsTcs, EtcsTcs};
        let base = BasicEtcsTcs::default().compute(self);
        self.script_tcs
            .as_ref()
            .map_or_else(|| base.clone(), |host| host.status(base.clone()))
    }

    pub(crate) fn script_context(&self, dt_s: f64) -> crate::etcs::ScriptContext {
        use crate::etcs::{ScriptSignal, ScriptSpeedPost};
        let mut signals = vec![];
        let mut distance_signal = None;
        let mut speed_posts = vec![];
        let mut before = -self.state.pos_on_edge_m;
        let current_post_speed_limit_mps = self
            .path_data
            .get(self.state.edge_index)
            .map_or(self.speed_limit_mps(), |edge| {
                edge.speed_limit_at(self.state.pos_on_edge_m)
            });
        let mut previous_limit = current_post_speed_limit_mps;
        for (index, edge_id) in self
            .state
            .path_edges
            .iter()
            .enumerate()
            .skip(self.state.edge_index)
        {
            let mut heads: Vec<_> = self
                .graph
                .signals_on_edge(edge_id)
                .filter(|signal| before + signal.position_m >= 0.)
                .collect();
            heads.sort_by(|a, b| a.position_m.total_cmp(&b.position_m));
            for head in heads {
                let aspect = self.native_signal_aspect(&head.id).unwrap_or_else(|| {
                    match self
                        .signal_runtime
                        .get(&head.id)
                        .copied()
                        .unwrap_or(head.aspect)
                    {
                        SignalAspect::Stop => 0,
                        SignalAspect::Caution => 3,
                        SignalAspect::Clear => 7,
                    }
                });
                let signal = ScriptSignal {
                    distance_m: before + head.position_m,
                    aspect,
                };
                let distant = head
                    .script
                    .as_ref()
                    .and_then(|s| s.native.as_ref())
                    .is_some_and(|n| n.function.eq_ignore_ascii_case("DISTANCE"));
                let normal = head
                    .script
                    .as_ref()
                    .and_then(|s| s.native.as_ref())
                    .is_none_or(|n| n.function.eq_ignore_ascii_case("NORMAL"));
                if distant {
                    if distance_signal.is_none() {
                        distance_signal = Some(signal);
                    }
                } else if normal && signals.len() < 32 {
                    signals.push(signal);
                }
            }
            let edge = &self.path_data.edges[index];
            let changes = std::iter::once((0., edge.speed_limit_mps)).chain(
                edge.profile
                    .speed_posts
                    .iter()
                    .map(|p| (p.position_m, p.speed_limit_kmh / 3.6)),
            );
            for (position, limit) in changes {
                let distance = before + position;
                if distance >= 0. && (limit - previous_limit).abs() > 1e-6 {
                    if speed_posts.len() < 32 {
                        speed_posts.push(ScriptSpeedPost {
                            distance_m: distance,
                            speed_limit_mps: limit,
                        });
                    }
                    previous_limit = limit;
                }
            }
            before += edge.length_m;
        }
        let train_max_speed_mps = self
            .consist
            .vehicles
            .iter()
            .filter_map(|v| match v {
                openrailsrs_train::Vehicle::Loco(l) if l.max_velocity_mps > 0. => {
                    Some(l.max_velocity_mps)
                }
                _ => None,
            })
            .reduce(f64::min)
            .unwrap_or(self.speed_limit_mps());
        crate::etcs::ScriptContext {
            time_s: self.time_s(),
            dt_s,
            speed_mps: self.velocity_mps().abs(),
            speed_limit_mps: self.effective_speed_limit_mps(),
            next_signal_distance_m: self.next_signal_ahead().map(|(d, _)| d),
            next_signal_stop: self
                .next_signal_ahead()
                .is_some_and(|(_, aspect)| aspect == SignalAspect::Stop),
            next_stop_distance_m: self.distance_to_next_stop_m(),
            train_max_speed_mps,
            current_post_speed_limit_mps,
            signals,
            distance_signal,
            speed_posts,
        }
    }

    pub fn attach_script_tcs(
        &mut self,
        config: &crate::etcs::ScriptHostConfig,
    ) -> Result<(), String> {
        self.script_tcs = Some(crate::etcs::ScriptTcsHost::launch(
            config,
            &self.script_context(0.0),
        )?);
        Ok(())
    }

    /// Select an optional provider explicitly at startup. Presentation callers
    /// do not construct .NET processes or parse their protocol.
    pub fn configure_tcs_from_env(&mut self) -> Result<(), String> {
        let Ok(script) = std::env::var("OPENRAILSRS_TCS_SCRIPT") else {
            return Ok(());
        };
        let assembly = std::env::var("OPENRAILSRS_TCS_HOST_DLL")
            .map_err(|_| "OPENRAILSRS_TCS_HOST_DLL is required for an explicit C# TCS")?;
        self.attach_script_tcs(&crate::etcs::ScriptHostConfig {
            executable: std::env::var("OPENRAILSRS_DOTNET")
                .unwrap_or_else(|_| "dotnet".into())
                .into(),
            arguments: vec![assembly],
            script: script.into(),
            type_name: std::env::var("OPENRAILSRS_TCS_TYPE")
                .unwrap_or_else(|_| "MinimalTcs".into()),
            timeout: std::time::Duration::from_millis(250),
        })
    }

    pub fn send_tcs_input(&mut self, input: crate::etcs::TcsInput) {
        if let Some(host) = &mut self.script_tcs {
            host.push_input(input);
        }
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
        let head_vented = self
            .state
            .brake_system
            .cylinders
            .first()
            .is_some_and(|b| b.air_vented);
        let brake_pipe_bar = if head_vented {
            0.0
        } else {
            self.state
                .brake_system
                .cylinders
                .first()
                .and_then(|c| c.pipe_pressure_bar())
                .unwrap_or_else(|| (5.0 - self.driver_brake * 3.5).max(0.0))
        };
        let cylinders = &self.state.brake_system.cylinders;
        // The driving cab reads the lead vehicle's cylinder, not the average of
        // trailer/motor pressures with different native full-pressure ratings.
        let brake_cyl_bar = cylinders
            .first()
            .map_or(self.driver_brake * 4.5, |b| b.pressure_bar());
        CabTelemetry {
            pantograph_fraction: self
                .state
                .electric
                .cars
                .first()
                .map_or(0., |c| c.pantograph_fraction),
            line_voltage_v: self
                .state
                .electric
                .cars
                .first()
                .map_or(0., |c| c.contact_voltage_v),
            circuit_breaker_state: self.state.electric.cars.first().map_or(0, |c| {
                match c.breaker {
                    crate::electric::BreakerState::Open => 0,
                    crate::electric::BreakerState::Closing => 1,
                    crate::electric::BreakerState::Closed => 2,
                }
            }),
            main_power: self
                .state
                .electric
                .cars
                .first()
                .is_some_and(|c| c.main_power),
            speed_kmh,
            limit_kmh,
            throttle_pct: self.driver_throttle * 100.0,
            brake_pct: self.driver_brake * 100.0,
            direction: self.driver_direction.clamp(0.0, 1.0),
            horn_active: self.time_s() < self.horn_pressed_until_s,
            wiper_active: self.wiper_active,
            headlights: self.headlights,
            cab_light: self.cab_light,
            main_res_bar,
            brake_pipe_bar,
            brake_cyl_bar,
            brake_force_kn,
            diesel_rpm,
            boiler_bar,
            traction_load_fraction: if self.physics.max_tractive_effort_n > 0.0 {
                if self.state.diesel_traction_force_n.is_empty() {
                    self.state.throttle
                } else {
                    (self.state.diesel_traction_force_n.iter().sum::<f64>()
                        / self.physics.max_tractive_effort_n)
                        .clamp(0.0, 1.0)
                }
            } else {
                0.0
            },
            overspeed: self.gameplay.overspeed_active,
        }
    }
}

/// Driver-facing gauges for the 3D cab panel.
#[derive(Clone, Debug, PartialEq)]
pub struct CabTelemetry {
    pub pantograph_fraction: f64,
    pub line_voltage_v: f64,
    pub circuit_breaker_state: u8,
    pub main_power: bool,
    pub speed_kmh: f64,
    pub limit_kmh: f64,
    pub throttle_pct: f64,
    pub brake_pct: f64,
    /// Reverser position 0–1 (0 = REV, 0.5 = neutral, 1 = FWD).
    pub direction: f64,
    pub horn_active: bool,
    pub wiper_active: bool,
    pub headlights: u8,
    pub cab_light: bool,
    pub main_res_bar: f64,
    pub brake_pipe_bar: f64,
    pub brake_cyl_bar: f64,
    pub brake_force_kn: f64,
    pub diesel_rpm: Option<f64>,
    pub boiler_bar: Option<f64>,
    pub traction_load_fraction: f64,
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
        self.state.electric.pantograph_command_up = self.exterior.pantograph_command_up;
        while budget + 1e-12 >= dt {
            if let Some(throttle) = automatic {
                self.autodrive_inputs(throttle);
            }
            self.state.throttle = if (self.driver_direction >= 0.75
                || self.driver_direction <= 0.25)
                && self.exterior.door == crate::exterior::DoorState::Closed
            {
                self.driver_throttle
            } else {
                0.0
            };
            self.state.brake = self.driver_brake;
            if self
                .script_tcs
                .as_ref()
                .is_some_and(|host| host.applies_brake())
            {
                self.state.throttle = 0.0;
                self.state.brake = 1.0;
            }
            if self
                .distance_to_occupied_block_m()
                .is_some_and(|distance| distance < self.velocity_mps().powi(2) / 0.44 + 12.0)
            {
                self.state.throttle = 0.0;
                self.state.brake = 1.0;
            }
            let red_distance = self.distance_to_red_signal_m();
            let previous_odometer = self.state.odometer_m;
            self.previous_render_chainage_m = self.head_chainage_m();
            let backwards = self.driver_direction <= 0.25;
            let res = if backwards {
                let previous = self.head_chainage_m();
                let old_odometer = self.state.odometer_m;
                let mut edge = self.path_data.edges[self.state.edge_index].clone();
                edge.length_m = 1.0e12;
                edge.grade_percent = -edge.grade_percent;
                self.state.edge_index = 0;
                self.state.pos_on_edge_m = 0.0;
                step(
                    &mut self.state,
                    &PathData { edges: vec![edge] },
                    &self.physics,
                    dt,
                );
                let traveled = (self.state.odometer_m - old_odometer).min(previous);
                apply_start_offset(&mut self.state, &self.path_data, previous - traveled);
                self.state.odometer_m = old_odometer + traveled;
                if previous - traveled <= 0.001 {
                    self.state.velocity_mps = 0.0;
                    for vehicle in &mut self.state.vehicles {
                        vehicle.velocity_mps = 0.0;
                    }
                    self.driver_throttle = 0.0;
                    self.driver_brake = 1.0;
                }
                crate::physics::StepResult { arrived: false }
            } else {
                step(&mut self.state, &self.path_data, &self.physics, dt)
            };
            if !backwards
                && red_distance.is_some_and(|distance| {
                    self.state.odometer_m - previous_odometer > distance + 0.01
                })
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
            .into_iter()
            .chain(self.distance_to_occupied_block_m())
            .reduce(f64::min)
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
        if self.script_tcs.is_some() {
            let context = self.script_context(step_dt);
            if let Some(host) = &mut self.script_tcs {
                host.tick(&context);
            }
        }

        if let Some(edge_id) = self.state.current_edge() {
            let transitions = self.region_tracker.step(edge_id, self.state.pos_on_edge_m);
            for t in &transitions {
                on_region_transition(t);
            }
        }
    }

    pub(crate) fn tick_signals(&mut self, step_dt: f64) {
        self.signal_steps += 1;
        let every = (1.0 / step_dt).round().max(1.0) as u64;
        if every > 0 && self.signal_steps.is_multiple_of(every) {
            let block_map = self.occupied_edges();
            self.graph.evaluate_signals(&block_map);
            for sig in self.graph.signals() {
                if let Some(aspect) = self.signal_overrides.get(&sig.id) {
                    let protection = self.graph.signal_occupancy_constraint(
                        &sig.id,
                        &self.external_occupancy,
                        None,
                    );
                    let aspect = match (aspect, protection) {
                        (_, Some(SignalAspect::Stop)) => SignalAspect::Stop,
                        (SignalAspect::Clear, Some(SignalAspect::Caution)) => SignalAspect::Caution,
                        _ => *aspect,
                    };
                    self.signal_runtime.insert(sig.id.clone(), aspect);
                    continue;
                }
                if self.external_occupancy.is_empty()
                    && sig
                        .clear_after_s
                        .is_some_and(|clear_t| self.state.time_s() >= clear_t)
                {
                    self.signal_runtime
                        .insert(sig.id.clone(), SignalAspect::Clear);
                    continue;
                }
                if self.external_occupancy.is_empty()
                    && self.assume_signals_clear
                    && sig.script.is_none()
                {
                    continue;
                }
                let governs_player = self
                    .state
                    .path_edges
                    .iter()
                    .enumerate()
                    .skip(self.state.edge_index)
                    .any(|(index, edge)| {
                        edge == &sig.edge_id
                            && (index > self.state.edge_index
                                || sig.position_m >= self.pos_on_edge_m())
                    });
                let aspect = if governs_player {
                    self.graph
                        .signal_aspect_for_occupancy(&sig.id, &block_map, Some(&self.service_id))
                        .unwrap_or(sig.aspect)
                } else {
                    sig.aspect
                };
                self.signal_runtime.insert(sig.id.clone(), aspect);
            }
            self.evaluate_native_signals();
        }
    }

    pub(crate) fn refresh_traffic_signals(&mut self) {
        let dt = self.realtime_physics_dt();
        self.signal_steps = (1.0 / dt).round().max(1.0) as u64 - 1;
        self.tick_signals(dt);
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
    fn native_service_starts_with_brakes_holding_its_real_gradient() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chiltern_extended/scenario.toml");
        let scenario = load_scenario(&path).unwrap();
        let mut session =
            LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap();
        let cylinders = &session.state.brake_system.cylinders;
        assert_eq!(cylinders.len(), 8);
        assert!(cylinders[..7].iter().all(|c| c.ep_instant));
        assert!(!cylinders[7].ep_instant);
        for (index, pressure_psi) in [(0, 45.0), (1, 90.0), (7, 70.0)] {
            assert!(
                (cylinders[index].full_pressure_bar - pressure_psi * 0.0689475729).abs() < 1e-6
            );
        }
        assert_eq!(session.driver_brake, 1.0);
        session.step_realtime(8.0, |_| {});
        assert_eq!(session.velocity_mps(), 0.0);
        assert_eq!(session.gameplay.next_stop_idx, 0);
        assert!(session.gameplay.stop_results.is_empty());
    }
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
    fn render_interpolation_moves_between_physics_steps_without_advancing_state() {
        let scenario_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/scenario.toml");
        let scenario = load_scenario(&scenario_path).unwrap();
        let mut session =
            LiveDriveSession::from_scenario(scenario_path.parent().unwrap(), &scenario).unwrap();
        session.state.velocity_mps = 10.0;
        let before = session.head_chainage_m();
        session.step_realtime(0.05, |_| {});
        let after = session.head_chainage_m();
        let physics_time = session.time_s();
        assert!(after > before);
        for index in 0..=10 {
            let (edge, position) = session
                .render_position_at_head_offset(0.0, f64::from(index) * 0.005)
                .unwrap();
            assert_eq!(edge, session.state.path_edges[0]);
            let expected = before + (after - before) * f64::from(index) / 10.0;
            assert!((position - expected).abs() < 1e-9);
        }
        assert_eq!(session.head_chainage_m(), after);
        assert_eq!(session.time_s(), physics_time);
        session.arrived = true;
        assert_eq!(
            session.render_position_at_head_offset(0.0, 0.0),
            session.position_at_head_offset(0.0)
        );
    }

    #[test]
    fn render_interpolation_keeps_consist_spacing_across_an_edge_boundary() {
        let scenario_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/scenario.toml");
        let scenario = load_scenario(&scenario_path).unwrap();
        let mut session =
            LiveDriveSession::from_scenario(scenario_path.parent().unwrap(), &scenario).unwrap();
        let boundary = session.path_data.edges[0].length_m;
        session.previous_render_chainage_m = boundary - 0.5;
        apply_start_offset(&mut session.state, &session.path_data, boundary + 0.5);
        for index in 0..=10 {
            let remainder = f64::from(index) * 0.005;
            let chainage = |offset| {
                let (edge, position) = session
                    .render_position_at_head_offset(offset, remainder)
                    .unwrap();
                let edge_index = session
                    .state
                    .path_edges
                    .iter()
                    .position(|id| id == &edge)
                    .unwrap();
                session
                    .path_data
                    .chainage_at_edge_position(edge_index, position)
            };
            let head = chainage(0.0);
            assert!((head - (boundary - 0.5 + f64::from(index) * 0.1)).abs() < 1e-9);
            assert!((head - chainage(-20.0) - 20.0).abs() < 1e-9);
        }
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
    fn caution_behind_or_after_a_clear_head_does_not_slow_the_current_block() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke");
        let scenario = load_scenario(directory.join("scenario.toml")).unwrap();
        let mut train = LiveDriveSession::from_scenario(&directory, &scenario).unwrap();
        let edge = train.current_edge_id().unwrap().to_string();
        let base = train.speed_limit_mps();
        let nearest_position = train
            .graph
            .signals_on_edge(&edge)
            .map(|signal| signal.position_m)
            .reduce(f64::min)
            .unwrap();
        train.state.pos_on_edge_m = nearest_position + 0.01;
        assert_eq!(
            train.effective_speed_limit_mps(),
            base,
            "a passed caution must not hold a train at half speed for the rest of a long vector"
        );
        train
            .graph
            .insert_signal(openrailsrs_track::TrackSignal {
                id: "closer-clear".into(),
                edge_id: edge.clone(),
                position_m: 100.0,
                aspect: SignalAspect::Clear,
                clear_after_s: None,
                script: None,
            })
            .unwrap();
        train
            .graph
            .insert_signal(openrailsrs_track::TrackSignal {
                id: "distant-caution".into(),
                edge_id: edge,
                position_m: 200.0,
                aspect: SignalAspect::Caution,
                clear_after_s: None,
                script: None,
            })
            .unwrap();
        train.state.pos_on_edge_m = 1.0;
        assert_eq!(
            train.effective_speed_limit_mps(),
            base,
            "a distant caution beyond a nearer clear head is not the current block"
        );
        train.state.pos_on_edge_m = 150.0;
        assert_eq!(
            train.effective_speed_limit_mps(),
            base * CAUTION_SPEED_FACTOR
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
#[cfg(test)]
mod native_lookahead_tests {
    use super::*;
    #[test]
    fn script_context_contains_directed_native_heads_posts_and_actual_train_maximum() {
        let directory =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern_extended");
        let scenario =
            openrailsrs_scenarios::load_scenario(directory.join("scenario.toml")).unwrap();
        let session = LiveDriveSession::from_scenario(&directory, &scenario).unwrap();
        let context = session.script_context(0.05);
        assert!(context.signals.len() > 1 && context.signals.len() <= 32);
        assert!(
            context
                .signals
                .windows(2)
                .all(|w| w[0].distance_m <= w[1].distance_m)
        );
        assert!(context.distance_signal.is_some());
        assert!(!context.speed_posts.is_empty());
        assert!(
            context
                .speed_posts
                .windows(2)
                .all(|w| w[0].distance_m <= w[1].distance_m)
        );
        assert!(context.train_max_speed_mps > context.speed_limit_mps);
        assert!(context.current_post_speed_limit_mps >= context.speed_limit_mps);
        assert_eq!(session.curve_parameters.len(), session.formation.cars.len());
    }
}
