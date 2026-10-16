//! Native starting/running resistance, controller notches and Pacha axle
//! dynamics from the pinned OR 1.6.1 sources. The reference's 50 ms fixed tick
//! selects Pacha; physical wheel speed remains distinct from vehicle speed.
use openrailsrs_formats::NativeVehiclePhysics;
use openrailsrs_scenarios::NativePhysicsEnvironment;
use openrailsrs_train::{Consist, DavisCoefficients, Vehicle};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone)]
pub struct NativeTrainPhysics {
    pub vehicles: Vec<NativeVehiclePhysics>,
    pub vehicle_masses_kg: Vec<f64>,
    pub environment: NativePhysicsEnvironment,
    pub control_vehicle_index: usize,
    pub locomotive_indices: Vec<usize>,
    pub adhesion_inputs: crate::native_adhesion_input::VehicleAdhesionInputs,
}
/// Literal Single force boundary from TrainCar.UpdateCurveForce. Geometry is
/// queried from our own pose; the reference values belong only in tests.
#[derive(Clone, Copy, Debug)]
pub struct NativeCurveForceInput {
    pub mass_kg: f32,
    pub speed_mps: f32,
    pub radius_m: f32,
    pub cant_m: f32,
    pub gauge_m: f32,
    pub rigid_wheelbase_m: f32,
    pub wagon_coefficient: f32,
}
impl NativeCurveForceInput {
    pub fn force_n(self) -> f32 {
        if self.radius_m <= 0. || self.gauge_m <= 0. {
            return 0.;
        }
        let base = 9.80665_f32
            * self.mass_kg
            * self.wagon_coefficient
            * (self.gauge_m + self.rigid_wheelbase_m)
            / (2_f32 * self.radius_m);
        // rspeedKgpTonne is computed first in the native expression. Moving
        // 1.5 or mass across those Single operations changes their rounding.
        // Kg.ToTonne multiplies by a pre-rounded Single reciprocal.
        let rspeed = 1.5_f32
            * (self.speed_mps * self.speed_mps / self.radius_m
                - 9.80665_f32 * self.cant_m / self.gauge_m)
                .abs();
        base + 9.80665_f32 * ((self.mass_kg * (1_f32 / 1000_f32)) * rspeed)
    }
}
fn initial_wagon_curve_coefficient() -> f32 {
    0.35
}
impl NativeTrainPhysics {
    /// UpdateFrictionCoefficient publishes a shared Train field while Cars are
    /// updated in order. Wagons before the first locomotive see the previous
    /// tick's bus; subsequent wagons see the nearest preceding locomotive's
    /// current coefficient. This must run before Pacha replaces its AbsSpeed
    /// cache. The stochastic IIR output is deliberately not part of this bus.
    pub fn prepare_curve_friction(
        &self,
        state: &mut NativeDynamicsState,
        diesel_vehicle_indices: &[usize],
        speed_mps: f64,
    ) -> Vec<f32> {
        let mut coefficient = state.wagon_curve_coefficient;
        let base = state.adhesion_factor.clamp(0.5, 1.) as f32;
        let mut coefficients = Vec::with_capacity(self.vehicles.len());
        for index in 0..self.vehicles.len() {
            if self.locomotive_indices.contains(&index) {
                let abs_speed = diesel_vehicle_indices
                    .iter()
                    .position(|i| *i == index)
                    .and_then(|i| state.axles.get(i))
                    .and_then(|a| a.integration.previous_train_speed_mps)
                    .unwrap_or(speed_mps)
                    .abs() as f32;
                coefficient = (7.5_f32 / (abs_speed * 3.6_f32 + 44_f32) + 0.161_f32) * base;
            }
            coefficients.push(coefficient);
        }
        state.wagon_curve_coefficient = coefficient;
        coefficients
    }
    /// TrainCar.UpdateCurvePhys samples both bogies. An infinite radius at one
    /// bogie makes the average radius infinite (the native straight condition).
    #[allow(clippy::too_many_arguments)]
    pub fn curve_resistance_n(
        &self,
        index: usize,
        path: &crate::path_data::PathData,
        center_m: f64,
        length_m: f64,
        mass_kg: f64,
        speed_mps: f64,
        wagon_coefficient: f32,
    ) -> f64 {
        let Some(p) = self.vehicles.get(index) else {
            return 0.;
        };
        let face = p.coupler_face_length_m.unwrap_or(length_m);
        let span = p.pitch_span_m.unwrap_or({
            if face >= 4.3 {
                face - 4.3
            } else {
                length_m * 0.65
            }
        });
        let (a, ca) = path.curve_at_chainage(center_m - span / 2.);
        let (b, cb) = path.curve_at_chainage(center_m + span / 2.);
        if a.abs() < 1e-12 || b.abs() < 1e-12 {
            return 0.;
        }
        // Enumerable.Average(Single[]) sums in Double and returns Single.
        let radius =
            ((f64::from(a.abs().recip() as f32) + f64::from(b.abs().recip() as f32)) * 0.5) as f32;
        let gauge = p.curve_gauge_m as f32;
        if radius <= 0. || gauge <= 0. {
            return 0.;
        }
        let physical_cant = path
            .physical_cant_at_chainage(center_m - span / 2.)
            .zip(path.physical_cant_at_chainage(center_m + span / 2.))
            .map(|(a, b)| 0.5 * (a.abs() + b.abs()))
            .unwrap_or((ca.sin().abs() + cb.sin().abs()) * 0.5 * p.curve_gauge_m);
        let cant = (physical_cant as f32).min(gauge);
        f64::from(
            NativeCurveForceInput {
                mass_kg: mass_kg as f32,
                speed_mps: speed_mps as f32,
                radius_m: radius,
                cant_m: cant,
                gauge_m: gauge,
                rigid_wheelbase_m: p.curve_rigid_wheelbase_m as f32,
                wagon_coefficient,
            }
            .force_n(),
        )
    }

