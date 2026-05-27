use openrailsrs_train::{DavisCoefficients, DieselTractionModel, SteamParams, TractiveCurve};

use crate::coupler::multi_body_step;
use crate::path_data::PathData;
use crate::state::TrainSimState;
use crate::steam::steam_step;

const G: f64 = 9.81;
/// Driver CSV stores OR brake pipe pressure / 121 PSI (see `openrailsrs_validate`).
const OR_DRIVER_BRAKE_FULL_PSI: f64 = 121.0;
/// Peak service pressure in OR evaluation logs (Chiltern AUTO_SIGNAL ~11 PSI). Driver
/// commands are stored as `PSI/121`; cylinder force uses `PSI/this` (calibrated ~35 PSI).
const OR_EVAL_BRAKE_PIPE_MAX_PSI: f64 = 35.0;

/// Convert scripted-driver brake command to cylinder force fraction for the Westinghouse model.
fn brake_command_fraction(command: f64) -> f64 {
    (command.clamp(0.0, 1.0) * OR_DRIVER_BRAKE_FULL_PSI / OR_EVAL_BRAKE_PIPE_MAX_PSI).min(1.0)
}
/// Full tractive effort below this fraction of the edge speed limit.
const SPEED_EPS_RATIO: f64 = 0.99;
/// Open Rails allows modest overspeed before the limiter fully cuts power (see `runner` overspeed at 1.05×).
const SPEED_OVERSPEED_RATIO: f64 = 1.05;

/// Tractive effort multiplier from edge speed limiting (1.0 = unrestricted, 0.0 = at/above overspeed).
fn speed_limit_traction_factor(v: f64, speed_cap: f64) -> f64 {
    if !speed_cap.is_finite() || speed_cap <= 0.0 {
        return 1.0;
    }
    let ratio = v / speed_cap;
    if ratio <= SPEED_EPS_RATIO {
        1.0
    } else if ratio >= SPEED_OVERSPEED_RATIO {
        0.0
    } else {
        (SPEED_OVERSPEED_RATIO - ratio) / (SPEED_OVERSPEED_RATIO - SPEED_EPS_RATIO)
    }
}

/// Fixed physical parameters for the consist, computed once before the simulation loop.
pub struct TrainPhysics {
    pub mass_kg: f64,
    pub max_power_w: f64,
    pub max_tractive_effort_n: f64,
    pub max_brake_n: f64,
    pub davis: DavisCoefficients,
    /// Aggregate traction curve. Empty curve → falls back to P/v law.
    pub tractive: TractiveCurve,
    /// ORTS per-notch diesel models (one per powered locomotive in the consist).
    pub diesel_engines: Vec<DieselTractionModel>,
    /// Fraction of braking energy recovered as electricity (0.0 = none, 0.7 = modern EMU).
    pub regen_factor: f64,
    /// Specific fuel consumption in g/kWh; `None` for electric traction.
    pub diesel_sfc_g_per_kwh: Option<f64>,
    /// Steam traction parameters.  When `Some`, bypasses the P/v electric model.
    pub steam_params: Option<SteamParams>,
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

    // ── Tractive force ────────────────────────────────────────────────────────
    // Steam path: boiler + cylinder model (updates boiler state in place).
    // Electric/diesel path: P/v law or explicit traction curve.
    let f_motor =
        if let (Some(params), Some(boiler)) = (&train.steam_params, state.boiler_state.as_mut()) {
            // Steam: the regulator is capped by the speed limiter.
            let effective_throttle = state.throttle * speed_limit_traction_factor(v, speed_cap);
            steam_step(boiler, params, effective_throttle, v, dt)
        } else if state.throttle > 0.0 {
            let speed_factor = speed_limit_traction_factor(v, speed_cap);
            let raw = if !train.diesel_engines.is_empty() {
                if state.diesel_rpm.len() != train.diesel_engines.len() {
                    state.diesel_rpm = train.diesel_engines.iter().map(|e| e.idle_rpm()).collect();
                    state.diesel_run_up = vec![0.0; train.diesel_engines.len()];
                    state.diesel_motor_heat = vec![0.0; train.diesel_engines.len()];
                } else if state.diesel_motor_heat.len() != train.diesel_engines.len() {
                    state.diesel_motor_heat = vec![0.0; train.diesel_engines.len()];
                }
                let mut f_total = 0.0;
                for (i, engine) in train.diesel_engines.iter().enumerate() {
                    let rpm = state.diesel_rpm[i];
                    let new_rpm = engine.advance_rpm(rpm, state.throttle, dt);
                    state.diesel_rpm[i] = new_rpm;
                    let mut run_up = state.diesel_run_up.get(i).copied().unwrap_or(0.0);
                    if let Some(tau) = engine.legacy_run_up_time_s() {
                        if state.throttle > 0.0 {
                            run_up = (run_up + dt / tau).min(1.0);
                        } else {
                            run_up = 0.0;
                        }
                        state.diesel_run_up[i] = run_up;
                    }
                    let run_factor = if engine.legacy_run_up_time_s().is_some() {
                        run_up
                    } else {
                        1.0
                    };
                    let heat = state.diesel_motor_heat.get(i).copied().unwrap_or(0.0);
                    // ORTS motor heating only applies to engines with a thermodynamic model.
                    let new_heat = if engine.engine.is_some() && engine.motor_heating_time_s > 0.0 {
                        engine.advance_motor_heat(heat, v, state.throttle, run_factor, dt)
                    } else {
                        0.0
                    };
                    state.diesel_motor_heat[i] = new_heat;
                    let power_reduction = DieselTractionModel::power_reduction_from_heat(new_heat);
                    let mut f_e =
                        engine.force_at_scaled(v, state.throttle, run_factor, power_reduction);
                    let p_e = engine.effective_power_w(new_rpm, state.throttle)
                        * run_factor
                        * (1.0 - power_reduction.clamp(0.0, 0.95));
                    if v > 0.5 && p_e > 0.0 {
                        f_e = f_e.min(p_e / v);
                    }
                    f_total += f_e;
                }
                f_total
            } else if let Some(f_curve) = train.tractive.interpolate(v) {
                f_curve * state.throttle
            } else {
                (train.max_power_w / v.max(0.5)).min(train.max_tractive_effort_n) * state.throttle
            };
            raw * speed_factor
        } else {
            0.0
        };

