use openrailsrs_train::{DavisCoefficients, DieselTractionModel, SteamParams, TractiveCurve};

use openrailsrs_validate::BrakeCommandMapping;

use crate::coupler::{
    mass_weighted_mean_velocity, multi_body_step_distributed, multi_body_substep_count,
    multi_body_substep_count_for_vehicles,
};
use crate::path_data::PathData;
use crate::state::TrainSimState;

const G: f64 = 9.80665;
/// OR holds speed with brakes set while diesel RPM can still rise; no tractive demand.
const BRAKE_TRACTION_CUTOFF: f64 = 0.001;
/// Full tractive effort below this fraction of the edge speed limit.
const SPEED_EPS_RATIO: f64 = 0.99;
/// Legacy scenario safety guard; native stock is driven by the requested power.
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
#[derive(Clone)]
pub struct TrainPhysics {
    pub power_supply: crate::power_supply::PowerTrainConfig,
    pub rail_adhesion: Option<crate::adhesion::RailAdhesionConfig>,
    pub diesel: crate::diesel_operation::DieselTrainConfig,
    pub electric: crate::electric::ElectricTrainConfig,
    pub native: Option<crate::native_dynamics::NativeTrainPhysics>,
    pub mass_kg: f64,
    pub max_power_w: f64,
    pub max_tractive_effort_n: f64,
    pub max_brake_n: f64,
    pub davis: DavisCoefficients,
    /// Per-vehicle Davis coefficients (consist order); used in multi-body mode.
    pub vehicle_davis: Vec<DavisCoefficients>,
    pub vehicle_lengths_m: Vec<f64>,
    pub diesel_vehicle_indices: Vec<usize>,
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
    /// Multi-body: below this speed (m/s) with throttle off, use scalar coast decay.
    pub multi_body_scalar_coast_below_v_mps: Option<f64>,
    /// Longest legacy `RunUpTimeToMaxForce` in consist (metadata; OR 1.6.x ignores for ORTS).
    pub partial_throttle_run_up_time_s: Option<f64>,
    /// Deprecated: no effect; OR traction run-up is RPM → apparent throttle.
    pub orts_inherit_partial_run_up: bool,
}

/// Longest legacy run-up time in the consist (for ORTS lead units at partial throttle).
pub fn max_partial_throttle_run_up_time_s(engines: &[DieselTractionModel]) -> Option<f64> {
    engines
        .iter()
        .filter_map(|e| e.legacy_run_up_time_s())
        .filter(|t| *t > 0.0)
        .max_by(|a, b| a.partial_cmp(b).unwrap())
}

pub struct StepResult {
    pub arrived: bool,
}

