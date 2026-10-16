// OR1.6.1 MSTSSteamLocomotive.cs UpdateBoiler / ApplyBoilerPressure.
// GPL-3.0-or-later. Native manual-fire branch, retaining its accounting order.
use super::*;
impl NativeSteamState {
    pub(super) fn update_boiler(&mut self, i: NativeSteamInput, dt: f32) {
        let maximum = self.hardware.source.maximum_pressure_psi;
        let total_diameter =
            0.036 * (sqft(self.hardware.EvaporationAreaM2) / (0.1 * (maximum + ATM_PSI))) + 1.;
        let mut tested = total_diameter / 2.;
        if tested > 4. {
            tested = total_diameter / 3.;
            if tested > 4. {
                tested = total_diameter / 4.;
            }
        }
        let diameter = if tested <= 2.5 {
            2.5
        } else if tested <= 3. {
            3.
        } else if tested <= 3.5 {
            3.5
        } else {
            4.
        };
        let area = if diameter == 2.5 {
            0.610369021
        } else if diameter == 3. {
            0.799264656
        } else if diameter == 3.5 {
            0.932672199
        } else {
            0.977534912
        };
        if self.BoilerPressurePSI > maximum + 0.1 {
            self.SafetyIsOn = true;
        } else if self.BoilerPressurePSI < maximum - 4. {
            self.SafetyIsOn = false;
            self.SafetyValveUsage[0] = 0.;
        }
        if self.SafetyIsOn {
            self.SafetyValveUsageLBpS = 0.;
            let rate = (area * (self.BoilerPressurePSI + ATM_PSI)) / 70.;
            self.SafetyValveUsage[0] = rate;
            // Preserve the source's second-valve close condition at max-4,
            // even though its otherwise-declared close2 value is max-3.
            if self.BoilerPressurePSI > maximum + 2. {
                self.safety2IsOn = true;
                self.SafetyValveUsage[1] = rate;
            } else if self.BoilerPressurePSI < maximum - 4. {
                self.safety2IsOn = false;
                self.SafetyValveUsage[1] = 0.;
            } else {
                self.SafetyValveUsage[1] = if self.safety2IsOn { rate } else { 0. };
            }
            if self.BoilerPressurePSI > maximum + 4. {
                self.safety3IsOn = true;
                self.SafetyValveUsage[2] = rate;
            } else if self.BoilerPressurePSI < maximum - 2. {
                self.safety3IsOn = false;
                self.SafetyValveUsage[2] = 0.;
            } else {
                self.SafetyValveUsage[2] = if self.safety3IsOn { rate } else { 0. };
            }
            if self.BoilerPressurePSI > maximum + 6. {
                self.safety4IsOn = true;
                self.SafetyValveUsage[3] = rate;
            } else if self.BoilerPressurePSI < maximum - 1. {
                self.safety4IsOn = false;
                self.SafetyValveUsage[3] = 0.;
            } else {
                self.SafetyValveUsage[3] = if self.safety4IsOn { rate } else { 0. };
            }
            self.SafetyValveUsageLBpS = self.SafetyValveUsage[0]
                + self.SafetyValveUsage[1]
                + self.SafetyValveUsage[2]
                + self.SafetyValveUsage[3];
            self.withdraw(self.SafetyValveUsageLBpS, dt);
        } else {
            self.SafetyValveUsageLBpS = 0.;
        }
        if i.blowdown_open {
            let valve_area = std::f32::consts::PI * (1.5 / 2.) * (1.5 / 2.);
            self.BlowdownSteamUsageLBpS = (valve_area * (maximum + ATM_PSI)) / 70.;
            self.withdraw(self.BlowdownSteamUsageLBpS, dt);
        } else {
            self.BlowdownSteamUsageLBpS = 0.;
        }
        if self.BlowerIsOn {
            self.withdraw(self.BlowerSteamUsageLBpS, dt);
        }
        self.BoilerWaterTempK =
            kelvin_from_f(PressureToTemperatureInterpolatorPSItoF.at(self.BoilerPressurePSI));
        if self.FlueTempK < self.BoilerWaterTempK {
            self.FlueTempK = self.BoilerWaterTempK + 10.;
        }
        let coefficient = 45. * (1. / 1000.);
        self.BoilerKW = (self.FlueTempK - self.BoilerWaterTempK)
            * coefficient
            * self.hardware.EvaporationAreaM2
            / 1.;
        // Native name says BTUpHtoKJpS but source uses this constant on BTU/s.
        let flue_diff = ((self.BoilerHeatInBTUpS - self.BoilerHeatOutBTUpS) * 0.000293071)
            / (coefficient * self.hardware.EvaporationAreaM2);
        self.FlueTempK += dt * flue_diff;
        self.FlueTempK = clamp(self.FlueTempK, 0., 3000.);
        self.EvaporationLBpS = if self.FusiblePlugIsBlown {
            0.0333
        } else {
            lb_from_kg(self.BoilerKW / kw_from_btu_s(self.BoilerSteamHeatBTUpLB))
        };
        let fire_kw = self.hardware.source.coal_heat_kj_kg
            * self.FuelBurnRateSmoothedKGpS
            * self.FireHeatLossPercent;
        let grate_rate =
            per_hour(lb_from_kg(self.FuelBurnRateSmoothedKGpS)) / sqft(self.hardware.GrateAreaM2);
        self.BoilerHeatInBTUpS =
            ((fire_kw * 1000.) * self.hardware.efficiency(grate_rate)) * (1. / 1055.05585);
        // Do not use EvaporationLBpS to create boiler mass; OR does not.
        self.BoilerHeatBTU +=
            dt * (((fire_kw * 1000.) * self.hardware.efficiency(grate_rate)) * (1. / 1055.05585));
        let temperature_diff = fahrenheit(celsius_from_k(self.BoilerWaterTempK) - i.outside_c);
        let min_k = 10.45 - 2. + 10. * ((2_f32 as f64).powf(0.5) as f32);
        let max_k = 10.45 - 20. + 10. * ((20_f32 as f64).powf(0.5) as f32);
        let actual_k = 10.45 - i.speed_mps + 10. * ((i.speed_mps as f64).powf(0.5) as f32);
        let movement = if i.speed_mps > 2. && i.speed_mps < 20. {
            actual_k / min_k
        } else if i.speed_mps < 2. {
            1.
        } else {
            max_k / min_k
        };
        let uninsulated = self.hardware.BoilerSurfaceAreaFt2
            * (1. - self.hardware.FractionBoilerAreaInsulated)
            * movement
            * 1.67
            * temperature_diff;
        let insulated = self.hardware.BoilerSurfaceAreaFt2
            * self.hardware.FractionBoilerAreaInsulated
            * movement
            * self.hardware.KcInsulation
            * temperature_diff;
        let radiation = uninsulated + insulated;
        self.RadiationSteamLossLBpS =
            per_second(radiation) / (self.BoilerSteamHeatBTUpLB - self.BoilerWaterHeatBTUpLB);
        self.BoilerHeatBTU -= dt * per_second(radiation);
        self.BoilerHeatOutBTUpS += per_second(radiation);
        self.WaterFraction = ((self.BoilerMassLB / self.hardware.BoilerVolumeFT3)
            - self.BoilerSteamDensityLBpFT3)
            / (self.BoilerWaterDensityLBpFT3 - self.BoilerSteamDensityLBpFT3);
        self.WaterFraction = clamp(self.WaterFraction, 0., 1.01);
        let smooth = self.BoilerHeatSmooth.update(240., dt, self.BoilerHeatBTU);
        self.BoilerHeatSmoothedBTU =
            clamp(smooth, 0., self.hardware.MaxBoilerSafetyPressHeatBTU * 1.05);
        let water_l = self.WaterFraction * self.hardware.BoilerVolumeFT3 * 28.31;
        if self.PreviousBoilerHeatSmoothedBTU != 0. {
            self.BkW_Diff =
                per_hour(self.BoilerHeatSmoothedBTU - self.PreviousBoilerHeatSmoothedBTU)
                    * 0.0002931;
        }
        let specific_heat = SpecificHeatInterpolatorKtoKJpKGpK.at(self.WaterTempNewK) * water_l;
        let temp_input = self.BkW_Diff / specific_heat;
        self.WaterTempNewK += temp_input;
        self.WaterTempNewK = clamp(self.WaterTempNewK, 274., 496.);
        self.PreviousBoilerHeatSmoothedBTU = self.BoilerHeatSmoothedBTU;
        self.BoilerPressurePSI = if self.FusiblePlugIsBlown {
            0.50
        } else {
            SaturationPressureInterpolatorKtoPSI.at(self.WaterTempNewK)
        };
        self.BoilerPressurePSI = clamp(self.BoilerPressurePSI, 0., maximum + 7.);
        self.BoilerSteamHeatBTUpLB = SteamHeatInterpolatorPSItoBTUpLB.at(self.BoilerPressurePSI);
        self.BoilerWaterHeatBTUpLB = WaterHeatInterpolatorPSItoBTUpLB.at(self.BoilerPressurePSI);
        self.BoilerSteamDensityLBpFT3 =
            SteamDensityInterpolatorPSItoLBpFT3.at(self.BoilerPressurePSI);
        self.BoilerWaterDensityLBpFT3 =
            WaterDensityInterpolatorPSItoLBpFT3.at(self.BoilerPressurePSI);
        self.PreviousBoilerHeatOutBTUpS = self.BoilerHeatOutBTUpS;
        self.PreviousTotalSteamUsageLBpS = self.TotalSteamUsageLBpS;
        self.BoilerHeatOutBTUpS = 0.;
        self.TotalSteamUsageLBpS = 0.;
        // UpdateBoiler calls ApplyBoilerPressure before advancing this counter.
        self.CumulativeTotalSteamConsumptionLbs += self.PreviousTotalSteamUsageLBpS * dt;
    }
}
