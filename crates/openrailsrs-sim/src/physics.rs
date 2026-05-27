use openrailsrs_train::{DavisCoefficients, DieselTractionModel, SteamParams, TractiveCurve};

use openrailsrs_validate::BrakeCommandMapping;

use crate::coupler::{mass_weighted_mean_velocity, multi_body_step, multi_body_substep_count};
use crate::path_data::PathData;
use crate::state::TrainSimState;
use crate::steam::steam_step;

const G: f64 = 9.81;
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
    /// Per-vehicle Davis coefficients (consist order); used in multi-body mode.
    pub vehicle_davis: Vec<DavisCoefficients>,
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
    /// OR driver brake command → cylinder force mapping (from scenario `[simulation]`).
    pub brake_mapping: BrakeCommandMapping,
    /// When true, use pre-OR-P1 `DieselPowerTab` P/v cap and skip apparent throttle.
    pub legacy_power_cap: bool,
    /// When true, per-cylinder brake force is capped at mass × g × μ_adhesion (OR-P6c).
    pub brake_skid_limit: bool,
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
    let f_motor = if let (Some(params), Some(boiler)) =
        (&train.steam_params, state.boiler_state.as_mut())
    {
        // Steam: the regulator is capped by the speed limiter.
        let effective_throttle = state.throttle * speed_limit_traction_factor(v, speed_cap);
        steam_step(boiler, params, effective_throttle, v, dt)
    } else if state.throttle > 0.0 {
        let speed_factor = speed_limit_traction_factor(v, speed_cap);
        let raw = if !train.diesel_engines.is_empty() {
            let n = train.diesel_engines.len();
            if state.diesel_rpm.len() != n {
                state.diesel_rpm = train.diesel_engines.iter().map(|e| e.idle_rpm()).collect();
                state.diesel_run_up = vec![0.0; n];
                state.diesel_motor_heat = vec![0.0; n];
                state.diesel_traction_force_n = vec![0.0; n];
                state.diesel_average_force_n = vec![0.0; n];
            } else {
                if state.diesel_motor_heat.len() != n {
                    state.diesel_motor_heat = vec![0.0; n];
                }
                if state.diesel_traction_force_n.len() != n {
                    state.diesel_traction_force_n = vec![0.0; n];
                }
                if state.diesel_average_force_n.len() != n {
                    state.diesel_average_force_n = vec![0.0; n];
                }
            }
            let prev_v = v;
            let mut f_total = 0.0;
            for (i, engine) in train.diesel_engines.iter().enumerate() {
                let rpm = state.diesel_rpm[i];
                let new_rpm = engine.advance_rpm(rpm, state.throttle, dt);
                state.diesel_rpm[i] = new_rpm;
                let mut run_up = state.diesel_run_up.get(i).copied().unwrap_or(0.0);
                let has_msts_run_up = engine.legacy_run_up_time_s().is_some()
                    && (train.legacy_power_cap || engine.engine.is_some());
                if has_msts_run_up {
                    if let Some(tau) = engine.legacy_run_up_time_s() {
                        if state.throttle <= 0.0 {
                            run_up = 0.0;
                        } else if state.throttle >= 1.0 {
                            run_up = 1.0;
                        } else {
                            run_up = (run_up + dt / tau).min(1.0);
                        }
                        state.diesel_run_up[i] = run_up;
                    }
                }
                // OR diesel ignores RunUpTimeToMaxForce at full notch; partial throttle keeps ramp.
                let run_factor = if has_msts_run_up && state.throttle < 1.0 {
                    run_up
                } else {
                    1.0
                };
                let heat = state.diesel_motor_heat.get(i).copied().unwrap_or(0.0);
                let new_heat = if engine.engine.is_some() && engine.motor_heating_time_s > 0.0 {
                    engine.advance_motor_heat(heat, v, state.throttle, run_factor, dt)
                } else {
                    0.0
                };
                state.diesel_motor_heat[i] = new_heat;
                let power_reduction = DieselTractionModel::power_reduction_from_heat(new_heat);
                let legacy = train.legacy_power_cap;

                let mut force_n = if state.throttle <= 0.0 {
                    0.0
                } else {
                    let target = engine.target_traction_force_n(
                        v,
                        state.throttle,
                        new_rpm,
                        run_factor,
                        power_reduction,
                        legacy,
                    );
                    let max_force_limit = if target.is_finite() {
                        target
                    } else {
                        f64::INFINITY
                    };
                    let current = state.diesel_traction_force_n.get(i).copied().unwrap_or(0.0);
                    engine.update_force_with_ramp(current, dt, target, max_force_limit, v, prev_v)
                };

                force_n = engine.apply_continuous_force_limit(
                    force_n,
                    state.diesel_average_force_n.get(i).copied().unwrap_or(0.0),
                    power_reduction,
                );
                state.diesel_traction_force_n[i] = force_n;
                state.diesel_average_force_n[i] = engine.advance_average_force(
                    state.diesel_average_force_n.get(i).copied().unwrap_or(0.0),
                    force_n,
                    dt,
                );
                f_total += force_n;
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
    let brake_frac = train
        .brake_mapping
        .command_to_cylinder_fraction(state.brake);
    state.brake_system.step(brake_frac, dt);
    let effective_mass = train.mass_kg + state.extra_mass_kg;
    let f_brake = if !state.brake_system.cylinders.is_empty() {
        state.brake_system.total_force_n(v)
    } else {
        let raw = brake_frac * train.max_brake_n;
        if train.brake_skid_limit {
            use crate::brake::OR_DEFAULT_BRAKE_ADHESION_MU;
            raw.min(effective_mass * G * OR_DEFAULT_BRAKE_ADHESION_MU)
        } else {
            raw
        }
    };
    let f_resist = train.davis.resistance_n(v);
    let grade_fraction = edge_data.grade_percent / 100.0;
    let f_grade = effective_mass * G * grade_fraction;

    // ── Multi-body coupler path ───────────────────────────────────────────────
    // When the state has per-vehicle data (initialised by the runner), delegate
    // to the spring-damper solver.  The resulting mean velocity is used for
    // position integration and energy accounting below.
    let v_new = if !state.vehicles.is_empty() {
        let total_mass = effective_mass;
        let n_sub = multi_body_substep_count(dt);
        let sub_dt = dt / n_sub as f64;
        let masses = state.vehicle_masses.clone();
        let mut mean_v = v;
        for _ in 0..n_sub {
            let coupling_v =
                mass_weighted_mean_velocity(&state.vehicles, &masses).max(0.0);
            let brake_forces: Vec<f64> = if !state.brake_system.cylinders.is_empty() {
                state.brake_system.cylinder_forces_n(coupling_v)
            } else {
                state
                    .vehicle_masses
                    .iter()
                    .map(|m| f_brake * m / total_mass)
                    .collect()
            };
            let grade_resist: Vec<f64> = if train.vehicle_davis.len() == state.vehicles.len() {
                state
                    .vehicles
                    .iter()
                    .zip(train.vehicle_davis.iter())
                    .zip(state.vehicle_masses.iter())
                    .map(|((veh, davis), mass)| {
                        davis.resistance_n(veh.velocity_mps) + mass * G * grade_fraction
                    })
                    .collect()
            } else {
                state
                    .vehicle_masses
                    .iter()
                    .map(|m| (f_resist + f_grade) * m / total_mass)
                    .collect()
            };
            mean_v = multi_body_step(
                &mut state.vehicles,
                &mut state.couplers,
                f_motor,
                &brake_forces,
                &grade_resist,
                &masses,
                sub_dt,
            )
            .max(0.0);
        }
        mean_v
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
    use openrailsrs_validate::BrakeCommandMapping;

    #[test]
    fn brake_command_maps_driver_psi_to_cylinder_fraction() {
        let mapping = BrakeCommandMapping::from_scenario_fields(None, Some(35.0));
        let cmd = 9.0 / openrailsrs_validate::OR_DEFAULT_BRAKE_FULL_SCALE_PSI;
        let frac = mapping.command_to_cylinder_fraction(cmd);
        assert!((frac - 9.0 / 35.0).abs() < 1e-6, "frac={frac}");
        assert!(
            frac > cmd,
            "cylinder fraction should exceed raw driver command"
        );
    }
}