/// Open Rails 1.6.x ignores MSTS `RunUpTimeToMaxForce`; traction run-up is RPM → apparent throttle.
fn advance_diesel_run_up(
    _engine: &DieselTractionModel,
    _train: &TrainPhysics,
    throttle: f64,
    _dt: f64,
    run_up: &mut f64,
) -> f64 {
    if throttle <= 0.0 {
        *run_up = 0.0;
        return 0.0;
    }
    *run_up = 1.0;
    1.0
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
    crate::surface_conditions::update_program_input(state);
    crate::vehicle_systems::advance_doors(state, train.vehicle_lengths_m.len(), dt);
    if let Some(program) = &state.weather_track
        && let Some(config) = &train.rail_adhesion
    {
        let frame = program.at_position(
            state.time_s(),
            path_data
                .edge_ids
                .get(state.edge_index)
                .map_or("", String::as_str),
            state.pos_on_edge_m,
        );
        state.ambient_temperature_c = f64::from(frame.temperature_c);
        let weather = crate::surface_conditions::SurfaceWeather {
            temperature_c: f64::from(frame.temperature_c),
            rain: f64::from(frame.rain),
            snow: f64::from(frame.snow),
            wind_mps: frame.wind_mps.map(f64::from),
        };
        state
            .surface_conditions
            .get_or_insert_with(|| crate::surface_conditions::SurfaceConditions::new(weather))
            .weather = weather;
        use crate::adhesion::{RailAdhesionState, RailWeather};
        let weather = if frame.snow > 0.02 && frame.snow > frame.rain {
            RailWeather::Snow
        } else if frame.rain > 0.02 {
            if frame.storm > 0.2 {
                RailWeather::Storm
            } else {
                RailWeather::Rain
            }
        } else if frame.visibility_m < 1000. || frame.fog_density > 0.004 {
            RailWeather::Fog
        } else {
            RailWeather::Dry
        };
        let rail = state
            .rail_adhesion
            .get_or_insert_with(|| RailAdhesionState::new(config, weather, v));
        rail.weather = weather;
        rail.weather_target_factor = Some(frame.rail_factor());
    }
    if let Some(surface) = &mut state.surface_conditions {
        surface.advance(dt);
    }
    crate::crosswind::advance(state, path_data, &train.vehicle_lengths_m, dt);
    crate::tilting::advance(state, path_data, &train.vehicle_lengths_m, dt);
    crate::electric::advance(state, path_data, &train.electric, dt);
    let speed_cap = edge_data.speed_limit_at(state.pos_on_edge_m);
    let brake_frac = train.brake_mapping.command_to_sim_fraction(state.brake);

    if let Some(native) = &train.native {
        if state.native_dynamics.as_ref().is_none_or(|d| {
            d.bearing_c.len() != native.vehicles.len()
                || d.axles.len() != train.diesel_engines.len()
        }) {
            let mut dynamics = native.initial_state(train.diesel_engines.len());
            for axle in &mut dynamics.axles {
                axle.speed_mps = v;
            }
            state.native_dynamics = Some(dynamics);
        }
        native.update_bearings(
            state.native_dynamics.as_mut().unwrap(),
            v,
            state.odometer_m,
            dt,
        );
        if let Some(config) = &train.rail_adhesion {
            let base = if native.environment.first_slippery_spot.is_some() {
                state.native_dynamics.as_ref().unwrap().adhesion_factor
            } else {
                native.environment.adhesion_factor
            };
            crate::adhesion::prepare(state, config, base, dt);
        }
        if let Some(dynamics) = state.native_dynamics.as_ref() {
            for (i, axle) in dynamics.axles.iter().enumerate() {
                let index = train.diesel_vehicle_indices.get(i).copied().unwrap_or(i);
                if let Some(cylinder) = state.brake_system.cylinders.get_mut(index) {
                    cylinder.set_native_air_skid(
                        state.throttle < 0.001
                            && crate::native_dynamics::NativeAxleState {
                                speed_mps: state
                                    .rail_adhesion
                                    .as_ref()
                                    .and_then(|r| r.cars.get(index))
                                    .map_or(axle.speed_mps, |c| c.wheel_speed_mps),
                            }
                            .wheel_slipping(
                                v,
                                state
                                    .rail_adhesion
                                    .as_ref()
                                    .and_then(|r| r.cars.get(index))
                                    .map_or(dynamics.adhesion_factor, |c| c.factor),
                            ),
                    );
                }
            }
        }
        // Native safety interlocks use the actual cylinder pressure after this
        // tick's EP update, independently of the driver's brake handle.
        state.brake_system.step_with_speed(brake_frac, dt, v);
    } else if let Some(config) = &train.rail_adhesion {
        crate::adhesion::prepare(state, config, 1., dt);
        if state.rail_adhesion.is_some() {
            state.brake_system.step_with_speed(brake_frac, dt, v);
        }
    }
    if train.native.is_none() && state.rail_adhesion.is_none() {
        state.brake_system.step_with_speed(brake_frac, dt, v);
    }
    crate::auxiliary_work::step_circuits(state, dt);
    let physical_throttle =
        if !crate::vehicle_systems::doors_allow_traction(state, train.vehicle_lengths_m.len()) {
            0.
        } else {
            train.native.as_ref().map_or(state.throttle, |native| {
                native.throttle(
                    state.throttle,
                    state
                        .brake_system
                        .cylinders
                        .first()
                        .map_or(0., |c| c.pressure_bar()),
                )
            })
        };
    crate::multiple_unit::advance(state, train, dt);
    let native_brake_forces = state.brake_system.cylinder_forces_n(v);
    crate::power_supply::advance(state, &train.power_supply);
    crate::cab_equipment::advance(state, dt);
    let mut diesel_config = train.diesel.clone();
    diesel_config.cars.retain(|c| {
        !state
            .vehicle_systems
            .iter()
            .any(|s| s.definition.vehicle == c.vehicle && !s.motors.is_empty())
    });
    // Preserve initialized native cars, but advance only the legacy subset.
    let declared = state
        .vehicle_systems
        .iter()
        .filter(|s| !s.motors.is_empty())
        .map(|s| s.definition.vehicle)
        .collect::<Vec<_>>();
    let retained = state
        .diesel
        .cars
        .iter()
        .filter(|c| declared.contains(&c.vehicle))
        .cloned()
        .collect::<Vec<_>>();
    state.diesel.cars.retain(|c| !declared.contains(&c.vehicle));
    let vehicle_throttles = diesel_config
        .cars
        .iter()
        .map(|c| {
            (
                c.vehicle,
                crate::multiple_unit::throttle(state, c.vehicle, physical_throttle),
            )
        })
        .collect();
    state.fuel_consumption_g += crate::diesel_operation::advance_with_vehicle_throttles(
        &mut state.diesel,
        &diesel_config,
        physical_throttle,
        dt,
        train.native.is_some(),
        &state.power_supply,
        &vehicle_throttles,
    );
    state.diesel.cars.extend(retained);
    state.diesel.cars.sort_by_key(|c| c.vehicle);
    crate::traction_energy::prepare(state, train.vehicle_lengths_m.len());
    crate::engine_equipment::advance(state, physical_throttle, train.vehicle_lengths_m.len(), dt);
    crate::power_supply::advance(state, &train.power_supply);
    crate::traction_energy::prepare(state, train.vehicle_lengths_m.len());
    crate::brake_equipment::prepare_traction(state, train.vehicle_lengths_m.len(), v, dt);
    crate::power_supply::update_loads(
        state,
        &train.power_supply,
        train
            .native
            .as_ref()
            .map_or(20.0, |n| n.environment.ambient_c),
    );
    state
        .motor_force_n
        .resize(train.vehicle_lengths_m.len(), 0.);
    state.motor_force_n.fill(0.);
    state.rail_force_n.resize(train.vehicle_lengths_m.len(), 0.);
    state.rail_force_n.fill(0.);
    let mut rail_motor_forces = Vec::with_capacity(train.diesel_engines.len());
    let mut contact_motor_forces = Vec::new();
    let steam_wheel_speed = train
        .rail_adhesion
        .as_ref()
        .zip(state.rail_adhesion.as_ref())
        .map_or(v, |(config, rail)| {
            config
                .vehicles
                .iter()
                .zip(&rail.cars)
                .filter(|(vehicle, _)| vehicle.enabled && vehicle.steam)
                .map(|(_, car)| car.wheel_speed_mps.abs())
                .fold(v, f64::max)
        });

    // ── Tractive force ────────────────────────────────────────────────────────
    // Steam path: boiler + cylinder model (updates boiler state in place).
    // Electric/diesel path: P/v law or explicit traction curve.
    let mut f_motor = if !state.steam_boilers.is_empty() {
        crate::steam_equipment::advance(
            state,
            train.vehicle_lengths_m.len(),
            physical_throttle,
            steam_wheel_speed,
            dt,
        )
    } else if let (Some(params), Some(boiler)) = (&train.steam_params, state.boiler_state.as_mut())
    {
        // Route limits are driving instructions. Only legacy toy scenarios use
        // the automatic power guard; a native regulator can produce overspeed.
        let factor = if train.legacy_power_cap {
            speed_limit_traction_factor(v, speed_cap)
        } else {
            1.0
        };
        let effective_throttle = physical_throttle * factor;
        let force_limit = if state.rail_adhesion.is_some() {
            let effort = if train.max_tractive_effort_n > 0. {
                train.max_tractive_effort_n
            } else {
                f64::INFINITY
            };
            let power = if train.max_power_w > 0. {
                train.max_power_w / steam_wheel_speed.max(0.5)
            } else {
                f64::INFINITY
            };
            effort.min(power)
        } else {
            f64::INFINITY
        };
        crate::steam::steam_step_with_equipment(
            boiler,
            params,
            effective_throttle,
            steam_wheel_speed,
            dt,
            force_limit,
            state
                .vehicle_systems
                .iter()
                .find(|c| c.definition.vehicle == state.active_cab_vehicle.unwrap_or(0))
                .and_then(|c| c.definition.auxiliaries.as_ref()),
        )
    } else if state.throttle > 0.0 || !train.diesel_engines.is_empty() {
        let speed_factor = if train.legacy_power_cap {
            speed_limit_traction_factor(v, speed_cap)
        } else {
            1.0
        };
        let raw = if !train.diesel_engines.is_empty() {
            let n = train.diesel_engines.len();
            if state.diesel_rpm.len() != n {
                state.diesel_rpm = train.diesel_engines.iter().map(|e| e.idle_rpm()).collect();
                state.diesel_run_up = vec![0.0; n];
                state.diesel_motor_heat = vec![0.0; n];
                state.diesel_traction_force_n = vec![0.0; n];
                state.diesel_average_force_n = vec![0.0; n];
                state.diesel_apparent_throttle = vec![0.0; n];
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
                if state.diesel_run_up.len() != n {
                    state.diesel_run_up = vec![0.0; n];
                }
                if state.diesel_apparent_throttle.len() != n {
                    state.diesel_apparent_throttle = vec![0.0; n];
                }
            }
            let prev_v = v;
            let mut f_total = 0.0;
            let traction_throttle = if train.native.is_some() {
                physical_throttle
            } else if brake_frac > BRAKE_TRACTION_CUTOFF {
                0.0
            } else {
                physical_throttle
            };
            for (i, engine) in train.diesel_engines.iter().enumerate() {
                let rpm = state.diesel_rpm[i];
                let vehicle_index = train.diesel_vehicle_indices.get(i).copied().unwrap_or(i);
                let new_rpm = if let Some(fraction) =
                    crate::engine_equipment::apparent_fraction(state, vehicle_index)
                {
                    engine.engine.as_ref().map_or(engine.idle_rpm(), |p| {
                        p.idle_rpm + (p.max_rpm - p.idle_rpm) * fraction
                    })
                } else if let Some(car) = state.diesel.car(vehicle_index) {
                    car.rpm
                } else if train.native.is_some() {
                    engine.advance_native_rpm(rpm, physical_throttle, dt)
                } else {
                    engine.advance_rpm(rpm, physical_throttle, dt)
                };
                state.diesel_rpm[i] = new_rpm;
                let vehicle_index = train.diesel_vehicle_indices.get(i).copied().unwrap_or(i);
                let curve_throttle = if crate::traction_energy::available(state, vehicle_index) {
                    crate::multiple_unit::throttle(state, vehicle_index, traction_throttle)
                } else {
                    0.
                };
                state.diesel_apparent_throttle[i] = if curve_throttle > 0.0 {
                    engine.effective_traction_throttle(curve_throttle, new_rpm)
                } else if state.throttle > 0.0 && brake_frac > BRAKE_TRACTION_CUTOFF {
                    engine.effective_traction_throttle(state.throttle, new_rpm)
                } else {
                    0.0
                };
                let mut run_up = state.diesel_run_up.get(i).copied().unwrap_or(0.0);
                let run_factor =
                    advance_diesel_run_up(engine, train, state.throttle, dt, &mut run_up);
                state.diesel_run_up[i] = run_up;
                let heat = state.diesel_motor_heat.get(i).copied().unwrap_or(0.0);
                let new_heat = if train.native.is_none()
                    && engine.engine.is_some()
                    && engine.motor_heating_time_s > 0.0
                {
                    engine.advance_motor_heat(heat, v, state.throttle, run_factor, dt)
                } else {
                    0.0
                };
                state.diesel_motor_heat[i] = new_heat;
                let power_reduction = DieselTractionModel::power_reduction_from_heat(new_heat);
                let legacy = train.legacy_power_cap;

                let mut force_n = if curve_throttle <= 0.0 {
                    0.0
                } else {
                    let wheel_speed = state
                        .native_dynamics
                        .as_ref()
                        .and_then(|d| d.axles.get(i))
                        .map_or(v, |a| a.speed_mps.abs());
                    let target = if let Some(fraction) =
                        crate::engine_equipment::apparent_fraction(state, vehicle_index)
                    {
                        train
                            .electric
                            .fallback_cars
                            .iter()
                            .find(|(index, _)| *index == vehicle_index)
                            .and_then(|(_, curve)| curve.interpolate(wheel_speed))
                            .unwrap_or(0.)
                            * curve_throttle.min(fraction)
                    } else if let Some(native) = &train.native {
                        let vehicle_index =
                            train.diesel_vehicle_indices.get(i).copied().unwrap_or(i);
                        let profile = &native.vehicles[vehicle_index];
                        engine.native_traction_force_n(
                            wheel_speed,
                            curve_throttle,
                            new_rpm,
                            profile.authored_force_curves,
                            profile.rail_power_limit_w,
                        )
                    } else {
                        engine.target_traction_force_n(
                            v,
                            curve_throttle,
                            new_rpm,
                            run_factor,
                            power_reduction,
                            legacy,
                        )
                    };
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
                force_n *= crate::diesel_operation::available_fraction(
                    &state.diesel,
                    &train.diesel,
                    vehicle_index,
                );
                let load = state.power_supply.vehicle_load_w(vehicle_index);
                if load > 0.0 {
                    force_n =
                        force_n.min((engine.engine_power_w(new_rpm) - load).max(0.0) / v.max(0.5));
                }
                force_n = crate::engine_equipment::limit_force(state, vehicle_index, force_n, v);
                force_n = crate::brake_equipment::traction_limit(state, vehicle_index, force_n);
                force_n = crate::traction_energy::limit_motor(
                    state,
                    vehicle_index,
                    force_n,
                    v,
                    dt,
                    train,
                    path_data,
                );
                state.diesel_traction_force_n[i] = force_n;
                if let Some(slot) = state.motor_force_n.get_mut(vehicle_index) {
                    *slot = force_n;
                }
                state.diesel_average_force_n[i] = engine.advance_average_force(
                    state.diesel_average_force_n.get(i).copied().unwrap_or(0.0),
                    force_n,
                    dt,
                );
                let grip = crate::adhesion::factor(
                    state,
                    train.diesel_vehicle_indices.get(i).copied().unwrap_or(i),
                    state
                        .native_dynamics
                        .as_ref()
                        .map_or(1., |d| d.adhesion_factor),
                );
                let rail_force = if let (Some(native), Some(dynamics)) =
                    (&train.native, &mut state.native_dynamics)
                {
                    let vehicle_index = train.diesel_vehicle_indices.get(i).copied().unwrap_or(i);
                    let profile = &native.vehicles[vehicle_index];
                    let base_mass = state
                        .vehicle_masses
                        .get(vehicle_index)
                        .copied()
                        .or_else(|| native.vehicle_masses_kg.get(vehicle_index).copied())
                        .unwrap_or(engine.adhesion_mass_kg);
                    let mass = match (
                        train
                            .diesel
                            .cars
                            .iter()
                            .find(|c| c.vehicle == vehicle_index),
                        state.diesel.car(vehicle_index),
                    ) {
                        (Some(config), Some(fuel)) => crate::consumable_mass::diesel_mass_kg(
                            base_mass,
                            config.params.capacity_l,
                            fuel.fuel_l,
                        ),
                        _ => base_mass,
                    };
                    let speed = state
                        .vehicles
                        .get(vehicle_index)
                        .map_or(v, |vehicle| vehicle.velocity_mps);
                    let force = dynamics.axles[i].step(
                        profile,
                        mass,
                        engine.adhesion_mass_kg * mass / base_mass.max(1.0),
                        speed,
                        force_n,
                        native_brake_forces
                            .get(vehicle_index)
                            .copied()
                            .unwrap_or(0.),
                        grip,
                        dt,
                    );
                    let wheel_speed = dynamics.axles[i].speed_mps;
                    crate::adhesion::record(
                        state,
                        vehicle_index,
                        speed,
                        wheel_speed,
                        force_n,
                        force,
                        dt,
                    );
                    force
                } else if let Some(config) = &train.rail_adhesion {
                    let index = train.diesel_vehicle_indices.get(i).copied().unwrap_or(i);
                    crate::adhesion::transmit(
                        state,
                        config,
                        index,
                        v,
                        force_n,
                        native_brake_forces.get(index).copied().unwrap_or(0.),
                        dt,
                    )
                } else {
                    force_n
                };
                rail_motor_forces.push(rail_force);
                f_total += rail_force;
            }
            f_total
        } else if !train.electric.cars.is_empty() || !train.diesel.cars.is_empty() {
            let mut total = 0.;
            for (vehicle, curve) in &train.electric.fallback_cars {
                let force = if crate::traction_energy::available(state, *vehicle) {
                    (curve.interpolate(v).unwrap_or(0.)
                        * crate::multiple_unit::throttle(state, *vehicle, physical_throttle)
                        - state.power_supply.vehicle_load_w(*vehicle) / v.max(0.5))
                    .max(0.)
                } else {
                    0.
                };
                let force = crate::engine_equipment::limit_force(state, *vehicle, force, v);
                let force = crate::brake_equipment::traction_limit(state, *vehicle, force);
                let force = crate::traction_energy::limit_motor(
                    state, *vehicle, force, v, dt, train, path_data,
                );
                if let Some(slot) = state.motor_force_n.get_mut(*vehicle) {
                    *slot = force;
                }
                total += force;
            }
            total
        } else if let Some(f_curve) = train.tractive.interpolate(v) {
            f_curve * physical_throttle
        } else {
            (train.max_power_w / v.max(0.5)).min(train.max_tractive_effort_n) * physical_throttle
        };
        raw * speed_factor
    } else {
        0.0
    };

    if train.steam_params.is_some() || train.diesel_engines.is_empty() {
        let weights = train
            .electric
            .fallback_cars
            .iter()
            .map(|(i, curve)| {
                (
                    *i,
                    if crate::traction_energy::available(state, *i) {
                        curve.interpolate(v).unwrap_or(0.)
                    } else {
                        0.
                    },
                )
            })
            .collect::<Vec<_>>();
        let sum = weights.iter().map(|(_, force)| force).sum::<f64>();
        if sum > 0. && train.steam_params.is_none() && train.electric.fallback_cars.is_empty() {
            for (i, force) in weights {
                if let Some(slot) = state.motor_force_n.get_mut(i) {
                    *slot = f_motor * force / sum;
                }
            }
        } else if train.steam_params.is_some() && state.steam_boilers.is_empty() {
            let steam_weights = train
                .rail_adhesion
                .as_ref()
                .map(|config| {
                    config
                        .vehicles
                        .iter()
                        .enumerate()
                        .filter(|(_, c)| c.enabled && c.steam)
                        .map(|(i, c)| (i, c.profile.driven_mass_kg))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let total = steam_weights.iter().map(|(_, w)| w).sum::<f64>();
            if total > 0.0 {
                for (i, w) in steam_weights {
                    if let Some(slot) = state.motor_force_n.get_mut(i) {
                        *slot = f_motor * w / total;
                    }
                }
            }
        }
    }

    // Steam stock can carry a legacy force table parsed as a fallback diesel
    // model. Its selected boiler branch still needs wheel/rail contact.
    if (train.steam_params.is_some() || train.diesel_engines.is_empty())
        && state.rail_adhesion.is_some()
        && let Some(config) = &train.rail_adhesion
    {
        let active: Vec<_> = config
            .vehicles
            .iter()
            .enumerate()
            .filter(|(_, c)| c.enabled)
            .map(|(i, c)| {
                let weight = if train.steam_params.is_some() {
                    if c.steam {
                        if state.steam_boilers.is_empty() {
                            c.profile.driven_mass_kg
                        } else {
                            state.motor_force_n.get(i).copied().unwrap_or(0.)
                        }
                    } else {
                        0.
                    }
                } else if crate::traction_energy::available(state, i) {
                    train
                        .electric
                        .fallback_cars
                        .iter()
                        .find(|(index, _)| *index == i)
                        .map(|_| state.motor_force_n.get(i).copied().unwrap_or(0.))
                        .unwrap_or(0.)
                } else {
                    0.
                };
                (i, weight)
            })
            .collect();
        let weight_sum: f64 = active.iter().map(|(_, weight)| weight).sum();
        let total = f_motor;
        f_motor = 0.;
        for (index, weight) in active {
            let speed = state.vehicles.get(index).map_or(v, |c| c.velocity_mps);
            let force = crate::adhesion::transmit(
                state,
                config,
                index,
                speed,
                if weight_sum > 0. {
                    total * weight / weight_sum
                } else {
                    0.
                },
                native_brake_forces.get(index).copied().unwrap_or(0.),
                dt,
            );
            f_motor += force;
            contact_motor_forces.push((index, force));
        }
    }

    state.rail_force_n.clone_from(&state.motor_force_n);
    for (&index, &force) in train.diesel_vehicle_indices.iter().zip(&rail_motor_forces) {
        if let Some(slot) = state.rail_force_n.get_mut(index) {
            *slot = force;
        }
    }
    for &(index, force) in &contact_motor_forces {
        if let Some(slot) = state.rail_force_n.get_mut(index) {
            *slot = force;
        }
    }

    // Advance the air-brake system and read the total cylinder force.
    // When no cylinders are registered (default state), fall back to the
    // instantaneous scalar model so existing single-mass simulations are unchanged.
    let effective_mass = crate::consumable_mass::total_mass_kg(train, state);
    let effective_vehicle_masses: Vec<_> = state
        .vehicle_masses
        .iter()
        .enumerate()
        .map(|(i, m)| crate::consumable_mass::vehicle_mass_kg(train, state, i, *m))
        .collect();
    let mut f_brake = if !state.brake_system.cylinders.is_empty() {
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
    if state.rail_adhesion.is_some()
        && let Some(config) = &train.rail_adhesion
    {
        let mut forces = state.brake_system.cylinder_forces_n(v);
        if !forces.is_empty() {
            crate::adhesion::cap_brakes(state, config, &mut forces);
            f_brake = forces.iter().sum();
        } else if let Some(rail) = &state.rail_adhesion {
            let cap: f64 = config
                .vehicles
                .iter()
                .take(train.vehicle_lengths_m.len())
                .map(|c| c.mass_kg * G * crate::adhesion::friction(v, rail.weather_factor))
                .sum();
            f_brake = f_brake.min(cap);
        }
    }
    let mut equipment_brake_forces = state.brake_system.cylinder_forces_n(v);
    if equipment_brake_forces.is_empty() {
        equipment_brake_forces = vec![
            f_brake / train.vehicle_lengths_m.len().max(1) as f64;
            train.vehicle_lengths_m.len()
        ];
    }

    let equipped_brakes = state.vehicle_systems.iter().any(|s| {
        s.definition.electric_brake.is_some()
            || s.definition.brake_heat.is_some()
            || s.definition.wheel_slide.is_some()
    });
    if equipped_brakes {
        equipment_brake_forces =
            crate::brake_equipment::forces(state, train, v, dt, equipment_brake_forces);
        f_brake = equipment_brake_forces.iter().sum();
    }
    let f_resist = train.davis.resistance_n(v);
    let grade_fraction = edge_data.grade_at(state.pos_on_edge_m) / 100.0;
    let f_grade = effective_mass * G * grade_fraction;
    let head = path_data.chainage_at_edge_position(state.edge_index, state.pos_on_edge_m);
    let mut front = head;
    let vehicle_grades: Vec<_> = train
        .vehicle_lengths_m
        .iter()
        .enumerate()
        .map(|(i, length)| {
            let nominal = front - head - length / 2.;
            let center = head
                + if state.path_reversed {
                    -train.vehicle_lengths_m.iter().sum::<f64>() - nominal
                } else {
                    nominal
                };
            let span = train
                .native
                .as_ref()
                .and_then(|n| n.vehicles.get(i))
                .and_then(|p| p.pitch_span_m)
                .unwrap_or(*length);
            let grade =
                path_data.average_grade_between(center - span / 2., center + span / 2.) / 100.0;
            front -= length;
            grade
        })
        .collect();
    let resistance = |i: usize, mass: f64, davis: &DavisCoefficients, speed: f64| {
        if let (Some(native), Some(dynamics)) = (&train.native, &state.native_dynamics) {
            native.resistance(i, mass, davis, speed, dynamics)
        } else {
            davis.resistance_n(speed)
        }
    };
    // ── Multi-body coupler path ───────────────────────────────────────────────
    // When the state has per-vehicle data (initialised by the runner), delegate
    // to the spring-damper solver.  The resulting mean velocity is used for
    // position integration and energy accounting below.
    let rigid_native = train.native.as_ref().is_some_and(|n| {
        n.vehicles.len() > 1
            && n.vehicles
                .iter()
                .take(n.vehicles.len() - 1)
                .all(|v| v.rigid_connection)
    });
    let v_new = if rigid_native && !state.vehicles.is_empty() {
        // Native CouplingHasRigidConnection constrains linked vehicles to one
        // longitudinal velocity. Summing their forces conserves momentum and
        // prevents artificial spring rebound from driving the wheel-slip model.
        let resist: f64 = train
            .vehicle_davis
            .iter()
            .enumerate()
            .map(|(i, d)| resistance(i, effective_vehicle_masses[i], d, v))
            .sum();
        let grade: f64 = vehicle_grades
            .iter()
            .zip(&effective_vehicle_masses)
            .map(|(g, m)| m * G * g)
            .sum();
        let new_speed =
            (v + dt * (f_motor - f_brake - resist - grade) / effective_mass.max(1.)).max(0.);
        for vehicle in &mut state.vehicles {
            vehicle.velocity_mps = new_speed;
            vehicle.position_m += 0.5 * (v + new_speed) * dt;
        }
        for coupler in &mut state.couplers {
            coupler.extension_m = 0.;
        }
        new_speed
    } else if !state.vehicles.is_empty() {
        let total_mass = effective_mass;
        let masses = effective_vehicle_masses;
        let scalar_coast = train
            .multi_body_scalar_coast_below_v_mps
            .is_some_and(|threshold| state.throttle <= 0.0 && f_motor <= 1.0 && v < threshold);
        if scalar_coast {
            let f_brake_coast = if !equipped_brakes
                && !state.brake_system.cylinders.is_empty()
                && state.rail_adhesion.is_none()
            {
                state.brake_system.total_force_n(v)
            } else {
                f_brake
            };
            let f_resist_coast = if train.vehicle_davis.len() == state.vehicles.len() {
                state
                    .vehicles
                    .iter()
                    .zip(train.vehicle_davis.iter())
                    .enumerate()
                    .map(|(i, (veh, davis))| resistance(i, masses[i], davis, veh.velocity_mps))
                    .sum::<f64>()
            } else {
                train.davis.resistance_n(v)
            };
            let f_grade_coast = if vehicle_grades.len() == masses.len() {
                masses
                    .iter()
                    .zip(&vehicle_grades)
                    .map(|(mass, grade)| mass * G * grade)
                    .sum()
            } else {
                effective_mass * G * grade_fraction
            };
            let accel = (-f_brake_coast - f_resist_coast - f_grade_coast) / effective_mass.max(1.0);
            let mean_v = (v + accel * dt).max(0.0);
            for veh in &mut state.vehicles {
                veh.velocity_mps = mean_v;
            }
            for coupler in &mut state.couplers {
                coupler.extension_m = 0.0;
            }
            mean_v
        } else {
            let in_free_coast = state.throttle <= 0.0 && f_motor <= 1.0 && brake_frac <= 0.001;
            let n_sub = if in_free_coast {
                multi_body_substep_count(dt)
            } else {
                multi_body_substep_count_for_vehicles(dt, &state.vehicles)
            };
            let sub_dt = dt / n_sub as f64;
            let mut mean_v = v;
            let mut motor_forces = vec![0.; state.vehicles.len()];
            if train.diesel_vehicle_indices.len() == rail_motor_forces.len()
                && !train.diesel_vehicle_indices.is_empty()
            {
                let raw: f64 = rail_motor_forces.iter().sum();
                for (&i, &force) in train.diesel_vehicle_indices.iter().zip(&rail_motor_forces) {
                    if let Some(f) = motor_forces.get_mut(i) {
                        *f += if raw > 0. { force * f_motor / raw } else { 0. };
                    }
                }
            } else if !contact_motor_forces.is_empty() {
                for &(index, force) in &contact_motor_forces {
                    if let Some(value) = motor_forces.get_mut(index) {
                        *value += force;
                    }
                }
            } else if let Some(first) = motor_forces.first_mut() {
                *first = f_motor;
            }
            for _ in 0..n_sub {
                let coupling_v = mass_weighted_mean_velocity(&state.vehicles, &masses).max(0.0);
                let mut brake_forces: Vec<f64> = if equipped_brakes {
                    equipment_brake_forces.clone()
                } else if !state.brake_system.cylinders.is_empty() {
                    state.brake_system.cylinder_forces_n(coupling_v)
                } else {
                    masses.iter().map(|m| f_brake * m / total_mass).collect()
                };
                if let Some(config) = &train.rail_adhesion {
                    crate::adhesion::cap_brakes(state, config, &mut brake_forces);
                }
                let grade_resist: Vec<f64> = if train.vehicle_davis.len() == state.vehicles.len() {
                    state
                        .vehicles
                        .iter()
                        .zip(train.vehicle_davis.iter())
                        .zip(masses.iter())
                        .enumerate()
                        .map(|(i, ((veh, davis), mass))| {
                            resistance(i, *mass, davis, veh.velocity_mps)
                                + mass
                                    * G
                                    * vehicle_grades.get(i).copied().unwrap_or(grade_fraction)
                        })
                        .collect()
                } else {
                    masses
                        .iter()
                        .map(|m| (f_resist + f_grade) * m / total_mass)
                        .collect()
                };
                mean_v = multi_body_step_distributed(
                    &mut state.vehicles,
                    &mut state.couplers,
                    &motor_forces,
                    &brake_forces,
                    &grade_resist,
                    &masses,
                    sub_dt,
                    1.0,
                )
                .max(0.0);
            }
            mean_v
        }
    } else {
        // ── Single-mass path (default) ────────────────────────────────────────
        // Native stock retains each car's thermal resistance and bogie grade
        // even when elastic coupler simulation is disabled in the scenario.
        let (f_resist, f_grade) = if let Some(native) = &train.native {
            let resist = native
                .vehicle_masses_kg
                .iter()
                .zip(&train.vehicle_davis)
                .enumerate()
                .map(|(i, (mass, davis))| {
                    resistance(
                        i,
                        crate::consumable_mass::vehicle_mass_kg(train, state, i, *mass),
                        davis,
                        v,
                    )
                })
                .sum();
            let grade = native
                .vehicle_masses_kg
                .iter()
                .zip(&vehicle_grades)
                .enumerate()
                .map(|(i, (mass, grade))| {
                    crate::consumable_mass::vehicle_mass_kg(train, state, i, *mass) * G * grade
                })
                .sum();
            (resist, grade)
        } else {
            (f_resist, f_grade)
        };
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
    state.cumulative_energy_j +=
        (f_motor.max(0.0) * v_avg + state.power_supply.auxiliary_w + state.power_supply.hotel_w)
            * effective_dt;
    // Regenerative braking: recover fraction of braking work.
    let regen_factor = if state
        .vehicle_systems
        .iter()
        .any(|c| c.definition.electric_brake.is_some())
        || (!train.electric.cars.is_empty() && !state.electric.cars.iter().any(|c| c.main_power))
    {
        0.
    } else {
        train.regen_factor
    };
    let regen_j = f_brake * v_avg * regen_factor * effective_dt;
    state.regen_energy_j += regen_j;
    state.cumulative_energy_j -= regen_j; // net consumed = gross - regen
    // Diesel fuel: proportional to mechanical energy output.
    if let Some(sfc) = train
        .diesel_sfc_g_per_kwh
        .filter(|_| train.diesel.cars.is_empty())
    {
        let kwh = f_motor.max(0.0) * v_avg * effective_dt / 3_600_000.0;
        state.fuel_consumption_g += kwh * sfc;
    }
    crate::auxiliary_work::steam_extras(state, effective_dt);
    crate::traction_energy::account(state, v_avg, effective_dt);
    crate::brake_equipment::account(state, v_avg, effective_dt);
    state.odometer_m += traveled;
    state.time = state.time + effective_dt;
    state.velocity_mps = if arrived { 0.0 } else { v_new };
    crate::diesel_operation::advance_temperature(
        state,
        &train.diesel,
        &train.diesel_vehicle_indices,
        dt,
    );
    crate::adhesion::finish(state, dt);
    if let Some(rail) = &mut state.rail_adhesion {
        for c in &state.vehicle_systems {
            if c.definition.wheel_slide.is_some()
                && c.definition.vehicle < train.vehicle_lengths_m.len()
                && let Some(w) = rail.cars.get_mut(c.definition.vehicle)
            {
                w.wheel_speed_mps = c.brakes.wheel_speed_mps;
                w.slipping = c.brakes.sliding;
            }
        }
    }

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
