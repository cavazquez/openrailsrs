// OR1.6.1 MSTSSteamLocomotive.cs UpdateTender, UpdateFirebox(manual coal),
// UpdateSuperHeat, UpdateWaterGauge and UpdateFiring(manual), GPL-3.0-or-later.
use super::*;
impl NativeSteamState {
    pub(super) fn update_tender(&mut self, i: NativeSteamInput, dt: f32) {
        let h = &self.hardware;
        let p = &h.source;
        let usage = clamp(
            self.CylinderSteamUsageLBpS,
            0.1,
            h.TheoreticalMaxSteamOutputLBpS,
        );
        let coal = if !i.has_tender_coupled && p.tender_required {
            0.
        } else {
            clamp(
                self.tender_coal_kg() - dt * per_second(kg_from_lb(h.burn(per_hour(usage)))),
                0.,
                p.tender_coal_capacity_kg,
            )
        };
        // Native TenderFuelMassKG is a property backed by a controller ratio.
        self.FuelControllerValue = coal / p.tender_coal_capacity_kg;
        self.CoalIsExhausted = self.tender_coal_kg() < 1.;
        let water = if !i.has_tender_coupled && p.tender_required {
            0.
        } else {
            self.tender_water_ukg() - self.BoilerWaterInputLB / 10.021
        };
        self.WaterControllerValue = water / self.maximum_tender_water_ukg();
        let percent = self.tender_water_ukg() / self.maximum_tender_water_ukg();
        self.CurrentLocoTenderWaterVolumeUKG =
            (lb_from_kg(p.tender_water_capacity_kg) / 10.021) * percent;
        self.WaterConsumptionLbpS = self.BoilerWaterInputLB / dt;
        self.WaterConsumptionLbpS = clamp(self.WaterConsumptionLbpS, 0., self.WaterConsumptionLbpS);
        self.CumulativeWaterConsumptionLbs += self.BoilerWaterInputLB;
        self.WaterIsExhausted = self.tender_water_ukg() < 1.;
    }
    pub(super) fn update_firebox(&mut self, i: NativeSteamInput, dt: f32) {
        let h = &self.hardware;
        self.BurnRateRawKGpS = per_second(kg_from_lb(h.burn(per_hour(
            (self.RadiationSteamLossLBpS + 0.) + self.BlowerBurnEffect + self.DamperBurnEffect,
        ))));
        self.BurnRateRawKGpS = clamp(self.BurnRateRawKGpS, 0.0012626, self.BurnRateRawKGpS);
        self.FuelFeedRateKGpS = self.BurnRateRawKGpS;
        if self.FireMassKG / h.IdealFireMassKG < 0.05 {
            self.BurnRateRawKGpS = 0.;
            self.FireIsExhausted = true;
        }
        self.FireRatio = self.FireMassKG / h.IdealFireMassKG;
        let mut heat_ratio = 1.;
        if self.FireRatio < 1. {
            heat_ratio -= self.FireRatio;
            self.TempFireHeatLossPercent = clamp(
                0.0058 * heat_ratio * heat_ratio + 0.035 * heat_ratio,
                0.,
                1.,
            );
        } else if self.FireRatio > 1. {
            heat_ratio -= self.FireRatio - 1.;
            self.TempFireHeatLossPercent = clamp(
                0.0434 * heat_ratio * heat_ratio - 0.1276 * heat_ratio,
                0.,
                1.,
            );
        } else {
            self.FireHeatLossPercent = 1.;
        }
        self.FireHeatLossPercent = 1. - self.TempFireHeatLossPercent;
        self.FireHeatLossPercent = clamp(self.FireHeatLossPercent, 0., self.FireHeatLossPercent);
        if self.FusiblePlugIsBlown {
            self.BurnRateRawKGpS = 0.;
        }
        self.FuelBurnRateSmoothedKGpS =
            self.BurnRateCoalSmooth
                .update(150., dt, self.BurnRateRawKGpS);
        self.FuelBurnRateSmoothedKGpS =
            clamp(self.FuelBurnRateSmoothedKGpS, 0., h.MaxFuelBurnGrateKGpS);
        let feed_fraction = if self.CoalIsExhausted { 0. } else { i.firing };
        self.FuelFeedRateKGpS = h.MaxFiringRateKGpS * feed_fraction;
        self.FireMassKG += dt * (self.FuelFeedRateKGpS - self.FuelBurnRateSmoothedKGpS);
        self.FireMassKG = clamp(self.FireMassKG, 0., h.MaxFireMassKG);
    }
    pub(super) fn update_superheat(&mut self, i: NativeSteamInput) {
        let h = &self.hardware;
        if h.HasSuperheater {
            self.CurrentSuperheatTempF = SuperheatTempInterpolatorLbpHtoDegF
                .at(per_hour(self.CylinderSteamUsageLBpS))
                * h.SuperheatTempRatio;
            self.CurrentSuperheatTempF =
                clamp(self.CurrentSuperheatTempF, 0., h.MaxSuperheatRefTempF);
            // DrvWheelRevRpS still belongs to the preceding cylinder update.
            let speed_factor = clamp(
                1. - 0.00214 * per_minute(self.engine.DriveWheelRevRpS),
                0.25,
                1.,
            );
            let limit = SuperheatTempLimitInterpolatorXtoDegF.at(i.cutoff);
            let difference = self.CurrentSuperheatTempF - (limit * speed_factor);
            self.SuperheatVolumeRatio = 1. + 0.0015 * difference;
            let future_usage = self.CylinderSteamUsageLBpS * 1. / self.SuperheatVolumeRatio;
            let future_temp = SuperheatTempInterpolatorLbpHtoDegF.at(per_hour(future_usage))
                * h.SuperheatTempRatio;
            let threshold = limit - 25.;
            if self.CurrentSuperheatTempF > limit * speed_factor {
                self.IsSuperSet = true;
            } else if future_temp < threshold * speed_factor {
                self.IsSuperSet = false;
            }
            if self.IsSuperSet {
                self.SuperheaterSteamUsageFactor = 1. / self.SuperheatVolumeRatio;
            } else {
                let condensation = 1. + CylinderCondensationFractionInterpolatorX.at(i.cutoff);
                let temp = condensation - 1.;
                let mut multiplier = (1. - self.CurrentSuperheatTempF / limit) * temp;
                multiplier = clamp(multiplier, 0., multiplier);
                self.SuperheaterSteamUsageFactor = clamp(1. + multiplier, 0., 1.);
            }
        } else {
            self.SuperheaterSteamUsageFactor =
                1. + CylinderCondensationFractionInterpolatorX.at(i.cutoff);
        }
        self.SuperheaterSteamUsageFactor = clamp(
            self.SuperheaterSteamUsageFactor,
            0.60,
            self.SuperheaterSteamUsageFactor,
        );
    }
    pub(super) fn update_water_gauge(&mut self) {
        self.WaterGlassLevelIN = clamp(((self.WaterFraction - 0.73) / (0.89 - 0.73)) * 8., 0., 8.);
        if self.WaterFraction < 0.7 {
            self.FusiblePlugIsBlown = true;
        }
        if self.WaterFraction >= 0.91 {
            self.BoilerIsPriming = true;
        } else if self.WaterFraction < 0.90 {
            self.BoilerIsPriming = false;
        }
    }
    pub(super) fn update_firing(&mut self, i: NativeSteamInput) {
        // Last in Update. These effects deliberately influence the next tick.
        if i.blower > 0. {
            self.BlowerIsOn = true;
            self.BlowerSteamUsageLBpS =
                self.hardware.BlowerSteamUsageFactor * i.blower * self.BoilerPressurePSI;
            self.BlowerBurnEffect =
                20. * self.hardware.BlowerSteamUsageFactor * i.blower * self.BoilerPressurePSI;
        } else {
            self.BlowerIsOn = false;
            self.BlowerSteamUsageLBpS = 0.;
            self.BlowerBurnEffect = 0.;
        }
        if i.injectors_on[0] {
            self.InjectorFraction[0] = i.injectors[0];
        }
        if i.injectors_on[1] {
            self.InjectorFraction[1] = i.injectors[1];
        }
        self.DamperBurnEffect = if i.speed_mps < 1. {
            0.
        } else {
            i.damper * self.CylinderSteamUsageLBpS * 1.2
        };
        self.DamperBurnEffect = clamp(
            self.DamperBurnEffect,
            0.,
            self.hardware.TheoreticalMaxSteamOutputLBpS * 1.5,
        );
    }
}
