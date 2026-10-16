use openrailsrs_train::{DavisCoefficients, DieselTractionModel, SteamParams, TractiveCurve};

use openrailsrs_validate::BrakeCommandMapping;

use crate::coupler::{
    mass_weighted_mean_velocity, multi_body_step_distributed_oriented, multi_body_substep_count,
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
    pub bogies: Vec<openrailsrs_train::bogie_geometry::VehicleBogieModel>,
    pub rack: crate::rack::RackTrainConfig,
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
    step_direction(state, path_data, train, dt, false)
}

/// Reverse motion keeps the real multi-edge path for contact, electrical,
/// weather, yard and rack queries. A virtual long edge loses world chainage.
pub fn step_backwards(
    state: &mut TrainSimState,
    path_data: &PathData,
    train: &TrainPhysics,
    dt: f64,
) -> StepResult {
    step_direction(state, path_data, train, dt, true)
}

fn step_direction(
    state: &mut TrainSimState,
    path_data: &PathData,
    train: &TrainPhysics,
    dt: f64,
    backwards: bool,
) -> StepResult {
    let edge_data = match path_data.get(state.edge_index) {
        Some(e) => e,
        None => return StepResult { arrived: true },
    };

    let v = state.velocity_mps.max(0.0);
    let head = path_data.chainage_at_edge_position(state.edge_index, state.pos_on_edge_m);
    let mut front = head;
    let mut vehicle_centers = Vec::with_capacity(train.vehicle_lengths_m.len());
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
                .map(|p| {
                    p.pitch_span_m.unwrap_or_else(|| {
                        let face = p.coupler_face_length_m.unwrap_or(*length);
                        if face >= 4.3 {
                            face - 4.3
                        } else {
                            *length * 0.65
                        }
                    })
                })
                .unwrap_or(*length);
            vehicle_centers.push(center);
            let grade = path_data.average_grade_between(center - span / 2., center + span / 2.)
                / 100.0
                * if backwards { -1. } else { 1. };
            front -= length;
            if train.native.is_some() {
                // TrainCar.UpdateGravity projects weight onto the normalized
                // car axis (sin pitch); a percentage grade is tan pitch.
                grade / grade.hypot(1.)
            } else {
                grade
            }
        })
        .collect();
    crate::bogie_dynamics::advance_train(state, train, path_data, &vehicle_centers, dt);
    if crate::derailment::any_active(state, train) {
        state.brake_system.emergency = true;
    }
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
    let brake_command = if train.native.is_some() {
        state
            .diesel
            .car(state.active_cab_vehicle.unwrap_or(0))
            .and_then(|c| c.gearbox.as_ref())
            .map_or(state.brake, |gear| gear.previous_brake)
    } else {
        state.brake
    };
    let brake_frac = if state.brake_system.native_start_handle().is_some() {
        // An authored MSTS valve takes the actual controller handle; the
        // generic pressure-to-force mapping would rescale its notch position.
        brake_command.clamp(0., 1.)
    } else {
        train.brake_mapping.command_to_sim_fraction(brake_command)
    };
    for car in &mut state.diesel.cars {
        if let Some(gear) = &mut car.gearbox {
            gear.previous_brake = state.brake;
        }
    }

    let native_steam_brakes = crate::steam_equipment::prepare_native_brakes(
        state,
        state.brake.clamp(0., 1.),
        dt,
        backwards,
    );
    let mut native_curve_coefficients = Vec::new();
    if let Some(native) = &train.native {
        if state.native_dynamics.as_ref().is_none_or(|d| {
            d.bearing_c.len() != train.vehicle_lengths_m.len()
                || d.axles.len() != train.diesel_engines.len()
        }) {
            let mu_throttle = state.native_dynamics.as_ref().map_or(0., |d| d.mu_throttle);
            let mut dynamics = native.initial_state(train.diesel_engines.len());
            dynamics.mu_throttle = mu_throttle;
            dynamics.bearing_c.truncate(train.vehicle_lengths_m.len());
            if let Some(previous) = &state.native_dynamics {
                dynamics
                    .adhesion_filters
                    .clone_from(&previous.adhesion_filters);
                dynamics.adhesion_factor = previous.adhesion_factor;
                dynamics.wagon_curve_coefficient = previous.wagon_curve_coefficient;
            }
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
        let adhesion_time = state.time_s();
        native.advance_adhesion_inputs(state.native_dynamics.as_mut().unwrap(), adhesion_time, dt);
        native_curve_coefficients = native.prepare_curve_friction(
            state.native_dynamics.as_mut().unwrap(),
            &train.diesel_vehicle_indices,
            v,
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
                            && state
                                .rail_adhesion
                                .as_ref()
                                .and_then(|r| r.cars.get(index))
                                .map_or_else(
                                    || axle.clone(),
                                    |c| crate::native_dynamics::NativeAxleState {
                                        speed_mps: c.wheel_speed_mps,
                                        integration: c.axle_integration.clone(),
                                    },
                                )
                                .wheel_slipping(
                                    v,
                                    state
                                        .rail_adhesion
                                        .as_ref()
                                        .and_then(|r| r.cars.get(index))
                                        .map_or(native.adhesion_factor_for(dynamics, index), |c| {
                                            c.factor
                                        }),
                                ),
                    );
                }
            }
        }
        // Native safety interlocks use the actual cylinder pressure after this
        // tick's EP update, independently of the driver's brake handle.
        if !native_steam_brakes {
            state.brake_system.step_with_speed(brake_frac, dt, v);
        }
    } else if let Some(config) = &train.rail_adhesion {
        crate::adhesion::prepare(state, config, 1., dt);
        if state.rail_adhesion.is_some() && !native_steam_brakes {
            state.brake_system.step_with_speed(brake_frac, dt, v);
        }
    }
    if !native_steam_brakes && train.native.is_none() && state.rail_adhesion.is_none() {
        state.brake_system.step_with_speed(brake_frac, dt, v);
    }
    crate::auxiliary_work::step_circuits(state, dt);
    let physical_throttle = if crate::derailment::any_active(state, train)
        || !crate::vehicle_systems::doors_allow_traction(state, train.vehicle_lengths_m.len())
    {
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
    let native_retard_forces = state.brake_system.previous_native_retard_forces_n();
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
    let vehicle_throttles: std::collections::BTreeMap<usize, f64> = diesel_config
        .cars
        .iter()
        .map(|c| {
            (
                c.vehicle,
                crate::multiple_unit::throttle(state, c.vehicle, physical_throttle),
            )
        })
        .collect();
    let engine_throttles = vehicle_throttles
        .iter()
        .map(|(&index, &command)| (index, command))
        .chain(train.diesel_vehicle_indices.iter().map(|&index| {
            (
                index,
                crate::multiple_unit::throttle(state, index, physical_throttle),
            )
        }))
        .map(|(index, command)| {
            let explicit_mu = state.vehicle_systems.iter().any(|car| {
                car.definition.vehicle == index && car.definition.multiple_unit.is_some()
            });
            let governor_command = if let (Some(native), Some(dynamics)) =
                (&train.native, &state.native_dynamics)
                && !explicit_mu
            {
                native.governor_throttle(dynamics, index, command)
            } else if train.native.is_some() {
                // Explicit Rust MU protocols already have their own delivery
                // time. Preserve the gearbox's local controller phase there.
                state
                    .diesel
                    .car(index)
                    .and_then(|car| car.gearbox.as_ref())
                    .map_or(command, |gear| gear.previous_throttle)
            } else {
                command
            };
            (index, governor_command)
        })
        .collect();
    state.fuel_consumption_g += crate::diesel_operation::advance_at_speed(
        &mut state.diesel,
        &diesel_config,
        physical_throttle,
        dt,
        train.native.is_some(),
        &state.power_supply,
        &engine_throttles,
        v,
    );
    for car in &mut state.diesel.cars {
        if let Some(gear) = &mut car.gearbox {
            gear.previous_throttle = vehicle_throttles.get(&car.vehicle).copied().unwrap_or(0.);
        }
    }
    state.diesel.cars.extend(retained);
    state.diesel.cars.sort_by_key(|c| c.vehicle);
    if let Some(dynamics) = &mut state.native_dynamics {
        // All governor commands above were sampled before this publication;
        // it must survive a save between physical ticks or a cab change.
        dynamics.mu_throttle = physical_throttle;
    }
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
        crate::steam_equipment::advance(state, train, physical_throttle, steam_wheel_speed, dt)
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
                    engine.advance_native_rpm(
                        rpm,
                        engine_throttles
                            .get(&vehicle_index)
                            .copied()
                            .unwrap_or(physical_throttle),
                        dt,
                    )
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

                let geared_force = state.diesel.car(vehicle_index).and_then(|s| {
                    if let Some(hydraulic) = &s.hydraulic {
                        return Some(
                            if s.phase == crate::diesel_operation::EnginePhase::Running {
                                hydraulic.force_n
                            } else {
                                0.
                            },
                        );
                    }
                    let gear = s.gearbox.as_ref()?;
                    let config = train
                        .diesel
                        .cars
                        .iter()
                        .find(|c| c.vehicle == vehicle_index)?;
                    let profile = config.params.gearbox.as_ref()?;
                    if !profile.mechanical
                        && train
                            .native
                            .as_ref()
                            .and_then(|n| n.vehicles.get(vehicle_index))
                            .is_some_and(|p| p.authored_force_curves)
                    {
                        return None; // Native ORTS force tables take precedence over legacy gearing.
                    }
                    let force = gear.force_n(
                        profile,
                        config.governor.idle_rpm,
                        config.governor.max_rpm,
                        crate::gearbox::GearboxInput {
                            speed_mps: v,
                            throttle: engine_throttles
                                .get(&vehicle_index)
                                .copied()
                                .unwrap_or(curve_throttle),
                            rpm: new_rpm,
                            output_power_w: crate::gearbox::interpolate_extrapolated(
                                &config.governor.power_tab,
                                new_rpm,
                            )
                            .max(0.),
                            running: s.phase == crate::diesel_operation::EnginePhase::Running,
                        },
                    );
                    // Legacy DieselEngines.TractiveForceN applies the same
                    // per-engine demanded throttle, including controller lag,
                    // a second time outside GearBox.TractiveForceN.
                    Some(if profile.mechanical {
                        force
                    } else {
                        force
                            * engine_throttles
                                .get(&vehicle_index)
                                .copied()
                                .unwrap_or(curve_throttle)
                    })
                });
                let mut force_n = if let Some(force) = geared_force {
                    force
                } else if curve_throttle <= 0.0 {
                    0.0
                } else {
                    let wheel_speed = state
                        .native_dynamics
                        .as_ref()
                        .and_then(|d| d.axles.get(i))
                        .map_or(v, |a| {
                            let speed = state
                                .vehicles
                                .get(vehicle_index)
                                .map_or(v, |car| car.velocity_mps);
                            a.traction_speed_mps(speed)
                        });
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

                // MSTSDieselLocomotive.UpdateTractionForce returns immediately
                // for a gearbox; the electric motor continuous-force model is
                // only reached by the non-geared base locomotive path.
                if geared_force.is_none() {
                    force_n = engine.apply_continuous_force_limit(
                        force_n,
                        state.diesel_average_force_n.get(i).copied().unwrap_or(0.0),
                        power_reduction,
                    );
                }
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
                    state.native_dynamics.as_ref().map_or(1., |d| {
                        train.native.as_ref().map_or(d.adhesion_factor, |n| {
                            n.adhesion_factor_for(d, vehicle_index)
                        })
                    }),
                );
                let vertical_normal_factor =
                    crate::track_irregularity::normal_factor(state, vehicle_index);
                let vertical_contact_enabled = state
                    .bogies
                    .get(vehicle_index)
                    .and_then(|b| b.vertical.as_ref())
                    .is_some_and(|s| s.enabled_override.unwrap_or(s.configured_enabled));
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
                    let local_frame_sign = if (0.25..0.75).contains(&state.driver_reverser)
                        && dynamics.axles[i].integration.local_frame_sign != 0.
                    {
                        dynamics.axles[i].integration.local_frame_sign
                    } else if profile.flipped ^ (backwards ^ state.path_reversed) {
                        -1.
                    } else {
                        1.
                    };
                    let force = if native.environment.simple_control_physics {
                        dynamics.axles[i].speed_mps = speed;
                        if vertical_contact_enabled {
                            force_n.min(
                                engine.adhesion_mass_kg * mass / base_mass.max(1.)
                                    * vertical_normal_factor
                                    * G
                                    * crate::adhesion::friction(speed, grip),
                            )
                        } else {
                            force_n
                        }
                    } else {
                        dynamics.axles[i].step_oriented(
                            profile,
                            mass,
                            engine.adhesion_mass_kg * mass / base_mass.max(1.0)
                                * vertical_normal_factor,
                            speed,
                            force_n,
                            native_retard_forces
                                .get(vehicle_index)
                                .copied()
                                .unwrap_or(0.),
                            grip,
                            dt,
                            local_frame_sign,
                        )
                    };
                    let wheel_speed = dynamics.axles[i].speed_mps;
                    let integration = dynamics.axles[i].integration.clone();
                    crate::adhesion::record(
                        state,
                        vehicle_index,
                        speed,
                        wheel_speed,
                        force_n,
                        force,
                        dt,
                    );
                    if let Some(car) = state
                        .rail_adhesion
                        .as_mut()
                        .and_then(|rail| rail.cars.get_mut(vehicle_index))
                    {
                        car.axle_integration = integration;
                    }
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

    if !train.rack.vehicles.is_empty()
        && f_motor > 0.
        && state.motor_force_n.iter().all(|f| f.abs() < 1e-9)
        && let Some(vehicle) = train
            .rack
            .vehicles
            .iter()
            .find(|p| p.vehicle < train.vehicle_lengths_m.len())
            .map(|p| p.vehicle)
    {
        if let Some(f) = state.motor_force_n.get_mut(vehicle) {
            *f = f_motor;
        }
        if let Some(f) = state.rail_force_n.get_mut(vehicle) {
            *f = f_motor;
        }
    }
    f_motor += crate::rack::apply_traction(state, train, path_data, dt);
    if crate::derailment::any_active(state, train) {
        f_motor = 0.;
        state.motor_force_n.fill(0.);
        state.rail_force_n.fill(0.);
    }
    for (slot, &vehicle) in rail_motor_forces
        .iter_mut()
        .zip(&train.diesel_vehicle_indices)
    {
        *slot = state.rail_force_n.get(vehicle).copied().unwrap_or(0.);
    }
    for (vehicle, force) in &mut contact_motor_forces {
        *force = state.rail_force_n.get(*vehicle).copied().unwrap_or(0.);
    }

    // Advance the air-brake system and read the total cylinder force.
    // When no cylinders are registered (default state), fall back to the
    // instantaneous scalar model so existing single-mass simulations are unchanged.
    if let Some(dynamics) = &state.native_dynamics {
        for (motor, axle) in dynamics.axles.iter().enumerate() {
            let vehicle = train
                .diesel_vehicle_indices
                .get(motor)
                .copied()
                .unwrap_or(motor);
            if let Some(cylinder) = state.brake_system.cylinders.get_mut(vehicle) {
                cylinder.set_native_air_skid(
                    state.throttle < 0.001
                        && axle.integration.wheel_slip
                        && cylinder.retard_force_n(v) > 25.,
                );
            }
        }
        state.brake_system.record_native_brake_forces(v);
    }
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
    // Tooth brakes, yard retarders and off-rail drag are mechanical contacts.
    // Apply them after wheel adhesion limits, once in every integration branch.
    let rack_brakes = crate::rack::brake_forces(state, train.vehicle_lengths_m.len());
    let offrail_drag = crate::derailment::drag_forces(state, train);
    let before_yard = equipment_brake_forces.clone();
    crate::yard_gravity::apply_retarder_brakes(
        state,
        path_data,
        &train.vehicle_lengths_m,
        &mut equipment_brake_forces,
    );
    let yard_brakes = equipment_brake_forces
        .iter()
        .zip(&before_yard)
        .map(|(after, before)| (after - before).max(0.))
        .collect::<Vec<_>>();
    f_brake += yard_brakes.iter().sum::<f64>();
    let contact_brakes = rack_brakes
        .iter()
        .enumerate()
        .map(|(i, r)| {
            r + offrail_drag.get(i).copied().unwrap_or(0.)
                + yard_brakes.get(i).copied().unwrap_or(0.)
        })
        .collect::<Vec<_>>();
    let mechanical_brakes = contact_brakes.iter().any(|f| *f > 0.);
    for (i, force) in equipment_brake_forces.iter_mut().enumerate() {
        let extra =
            rack_brakes.get(i).copied().unwrap_or(0.) + offrail_drag.get(i).copied().unwrap_or(0.);
        *force += extra;
        f_brake += extra;
    }
    if mechanical_brakes {
        let mut wheel = before_yard;
        if let Some(config) = &train.rail_adhesion {
            crate::adhesion::cap_brakes(state, config, &mut wheel);
        }
        for (i, f) in equipment_brake_forces.iter_mut().enumerate() {
            *f = wheel[i] + contact_brakes.get(i).copied().unwrap_or(0.);
        }
        f_brake = equipment_brake_forces.iter().sum();
    }
    let equipped_brakes = equipped_brakes || mechanical_brakes;
    let vertical_resistances = crate::track_irregularity::longitudinal_forces(state);
    // OR evaluates tunnel drag once per vehicle before its body/coupler tick.
    // Geometry comes from the current pose, published at the preceding tick;
    // observed native forces or positions never enter this calculation.
    let tunnel_forces: Vec<f64> = train.native.as_ref().map_or_else(Vec::new, |native| {
        let train_length_m = train
            .vehicle_lengths_m
            .iter()
            .fold(0_f32, |sum, length| sum + *length as f32) as f64;
        native
            .vehicles
            .iter()
            .enumerate()
            .map(|(i, vehicle)| {
                let mass = effective_vehicle_masses.get(i).copied().unwrap_or_else(|| {
                    crate::consumable_mass::vehicle_mass_kg(
                        train,
                        state,
                        i,
                        native.vehicle_masses_kg[i],
                    )
                });
                let input = crate::native_tunnel::TunnelForceInput {
                    train_length_m,
                    train_mass_kg: effective_mass,
                    car_mass_kg: mass,
                    car_cross_section_m2: vehicle.body_cross_section_m2,
                    speed_mps: state.vehicles.get(i).map_or(v, |car| car.velocity_mps),
                };
                crate::native_tunnel::force_at(
                    path_data,
                    vehicle_centers[i],
                    train.vehicle_lengths_m[i],
                    state.path_reversed,
                    &input,
                )
            })
            .collect()
    });
    let f_resist = train.davis.resistance_n(v) + vertical_resistances.iter().sum::<f64>();
    let grade_fraction =
        edge_data.grade_at(state.pos_on_edge_m) / 100.0 * if backwards { -1. } else { 1. };
    let f_grade = effective_mass * G * grade_fraction;
    let resistance = |i: usize, mass: f64, davis: &DavisCoefficients, speed: f64| {
        let roughness = vertical_resistances.get(i).copied().unwrap_or(0.);
        if let (Some(native), Some(dynamics)) = (&train.native, &state.native_dynamics) {
            roughness
                + tunnel_forces.get(i).copied().unwrap_or(0.)
                + native.resistance(i, mass, davis, speed, dynamics)
                + native.curve_resistance_n(
                    i,
                    path_data,
                    vehicle_centers[i],
                    train.vehicle_lengths_m[i],
                    mass,
                    speed,
                    native_curve_coefficients[i],
                )
        } else {
            roughness + davis.resistance_n(speed)
        }
    };
    // ── Multi-body coupler path ───────────────────────────────────────────────
    // When the state has per-vehicle data (initialised by the runner), delegate
    // to the spring-damper solver.  The resulting mean velocity is used for
    // position integration and energy accounting below.
    let coupler_orientation = if state.driver_reverser <= 0.25 {
        -1.
    } else if state.driver_reverser >= 0.75 {
        1.
    } else {
        state
            .couplers
            .first()
            .and_then(|c| c.memory.position_orientation)
            .unwrap_or(1.)
    };
    let rigid_native = train.native.as_ref().is_some_and(|n| {
        n.vehicles.len() > 1
            && n.vehicles
                .iter()
                .take(n.vehicles.len() - 1)
                .all(|v| v.rigid_connection)
    }) && state
        .couplers
        .iter()
        .all(|c| !c.broken && c.native_profile.as_ref().is_none_or(|p| p.rigid));
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
        let new_speed = (v + dt * (f_motor - f_brake - resist - grade) / effective_mass.max(1.))
            .max(0.) as f32 as f64;
        if state.couplers.iter().any(|c| c.native_profile.is_some()) {
            let acceleration = if dt > 0. { (new_speed - v) / dt } else { 0. };
            let requested = if equipped_brakes {
                equipment_brake_forces.clone()
            } else {
                state.brake_system.cylinder_forces_n(v)
            };
            let requested_total = requested.iter().sum::<f64>();
            // A brake is a static reaction once the train has stopped. It
            // cannot supply the unused portion of a backwards acceleration.
            let actual_brake = if new_speed == 0. {
                (f_motor - resist - grade - effective_mass * acceleration)
                    .max(0.)
                    .min(f_brake)
            } else {
                f_brake
            };
            let raw_motor = state.rail_force_n.iter().sum::<f64>();
            let mut joint_force = 0.;
            for i in 0..state.couplers.len() {
                let mass = effective_vehicle_masses[i];
                let motor = if raw_motor.abs() > 1e-9 {
                    state.rail_force_n.get(i).copied().unwrap_or(0.) * f_motor / raw_motor
                } else if i == 0 {
                    f_motor
                } else {
                    0.
                };
                let brake = if requested_total > 0. {
                    requested.get(i).copied().unwrap_or(0.) * actual_brake / requested_total
                } else {
                    actual_brake * mass / effective_mass.max(1.)
                };
                let drag = resistance(i, mass, &train.vehicle_davis[i], v);
                joint_force +=
                    motor - brake - drag - mass * G * vehicle_grades[i] - mass * acceleration;
                let coupler = &mut state.couplers[i];
                if let Some(profile) = &coupler.native_profile {
                    let physical_force = coupler_orientation * joint_force;
                    coupler.memory.constraint_force_n = physical_force;
                    coupler.memory.position_orientation = Some(coupler_orientation);
                    coupler.memory.previous_force_n = physical_force.clamp(
                        -profile.compression.max_force_n(),
                        profile.tension.max_force_n(),
                    );
                    coupler.memory.warning_exceeded = joint_force.abs() > profile.warning_force_n;
                    if coupler.break_force_n > 0. && joint_force.abs() > coupler.break_force_n {
                        coupler.broken = true;
                    }
                }
            }
        }
        for vehicle in &mut state.vehicles {
            vehicle.velocity_mps = new_speed;
            vehicle.position_m += (new_speed as f32 * dt as f32) as f64;
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
            .is_some_and(|threshold| state.throttle <= 0.0 && f_motor <= 1.0 && v < threshold)
            && state.couplers.iter().all(|c| c.native_profile.is_none());
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
                    if mechanical_brakes {
                        let mut wheel = brake_forces.clone();
                        for (i, f) in wheel.iter_mut().enumerate() {
                            *f = (*f - contact_brakes.get(i).copied().unwrap_or(0.)).max(0.);
                        }
                        crate::adhesion::cap_brakes(state, config, &mut wheel);
                        for (i, f) in brake_forces.iter_mut().enumerate() {
                            *f = wheel[i] + contact_brakes.get(i).copied().unwrap_or(0.);
                        }
                    } else {
                        crate::adhesion::cap_brakes(state, config, &mut brake_forces);
                    }
                }
                let authored_joints = state.couplers.iter().any(|c| c.native_profile.is_some());
                let grade_resist: Vec<f64> = if train.vehicle_davis.len() == state.vehicles.len() {
                    state
                        .vehicles
                        .iter()
                        .zip(train.vehicle_davis.iter())
                        .zip(masses.iter())
                        .enumerate()
                        .map(|(i, ((veh, davis), mass))| {
                            let drag = resistance(i, *mass, davis, veh.velocity_mps.abs());
                            let gravity =
                                mass * G * vehicle_grades.get(i).copied().unwrap_or(grade_fraction);
                            if authored_joints {
                                // Dissipative forces oppose each car's motion;
                                // gravity retains its sign through a rebound.
                                brake_forces[i] += drag;
                                gravity
                            } else {
                                drag + gravity
                            }
                        })
                        .collect()
                } else {
                    masses
                        .iter()
                        .map(|m| (f_resist + f_grade) * m / total_mass)
                        .collect()
                };
                mean_v = multi_body_step_distributed_oriented(
                    &mut state.vehicles,
                    &mut state.couplers,
                    &motor_forces,
                    &brake_forces,
                    &grade_resist,
                    &masses,
                    sub_dt,
                    1.0,
                    coupler_orientation,
                )
                .max(0.0);
                if !state.articulated_joints.is_empty() {
                    crate::articulation::constrain(
                        &mut state.vehicles,
                        &mut state.couplers,
                        &state.articulated_joints,
                        &masses,
                    );
                }
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

    let simple_native = train
        .native
        .as_ref()
        .is_some_and(|n| n.environment.simple_control_physics);
    // Train.Update uses final Single speed and distanceM for native scalar and
    // rigid formations. Elastic couplers retain their Rust body integrator;
    // they are not declared a port of OR's constraint/slack integrator.
    let native_scalar_motion =
        train.native.is_some() && (state.vehicles.is_empty() || rigid_native);
    let v_new = if simple_native || native_scalar_motion {
        v_new as f32 as f64
    } else {
        v_new
    };
    let v_avg = 0.5 * (v + v_new);
    let travel_max = if native_scalar_motion {
        // Train.Update's distanceM uses the final Single speed of the tick.
        (v_new as f32 * dt as f32) as f64
    } else {
        v_avg * dt
    };
    let mut travel = travel_max;
    let mut traveled = 0.0;
    let mut arrived = false;

    while travel > 0.0 && state.edge_index < path_data.edges.len() {
        if backwards {
            let room = state.pos_on_edge_m;
            if travel <= room {
                state.pos_on_edge_m -= travel;
                traveled += travel;
                travel = 0.;
            } else {
                let consumed = room.max(0.);
                travel -= consumed;
                traveled += consumed;
                if state.edge_index == 0 {
                    state.pos_on_edge_m = 0.;
                    arrived = true;
                    break;
                }
                state.edge_index -= 1;
                state.pos_on_edge_m = path_data.edges[state.edge_index].length_m;
            }
            continue;
        }
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
    state.odometer_m = if native_scalar_motion {
        // OR DistanceTravelledM is a Single accumulated after each native tick.
        f64::from(state.odometer_m as f32 + traveled as f32)
    } else {
        state.odometer_m + traveled
    };
    state.time = state.time + effective_dt;
    state.velocity_mps = if arrived { 0.0 } else { v_new };
    crate::yard_gravity::account_retarder_work(state, effective_dt);
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
