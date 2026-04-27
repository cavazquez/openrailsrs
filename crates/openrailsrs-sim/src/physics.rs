use openrailsrs_train::{DavisCoefficients, TractiveCurve};

use crate::path_data::PathData;
use crate::state::TrainSimState;

const G: f64 = 9.81;
const SPEED_EPS_RATIO: f64 = 0.99;

/// Fixed physical parameters for the consist, computed once before the simulation loop.
pub struct TrainPhysics {
    pub mass_kg: f64,
    pub max_power_w: f64,
    pub max_tractive_effort_n: f64,
    pub max_brake_n: f64,
    pub davis: DavisCoefficients,
    /// Aggregate traction curve.  Empty curve → falls back to P/v law.
    pub tractive: TractiveCurve,
}

pub struct StepResult {
    pub arrived: bool,
}

/// Advance state by `dt` seconds using a longitudinal model.
///
/// Uses pre-computed [`PathData`] for direct `Vec` indexing instead of
/// repeated `HashMap::get` calls — the main hot-loop optimization.
pub fn step(
    state: &mut TrainSimState,
    path_data: &PathData,
    train: &TrainPhysics,
    dt: f64,
) -> StepResult {
    let edge_data = match path_data.get(state.edge_index) {
        Some(e) => e,
        None => return StepResult { arrived: true },
    };

    let v = state.velocity_mps.max(0.0);
    let speed_cap = edge_data.speed_limit_mps;

    let f_motor = if state.throttle > 0.0 {
        let raw = if let Some(f_curve) = train.tractive.interpolate(v) {
            f_curve
        } else {
            (train.max_power_w / v.max(0.5)).min(train.max_tractive_effort_n)
        };
        raw * state.throttle
            * (if v >= speed_cap * SPEED_EPS_RATIO {
                0.0
            } else {
                1.0
            })
    } else {
        0.0
    };

    let f_brake = state.brake.clamp(0.0, 1.0) * train.max_brake_n;
    let f_resist = train.davis.a_n + train.davis.b_n_per_mps * v + train.davis.c_n_per_mps2 * v * v;
    let f_grade = train.mass_kg * G * (edge_data.grade_percent / 100.0);

    let f_net = f_motor - f_brake - f_resist - f_grade;
    let accel = f_net / train.mass_kg;
    let v_new = (v + accel * dt).max(0.0);

    let v_avg = 0.5 * (v + v_new);
    let travel_max = v_avg * dt;
    let mut travel = travel_max;
    let mut traveled = 0.0;
    let mut arrived = false;

    while travel > 0.0 && state.edge_index < path_data.edges.len() {
        // Direct vec index — no hash lookup.
        let len = path_data.edges[state.edge_index].length_m;
        let room = len - state.pos_on_edge_m;
        if travel < room {
            state.pos_on_edge_m += travel;
            traveled += travel;
            travel = 0.0;
        } else {
            let consumed = room.max(0.0);
            travel -= consumed;
            traveled += consumed;
            state.pos_on_edge_m = 0.0;
            state.edge_index += 1;
            if state.edge_index >= path_data.edges.len() {
                arrived = true;
                break;
            }
        }
    }

    let effective_dt = if travel_max > 0.0 {
        dt * (traveled / travel_max).clamp(0.0, 1.0)
    } else {
        dt
    };
    state.cumulative_energy_j += f_motor.max(0.0) * v_avg * effective_dt;
    state.odometer_m += traveled;
    state.time = state.time + effective_dt;
    state.velocity_mps = if arrived { 0.0 } else { v_new };

    StepResult { arrived }
}
