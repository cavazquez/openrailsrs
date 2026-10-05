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
}
impl NativeTrainPhysics {
    pub fn load(
        path: &Path,
        base: &Path,
        consist: &Consist,
        environment: Option<&NativePhysicsEnvironment>,
    ) -> Result<Option<Self>, crate::SimError> {
        let masses: Vec<_> = consist
            .vehicles
            .iter()
            .map(|v| match v {
                Vehicle::Loco(l) => l.mass_kg,
                Vehicle::Wagon(w) => w.mass_kg,
            })
            .collect();
        let vehicles = openrailsrs_train::load_consist_native_parameters(path, base, &masses)?;
        if environment.is_none()
            && !vehicles
                .iter()
                .any(|v| v.roller_bearing || v.rigid_connection || v.traction_cutoff_bar.is_some())
        {
            return Ok(None);
        }
        Ok(Some(Self {
            vehicles,
            vehicle_masses_kg: masses,
            environment: environment.cloned().unwrap_or_default(),
        }))
    }
    pub fn initial_state(&self, engines: usize) -> NativeDynamicsState {
        NativeDynamicsState {
            bearing_c: vec![self.environment.ambient_c; self.vehicles.len()],
            axles: vec![NativeAxleState::default(); engines],
            adhesion_factor: self.environment.adhesion_factor,
        }
    }
    pub fn throttle(&self, command: f64, pressure_bar: f64) -> f64 {
        let Some(lead) = self.vehicles.first() else {
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
                davis.a_n * 2.
            } else {
                davis.a_n
                    + davis.b_n_per_mps * v
                    + davis.c_n_per_mps2
                        * v
                        * v
                        * if i > 0 && vehicle.drive_axles > 0. {
                            0.2
                        } else {
                            1.
                        }
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeDynamicsState {
    pub bearing_c: Vec<f64>,
    pub axles: Vec<NativeAxleState>,
    pub adhesion_factor: f64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NativeAxleState {
    pub speed_mps: f64,
}
impl NativeAxleState {
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
        let radius = vehicle.wheel_radius_m;
        let inertia =
            (vehicle.drive_axles.max(1.) * 500. * radius.powi(4) / 0.25 + 500.).min(40000.);
        let acceleration = radius.powi(2) / inertia;
        let adhesion = adhesion_factor * (7.5 / (train_speed.abs() * 3.6 + 44.) + 0.161);
        let weight = driven_mass * 9.81;
        let motion = |speed: f64, h: f64| {
            let slip = speed - train_speed;
            let force_in = drive * 0.99;
            let friction = brake + mass / 1000.;
            let motion = force_in - mass / 1000. * slip;
            if train_speed.abs() < 0.001 && slip.abs() < 0.001 && motion.abs() < friction {
                return (-slip / h, 0.);
            };
            let x = slip * 3.6 * adhesion / 0.7;
            let sqrt3 = 3f64.sqrt();
            let mu = if x.abs() > sqrt3 {
                x.signum()
                    * adhesion
                    * ((sqrt3 / 2. - 0.4) * ((sqrt3 - x.abs()) / (2. * sqrt3 - 1.6)).exp() + 0.4)
            } else {
                2. * adhesion * x / (1. + x * x)
            };
            let out = weight * mu;
            (
                (motion - speed.signum() * friction - out) * acceleration,
                out,
            )
        };
        // Fixed substeps give deterministic wheel integration independent of
        // frame rate; RK4 and force compensation follow Axle.cs.
        let n = (dt / 0.001).ceil().max(1.) as usize;
        let h = dt / n as f64;
        let mut out = 0.;
        for _ in 0..n {
            let k1 = motion(self.speed_mps, h);
            if (self.speed_mps + k1.0 * h).signum() != self.speed_mps.signum()
                && brake + mass / 1000. > (drive - k1.1).abs()
            {
                self.speed_mps = 0.;
                return 0.;
            }
            let k2 = motion(self.speed_mps + k1.0 * h / 2., h / 2.);
            let k3 = motion(self.speed_mps + k2.0 * h / 2., h / 2.);
            let k4 = motion(self.speed_mps + k3.0 * h, h);
            self.speed_mps += (k1.0 + 2. * k2.0 + 2. * k3.0 + k4.0) * h / 6.;
            out += (k1.1 + 2. * k2.1 + 2. * k3.1 + k4.1) / 6.;
        }
        out /= n as f64;
        if train_speed.abs() < 0.001 && out == 0. {
            0.
        } else {
            out + (brake + mass / 1000.).min(out.abs()) * if train_speed < 0. { -1. } else { 1. }
        }
    }
}
