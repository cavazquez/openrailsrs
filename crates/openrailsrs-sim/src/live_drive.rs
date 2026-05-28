//! Real-time single-train session for interactive viewers (`openrailsrs-viewer3d --live`).

use std::path::Path;

use openrailsrs_route::load_track_graph_from_route_dir;
use openrailsrs_scenarios::{ScenarioFile, SwitchPositionDef};
use openrailsrs_track::SwitchPosition;
use openrailsrs_train::{DavisCoefficients, TractiveCurve, load_consist_with_asset_root};

use crate::brake::BrakeSystem;
use crate::coupler::CouplerKind;
use crate::path::edge_path;
use crate::path_data::PathData;
use crate::physics::{TrainPhysics, max_partial_throttle_run_up_time_s, step};
use crate::runner::consist_root;
use crate::state::TrainSimState;
use crate::SimError;

const BRAKE_PIPE_SPEED_MPS: f64 = 200.0;

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

/// Interactive session: same physics as headless `sim` / `cab`, stepped from a real-time loop.
pub struct LiveDriveSession {
    pub scenario_name: String,
    pub state: TrainSimState,
    pub physics: TrainPhysics,
    pub path_data: PathData,
    pub dt: f64,
    /// Driver notch [0, 1] (not yet written to `state` until step).
    pub driver_throttle: f64,
    pub driver_brake: f64,
    pub speed_mul: f64,
    sim_time_remainder: f64,
    pub arrived: bool,
}

impl LiveDriveSession {
    pub fn from_scenario(scenario_dir: &Path, scenario: &ScenarioFile) -> Result<Self, SimError> {
        let route_dir = scenario_dir.join(&scenario.route.path);
        let mut graph = load_track_graph_from_route_dir(&route_dir)?;

        for sw in &scenario.route.switches {
            let pos = match sw.position {
                SwitchPositionDef::Straight => SwitchPosition::Straight,
                SwitchPositionDef::Diverging => SwitchPosition::Diverging,
            };
            graph.set_switch(&sw.node, pos)?;
        }
        for cap in &scenario.route.edge_speed_limits {
            graph.cap_edge_speed_limit_kmh(&cap.edge, cap.speed_limit_kmh);
        }

        let path_edges = edge_path(&graph, &scenario.route.start, &scenario.route.destination)?;
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
        let mut state = TrainSimState::new(path_edges);
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

        Ok(Self {
            scenario_name: scenario.scenario.name.clone(),
            state,
            physics,
            path_data,
            dt: scenario.simulation.time_step,
            driver_throttle: 0.0,
            driver_brake: 0.0,
            speed_mul: 1.0,
            sim_time_remainder: 0.0,
            arrived: false,
        })
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

    pub fn speed_limit_mps(&self) -> f64 {
        self.path_data
            .get(self.state.edge_index)
            .map(|e| e.speed_limit_mps)
            .unwrap_or(f64::INFINITY)
    }

    /// Advance simulation by `real_dt` seconds of wall-clock time (scaled by `speed_mul`).
    pub fn step_realtime(&mut self, real_dt: f64) {
        if self.arrived || real_dt <= 0.0 {
            return;
        }
        let mut budget = self.sim_time_remainder + real_dt * self.speed_mul;
        let dt = self.dt;
        while budget >= dt {
            self.state.throttle = self.driver_throttle;
            self.state.brake = self.driver_brake;
            let res = step(&mut self.state, &self.path_data, &self.physics, dt);
            if res.arrived {
                self.arrived = true;
                break;
            }
            budget -= dt;
        }
        self.sim_time_remainder = budget;
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
        assert_eq!(session.time_s(), 0.0);
        session.step_realtime(5.0);
        assert!(session.time_s() > 0.0);
    }
}
