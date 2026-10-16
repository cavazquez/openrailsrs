// OR1.6.1 UpdateWaterInjection, injector branch. GPL-3.0-or-later.
use super::*;
impl NativeSteamState {
    pub(super) fn update_injectors(&mut self, i: NativeSteamInput, dt: f32) {
        self.BoilerWaterInputLB = 0.;
        let p = &self.hardware.source;
        let equivalent = (p.cylinders as f32 / 2.) * inches(p.cylinder_diameter_m);
        let possible = |t: Table| {
            2. * (per_hour(from_minute(t.at(p.maximum_pressure_psi))) * 10.021)
                > self.hardware.MaxBoilerOutputLBpH
        };
        let (size, t) = if equivalent <= 19. && possible(Injector09FlowrateInterpolatorPSItoUKGpM) {
            (9., Injector09FlowrateInterpolatorPSItoUKGpM)
        } else if equivalent <= 24. && possible(Injector10FlowrateInterpolatorPSItoUKGpM) {
            (10., Injector10FlowrateInterpolatorPSItoUKGpM)
        } else if equivalent <= 26. && possible(Injector11FlowrateInterpolatorPSItoUKGpM) {
            (11., Injector11FlowrateInterpolatorPSItoUKGpM)
        } else if equivalent <= 28. && possible(Injector13FlowrateInterpolatorPSItoUKGpM) {
            (13., Injector13FlowrateInterpolatorPSItoUKGpM)
        } else if equivalent <= 30. && possible(Injector14FlowrateInterpolatorPSItoUKGpM) {
            (14., Injector14FlowrateInterpolatorPSItoUKGpM)
        } else {
            (15., Injector15FlowrateInterpolatorPSItoUKGpM)
        };
        self.MaxInjectorFlowRateLBpS = from_minute(t.at(p.maximum_pressure_psi)) * 10.021;
        self.InjectorFlowRateLBpS = from_minute(t.at(self.BoilerPressurePSI)) * 10.021;
        self.InjectorSize = size;
        if self.WaterIsExhausted {
            self.InjectorFlowRateLBpS = 0.;
        }
        if !self.WaterIsExhausted {
            for (n, on) in i.injectors_on.into_iter().enumerate() {
                if !on {
                    continue;
                }
                let fraction = self.InjectorFraction[n];
                let min_capacity = InjCapMinFactorInterpolatorX.at(self.BoilerPressurePSI);
                if fraction < min_capacity {
                    self.InjectorWaterDeliveryF[n] =
                        InjDelWaterTempMinPressureInterpolatorFtoPSI.at(self.BoilerPressurePSI);
                } else {
                    let temp_fraction = (fraction - min_capacity)
                        / (1. - InjCapMinFactorInterpolatorX.at(p.maximum_pressure_psi));
                    self.InjectorWaterDeliveryF[n] = InjDelWaterTempMinPressureInterpolatorFtoPSI
                        .at(self.BoilerPressurePSI)
                        - ((InjDelWaterTempMinPressureInterpolatorFtoPSI
                            .at(self.BoilerPressurePSI)
                            - InjDelWaterTempMaxPressureInterpolatorFtoPSI
                                .at(self.BoilerPressurePSI))
                            * temp_fraction);
                    self.InjectorWaterDeliveryF[n] =
                        clamp(self.InjectorWaterDeliveryF[n], 65., 500.);
                }
                let water_pressure =
                    TemperatureToPressureInterpolatorFtoPSI.at(self.InjectorWaterDeliveryF[n]);
                let max_steam =
                    InjWaterFedSteamPressureInterpolatorFtoPSI.at(self.BoilerPressurePSI);
                let steam_used = (fraction * self.InjectorFlowRateLBpS) / max_steam;
                let delivery_h = WaterHeatInterpolatorPSItoBTUpLB.at(water_pressure);
                let steam_heat_loss = steam_used * (self.BoilerSteamHeatBTUpLB - delivery_h);
                let water_heat_loss = fraction
                    * self.InjectorFlowRateLBpS
                    * (self.BoilerWaterHeatBTUpLB - delivery_h);
                self.BoilerMassLB += dt * fraction * self.InjectorFlowRateLBpS;
                self.BoilerHeatBTU -= dt * (water_heat_loss + steam_heat_loss);
                self.BoilerWaterInputLB += dt * fraction * self.InjectorFlowRateLBpS;
                self.BoilerHeatOutBTUpS += water_heat_loss + steam_heat_loss;
            }
        }
        if i.injectors_on[0] || i.injectors_on[1] {
            if self.InjectorLockedOut {
                self.InjectorLockOutTimeS += dt;
            }
            if self.InjectorLockOutTimeS > 15. {
                self.InjectorLockedOut = false;
                self.InjectorLockOutTimeS = 0.;
            }
        }
    }
}
