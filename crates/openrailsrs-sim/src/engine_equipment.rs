//! Optional independent engines and conservative oil/coolant circuits.
use openrailsrs_scenarios::vehicle_systems::{DieselCircuitsDef, MotorDef};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MotorState {
    #[serde(default)]
    pub refilled_l: f64,
    pub running: bool,
    pub isolated: bool,
    pub rpm: f64,
    pub fuel_l: f64,
    pub consumed_l: f64,
    pub power_w: f64,
    pub oil_l: f64,
    pub coolant_l: f64,
    pub oil_c: f64,
    pub coolant_c: f64,
    pub fan: bool,
    pub fan_enabled: bool,
    pub fault: Option<MotorFault>,
    pub generated_heat_j: f64,
    pub rejected_heat_j: f64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MotorFault {
    OilLevel,
    CoolantLevel,
    OilTemperature,
    CoolantTemperature,
}
impl MotorFault {
    pub fn label(self) -> &'static str {
        match self {
            Self::OilLevel => "Reserva de aceite insuficiente",
            Self::CoolantLevel => "Reserva de refrigerante insuficiente",
            Self::OilTemperature => "Sobretemperatura de aceite",
            Self::CoolantTemperature => "Sobretemperatura de refrigerante",
        }
    }
}
impl MotorState {
    pub fn new(p: &MotorDef, c: Option<&DieselCircuitsDef>) -> Self {
        Self {
            refilled_l: 0.,
            running: p.fuel_l > 0.,
            isolated: false,
            rpm: if p.fuel_l > 0. { p.idle_rpm } else { 0. },
            fuel_l: p.fuel_l,
            consumed_l: 0.,
            power_w: 0.,
            oil_l: c.map_or(0., |c| c.oil_capacity_l),
            coolant_l: c.map_or(0., |c| c.coolant_capacity_l),
            oil_c: 40.,
            coolant_c: 40.,
            fan: false,
            fan_enabled: true,
            fault: None,
            generated_heat_j: 0.,
            rejected_heat_j: 0.,
        }
    }
    pub fn valid(&self, p: &MotorDef, c: Option<&DieselCircuitsDef>) -> bool {
        self.rejected_heat_j.is_finite()
            && self.rejected_heat_j.abs() <= 1e15
            && [
                self.rpm,
                self.refilled_l,
                self.fuel_l,
                self.consumed_l,
                self.power_w,
                self.oil_l,
                self.coolant_l,
                self.generated_heat_j,
            ]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.)
            && [self.oil_c, self.coolant_c]
                .iter()
                .all(|v| v.is_finite() && (-100. ..=2000.).contains(v))
            && self.rpm <= p.maximum_rpm
            && self.power_w <= p.rated_power_w + 1e-6
            && self.fuel_l <= p.fuel_l + 1e-6
            && (self.fuel_l + self.consumed_l - self.refilled_l - p.fuel_l).abs() < 1e-6
            && c.map_or(self.oil_l == 0. && self.coolant_l == 0., |c| {
                self.oil_l <= c.oil_capacity_l && self.coolant_l <= c.coolant_capacity_l
            })
    }
    pub fn cause(&self, c: &DieselCircuitsDef) -> Option<MotorFault> {
        if self.oil_l < c.oil_capacity_l * c.minimum_oil_fraction {
            Some(MotorFault::OilLevel)
        } else if self.coolant_l < c.coolant_capacity_l * c.minimum_coolant_fraction {
            Some(MotorFault::CoolantLevel)
        } else if self.oil_c >= c.shutdown_oil_c {
            Some(MotorFault::OilTemperature)
        } else if self.coolant_c >= c.shutdown_coolant_c {
            Some(MotorFault::CoolantTemperature)
        } else {
            None
        }
    }
    pub fn advance(
        &mut self,
        p: &MotorDef,
        c: Option<&DieselCircuitsDef>,
        throttle: f64,
        ambient: f64,
        dt: f64,
    ) {
        if dt <= 0. {
            return;
        }
        if let Some(c) = c
            && c.failures_enabled
            && let Some(f) = self.cause(c)
        {
            self.fault = Some(f);
            self.running = false;
        }
        let running = self.running && self.fuel_l > 0. && self.fault.is_none();
        let demand = if running {
            p.idle_rpm
                + (p.maximum_rpm - p.idle_rpm)
                    * if self.isolated {
                        0.
                    } else {
                        throttle.clamp(0., 1.)
                    }
        } else {
            0.
        };
        let rate = if demand > self.rpm {
            p.rise_rpm_s
        } else {
            p.fall_rpm_s
        };
        if let Some(native) = &p.native_governor {
            let mut governor = openrailsrs_train::diesel::DieselEngineParams::from_msts_defaults(
                p.rated_power_w,
                p.idle_rpm,
                p.maximum_rpm,
                p.rise_rpm_s,
            );
            if !native.power_curve.is_empty() {
                governor.power_tab = native.power_curve.iter().map(|q| (q[0], q[1])).collect();
            }
            if !native.throttle_rpm_curve.is_empty() {
                governor.throttle_rpm_tab = native
                    .throttle_rpm_curve
                    .iter()
                    .map(|q| (q[0], q[1]))
                    .collect();
            }
            governor.rate_of_change_up_rpm_pss = native.rate_up_rpm_s2;
            governor.rate_of_change_down_rpm_pss = native.rate_down_rpm_s2;
            governor.change_up_rpm_ps = p.rise_rpm_s;
            governor.change_down_rpm_ps = p.fall_rpm_s;
            self.rpm = governor.advance_native_target_rpm(
                self.rpm,
                if running {
                    governor.target_rpm(if self.isolated { 0. } else { throttle })
                } else {
                    0.
                },
                dt,
            );
        } else {
            self.rpm += (demand - self.rpm).clamp(-rate * dt, rate * dt);
        }
        let fraction = ((self.rpm - p.idle_rpm) / (p.maximum_rpm - p.idle_rpm)).clamp(0., 1.);
        self.power_w = if running && !self.isolated {
            if let Some(g) = &p.native_governor
                && g.power_curve.len() >= 2
            {
                let pair = g
                    .power_curve
                    .windows(2)
                    .find(|q| self.rpm <= q[1][0])
                    .unwrap_or(&g.power_curve[g.power_curve.len() - 2..]);
                let mix = ((self.rpm - pair[0][0]) / (pair[1][0] - pair[0][0])).clamp(0., 1.);
                (pair[0][1] + mix * (pair[1][1] - pair[0][1])).min(p.rated_power_w)
            } else {
                p.rated_power_w * fraction.min(throttle)
            }
        } else {
            0.
        };
        let used = if running {
            (p.idle_lph + (p.full_lph - p.idle_lph) * fraction) * dt / 3600.
        } else {
            0.
        };
        let burn = used.min(self.fuel_l);
        self.fuel_l -= burn;
        self.consumed_l += burn;
        if self.fuel_l <= 0. {
            self.power_w = 0.;
            self.running = false;
        }
        if let Some(c) = c {
            let count = (dt / 0.02).ceil().max(1.) as usize;
            let tick = dt / count as f64;
            for _ in 0..count {
                if !self.fan_enabled {
                    self.fan = false;
                } else if self.coolant_c >= c.fan_start_c {
                    self.fan = true;
                } else if self.coolant_c <= c.fan_stop_c {
                    self.fan = false;
                }
                let power = self.power_w * c.heat_fraction;
                let radiator = c.radiator_w_k * if self.fan { 1. } else { 0.1 };
                let co = c.oil_heat_capacity_j_k;
                let cc = c.coolant_heat_capacity_j_k;
                let k = c.transfer_w_k;
                let oil_eq = ambient + power / radiator + power / k;
                let water_eq = ambient + power / radiator;
                let a = -k / co;
                let b = k / co;
                let d = k / cc;
                let e = -(k + radiator) / cc;
                let half = (a + e) * 0.5;
                let z = (((a - e) * 0.5).powi(2) + b * d).sqrt();
                let (average, factor) = if z < 1e-12 {
                    let decay = (half * tick).exp();
                    (decay, decay * tick)
                } else {
                    let r1 = ((half + z) * tick).exp();
                    let r2 = ((half - z) * tick).exp();
                    ((r1 + r2) * 0.5, (r1 - r2) / (2. * z))
                };
                let old_oil = self.oil_c;
                let old_water = self.coolant_c;
                let x = old_oil - oil_eq;
                let y = old_water - water_eq;
                self.oil_c = oil_eq + average * x + factor * ((a - half) * x + b * y);
                self.coolant_c = water_eq + average * y + factor * (d * x + (e - half) * y);
                let energy = power * tick;
                self.generated_heat_j += energy;
                self.rejected_heat_j +=
                    energy - co * (self.oil_c - old_oil) - cc * (self.coolant_c - old_water);
                if c.failures_enabled
                    && let Some(f) = self.cause(c)
                {
                    self.fault = Some(f);
                    self.running = false;
                    self.power_w = 0.;
                }
            }
        }
    }
}