    pub fn load(
        path: &Path,
        base: &Path,
        consist: &Consist,
        environment: Option<&NativePhysicsEnvironment>,
    ) -> Result<Option<Self>, crate::SimError> {
        Self::load_with_cab(path, base, consist, environment).map(|(physics, _)| physics)
    }
    /// Cab metadata is required even when optional native axle dynamics are off.
    pub fn load_with_cab(
        path: &Path,
        base: &Path,
        consist: &Consist,
        environment: Option<&NativePhysicsEnvironment>,
    ) -> Result<(Option<Self>, Option<f64>), crate::SimError> {
        let masses: Vec<_> = consist
            .vehicles
            .iter()
            .map(|v| match v {
                Vehicle::Loco(l) => l.mass_kg,
                Vehicle::Wagon(w) => w.mass_kg,
            })
            .collect();
        let mut vehicles = openrailsrs_train::load_consist_native_parameters(path, base, &masses)?;
        for (parameters, vehicle) in vehicles.iter_mut().zip(&consist.vehicles) {
            parameters.flipped = vehicle.flipped();
        }
        let cab = consist
            .vehicles
            .iter()
            .position(|v| matches!(v, Vehicle::Loco(_)));
        let max_current_a = cab
            .and_then(|index| vehicles.get(index))
            .and_then(|v| v.max_current_a);
        if environment.is_none()
            && !vehicles
                .iter()
                .any(|v| v.roller_bearing || v.rigid_connection || v.traction_cutoff_bar.is_some())
        {
            return Ok((None, max_current_a));
        }
        Ok((
            Some(Self {
                vehicles,
                vehicle_masses_kg: masses,
                environment: environment.cloned().unwrap_or_default(),
                control_vehicle_index: cab.unwrap_or(0),
                locomotive_indices: consist
                    .vehicles
                    .iter()
                    .enumerate()
                    .filter_map(|(i, v)| matches!(v, Vehicle::Loco(_)).then_some(i))
                    .collect(),
                adhesion_inputs: Vec::new(),
            }),
            max_current_a,
        ))
    }
    /// Bind a scenario-contained, hashed environmental input series before the
    /// first tick. File bytes/vehicle source identities remain immutable across
    /// reordering, splitting and joining; only filter memory and cursor evolve.
    pub fn configure_adhesion_inputs(
        &mut self,
        scenario_dir: &Path,
        step_s: f64,
        duration_s: f64,
    ) -> Result<(), crate::SimError> {
        match (
            &self.environment.adhesion_input_file,
            self.environment.adhesion_input_step_s,
        ) {
            (None, None) => {
                self.adhesion_inputs.clear();
                Ok(())
            }
            (Some(path), Some(input_step)) if (input_step - step_s).abs() <= 1e-8 => {
                self.adhesion_inputs = crate::native_adhesion_input::load(
                    scenario_dir,
                    path,
                    input_step,
                    duration_s,
                    self.vehicles.len(),
                    &self.locomotive_indices,
                )
                .map_err(crate::SimError::Msg)?;
                Ok(())
            }
            _ => Err(crate::SimError::Msg(
                "Las entradas ambientales deben usar el paso fijo del escenario".into(),
            )),
        }
    }
    pub fn has_adhesion_inputs(&self) -> bool {
        self.adhesion_inputs.iter().any(Option::is_some)
    }
    pub fn initial_adhesion_filters(
        &self,
    ) -> Vec<Option<crate::native_adhesion_input::AdhesionInputState>> {
        if !self.has_adhesion_inputs() {
            return Vec::new();
        }
        self.adhesion_inputs
            .iter()
            .map(|s| s.as_ref().map(|s| s.initial.clone()))
            .collect()
    }
    pub fn valid_adhesion_state(
        &self,
        dynamics: Option<&NativeDynamicsState>,
        time_s: f64,
        terminal: bool,
    ) -> bool {
        let filters = dynamics
            .map(|d| d.adhesion_filters.as_slice())
            .unwrap_or_default();
        if !self.has_adhesion_inputs() {
            return filters.iter().all(Option::is_none);
        }
        if dynamics.is_none() {
            return time_s.abs() <= 1e-7;
        }
        filters.len() == self.adhesion_inputs.len()
            && self
                .adhesion_inputs
                .iter()
                .zip(filters)
                .all(|(series, state)| match (series, state) {
                    (None, None) => true,
                    (Some(series), Some(state)) => series.valid_state(state, time_s, terminal),
                    _ => false,
                })
    }
    /// Validate before a caller advances any physical state. An exhausted or
    /// mismatched recording is an explicit simulation error, never a held value.
    pub fn validate_adhesion_tick(
        &self,
        state: &crate::TrainSimState,
        dt: f64,
    ) -> Result<(), String> {
        if !self.valid_adhesion_state(state.native_dynamics.as_ref(), state.time_s(), false) {
            return Err("El reloj, vehículo o SHA de las entradas ambientales no corresponde a esta partida".into());
        }
        for (i, series) in self.adhesion_inputs.iter().enumerate() {
            if let Some(series) = series {
                let history = state
                    .native_dynamics
                    .as_ref()
                    .and_then(|d| d.adhesion_filters.get(i))
                    .and_then(Option::as_ref)
                    .unwrap_or(&series.initial);
                series.next(history, state.time_s(), dt)?;
            }
        }
        Ok(())
    }
    pub fn advance_adhesion_inputs(&self, state: &mut NativeDynamicsState, time_s: f64, dt: f64) {
        for (series, history) in self.adhesion_inputs.iter().zip(&mut state.adhesion_filters) {
            if let (Some(series), Some(history)) = (series, history) {
                // The captured base is used only to recover the external noise.
                // Our own position/weather determines the slippery-spot base.
                let sample = series
                    .next(history, time_s, dt)
                    .expect("environment input tick was validated before physics");
                let noise = sample.input_factor - sample.base_factor;
                history.base_factor = state.adhesion_factor as f32;
                history
                    .filter
                    .advance(history.base_factor + noise, dt as f32);
                history.cursor += 1;
            }
        }
    }
    pub fn adhesion_factor_for(&self, state: &NativeDynamicsState, vehicle: usize) -> f64 {
        state
            .adhesion_filters
            .get(vehicle)
            .and_then(Option::as_ref)
            .map_or(state.adhesion_factor, |s| {
                f64::from(s.filter.y[0].clamp(0.05, 2.5))
            })
    }
    pub fn initial_state(&self, engines: usize) -> NativeDynamicsState {
        NativeDynamicsState {
            bearing_c: vec![self.environment.ambient_c; self.vehicles.len()],
            axles: vec![NativeAxleState::default(); engines],
            adhesion_factor: self.environment.adhesion_factor,
            mu_throttle: 0.,
            wagon_curve_coefficient: initial_wagon_curve_coefficient(),
            adhesion_filters: self.initial_adhesion_filters(),
        }
    }
    /// DieselEngines.Update precedes the locomotive's UpdateControllers. The
    /// latter publishes the lead's controller to Train.MUThrottlePercent, so
    /// engines through the lead see the previous bus value and following cars
    /// see the newly published value. This phase belongs to the governor; the
    /// later traction calculation still uses the current controller command.
    pub fn governor_throttle(
        &self,
        state: &NativeDynamicsState,
        vehicle: usize,
        current_throttle: f64,
    ) -> f64 {
        if vehicle <= self.control_vehicle_index {
            state.mu_throttle
        } else {
            current_throttle
        }
    }
    pub fn throttle(&self, command: f64, pressure_bar: f64) -> f64 {
        let Some(lead) = self.vehicles.get(self.control_vehicle_index) else {
            return command;
        };
        if lead
            .traction_cutoff_bar
            .is_some_and(|limit| pressure_bar >= limit)
        {
            return 0.;
        }
        let points = &lead.throttle_notches;
        if points.is_empty() {
            return command;
        }
        let i = points
            .partition_point(|point| point.0 <= command)
            .saturating_sub(1);
        let (lo, smooth) = points[i];
        let Some(&(hi, next_smooth)) = points.get(i + 1) else {
            return lo;
        };
        if smooth || next_smooth {
            command
        } else if command - lo >= 0.55 * (hi - lo) {
            hi
        } else {
            lo
        }
    }
    pub fn update_bearings(
        &self,
        state: &mut NativeDynamicsState,
        speed: f64,
        odometer: f64,
        dt: f64,
    ) {
        for temperature in &mut state.bearing_c {
            if speed > 7. {
                *temperature = (90. + (*temperature - 90.) * (-0.000790635114477831 * dt).exp())
                    .min(55. + speed * 0.25);
            } else if *temperature > self.environment.ambient_c {
                *temperature = self.environment.ambient_c
                    + (*temperature - self.environment.ambient_c)
                        * (-0.0003355569417321907 * dt).exp();
            }
        }
        if let Some([distance, length]) = self.environment.first_slippery_spot {
            if odometer >= distance - length
                && (self.environment.paused_weather || odometer <= distance)
            {
                state.adhesion_factor = if (0.6..0.8).contains(&self.environment.adhesion_factor) {
                    0.6
                } else {
                    0.8
                };
            } else {
                state.adhesion_factor = self.environment.adhesion_factor;
            }
        }
    }
    pub fn resistance(
        &self,
        i: usize,
        mass: f64,
        davis: &DavisCoefficients,
        speed: f64,
        state: &NativeDynamicsState,
    ) -> f64 {
        let Some(vehicle) = self.vehicles.get(i) else {
            return davis.resistance_n(speed);
        };
        let v = speed.abs();
        if vehicle.legacy_friction {
            return if v < 0.1 {
                vehicle.legacy_start_force_n.unwrap_or(davis.a_n * 2.)
            } else {
                // MSTSWagon's legacy branch evaluates the nested polynomial
                // in Single precision, including its trailing-engine factor.
                let v = v as f32;
                let tail = if i > 0 && vehicle.drive_axles > 0. {
                    0.2
                } else {
                    1.
                };
                (davis.a_n as f32
                    + v * (davis.b_n_per_mps as f32 + v * davis.c_n_per_mps2 as f32 * tail))
                    as f64
            };
        }
        if !vehicle.roller_bearing {
            return davis.resistance_n(v);
        };
        let t = state.bearing_c[i].clamp(-10., 25.);
        let running = davis.a_n * (1.3 - (t + 10.) * 0.3 / 35.);
        if v >= 2.2352 {
            return running + davis.b_n_per_mps * v + davis.c_n_per_mps2 * v * v;
        };
        let diameter_factor = 2. * vehicle.wheel_radius_m / 0.0254 / 37.;
        let internal = (12. - (t + 10.) * 7.5 / 35.) * 4.448221615 * diameter_factor;
        let axle_tons = vehicle.axle_load_kg / 1016.047;
        let grade = (800. - (axle_tons - 5.) * 400. / 21.).max(100.);
        let track = 4.448221615 * 1120. * axle_tons / grade;
        let mut start = mass / 1000. * internal + track;
        if start < running {
            start = running * 1.2
        };
        let merge = running + davis.b_n_per_mps * 2.2352 + davis.c_n_per_mps2 * 2.2352f64.powi(2);
        start + (merge - start) * v / 2.2352
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controller_uses_the_driven_cab_behind_a_leading_wagon() {
        let parse = |text, engine| {
            openrailsrs_formats::parse_native_vehicle_physics(
                &openrailsrs_formats::parse(text).unwrap(),
                engine,
                80000.0,
            )
        };
        let wagon = parse("(Wagon (Mass 80t))", false);
        let loco = parse(
            "(Engine (MaxCurrent 900A) (ORTSTractionCutOff 1.2bar))",
            true,
        );
        let mut physics = NativeTrainPhysics {
            vehicles: vec![wagon, loco],
            vehicle_masses_kg: vec![80000.0; 2],
            environment: Default::default(),
            control_vehicle_index: 1,
            locomotive_indices: vec![1],
            adhesion_inputs: Vec::new(),
        };
        physics.vehicles[1].traction_cutoff_bar = Some(1.2);
        physics.vehicles[1].throttle_notches = vec![(0.0, false), (0.5, false), (1.0, false)];
        assert_eq!(physics.throttle(0.8, 0.0), 1.0);
        assert_eq!(physics.throttle(0.8, 1.3), 0.0);
    }
    #[test]
    fn legacy_starting_force_is_distinct_from_running_davis_and_tail_drag() {
        let mut parameters = openrailsrs_formats::parse_native_vehicle_physics(
            &openrailsrs_formats::parse_named_stf("Wagon (w) Engine (e NumWheels(1))").unwrap(),
            true,
            25000.,
        );
        parameters.legacy_friction = true;
        parameters.legacy_start_force_n = Some(707.);
        let native = NativeTrainPhysics {
            vehicles: vec![parameters.clone(), parameters],
            vehicle_masses_kg: vec![25000.; 2],
            environment: Default::default(),
            control_vehicle_index: 0,
            locomotive_indices: vec![0, 1],
            adhesion_inputs: Vec::new(),
        };
        let state = native.initial_state(2);
        let davis = DavisCoefficients {
            a_n: 100.,
            b_n_per_mps: 10.,
            c_n_per_mps2: 1.,
        };
        assert_eq!(native.resistance(0, 25000., &davis, 0., &state), 707.);
        assert_eq!(native.resistance(1, 25000., &davis, 0.05, &state), 707.);
        assert_eq!(native.resistance(0, 25000., &davis, 3., &state), 139.);
        assert_eq!(
            native.resistance(1, 25000., &davis, -3., &state),
            131.8_f32 as f64
        );
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeDynamicsState {
    pub bearing_c: Vec<f64>,
    pub axles: Vec<NativeAxleState>,
    pub adhesion_factor: f64,
    /// Published native MU bus at the end of the previous physical tick.
    /// A new stationary native train starts with Train.MUThrottlePercent=0.
    #[serde(default)]
    pub mu_throttle: f64,
    /// Last coefficient published by a locomotive in the ordered native tick.
    #[serde(default = "initial_wagon_curve_coefficient")]
    pub wagon_curve_coefficient: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adhesion_filters: Vec<Option<crate::native_adhesion_input::AdhesionInputState>>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NativeAxleState {
    /// Wheel velocity in the normalized train frame. The integrator receives
    /// and returns signed vehicle-local coordinates through step_oriented.
    pub speed_mps: f64,
    #[serde(default)]
    pub integration: NativeAxleIntegration,
}

/// Persistent Pacha state from Axle.Integrate. Substep history is physical
/// integration state, not a render-frame cache; save/cut/pickup must retain it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NativeAxleIntegration {
    pub num_substeps: usize,
    pub integrator_error: f64,
    pub wait_before_speeding_up: i32,
    pub axle_force_local_n: f64,
    pub compensated_force_local_n: f64,
    pub drive_force_local_n: f64,
    pub previous_slip_mps: f64,
    pub previous_train_speed_mps: Option<f64>,
    pub previous_slip_percent: f64,
    pub wheel_slip: bool,
    pub wheel_slip_warning: bool,
    pub local_frame_sign: f64,
}

// C# Math.Sign(0) is zero. Rust signum(0.0) is +1 and must not be used at axle
// standstill or in the native zero-crossing brake constraint.
fn native_sign(value: f64) -> f64 {
    if value > 0. {
        1.
    } else if value < 0. {
        -1.
    } else {
        0.
    }
}

// elapsedClockSeconds is Single in Axle.Integrate. Its division by an Int32
// is therefore rounded before the result is assigned to the Double RK step.
fn native_substep_dt(elapsed: f32, count: usize) -> f64 {
    (elapsed / count as f32) as f64
}
fn native_curtius(vehicle: &NativeVehiclePhysics, speed: f32, factor: f64) -> f32 {
    let [a, b, c] = vehicle.curtius_kniffler;
    let a = if a > 0. { a as f32 } else { 7.5 };
    let b = if b > 0. { b as f32 } else { 44. };
    let c = if c > 0. { c as f32 } else { 0.161 };
    factor as f32 * (a / (speed.abs() * 3.6 + b) + c)
}
fn pacha_coefficient(slip: f32, adhesion: f32, k: f32) -> f32 {
    let x = slip * 3.6 * adhesion / k;
    let sqrt3 = 3f32.sqrt();
    if x.abs() > sqrt3 {
        native_sign(slip as f64) as f32
            * adhesion
            * ((sqrt3 / 2. - 0.4) * (((sqrt3 - x.abs()) / (2. * sqrt3 - 1.6)) as f64).exp() as f32
                + 0.4)
    } else {
        2. * adhesion * x / (1. + x * x)
    }
}

impl NativeAxleState {
    pub fn valid(&self) -> bool {
        let m = &self.integration;
        self.speed_mps.is_finite()
            && [
                m.integrator_error,
                m.axle_force_local_n,
                m.compensated_force_local_n,
                m.drive_force_local_n,
                m.previous_slip_mps,
                m.previous_slip_percent,
                m.local_frame_sign,
            ]
            .iter()
            .all(|v| v.is_finite())
            && m.previous_train_speed_mps.is_none_or(f64::is_finite)
            && m.num_substeps <= 50
            && (0..=100).contains(&m.wait_before_speeding_up)
            && m.previous_slip_mps >= 0.
            && m.previous_slip_percent >= 0.
            && [-1., 0., 1.].contains(&m.local_frame_sign)
    }
    /// Original UpdateTraction chooses wheel velocity only after the previous
    /// adhesion tick reported slip; otherwise it uses the vehicle velocity.
    pub fn traction_speed_mps(&self, train_speed: f64) -> f64 {
        if self.integration.wheel_slip {
            self.speed_mps.abs()
        } else {
            // TrainCar.Update caches AbsSpeed at the end of the locomotive
            // tick; Train.Update integrates Speed afterwards. This cache is
            // also used by the native Curtius/Pacha phase and survives saves.
            self.integration
                .previous_train_speed_mps
                .unwrap_or(train_speed)
                .abs()
        }
    }
    /// Preserve the public helper for existing skid/diagnostic callers.
    pub fn wheel_slipping(&self, train_speed: f64, adhesion_factor: f64) -> bool {
        if self.integration.num_substeps > 0 {
            return self.integration.wheel_slip;
        }
        let adhesion = adhesion_factor * (7.5 / (train_speed.abs() * 3.6 + 44.) + 0.161);
        adhesion > 0. && (self.speed_mps - train_speed).abs() > 0.7 / adhesion / 3.6
    }
    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        vehicle: &NativeVehiclePhysics,
        mass: f64,
        driven_mass: f64,
        train_speed: f64,
        drive: f64,
        brake: f64,
        adhesion_factor: f64,
        dt: f64,
    ) -> f64 {
        self.step_oriented(
            vehicle,
            mass,
            driven_mass,
            train_speed,
            drive,
            brake,
            adhesion_factor,
            dt,
            1.,
        )
    }
    /// Pinned OR1.6.1 Pacha/ForceDriven virtual axle, including adaptive RK4
    /// state and float boundaries. Inputs/return are normalized train-frame
    /// values; orientation is the vehicle-local sign for this physical step.
    #[allow(clippy::too_many_arguments)]
    pub fn step_oriented(
        &mut self,
        vehicle: &NativeVehiclePhysics,
        mass: f64,
        driven_mass: f64,
        train_speed: f64,
        drive: f64,
        brake: f64,
        adhesion_factor: f64,
        dt: f64,
        orientation: f64,
    ) -> f64 {
        // UpdateFrictionCoefficient runs before TrainCar.Update refreshes
        // AbsSpeed: Curtius therefore uses the preceding axle-input speed.
        let coefficient_speed = self
            .integration
            .previous_train_speed_mps
            .unwrap_or(train_speed);
        let adhesion_limit =
            native_curtius(vehicle, coefficient_speed as f32, adhesion_factor) as f64;
        self.step_oriented_with_limit(
            vehicle,
            mass,
            driven_mass,
            train_speed,
            drive,
            brake,
            adhesion_limit,
            dt,
            orientation,
        )
    }
    /// Original Axle input boundary for an independent subsystem replay.
    /// Gameplay derives its own limit in step_oriented; wheel velocity and
    /// expected output forces are never prescribed here.
    #[allow(clippy::too_many_arguments)]
    pub fn step_oriented_with_limit(
        &mut self,
        vehicle: &NativeVehiclePhysics,
        mass: f64,
        driven_mass: f64,
        train_speed: f64,
        drive: f64,
        brake: f64,
        adhesion_limit: f64,
        dt: f64,
        orientation: f64,
    ) -> f64 {
        let sign = if orientation < 0. { -1. } else { 1. };
        let previous_sign = if self.integration.local_frame_sign < 0. {
            -1.
        } else {
            1.
        };
        if sign != previous_sign {
            self.integration.integrator_error = -self.integration.integrator_error;
            self.integration.axle_force_local_n = -self.integration.axle_force_local_n;
            self.integration.compensated_force_local_n =
                -self.integration.compensated_force_local_n;
            self.integration.drive_force_local_n = -self.integration.drive_force_local_n;
        }
        self.integration.local_frame_sign = sign;
        if dt <= 0. {
            return self.integration.compensated_force_local_n * sign;
        }
        let elapsed = dt as f32;
        let train_speed = (train_speed * sign) as f32;
        let drive = (drive * sign) as f32;
        let brake = brake as f32;
        let radius = vehicle.wheel_radius_m as f32;
        let radius_squared = radius * radius;
        let wheel_mass = 500. * radius_squared / (0.5 * 0.5);
        let inertia =
            (vehicle.drive_axles.max(1.) as f32 * wheel_mass * radius_squared + 500.).min(40000.);
        let acceleration = (radius_squared / inertia) as f64;
        let adhesion = adhesion_limit as f32;
        let k = if vehicle.adhesion_k > 0. {
            vehicle.adhesion_k as f32
        } else {
            0.7
        };
        let threshold = k / adhesion / 3.6;
        let weight = driven_mass as f32 * 9.81;
        let damping = if vehicle.axle_damping_ns > 0. {
            vehicle.axle_damping_ns as f32
        } else {
            mass as f32 / 1000.
        };
        let friction = if vehicle.axle_friction_n > 0. {
            vehicle.axle_friction_n as f32
        } else {
            mass as f32 / 1000.
        };
        let dissipative = (brake + friction) as f64;
        let force_in = (drive * 0.99) as f64;
        let mut local_speed = self.speed_mps * sign;
        let slip_speed = ((local_speed - train_speed as f64) as f32).abs();
        let memory = &mut self.integration;
        // Axle.Integrate Pacha branch: previous last-substep error controls
        // next-tick rate; do not replace this with a fixed 1ms approximation.
        if memory.integrator_error.abs() > ((slip_speed - 1.) * 0.01).max(0.001) as f64 {
            memory.num_substeps = (memory.num_substeps + 1).min(50);
            memory.wait_before_speeding_up = 100;
        } else {
            memory.wait_before_speeding_up -= 1;
            if memory.wait_before_speeding_up <= 0 {
                memory.num_substeps = memory.num_substeps.saturating_sub(1);
                memory.wait_before_speeding_up = 10;
            }
        }
        memory.num_substeps = memory.num_substeps.clamp(1, 50);
        let motion = |speed: f64, h: f64| -> (f64, f64) {
            let slip = speed - train_speed as f64;
            let out = (weight * pacha_coefficient(speed as f32 - train_speed, adhesion, k)) as f64;
            let input = force_in - damping as f64 * slip;
            if train_speed.abs() < 0.001
                && slip.abs() < 0.001f32 as f64
                && input.abs() < dissipative
            {
                return (-slip / h, 0.);
            }
            (
                (input - native_sign(speed) * dissipative - out) * acceleration,
                out,
            )
        };
        let mut h = native_substep_dt(elapsed, memory.num_substeps);
        let mut half_h = h / 2.;
        let mut out_sum = 0.;
        let mut stopped = false;
        let mut i = 0;
        while i < memory.num_substeps {
            let k1 = motion(local_speed, h);
            if i == 0 {
                if k1.0 * h > (((slip_speed - 1.) * 10.).max(1.) / 100.) as f64 {
                    memory.num_substeps = (memory.num_substeps + 5).min(50);
                    h = native_substep_dt(elapsed, memory.num_substeps);
                    half_h = h / 2.;
                }
                if native_sign(local_speed + k1.0 * h) != native_sign(local_speed)
                    && dissipative > (drive as f64 - k1.1).abs()
                {
                    local_speed = 0.;
                    memory.axle_force_local_n = 0.;
                    stopped = true;
                    break;
                }
            }
            let k2 = motion(local_speed + k1.0 * half_h, half_h);
            let k3 = motion(local_speed + k2.0 * half_h, half_h);
            let k4 = motion(local_speed + k3.0 * h, h);
            memory.integrator_error = (k1.0 + 2. * (k2.0 + k3.0) + k4.0) * h / 6.;
            local_speed += memory.integrator_error;
            out_sum += k1.1 + 2. * (k2.1 + k3.1) + k4.1;
            i += 1;
        }
        if !stopped {
            memory.axle_force_local_n =
                ((out_sum / (memory.num_substeps as f64 * 6.)) as f32) as f64;
        }
        memory.drive_force_local_n = force_in as f32 as f64;
        let out = memory.axle_force_local_n as f32;
        let compensation = (brake + friction).min(out.abs());
        memory.compensated_force_local_n = (if train_speed.abs() < 0.001 && out == 0. {
            0.
        } else if train_speed < 0. {
            out - compensation
        } else {
            out + compensation
        }) as f64;
        memory.previous_slip_mps = ((local_speed - train_speed as f64) as f32).abs() as f64;
        memory.previous_slip_percent =
            (memory.previous_slip_mps as f32 / threshold * 100.).abs() as f64;
        memory.wheel_slip = memory.previous_slip_mps > threshold as f64;
        memory.wheel_slip_warning = memory.wheel_slip
            || memory.previous_slip_percent
                > if vehicle.slip_warning_percent > 0. {
                    vehicle.slip_warning_percent
                } else {
                    70.
                };
        self.speed_mps = local_speed * sign;
        memory.previous_train_speed_mps = Some(train_speed as f64 * sign);
        memory.compensated_force_local_n * sign
    }
}

#[cfg(test)]
mod axle_contract_tests {
    use super::*;
    fn profile() -> NativeVehiclePhysics {
        openrailsrs_formats::parse_native_vehicle_physics(
            &openrailsrs_formats::parse_named_stf(
                "Wagon (w WheelRadius(54.6in)) Engine (e WheelRadius(27.3in) NumWheels(1))",
            )
            .unwrap(),
            true,
            25000.,
        )
    }
    #[test]
    fn csharp_sign_preserves_standstill_and_negative_zero() {
        assert_eq!(native_sign(0.), 0.);
        assert_eq!(native_sign(-0.), 0.);
        assert_eq!(native_sign(-1.), -1.);
    }
    #[test]
    fn substep_division_rounds_single_before_the_double_runge_kutta_step() {
        // Native Single 50 ms / 3 is 0x3c888889, while division after
        // promotion to Double produces a different integration boundary.
        let h = native_substep_dt(0.05_f32, 3);
        assert_eq!(h, f32::from_bits(0x3c88_8889) as f64);
        assert_ne!(h, 0.05_f32 as f64 / 3.);
        assert_eq!(native_substep_dt(0.05_f32, 1), 0.05_f32 as f64);
        assert_eq!(native_substep_dt(0.05_f32, 50), 0.001_f32 as f64);
    }
    #[test]
    fn adhesion_limit_uses_the_previous_car_update_speed_in_both_frames() {
        let p = profile();
        for sign in [-1., 1.] {
            let mut productive = NativeAxleState::default();
            productive.step_oriented(&p, 25000., 25000., 2., 0., 0., 1., 0.05, sign);
            let mut boundary: NativeAxleState =
                serde_json::from_str(&serde_json::to_string(&productive).unwrap()).unwrap();
            let expected = boundary.step_oriented_with_limit(
                &p,
                25000.,
                25000.,
                8.,
                60000.,
                0.,
                native_curtius(&p, 2., 1.) as f64,
                0.05,
                sign,
            );
            let actual =
                productive.step_oriented(&p, 25000., 25000., 8., 60000., 0., 1., 0.05, sign);
            assert_eq!(actual, expected);
            assert_eq!(productive.speed_mps, boundary.speed_mps);
            assert_eq!(productive.integration.previous_train_speed_mps, Some(8.));
            assert!(productive.valid() && boundary.valid());
            boundary.integration.previous_train_speed_mps = Some(f64::NAN);
            assert!(!boundary.valid());
        }
    }
    #[test]
    fn adaptive_axle_replay_restores_all_integration_memory_in_both_frames() {
        let p = profile();
        for sign in [-1., 1.] {
            let mut a = NativeAxleState::default();
            for _ in 0..30 {
                a.step_oriented(&p, 25000., 25000., 0., 30000., 1000., 1., 0.05, sign);
            }
            let mut restored: NativeAxleState =
                serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
            for _ in 0..100 {
                let x = a.step_oriented(&p, 25000., 25000., 2., 60000., 0., 1., 0.05, sign);
                let y = restored.step_oriented(&p, 25000., 25000., 2., 60000., 0., 1., 0.05, sign);
                assert_eq!(x, y);
                assert_eq!(a.speed_mps, restored.speed_mps);
                assert_eq!(
                    a.integration.num_substeps,
                    restored.integration.num_substeps
                );
                assert!(a.valid() && restored.valid());
            }
        }
    }
    #[test]
    fn brakes_hold_an_unpowered_axle_at_standstill_without_launching_it() {
        let p = profile();
        for sign in [-1., 1.] {
            let mut axle = NativeAxleState::default();
            for _ in 0..40 {
                assert_eq!(
                    axle.step_oriented(&p, 25000., 25000., 0., 0., 10000., 1., 0.05, sign),
                    0.
                );
                assert_eq!(axle.speed_mps, 0.);
            }
        }
    }
}
