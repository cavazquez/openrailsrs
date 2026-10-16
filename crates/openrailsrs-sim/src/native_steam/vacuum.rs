// OR1.6.1 MSTSLocomotive.UpdateSteamEjector and steam UpdateAuxiliaries;
// VacuumSinglePipe.PropagateBrakeLinePressures non-EQ steam branch.
// GPL-3.0-or-later. No air compressor, EQ vacuum or independent brake here.
use super::*;
use openrailsrs_formats::NativeSteamBrakeKind;
const VAC_ATM: f32 = 14.5037738;
pub fn vacuum_to_pressure(inhg: f32) -> f32 {
    VAC_ATM - (inhg * 0.03386389) * 14.5037738
}
fn pressure_to_vacuum(psi: f32) -> f32 {
    ((VAC_ATM - psi) * (1. / 14.5037738)) * (1. / 0.03386389)
}
fn cylinder_vacuum_psi(psi: f32) -> f32 {
    (pressure_to_vacuum(psi) * 3.386389) * (1. / 6.89475729)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NativeSteamVacuumCar {
    pub pipe_psi: f32,
    pub cylinder_psi: f32,
    pub reservoir_psi: f32,
    pub pipe_volume_m3: f32,
    pub cylinder_volume_m3: f32,
    pub reservoir_volume_m3: f32,
    pub max_force_psi: f32,
    pub application_psi_s: f32,
    pub release_psi_s: f32,
    pub direct_admission: bool,
    pub isolated: bool,
    pub vented: bool,
    pub bleed: bool,
    pub cylinder_fraction: f32,
}
impl NativeSteamVacuumCar {
    pub fn shoe_fraction(self) -> f32 {
        let reservoir = if self.reservoir_psi >= self.cylinder_psi {
            self.reservoir_psi
        } else {
            let change = self.reservoir_volume_m3
                / (self.reservoir_volume_m3 - self.cylinder_fraction * self.cylinder_volume_m3);
            (self.reservoir_psi + change).min(self.cylinder_psi)
        };
        if self.cylinder_psi <= reservoir {
            0.
        } else {
            ((self.cylinder_psi - reservoir) / self.max_force_psi).min(1.)
        }
    }
}
impl NativeSteamState {
    pub fn initialize_vacuum_network(&mut self, cars: &mut [NativeSteamVacuumCar]) {
        // Train.UnconditionalInitializeBrakes passes maxPressurePSI into the
        // parameter named maxVacuumInHg. Preserve its double conversion.
        for c in cars.iter_mut() {
            c.pipe_psi = vacuum_to_pressure(16.);
            c.cylinder_psi = c.pipe_psi;
            c.reservoir_psi = vacuum_to_pressure(self.hardware.source.maximum_vacuum_psi);
            c.cylinder_fraction = 0.;
        }
        self.feedback.brake_pipe_absolute_psi = cars.first().map_or(VAC_ATM, |c| c.pipe_psi);
        self.feedback.brake_release = false;
        self.previous_train_brake_kind = NativeSteamBrakeKind::Release;
    }
    pub(super) fn update_ejector_switches(&mut self, i: NativeSteamInput) {
        // This native call passes MaxPressurePSI to a conversion named Vac.
        // Preserve that source behavior instead of correcting its units.
        self.LargeSteamEjectorIsOn = i.brake_release
            && i.brake_pipe_absolute_psi
                > vacuum_to_pressure(self.hardware.source.maximum_vacuum_psi);
        if self.hardware.source.small_ejector_controller_fitted {
            self.SmallSteamEjectorIsOn = self.SmallEjectorFeedFraction > 0.05;
        }
    }
    pub(super) fn update_auxiliaries(&mut self, i: NativeSteamInput, dt: f32) {
        let h = &self.hardware;
        let p = &h.source;
        let small_pressure = self.BoilerPressurePSI * i.small_ejector;
        self.SmallEjectorFeedFraction = if small_pressure < p.vacuum_maximum_boiler_psi {
            (1.6122 * (small_pressure / p.vacuum_maximum_boiler_psi) - 0.6122)
                * (h.EjectorSmallSteamConsumptionLbpS
                    / (h.EjectorLargeSteamConsumptionLbpS + h.EjectorSmallSteamConsumptionLbpS))
        } else {
            (1.1063 - (0.1063 * (small_pressure / p.vacuum_maximum_boiler_psi)))
                * h.EjectorSmallSteamConsumptionLbpS
                / (h.EjectorLargeSteamConsumptionLbpS + h.EjectorSmallSteamConsumptionLbpS)
        };
        self.SmallEjectorFeedFraction = clamp(self.SmallEjectorFeedFraction, 0., 1.);
        self.SmallEjectorChargingInHgpS = self.SmallEjectorFeedFraction * p.vacuum_charging_inhg_s;
        // SimpleControlPhysics forces this rate, even when the large ejector
        // is not consuming steam. Its on/off state controls thermal usage.
        self.LargeEjectorFeedFraction = 1. * h.EjectorLargeSteamConsumptionLbpS
            / (h.EjectorLargeSteamConsumptionLbpS + h.EjectorSmallSteamConsumptionLbpS);
        self.LargeEjectorFeedFraction = clamp(self.LargeEjectorFeedFraction, 0., 1.);
        self.LargeEjectorChargingInHgpS = self.LargeEjectorFeedFraction * p.vacuum_charging_inhg_s;
        let small = if self.SmallSteamEjectorIsOn {
            h.EjectorSmallSteamConsumptionLbpS
        } else {
            0.
        };
        let large = if self.LargeSteamEjectorIsOn {
            h.EjectorLargeSteamConsumptionLbpS
        } else {
            0.
        };
        self.EjectorTotalSteamConsumptionLbpS = small + large;
        let vacuum_pump = p.vacuum_pump_fitted;
        let stroke = p.cylinder_stroke_m;
        self.withdraw(self.EjectorTotalSteamConsumptionLbpS, dt);
        if vacuum_pump {
            let inch_volume = inches(stroke) * 2.5 * 2.5;
            let cubic_ft = (inch_volume * (1. / 61023.7441)) * 35.3146665722;
            let output =
                cubic_ft * std::f32::consts::PI * 1.9 * per_minute(self.engine.DriveWheelRevRpS);
            self.VacuumPumpChargingInHgpS = (output / 138.) * 0.344;
            if i.speed_mps < 0.1 {
                self.VacuumPumpOperating = false;
                self.VacuumPumpChargingInHgpS = 0.;
            }
        }
        if i.cocks_open {
            self.withdraw(self.CylCockSteamUsageLBpS, dt);
        } else {
            self.CylCockSteamUsageLBpS = 0.;
        }
        if i.sander {
            // Set by the base-update phase before UpdateBoiler.
            self.withdraw(self.SandingSteamUsageLBpS, dt);
        } else {
            self.SandingSteamUsageLBpS = 0.;
        }
        if i.generator_steam_effects {
            self.GeneratorSteamUsageLBpS = 0.0291666;
            self.withdraw(self.GeneratorSteamUsageLBpS, dt);
        } else {
            self.GeneratorSteamUsageLBpS = 0.;
        }
        if self.hardware.source.mechanical_stoker {
            self.StokerSteamUsageLBpS = per_second(self.hardware.MaxBoilerOutputLBpH)
                * (0.005
                    + (0.01 - 0.005) * self.FuelFeedRateKGpS / self.hardware.MaxFiringRateKGpS);
            self.withdraw(self.StokerSteamUsageLBpS, dt);
        } else {
            self.StokerSteamUsageLBpS = 0.;
        }
    }
    pub fn brake_kind(&self, handle: f32, emergency: bool) -> NativeSteamBrakeKind {
        if emergency {
            return NativeSteamBrakeKind::Emergency;
        }
        self.hardware
            .source
            .train_brake_notches
            .iter()
            .rev()
            .find(|n| n.value <= handle)
            .unwrap_or(&self.hardware.source.train_brake_notches[0])
            .kind
    }
    /// Train brake propagation, called once BEFORE the thermal tick. This is
    /// the bounded non-EQ lead-at-head source branch, with arbitrary intact
    /// trailing vacuum cars. Disconnected/closed sections retain their fluid.
    pub fn advance_vacuum_network(
        &mut self,
        cars: &mut [NativeSteamVacuumCar],
        handle: f32,
        emergency: bool,
        height: f32,
        dt: f32,
    ) -> Result<(), String> {
        if cars.is_empty()
            || !dt.is_finite()
            || dt <= 0.
            || !handle.is_finite()
            || !height.is_finite()
        {
            return Err("Invalid native steam vacuum network input".into());
        }
        let p = &self.hardware.source;
        if cars.iter().any(|c| {
            [
                c.pipe_psi,
                c.cylinder_psi,
                c.reservoir_psi,
                c.pipe_volume_m3,
                c.cylinder_volume_m3,
                c.reservoir_volume_m3,
                c.max_force_psi,
                c.application_psi_s,
                c.release_psi_s,
            ]
            .iter()
            .any(|x| !x.is_finite())
                || c.pipe_volume_m3 <= 0.
                || c.cylinder_volume_m3 <= 0.
                || c.reservoir_volume_m3 <= c.cylinder_volume_m3
        }) {
            return Err("Invalid native vacuum car volumes".into());
        }
        let current_kind = self.brake_kind(handle, emergency);
        // Train.Update propagates the train line before updating the car's
        // controller wrapper. Only the emergency interlock acts immediately.
        // UpdateSteamEjector later in this tick uses current_kind instead.
        let kind = if emergency {
            NativeSteamBrakeKind::Emergency
        } else {
            self.previous_train_brake_kind
        };
        let mut total_pipe = 0.;
        let mut total_cyl = 0.;
        for c in cars.iter() {
            total_pipe += c.pipe_volume_m3;
            total_cyl += c.cylinder_volume_m3;
        }
        let total = total_pipe + total_cyl;
        let sea_pressure = 101325.;
        let exponent = (-0.02896 * 9.807 * height) / (288.15 * 8.3143);
        let reduced = sea_pressure * ((exponent as f64).exp() as f32);
        let maximum = p.maximum_vacuum_psi * (reduced / sea_pressure);
        let minimum = VAC_ATM - maximum;
        for c in cars.iter_mut() {
            c.cylinder_fraction =
                clamp(1. - cylinder_vacuum_psi(c.cylinder_psi) / maximum, 0.01, 1.);
        }
        let reference_volume = 200. * (1. / 35.3146665722);
        let steps_fraction = reference_volume / total;
        let steps = ((dt * steps_fraction) / p.pipe_time_factor_s + 1.) as usize;
        if steps == 0 || steps > 1_000_000 {
            return Err("Native vacuum step count outside supported bounds".into());
        }
        let time = dt / steps as f32;
        let ratio = total / reference_volume;
        let large = (reference_volume / total) * self.LargeEjectorChargingInHgpS;
        let small = if p.small_ejector_controller_fitted {
            (reference_volume / total) * self.SmallEjectorChargingInHgpS
        } else {
            0.
        };
        let mut pump = (reference_volume / total) * self.VacuumPumpChargingInHgpS;
        let leak = (total / reference_volume) * p.train_pipe_leak_psi_s;
        let service = ratio * p.service_time_factor_s;
        let emergency_time = clamp(
            ratio * p.emergency_time_factor_s,
            1.,
            ratio * p.emergency_time_factor_s,
        );
        let propagation = ratio * p.pipe_time_factor_s;
        let discharge = ratio * p.pipe_discharge_multiplier;
        let pump_min = (3. * 3.386389) * (1. / 6.89475729);
        if p.vacuum_pump_fitted
            && (cars[0].pipe_psi + time * pump > VAC_ATM
                || pressure_to_vacuum(cars[0].pipe_psi)
                    < pressure_to_vacuum(VAC_ATM - (maximum - pump_min)))
        {
            pump = 0.;
            self.VacuumPumpOperating = false;
        } else if p.vacuum_pump_fitted {
            self.VacuumPumpOperating = true;
        } else {
            pump = 0.;
            self.VacuumPumpOperating = false;
        }
        let running = leak - (small + pump);
        let release = ((0. + large) + small + pump) - leak;
        for _ in 0..steps {
            if !cars[0].isolated {
                let head = &mut cars[0];
                if p.train_pipe_leak_psi_s != 0.
                    && head.pipe_psi + time * running < VAC_ATM
                    && kind == NativeSteamBrakeKind::Running
                {
                    head.pipe_psi += time * running;
                } else if kind == NativeSteamBrakeKind::Lap && head.pipe_psi + time * leak < VAC_ATM
                {
                    head.pipe_psi += time * leak;
                } else if kind == NativeSteamBrakeKind::Emergency {
                    head.pipe_psi *= 1. + time / emergency_time;
                    head.pipe_psi = head.pipe_psi.min(VAC_ATM);
                } else if kind == NativeSteamBrakeKind::Apply {
                    head.pipe_psi *= 1. + time / service;
                    head.pipe_psi = head.pipe_psi.min(VAC_ATM);
                } else if matches!(
                    kind,
                    NativeSteamBrakeKind::Release | NativeSteamBrakeKind::FullQuickRelease
                ) {
                    head.pipe_psi -= time * release;
                }
                head.pipe_psi = clamp(head.pipe_psi, minimum, VAC_ATM);
            }
            // The source visits head once with p0==p1, then every adjacency.
            let mut previous = 0;
            let mut p0 = clamp(cars[0].pipe_psi, minimum, VAC_ATM);
            let mut v0 = cars[0].pipe_volume_m3;
            for n in 0..cars.len() {
                let p1 = clamp(cars[n].pipe_psi, minimum, VAC_ATM);
                let v1 = cars[n].pipe_volume_m3;
                if n == 0 || (!cars[n].isolated && !cars[previous].isolated && !cars[n].vented) {
                    let mut diff = if propagation == 0. {
                        0.
                    } else {
                        time * (p1 - p0) / propagation
                    };
                    if diff < 0. {
                        diff *= discharge;
                    }
                    if n != 0 {
                        cars[n].pipe_psi =
                            clamp(cars[n].pipe_psi - diff * (v0 / (v0 + v1)), minimum, VAC_ATM);
                        if previous != 0 {
                            cars[previous].pipe_psi = clamp(
                                cars[previous].pipe_psi + diff * (v1 / (v0 + v1)),
                                minimum,
                                VAC_ATM,
                            );
                        }
                    }
                }
                if cars[n].vented && !cars[n].isolated {
                    cars[n].pipe_psi = (cars[n].pipe_psi + time * p1 / propagation).min(VAC_ATM);
                }
                cars[previous].pipe_psi = clamp(cars[previous].pipe_psi, minimum, VAC_ATM);
                cars[n].pipe_psi = clamp(cars[n].pipe_psi, minimum, VAC_ATM);
                cars[0].pipe_psi = clamp(cars[0].pipe_psi, minimum, VAC_ATM);
                previous = n;
                p0 = cars[n].pipe_psi;
                v0 = v1;
            }
        }
        // VacuumSinglePipe.Update (no independent/steam brake). Source's
        // LeadLoco flag is true on every car whenever a lead exists.
        for c in cars.iter_mut() {
            if c.isolated {
                continue;
            }
            if c.bleed {
                let dp = dt * c.application_psi_s;
                c.reservoir_psi = (c.reservoir_psi + dp).min(VAC_ATM);
                c.cylinder_psi = (c.cylinder_psi + dp).min(VAC_ATM);
            } else if c.pipe_psi < c.reservoir_psi {
                let mut dp =
                    dt * c.application_psi_s * c.cylinder_volume_m3 / c.reservoir_volume_m3;
                let vr = c.reservoir_volume_m3 / c.pipe_volume_m3;
                if c.reservoir_psi - dp < c.pipe_psi + dp * vr {
                    dp = (c.reservoir_psi - c.pipe_psi) / (1. + vr);
                }
                c.reservoir_psi -= dp;
                c.cylinder_psi = c.reservoir_psi;
            } else if c.pipe_psi < c.cylinder_psi {
                let mut dp = dt * c.release_psi_s;
                let vr = c.cylinder_volume_m3 / c.pipe_volume_m3;
                if c.cylinder_psi - dp < c.pipe_psi + dp * vr {
                    dp = (c.cylinder_psi - c.pipe_psi) / (1. + vr);
                }
                c.cylinder_psi -= dp;
            } else if c.pipe_psi > c.cylinder_psi {
                let mut dp = dt * c.application_psi_s;
                let vr = c.cylinder_volume_m3 / c.pipe_volume_m3;
                if c.cylinder_psi + dp > c.pipe_psi - dp * vr {
                    dp = (c.pipe_psi - c.cylinder_psi) / (1. + vr);
                }
                c.cylinder_psi += dp;
                if !c.direct_admission {
                    c.pipe_psi -= dp * vr;
                }
            }
        }
        self.feedback.brake_pipe_absolute_psi = cars[0].pipe_psi;
        self.feedback.brake_release = matches!(
            current_kind,
            NativeSteamBrakeKind::Release | NativeSteamBrakeKind::FullQuickRelease
        );
        self.feedback.height_m = height;
        self.previous_train_brake_kind = current_kind;
        Ok(())
    }
}