pub fn advance(state: &mut crate::TrainSimState, throttle: f64, count: usize, dt: f64) {
    let throttles = state
        .vehicle_systems
        .iter()
        .map(|c| {
            (
                c.definition.vehicle,
                crate::multiple_unit::throttle(state, c.definition.vehicle, throttle),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    for car in &mut state.vehicle_systems {
        let i = car.definition.vehicle;
        if i >= count {
            continue;
        }
        let connected = state
            .power_supply
            .cars
            .iter()
            .find(|c| c.vehicle == i)
            .is_none_or(|c| {
                if car.definition.battery.is_some() {
                    c.supply.cab
                } else {
                    c.supply.main
                }
            });
        let driving_diesel = !matches!(
            car.energy.active,
            Some(
                openrailsrs_scenarios::vehicle_systems::TractionSource::Line
                    | openrailsrs_scenarios::vehicle_systems::TractionSource::Battery
            )
        );
        for (s, p) in car.motors.iter_mut().zip(&car.definition.motors) {
            let before = s.consumed_l;
            s.advance(
                p,
                car.definition.diesel_circuits.as_ref(),
                if connected && driving_diesel {
                    throttles.get(&i).copied().unwrap_or(throttle)
                } else {
                    0.
                },
                state.ambient_temperature_c,
                dt,
            );
            state.fuel_consumption_g +=
                (s.consumed_l - before) * crate::diesel_operation::DIESEL_KG_PER_L * 1000.;
        }
    }
}
pub fn limit_force(state: &crate::TrainSimState, i: usize, force: f64, speed: f64) -> f64 {
    let Some(c) = state
        .vehicle_systems
        .iter()
        .find(|c| c.definition.vehicle == i && !c.motors.is_empty())
    else {
        return force;
    };
    if matches!(
        c.energy.active,
        Some(
            openrailsrs_scenarios::vehicle_systems::TractionSource::Line
                | openrailsrs_scenarios::vehicle_systems::TractionSource::Battery
        )
    ) {
        return force;
    }
    force.min(c.motors.iter().map(|s| s.power_w).sum::<f64>() / speed.abs().max(0.5))
}
pub fn apparent_fraction(state: &crate::TrainSimState, vehicle: usize) -> Option<f64> {
    let c = state
        .vehicle_systems
        .iter()
        .find(|c| c.definition.vehicle == vehicle && !c.motors.is_empty())?;
    let total = c
        .definition
        .motors
        .iter()
        .map(|p| p.rated_power_w)
        .sum::<f64>();
    Some(
        c.motors
            .iter()
            .zip(&c.definition.motors)
            .map(|(s, p)| {
                if s.running && !s.isolated && s.fault.is_none() {
                    ((s.rpm - p.idle_rpm) / (p.maximum_rpm - p.idle_rpm)).clamp(0., 1.)
                        * p.rated_power_w
                } else {
                    0.
                }
            })
            .sum::<f64>()
            / total.max(1.),
    )
}
impl crate::LiveDriveSession {
    pub fn operate_motor(
        &mut self,
        vehicle: usize,
        motor: usize,
        action: MotorOperation,
    ) -> Result<(), String> {
        if vehicle >= self.formation.coupled_count {
            return Err("El motor está desacoplado".into());
        }
        let c = self
            .state
            .vehicle_systems
            .iter_mut()
            .find(|c| c.definition.vehicle == vehicle)
            .ok_or("Sin motores declarados")?;
        let s = c.motors.get_mut(motor).ok_or("Motor inexistente")?;
        match action {
            MotorOperation::StartStop => {
                if s.fault.is_some() || s.fuel_l <= 0. {
                    return Err("Resolvé la causa del fallo o reponé combustible".into());
                }
                s.running = !s.running;
            }
            MotorOperation::Isolate => s.isolated = !s.isolated,
            MotorOperation::Fan => s.fan_enabled = !s.fan_enabled,
            MotorOperation::Reset => {
                if c.definition
                    .diesel_circuits
                    .as_ref()
                    .is_some_and(|p| s.cause(p).is_some())
                {
                    return Err("La causa física continúa presente".into());
                }
                s.fault = None;
            }
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug)]
pub enum MotorOperation {
    StartStop,
    Isolate,
    Fan,
    Reset,
}
