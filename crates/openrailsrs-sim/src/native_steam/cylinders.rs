// OR1.6.1 MSTSSteamLocomotive.cs simple expansion branch5624–6008 and
// SimpleControlPhysics force branch, GPL-3.0-or-later.
use super::*;
impl NativeSteamState {
    pub(super) fn update_cylinders(&mut self, i: NativeSteamInput) {
        let h = &self.hardware;
        let p = &h.source;
        let e = &mut self.engine;
        e.DriveWheelRevRpS = i.speed_mps / (2. * std::f32::consts::PI * p.driver_radius_m);
        let exhaust = curve(&p.exhaust_event, CylinderEventExhausttoCutoff, i.cutoff);
        let compression = curve(
            &p.compression_event,
            CylinderEventCompressiontoCutoff,
            i.cutoff,
        );
        let admission = curve(&p.admission_event, CylinderEventAdmissiontoCutoff, i.cutoff);
        let clear = h.CylinderClearancePC;
        let volume_e = compression + clear;
        let volume_f = admission + clear;
        let expansion = (exhaust + clear) / (i.cutoff + clear);
        let rpm = per_minute(e.DriveWheelRevRpS * 1.);
        e.SteamChestPressurePSI = i.regulator * h.initial_drop(rpm) * self.BoilerPressurePSI;
        e.SteamChestPressurePSI = clamp(e.SteamChestPressurePSI, 0., e.SteamChestPressurePSI);
        e.Pressure_a_AtmPSI =
            ((i.regulator * self.BoilerPressurePSI) + ATM_PSI) * h.initial_drop(rpm);
        let upper = cutoff_initial_pressure(true, rpm, i.cutoff);
        let lower = cutoff_initial_pressure(false, rpm, i.cutoff);
        e.CutoffPressureDropRatio =
            (((h.CylinderPortOpeningFactor - 0.05) / (0.12 - 0.05)) * (upper - lower)) + lower;
        if h.HasSuperheater {
            e.CutoffPressureDropRatio =
                1. - ((1. / h.SuperheatCutoffPressureFactor) * ((rpm as f64).sqrt() as f32));
        }
        e.Pressure_b_AtmPSI = e.Pressure_a_AtmPSI * e.CutoffPressureDropRatio;
        e.Pressure_c_AtmPSI = e.Pressure_b_AtmPSI * (i.cutoff + clear) / (exhaust + clear);
        e.Pressure_d_AtmPSI = h.backpressure(e.IndicatedHorsePowerHP) + ATM_PSI;
        if i.regulator < 0.02 {
            e.Pressure_a_AtmPSI = 0.;
            e.Pressure_d_AtmPSI = 0.;
        }
        e.Pressure_e_AtmPSI = e.Pressure_d_AtmPSI;
        e.Pressure_f_AtmPSI = e.Pressure_e_AtmPSI * (compression + clear) / (admission + clear);
        let mean_ab = (e.Pressure_a_AtmPSI + e.Pressure_b_AtmPSI) / 2.;
        let length_ab = inches(p.cylinder_stroke_m * ((i.cutoff + clear) - clear));
        let work_ab = mean_ab * length_ab;
        let mean_bc = e.Pressure_b_AtmPSI * (((expansion as f64).ln() as f32) / (expansion - 1.));
        let length_bc = inches(p.cylinder_stroke_m) * ((exhaust + clear) - (i.cutoff + clear));
        let work_bc = mean_bc * length_bc;
        let mean_cd = (e.Pressure_c_AtmPSI + e.Pressure_d_AtmPSI) / 2.;
        let length_cd = inches(p.cylinder_stroke_m) * ((1. + clear) - (exhaust + clear));
        let work_cd = mean_cd * length_cd;
        let mean_de = (e.Pressure_d_AtmPSI + e.Pressure_e_AtmPSI) / 2.;
        let length_de = inches(p.cylinder_stroke_m) * ((1. + clear) - (compression + clear));
        let work_de = mean_de * length_de;
        let ratio = volume_e / volume_f;
        let mean_ef = e.Pressure_e_AtmPSI * ratio * (((ratio as f64).ln() as f32) / (ratio - 1.));
        let length_ef = inches(p.cylinder_stroke_m) * (volume_e - volume_f);
        let work_ef = mean_ef * length_ef;
        let mean_fa = (e.Pressure_a_AtmPSI + e.Pressure_f_AtmPSI) / 2.;
        let length_fa = admission * inches(p.cylinder_stroke_m);
        let work_fa = mean_fa * length_fa;
        e.CylinderWorksInLbf = [work_ab, work_bc, work_cd, work_de, work_ef, work_fa];
        let total = work_ab + work_bc + work_cd - work_de - work_ef - work_fa;
        e.MeanEffectivePressurePSI = clamp(
            total / inches(p.cylinder_stroke_m),
            0.,
            p.maximum_pressure_psi,
        );
        if i.regulator < 0.01 {
            e.MeanEffectivePressurePSI = 0.;
        }
        // Native cocks calculate pressure/steam from the previous cylinder
        // flow. Their factor affects reported cock pressure, not this MEP.
        if i.cocks_open {
            e.CylCockSteamUsageLBpS = if i.regulator > 0.01 && i.speed_mps > 0.1 {
                per_second(
                    p.cylinders as f32
                        * (24.24
                            * e.CylinderCocksPressureAtmPSI
                            * p.cylinder_cock_diameter_in
                            * p.cylinder_cock_diameter_in),
                )
            } else if i.regulator > 0.01 {
                per_second(
                    p.cylinders as f32
                        * (24.24
                            * e.Pressure_b_AtmPSI
                            * p.cylinder_cock_diameter_in
                            * p.cylinder_cock_diameter_in),
                )
            } else {
                0.
            };
            let reduction = if i.regulator > 0.01 && i.speed_mps > 0.1 {
                if h.HasSuperheater {
                    (e.CylinderSteamUsageLBpS / self.SuperheaterSteamUsageFactor)
                        / ((e.CylinderSteamUsageLBpS / self.SuperheaterSteamUsageFactor)
                            + e.CylCockSteamUsageLBpS)
                } else {
                    e.CylinderSteamUsageLBpS / (e.CylinderSteamUsageLBpS + e.CylCockSteamUsageLBpS)
                }
            } else {
                1.
            };
            e.CylinderCocksPressureAtmPSI =
                e.Pressure_b_AtmPSI - (e.Pressure_b_AtmPSI * (1. - reduction));
        } else {
            e.CylinderCocksPressureAtmPSI = e.Pressure_b_AtmPSI;
        }
        e.CylinderCocksPressureAtmPSI = clamp(
            e.CylinderCocksPressureAtmPSI,
            0.,
            p.maximum_pressure_psi + ATM_PSI,
        );
        let shaft_in2 = (std::f32::consts::PI
            * (p.piston_shaft_diameter_in / 2.)
            * (p.piston_shaft_diameter_in / 2.))
            / 2.;
        let shaft = sqft(shaft_in2 * (1. / 1550.0031));
        let area = sqft(std::f32::consts::PI * p.cylinder_diameter_m * p.cylinder_diameter_m / 4.);
        let swept = (area * feet(p.cylinder_stroke_m)) - shaft;
        let release_psi = clamp(e.Pressure_c_AtmPSI - ATM_PSI, 0., self.BoilerPressurePSI);
        let release_volume = swept * (exhaust + clear);
        let release_weight = release_volume * SteamDensityInterpolatorPSItoLBpFT3.at(release_psi);
        let admission_psi = clamp(e.Pressure_f_AtmPSI - ATM_PSI, 0., self.BoilerPressurePSI);
        let admission_volume = swept * (admission + clear);
        let admission_weight = if admission_psi > 0. {
            admission_volume * SteamDensityInterpolatorPSItoLBpFT3.at(admission_psi)
        } else {
            0.
        };
        let raw_weight = release_weight - admission_weight;
        let raw_usage = p.cylinders as f32 * e.DriveWheelRevRpS * 1. * 2. * raw_weight;
        let mut calculated = raw_usage * self.SuperheaterSteamUsageFactor;
        if (i.regulator < 0.01 && i.speed_mps > 0.1) || self.FusiblePlugIsBlown {
            calculated = 0.0001;
        }
        e.CalculatedCylinderSteamUsageLBpS = calculated;
        e.CylinderSteamUsageLBpS = 0.6 * e.CylinderSteamUsageLBpS + 0.4 * calculated;
    }
    pub(super) fn update_force(&mut self, i: NativeSteamInput) {
        let h = &self.hardware;
        let p = &h.source;
        let e = &mut self.engine;
        e.PistonSpeedFtpMin = feet(per_minute(p.cylinder_stroke_m * 2. * e.DriveWheelRevRpS)) * 1.;
        e.RealTractiveForceN = newtons(
            (p.cylinders as f32 / 2.)
                * (inches(p.cylinder_diameter_m)
                    * inches(p.cylinder_diameter_m)
                    * inches(p.cylinder_stroke_m)
                    / (2. * inches(p.driver_radius_m)))
                * (e.MeanEffectivePressurePSI * h.CylinderEfficiencyRate)
                * 1.,
        );
        e.IndicatedHorsePowerHP =
            (lbf(e.RealTractiveForceN) * per_hour(i.speed_mps * (1. / 1609.344))) / 375.;
        if i.speed_mps < 1. && i.cutoff > 0.70 && i.regulator > 0.98 {
            e.RealTractiveForceN =
                self.MaxForceN * (self.BoilerPressurePSI / p.maximum_pressure_psi);
        }
        if i.speed_mps == 0. && i.cutoff < 0.05 {
            e.RealTractiveForceN = 0.;
        }
        if self.BoilerIsPriming {
            e.RealTractiveForceN *= 0.1;
        }
        if self.FusiblePlugIsBlown {
            e.RealTractiveForceN = 0.5;
        }
        if e.IndicatedHorsePowerHP >= h.MaxIndicatedHorsePowerHP {
            e.RealTractiveForceN = newtons(
                (h.MaxIndicatedHorsePowerHP * 375.)
                    / per_hour(i.signed_speed_mps * (1. / 1609.344)),
            );
            e.IndicatedHorsePowerHP = h.MaxIndicatedHorsePowerHP;
        }
        // ApplyDirectionToTractiveForce and path orientation belong to the
        // train caller; the engine reports an unsigned rail effort here.
        self.MaxForceN = newtons(h.MaxTractiveEffortLbf * h.CylinderEfficiencyRate);
    }
}
