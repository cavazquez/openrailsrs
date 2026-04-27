use openrailsrs_track::TrackGraph;
use openrailsrs_train::DavisCoefficients;

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
}

pub struct StepResult {
    pub arrived: bool,
}

/// Advance state by `dt` seconds using a longitudinal model.
/// Davis resistance coefficients come from the consist; grade from the current edge.
pub fn step(
    state: &mut TrainSimState,
    graph: &TrackGraph,
    train: &TrainPhysics,
    dt: f64,
) -> StepResult {
    let edge_id = match state.current_edge() {
        Some(e) => e,
        None => return StepResult { arrived: true },
    };
    let edge = match graph.edge(edge_id) {
        Some(e) => e,
        None => return StepResult { arrived: true },
    };

    let v = state.velocity_mps.max(0.0);
    let speed_cap = edge.speed_limit_mps;

    let f_motor = if state.throttle > 0.0 {
        let raw = train.max_power_w * state.throttle / v.max(0.5);
        raw.min(train.max_tractive_effort_n)
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
    let f_grade = train.mass_kg * G * (edge.grade_percent / 100.0);

    let f_net = f_motor - f_brake - f_resist - f_grade;
    let accel = f_net / train.mass_kg;
    let v_new = (v + accel * dt).max(0.0);

    let v_avg = 0.5 * (v + v_new);
    let travel_max = v_avg * dt;
    let mut travel = travel_max;
    let mut traveled = 0.0;
    let mut arrived = false;

    while travel > 0.0 && state.edge_index < state.path_edges.len() {
        let eid = state.path_edges[state.edge_index].as_str();
        let len = graph.edge(eid).map(|e| e.length_m).unwrap_or(0.0);
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
            if state.edge_index >= state.path_edges.len() {
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
