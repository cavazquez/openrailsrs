//! Opt-in rheostatic/regen blending, thermal fade and wheel-slide protection.
//! Undeclared stock retains the reference pneumatic model.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrakeEquipmentState {
    pub temperature_c: f64,
    pub heat_j: f64,
    pub cooled_j: f64,
    pub friction_force_n: f64,
    pub electric_force_n: f64,
    pub regenerative_w: f64,
    pub rheostatic_w: f64,
    pub returned_j: f64,
    pub dissipated_j: f64,
    pub delay_s: f64,
    pub started_s: Option<f64>,
    pub previous_speed_mps: f64,
    pub ramped_force_n: f64,
    pub wheel_speed_mps: f64,
    pub pressure_fraction: f64,
    pub sliding: bool,
    pub traction_fraction: f64,
    pub traction_cut: bool,
}
impl Default for BrakeEquipmentState {
    fn default() -> Self {
        Self {
            temperature_c: 20.,
            heat_j: 0.,
            cooled_j: 0.,
            friction_force_n: 0.,
            electric_force_n: 0.,
            regenerative_w: 0.,
            rheostatic_w: 0.,
            returned_j: 0.,
            dissipated_j: 0.,
            delay_s: 0.,
            started_s: None,
            previous_speed_mps: 0.,
            ramped_force_n: 0.,
            wheel_speed_mps: 0.,
            pressure_fraction: 1.,
            sliding: false,
            traction_fraction: 1.,
            traction_cut: false,
        }
    }
}
impl BrakeEquipmentState {
    pub fn valid(&self) -> bool {
        self.started_s.is_none_or(|v| v.is_finite() && v >= 0.)
            && self.previous_speed_mps.is_finite()
            && self.previous_speed_mps >= 0.
            && self.ramped_force_n.is_finite()
            && self.ramped_force_n >= 0.
            && self.cooled_j.is_finite()
            && self.cooled_j.abs() <= 1e15
            && [self.temperature_c, self.wheel_speed_mps]
                .iter()
                .all(|v| v.is_finite() && (-100. ..=2000.).contains(v))
            && [
                self.heat_j,
                self.friction_force_n,
                self.electric_force_n,
                self.regenerative_w,
                self.rheostatic_w,
                self.returned_j,
                self.dissipated_j,
                self.delay_s,
            ]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.)
            && [self.pressure_fraction, self.traction_fraction]
                .iter()
                .all(|v| v.is_finite() && (0. ..=1.).contains(v))
    }
}
pub fn interpolate(curve: &[[f64; 2]], speed: f64) -> f64 {
    if speed <= curve[0][0] {
        return curve[0][1];
    }
    let Some(p) = curve.windows(2).find(|p| speed <= p[1][0]) else {
        return curve.last().unwrap()[1];
    };
    p[0][1] + (p[1][1] - p[0][1]) * (speed - p[0][0]) / (p[1][0] - p[0][0])
}
pub fn traction_limit(state: &crate::TrainSimState, i: usize, force: f64) -> f64 {
    state
        .vehicle_systems
        .iter()
        .find(|c| c.definition.vehicle == i)
        .map_or(force, |c| force * c.brakes.traction_fraction)
}
pub fn prepare_traction(state: &mut crate::TrainSimState, count: usize, speed: f64, dt: f64) {
    for c in &mut state.vehicle_systems {
        if c.definition.vehicle >= count {
            continue;
        }
        if let Some(p) = &c.definition.traction_control {
            let wheel = state
                .rail_adhesion
                .as_ref()
                .and_then(|r| r.cars.get(c.definition.vehicle))
                .map_or(speed, |w| w.wheel_speed_mps.abs());
            let slip = (wheel - speed).max(0.) / speed.max(1.);
            c.brakes.traction_cut = slip > p.slip_threshold;
            c.brakes.traction_fraction = (c.brakes.traction_fraction
                + dt * if c.brakes.traction_cut {
                    -p.cut_rate_s
                } else {
                    p.recover_rate_s
                })
            .clamp(0., 1.);
        }
    }
}
/// Resolve one common set of contact forces for single and multi-body solvers.
pub fn forces(
    state: &mut crate::TrainSimState,
    train: &crate::physics::TrainPhysics,
    speed: f64,
    dt: f64,
    mut friction: Vec<f64>,
) -> Vec<f64> {
    let clock = state.time_s();
    let mut receptive = state
        .vehicle_systems
        .iter()
        .filter(|c| {
            state
                .electric
                .cars
                .iter()
                .any(|e| e.vehicle == c.definition.vehicle && e.main_power)
        })
        .filter_map(|c| c.definition.electric_brake.as_ref())
        .map(|p| p.receptivity_w)
        .reduce(f64::min)
        .unwrap_or(0.);
    let commands = state
        .vehicle_systems
        .iter()
        .map(|c| {
            (
                c.definition.vehicle,
                crate::multiple_unit::brake(state, c.definition.vehicle),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    for c in &mut state.vehicle_systems {
        let i = c.definition.vehicle;
        let Some(raw) = friction.get_mut(i) else {
            continue;
        };
        let b = &mut c.brakes;
        let requested = *raw;
        let fade = c.definition.brake_heat.as_ref().map_or(1., |p| {
            1. - (1. - p.minimum_factor)
                * ((b.temperature_c - p.fade_start_c) / (p.fade_full_c - p.fade_start_c))
                    .clamp(0., 1.)
        });
        *raw *= fade;
        if let Some(p) = &c.definition.wheel_slide {
            let mass = state
                .vehicle_masses
                .get(i)
                .copied()
                .or_else(|| {
                    train
                        .rail_adhesion
                        .as_ref()
                        .and_then(|a| a.vehicles.get(i))
                        .map(|c| c.mass_kg)
                })
                .unwrap_or(train.mass_kg / train.vehicle_lengths_m.len().max(1) as f64);
            let grip = state
                .rail_adhesion
                .as_ref()
                .and_then(|r| r.cars.get(i))
                .map_or(1., |r| r.factor);
            let contact = mass * 9.80665 * crate::adhesion::friction(speed, grip);
            if b.wheel_speed_mps == 0. && speed > 0. && b.pressure_fraction == 1. {
                b.wheel_speed_mps = speed;
            }
            let over = (*raw * b.pressure_fraction - contact).max(0.);
            let under = (contact - *raw * b.pressure_fraction).max(0.);
            b.wheel_speed_mps = (b.wheel_speed_mps
                + dt * (under - over) * p.radius_m.powi(2) / p.wheel_inertia_kg_m2)
                .clamp(0., speed);
            b.sliding = speed > 0.5 && (speed - b.wheel_speed_mps) / speed > p.slip_threshold;
            b.pressure_fraction = if !p.enabled {
                1.
            } else {
                (b.pressure_fraction
                    + dt * if b.sliding {
                        -p.release_rate_s
                    } else {
                        p.reapply_rate_s
                    })
                .clamp(0., 1.)
            };
            if let Some(cylinder) = state.brake_system.cylinders.get_mut(i) {
                cylinder.limit_service_fraction(b.pressure_fraction);
            }
            *raw *= b.pressure_fraction;
        }
        b.friction_force_n = *raw;
        b.electric_force_n = 0.;
        b.regenerative_w = 0.;
        b.rheostatic_w = 0.;
        if let Some(p) = &c.definition.electric_brake {
            let handle = if p.blend_with_service {
                state.brake.max(commands.get(&i).copied().unwrap_or(0.))
            } else {
                commands.get(&i).copied().unwrap_or(0.)
            };
            let powered = state
                .power_supply
                .cars
                .iter()
                .find(|s| s.vehicle == i)
                .is_none_or(|s| s.supply.dynamic_brake_available);
            if handle > 0. && powered && state.throttle <= 0.001 {
                b.started_s.get_or_insert(clock);
                b.delay_s = (clock - b.started_s.unwrap()).max(0.);
            } else {
                b.started_s = None;
                b.delay_s = 0.;
                b.ramped_force_n = 0.;
            }
            let ready = b.started_s.is_some()
                && if p.native_response {
                    b.delay_s > p.apply_delay_s
                } else {
                    b.delay_s >= p.apply_delay_s
                };
            if ready && handle > 0. && speed > 0.001 {
                let curve = if p.notches.is_empty() {
                    interpolate(&p.force_curve, speed) * handle
                } else {
                    let upper = p
                        .notches
                        .partition_point(|(h, _)| *h < handle)
                        .min(p.notches.len() - 1);
                    if upper == 0 {
                        interpolate(&p.notches[0].1, speed) * handle / p.notches[0].0.max(1e-9)
                    } else {
                        let (a, b) = (&p.notches[upper - 1], &p.notches[upper]);
                        let mix = ((handle - a.0) / (b.0 - a.0)).clamp(0., 1.);
                        interpolate(&a.1, speed) * (1. - mix) + interpolate(&b.1, speed) * mix
                    }
                };
                let ramp = openrailsrs_train::DieselTractionModel {
                    traction_force_ramp_up_nps: p.force_ramp_n_s[0],
                    traction_force_ramp_down_nps: p.force_ramp_n_s[1],
                    traction_force_ramp_down_to_zero_nps: p.force_ramp_n_s[2],
                    traction_power_ramp_up_wps: p.power_ramp_w_s[0],
                    traction_power_ramp_down_wps: p.power_ramp_w_s[1],
                    traction_power_ramp_down_to_zero_wps: p.power_ramp_w_s[2],
                    ..Default::default()
                };
                b.ramped_force_n = ramp.update_force_with_ramp(
                    b.ramped_force_n,
                    dt,
                    curve,
                    f64::INFINITY,
                    speed,
                    b.previous_speed_mps,
                );
                let curve = b.ramped_force_n;
                let demand = if p.blend_with_service && state.brake > 0. {
                    curve.min(requested)
                } else {
                    curve
                };
                let connected = state
                    .electric
                    .cars
                    .iter()
                    .any(|e| e.vehicle == i && e.main_power);
                let regen = if connected {
                    p.regenerative_max_w.min(receptive / p.efficiency)
                } else {
                    0.
                };
                let power = (demand * speed).min(regen + p.rheostat_max_w);
                b.electric_force_n = power / speed;
                b.regenerative_w = power.min(regen) * p.efficiency;
                receptive = (receptive - b.regenerative_w).max(0.);
                b.rheostatic_w = power - b.regenerative_w;
                if p.blend_with_service {
                    b.friction_force_n = (*raw - b.electric_force_n).max(0.);
                }
                *raw = b.friction_force_n + b.electric_force_n;
            }
        }
        // Applied force and both energy paths share the same wheel/rail cap.
        // A locked wheel has lower sliding contact; WSP releases its cylinder
        // to restore rolling contact rather than creating additional adhesion.
        if let Some(rail) = &state.rail_adhesion {
            let mass = state
                .vehicle_masses
                .get(i)
                .copied()
                .or_else(|| {
                    train
                        .rail_adhesion
                        .as_ref()
                        .and_then(|p| p.vehicles.get(i))
                        .map(|p| p.mass_kg)
                })
                .unwrap_or(train.mass_kg / train.vehicle_lengths_m.len().max(1) as f64);
            let grip = rail.cars.get(i).map_or(rail.weather_factor, |r| r.factor);
            let cap = mass
                * 9.80665
                * crate::adhesion::friction(speed, grip)
                * if b.sliding { 0.65 } else { 1. };
            let scale = (cap / raw.max(1e-9)).min(1.);
            b.friction_force_n *= scale;
            b.electric_force_n *= scale;
            // Return any rejected regenerative allocation to following cars.
            receptive += b.regenerative_w * (1. - scale);
            b.regenerative_w *= scale;
            b.rheostatic_w *= scale;
            *raw *= scale;
        }
        b.previous_speed_mps = speed;
        if state.rail_adhesion.is_some()
            && let Some(config) = &train.rail_adhesion
            && let Some(vehicle) = config.vehicles.get(i)
        {
            let grip = state
                .rail_adhesion
                .as_ref()
                .and_then(|r| r.cars.get(i))
                .map_or(1., |r| r.factor);
            let cap = vehicle.mass_kg * 9.80665 * crate::adhesion::friction(speed, grip);
            let ratio = if *raw > 0. {
                (*raw).min(cap) / *raw
            } else {
                1.
            };
            *raw *= ratio;
            b.friction_force_n *= ratio;
            b.electric_force_n *= ratio;
            b.regenerative_w *= ratio;
            b.rheostatic_w *= ratio;
        }
    }
    friction
}
pub fn account(state: &mut crate::TrainSimState, speed: f64, dt: f64) {
    for c in &mut state.vehicle_systems {
        let b = &mut c.brakes;
        if let Some(p) = &c.definition.brake_heat {
            let generated = b.friction_force_n * speed * dt;
            let initial = b.temperature_c;
            let equilibrium =
                state.ambient_temperature_c + b.friction_force_n * speed / p.cooling_w_k;
            b.temperature_c = equilibrium
                + (initial - equilibrium) * (-p.cooling_w_k / p.heat_capacity_j_k * dt).exp();
            b.heat_j += generated;
            b.cooled_j += generated - (b.temperature_c - initial) * p.heat_capacity_j_k;
        }
        if c.definition.electric_brake.is_some() {
            let returned = b.regenerative_w * dt;
            b.returned_j += returned;
            b.dissipated_j += b.rheostatic_w * dt;
            state.regen_energy_j += returned;
            state.cumulative_energy_j -= returned;
        }
    }
}
impl crate::LiveDriveSession {
    pub fn set_electric_brake(&mut self, command: f64) -> Result<(), String> {
        if !command.is_finite() || !(0. ..=1.).contains(&command) {
            return Err("Mando eléctrico inválido".into());
        }
        if !self
            .state
            .vehicle_systems
            .iter()
            .any(|c| c.definition.electric_brake.is_some())
        {
            return Err("La formación no declara freno eléctrico".into());
        }
        if command > 0. && self.driver_throttle > 0.001 {
            return Err("Cerrá el regulador antes de aplicar freno eléctrico".into());
        }
        self.state.electric_brake_command = command;
        Ok(())
    }
}