    // Advance the air-brake system and read the total cylinder force.
    // When no cylinders are registered (default state), fall back to the
    // instantaneous scalar model so existing single-mass simulations are unchanged.
    state
        .brake_system
        .step(brake_command_fraction(state.brake), dt);
    let f_brake = if !state.brake_system.cylinders.is_empty() {
        state.brake_system.total_force_n()
    } else {
        brake_command_fraction(state.brake) * train.max_brake_n
    };
    let f_resist = train.davis.a_n + train.davis.b_n_per_mps * v + train.davis.c_n_per_mps2 * v * v;
    // Effective mass includes fixed consist mass plus any passenger load.
    let effective_mass = train.mass_kg + state.extra_mass_kg;
    let f_grade = effective_mass * G * (edge_data.grade_percent / 100.0);

    // ── Multi-body coupler path ───────────────────────────────────────────────
    // When the state has per-vehicle data (initialised by the runner), delegate
    // to the spring-damper solver.  The resulting mean velocity is used for
    // position integration and energy accounting below.
    let v_new = if !state.vehicles.is_empty() {
        // Per-vehicle brake and grade+resist forces (split proportionally by mass).
        let total_mass = effective_mass;
        let brake_forces: Vec<f64> = state
            .vehicle_masses
            .iter()
            .map(|m| f_brake * m / total_mass)
            .collect();
        let grade_resist: Vec<f64> = state
            .vehicle_masses
            .iter()
            .map(|m| (f_resist + f_grade) * m / total_mass)
            .collect();
        let masses: Vec<f64> = state.vehicle_masses.clone();
        multi_body_step(
            &mut state.vehicles,
            &mut state.couplers,
            f_motor,
            &brake_forces,
            &grade_resist,
            &masses,
            dt,
        )
        .max(0.0)
    } else {
        // ── Single-mass path (default) ────────────────────────────────────────
        let f_net = f_motor - f_brake - f_resist - f_grade;
        let accel = f_net / effective_mass;
        (v + accel * dt).max(0.0)
    };

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
    // Traction energy drawn from supply (gross).
    state.cumulative_energy_j += f_motor.max(0.0) * v_avg * effective_dt;
    // Regenerative braking: recover fraction of braking work.
    let regen_j = f_brake * v_avg * train.regen_factor * effective_dt;
    state.regen_energy_j += regen_j;
    state.cumulative_energy_j -= regen_j; // net consumed = gross - regen
    // Diesel fuel: proportional to mechanical energy output.
    if let Some(sfc) = train.diesel_sfc_g_per_kwh {
        let kwh = f_motor.max(0.0) * v_avg * effective_dt / 3_600_000.0;
        state.fuel_consumption_g += kwh * sfc;
    }
    state.odometer_m += traveled;
    state.time = state.time + effective_dt;
    state.velocity_mps = if arrived { 0.0 } else { v_new };

    StepResult { arrived }
}

#[cfg(test)]
mod tests {
    use super::brake_command_fraction;

    #[test]
    fn brake_command_maps_driver_psi_to_cylinder_fraction() {
        // 9 PSI service brake: driver stores 9/121, physics uses ~9/35 at Chiltern calibration.
        let cmd = 9.0 / 121.0;
        let frac = brake_command_fraction(cmd);
        assert!((frac - 9.0 / 35.0).abs() < 1e-6, "frac={frac}");
        assert!(
            frac > cmd,
            "cylinder fraction should exceed raw driver command"
        );
    }
}
