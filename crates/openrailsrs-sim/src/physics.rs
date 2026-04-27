use openrailsrs_track::TrackGraph;

use crate::state::TrainSimState;

const G: f64 = 9.81;
const SPEED_EPS_RATIO: f64 = 0.99;
const DAVIS_A_N: f64 = 800.0;
const DAVIS_B_N_PER_MPS: f64 = 12.0;
const DAVIS_C_N_PER_MPS2: f64 = 0.4;

pub struct StepResult {
    pub arrived: bool,
}

/// Advance state by `dt` seconds using a simple longitudinal model.
pub fn step(
    state: &mut TrainSimState,
    graph: &TrackGraph,
    mass_kg: f64,
    max_power_w: f64,
    max_tractive_effort_n: f64,
    max_brake_n: f64,
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

    // Motive force ~ P/v with low-speed cap (very coarse).
    let f_motor = if state.throttle > 0.0 {
        let raw = max_power_w * state.throttle / v.max(0.5);
        raw.min(max_tractive_effort_n)
            * (if v >= speed_cap * SPEED_EPS_RATIO {
                0.0
            } else {
                1.0
            })
    } else {
        0.0
    };

    let f_brake = state.brake.clamp(0.0, 1.0) * max_brake_n;
    let f_resist = DAVIS_A_N + DAVIS_B_N_PER_MPS * v + DAVIS_C_N_PER_MPS2 * v * v;
    let grade = edge.grade_percent;
    let f_grade = mass_kg * G * (grade / 100.0);

    let f_net = f_motor - f_brake - f_resist - f_grade;
    let accel = f_net / mass_kg;
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
