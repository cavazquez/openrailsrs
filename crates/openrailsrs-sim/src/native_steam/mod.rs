//! Bounded translation of MSTSSteamLocomotive OR1.6.1, commit
//! d16e670da333d26d2edfc97d5631a19dadf49ce5. Copyright Open Rails project,
//! GPL-3.0-or-later. Coal, manual fireman, one simple direct-drive engine,
//! injector feed and vacuum brakes, SimpleControlPhysics=true.
//!
//! This cycle intentionally retains native imperial units and Single order.
//! BoilerHeatBTU is OR's thermal inventory, not the own vessel's internal
//! energy. No trajectory value after initialization is an input to this model.
#![allow(non_snake_case)]
#![allow(clippy::excessive_precision)] // Literal constants belong to the pinned Single source port.
mod boiler;
mod bridge;
mod cylinders;
mod firebox;
mod injectors;
pub mod tables;
pub mod vacuum;
pub use bridge::{
    NativeSteamSupply, advance_boiler, boiler_valid, initialize_boiler, replenish_boiler,
};
use openrailsrs_formats::{NativeSteamBoilerType, NativeSteamBrakeKind, NativeSteamParams};
use serde::{Deserialize, Serialize};
use tables::*;
pub use vacuum::NativeSteamVacuumCar;

const ATM_PSI: f32 = 14.696;
pub(super) fn kg_from_lb(v: f32) -> f32 {
    v * (1. / 2.20462)
}
pub(super) fn lb_from_kg(v: f32) -> f32 {
    v * 2.20462
}
pub(super) fn feet(v: f32) -> f32 {
    v * (1. / 0.3048)
}
pub(super) fn inches(v: f32) -> f32 {
    v * (1. / 0.0254)
}
pub(super) fn sqft(v: f32) -> f32 {
    v * (1. / 0.092903)
}
pub(super) fn sqm(v: f32) -> f32 {
    v * 0.092903
}
pub(super) fn per_hour(v: f32) -> f32 {
    v * 3600.
}
pub(super) fn per_second(v: f32) -> f32 {
    v * (1. / 3600.)
}
pub(super) fn per_minute(v: f32) -> f32 {
    v * 60.
}
pub(super) fn from_minute(v: f32) -> f32 {
    v * (1. / 60.)
}
pub(super) fn kw_from_btu_s(v: f32) -> f32 {
    (v * 1055.05585) * (1. / 1000.)
}
pub(super) fn kelvin_from_f(v: f32) -> f32 {
    (v - 32.) * (100. / 180.) + 273.15
}
pub(super) fn celsius_from_k(v: f32) -> f32 {
    v - 273.15
}
pub(super) fn fahrenheit(v: f32) -> f32 {
    v * (180. / 100.) + 32.
}
pub(super) fn newtons(v: f32) -> f32 {
    v * (1. / 0.224808943871)
}
pub(super) fn lbf(v: f32) -> f32 {
    v * 0.224808943871
}
/// XNA MathHelper.Clamp compares with max and then with min. Some native
/// callsites use the current value as max, including when it is below min.
pub(super) fn clamp(v: f32, min: f32, max: f32) -> f32 {
    let v = if v > max { max } else { v };
    if v < min { min } else { v }
}
pub(super) fn curve(authored: &[(f32, f32)], fallback: Table, x: f32) -> f32 {
    if authored.is_empty() {
        fallback.at(x)
    } else {
        let mut i = 0;
        if x > authored[authored.len() - 2].0 {
            i = authored.len() - 2;
        } else if x >= authored[1].0 {
            let mut j = authored.len() - 1;
            while j - i > 1 {
                let k = (i + j) / 2;
                if authored[k].0 > x {
                    j = k;
                } else {
                    i = k;
                }
            }
        }
        let d = authored[i + 1].0 - authored[i].0;
        let a = (authored[i + 1].0 - x) / d;
        let b = (x - authored[i].0) / d;
        a * authored[i].1 + b * authored[i + 1].1
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NativeSmooth {
    pub value: Option<f32>,
}
impl NativeSmooth {
    fn update(&mut self, period: f32, dt: f32, new: f32) -> f32 {
        // 1/(60*period) and 1-... are Single in SmoothedData's constructor;
        // only Math.Log/Exp are Double. Do not use an f64 time constant.
        if dt < f32::from_bits(1) {
            return *self.value.get_or_insert(new);
        }
        let log_argument = 1. - 1. / (60. * period);
        let rate = (-60. * (log_argument as f64).ln()) as f32;
        let ratio = (-(rate * dt) as f64).exp() as f32;
        let v = match self.value {
            None => new,
            Some(v) if !v.is_finite() || ratio < 0.5 => new,
            Some(v) => v * ratio + new * (1. - ratio),
        };
        self.value = Some(v);
        v
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NativeSteamHardware {
    pub source: NativeSteamParams,
    pub BoilerVolumeFT3: f32,
    pub EvaporationAreaM2: f32,
    pub GrateAreaM2: f32,
    pub IdealFireMassKG: f32,
    pub MaxFireMassKG: f32,
    pub MaxFuelBurnGrateKGpS: f32,
    pub TheoreticalMaxSteamOutputLBpS: f32,
    pub MaxBoilerOutputLBpH: f32,
    pub MaxFiringRateLbpH: f32,
    pub MaxFiringRateKGpS: f32,
    pub GrateLimitLBpFt2: f32,
    pub MaxBoilerHeatBTU: f32,
    pub MaxBoilerSafetyPressHeatBTU: f32,
    pub HasSuperheater: bool,
    pub EfficiencyTableSuperheated: bool,
    pub InitialDropTableSuperheated: bool,
    pub MaxSuperheatRefTempF: f32,
    pub SuperheatTempRatio: f32,
    pub CylinderClearancePC: f32,
    pub SuperheatCutoffPressureFactor: f32,
    pub CylinderPortOpeningFactor: f32,
    pub CylinderEfficiencyRate: f32,
    pub MaxTractiveEffortLbf: f32,
    pub MaxIndicatedHorsePowerHP: f32,
    pub BoilerSurfaceAreaFt2: f32,
    pub FractionBoilerAreaInsulated: f32,
    pub KcInsulation: f32,
    pub BlowerSteamUsageFactor: f32,
    pub EjectorSmallSteamConsumptionLbpS: f32,
    pub EjectorLargeSteamConsumptionLbpS: f32,
    pub BurnCurve: Vec<(f32, f32)>,
}
impl NativeSteamHardware {
    pub fn new(p: &NativeSteamParams) -> Result<Self, String> {
        if !p.valid() {
            return Err("Invalid bounded native steam hardware".into());
        }
        let dimensions = inches(p.cylinder_diameter_m)
            * inches(p.cylinder_diameter_m)
            * inches(p.cylinder_stroke_m);
        let area_base = (p.cylinders as f32 / 2.) * dimensions * 0.7 * p.maximum_pressure_psi;
        let grate = if p.grate_area_m2 == 0. {
            let derived = sqm(area_base / (inches(p.driver_radius_m * 2.) * 500.));
            clamp(derived, sqm(6.), derived)
        } else {
            p.grate_area_m2
        };
        let ideal_fire = grate * (7. * 0.0254) * 864.5;
        // Native calculates the burn cap using the field's initial150lb/ft²,
        // then later replaces GrateLimit from the chosen efficiency curve.
        let max_fuel_burn = per_second(kg_from_lb(sqft(grate) * (150. * 1.10)));
        let evap = if p.evaporation_area_m2 == 0. {
            let derived = sqm(area_base / (inches(p.driver_radius_m * 2.) * 10.));
            clamp(derived, sqm(100.), derived)
        } else {
            p.evaporation_area_m2
        };
        let evap_rate = clamp(
            if p.boiler_evaporation_lb_ft2_h == 0. {
                15.
            } else {
                p.boiler_evaporation_lb_ft2_h
            },
            7.5,
            30.,
        );
        let max_steam = per_second(sqft(evap) * evap_rate);
        let mut volume = p.boiler_volume_ft3;
        if sqft(evap) / volume > 15. {
            volume = sqft(evap) / 8.3;
        }
        // This choice occurs BEFORE legacy SuperHeater inference in OR.
        let early_super = p.boiler_type == NativeSteamBoilerType::Superheated;
        let efficiency = if early_super {
            SuperBoilerEfficiencyGrateAreaInterpolatorLbstoX
        } else {
            SatBoilerEfficiencyGrateAreaInterpolatorLbstoX
        };
        let efficiency_at = |x| curve(&p.boiler_efficiency, efficiency, x);
        let mut max_firing_lb_h = (per_hour(max_steam) / (efficiency_at(0.) / 2.))
            * (SteamHeatInterpolatorPSItoBTUpLB.at(p.maximum_pressure_psi)
                / (p.coal_heat_kj_kg * (1. / 2.326)));
        let gradient = (efficiency_at(100.) - efficiency_at(0.)) / (100. - 0.);
        let grate_limit = ((efficiency_at(0.) / 2.) - efficiency_at(0.)) / gradient;
        let cylinder_eff = if p.cylinder_efficiency == 0. {
            1.
        } else {
            p.cylinder_efficiency
        };
        let max_te = (p.cylinders as f32 / 2.)
            * (dimensions / (2. * inches(p.driver_radius_m)))
            * p.maximum_pressure_psi
            * p.tractive_effort_factor
            * 1.
            * cylinder_eff;
        let has_super = early_super
            || (p.boiler_type == NativeSteamBoilerType::LegacyInferred
                && p.superheater_area_m2 == 0.
                && p.legacy_superheater > 1.);
        let mut max_super = p.maximum_superheat_f;
        let clearance = if early_super || p.boiler_type == NativeSteamBoilerType::Saturated {
            0.09
        } else if has_super {
            max_super = 250.;
            0.09
        } else {
            max_super = 0.;
            0.08
        };
        if early_super && max_super == 0. {
            max_super = (sqft(p.superheater_area_m2) * 15000.) / per_hour(max_steam);
        }
        let super_ratio = if has_super {
            max_super / SuperheatTempInterpolatorLbpHtoDegF.at(per_hour(max_steam))
        } else {
            0.
        };
        let max_piston = if has_super { 1000. } else { 700. };
        let speed_factor = if has_super {
            SuperheatedSpeedFactorSpeedDropFtpMintoX.at(max_piston)
        } else {
            SaturatedSpeedFactorSpeedDropFtpMintoX.at(max_piston)
        };
        let speed_mph = ((from_minute(max_piston) * 0.3048) * 2.23693629)
            * 2.
            * std::f32::consts::PI
            * p.driver_radius_m
            / (2. * p.cylinder_stroke_m);
        let max_ihp = if p.maximum_ihp == 0. {
            speed_factor * (max_te * speed_mph) / 375.
        } else {
            p.maximum_ihp
        };
        let burn = if p.burn_rate_lb_h.is_empty() {
            [0., 1., 1.1, 1.2, 1.3, 1.4]
                .into_iter()
                .zip([0., 1., 1.5, 2., 3., 4.])
                .map(|(x, y)| (per_hour(max_steam * x), max_firing_lb_h * y))
                .collect()
        } else {
            max_firing_lb_h = curve(
                &p.burn_rate_lb_h,
                Table {
                    x: &[0., 1.],
                    y: &[0., 1.],
                },
                per_hour(max_steam),
            );
            p.burn_rate_lb_h.clone()
        };
        let max_boiler_output = per_hour(max_steam);
        let mut firing = p.maximum_firing_rate_kg_s;
        if p.orts_maximum_firing_rate_kg_s != 0. {
            firing = p.orts_maximum_firing_rate_kg_s;
        }
        if firing == 0. {
            firing = if p.mechanical_stoker {
                per_second(kg_from_lb(14400.))
            } else {
                180. * max_boiler_output / 775. / 3600. / 2.2046
            };
        }
        let max_heat = |psi| {
            0.9 * volume
                * WaterDensityInterpolatorPSItoLBpFT3.at(psi)
                * WaterHeatInterpolatorPSItoBTUpLB.at(psi)
                + (1. - 0.9)
                    * volume
                    * SteamDensityInterpolatorPSItoLBpFT3.at(psi)
                    * SteamHeatInterpolatorPSItoBTUpLB.at(psi)
        };
        Ok(Self {
            source: p.clone(),
            BoilerVolumeFT3: volume,
            EvaporationAreaM2: evap,
            GrateAreaM2: grate,
            IdealFireMassKG: ideal_fire,
            MaxFireMassKG: if p.maximum_fire_mass_kg == 0. {
                2. * ideal_fire
            } else {
                p.maximum_fire_mass_kg
            },
            MaxFuelBurnGrateKGpS: max_fuel_burn,
            TheoreticalMaxSteamOutputLBpS: max_steam,
            MaxBoilerOutputLBpH: max_boiler_output,
            MaxFiringRateLbpH: max_firing_lb_h,
            MaxFiringRateKGpS: firing,
            GrateLimitLBpFt2: grate_limit,
            MaxBoilerHeatBTU: max_heat(p.maximum_pressure_psi),
            MaxBoilerSafetyPressHeatBTU: max_heat(p.maximum_pressure_psi + 0.1 + 6.),
            HasSuperheater: has_super,
            EfficiencyTableSuperheated: early_super,
            InitialDropTableSuperheated: early_super,
            MaxSuperheatRefTempF: max_super,
            SuperheatTempRatio: super_ratio,
            CylinderClearancePC: clearance,
            SuperheatCutoffPressureFactor: if p.superheat_cutoff_pressure_factor == 0. {
                40.
            } else {
                p.superheat_cutoff_pressure_factor
            },
            CylinderPortOpeningFactor: clamp(
                if p.cylinder_port_opening == 0. {
                    0.085
                } else {
                    p.cylinder_port_opening
                },
                0.05,
                0.12,
            ),
            CylinderEfficiencyRate: cylinder_eff,
            MaxTractiveEffortLbf: max_te,
            MaxIndicatedHorsePowerHP: max_ihp,
            BoilerSurfaceAreaFt2: if p.boiler_surface_ft2 == 0. {
                sqft(16. * grate)
            } else {
                p.boiler_surface_ft2
            },
            FractionBoilerAreaInsulated: if p.insulated_fraction == 0. {
                0.86
            } else {
                p.insulated_fraction
            },
            KcInsulation: if p.insulation_kc == 0. {
                0.4
            } else {
                p.insulation_kc
            },
            BlowerSteamUsageFactor: 0.04 * max_boiler_output / 3600. / p.maximum_pressure_psi,
            EjectorSmallSteamConsumptionLbpS: if p.small_ejector_lb_s == 0. {
                per_second(300.)
            } else {
                p.small_ejector_lb_s
            },
            EjectorLargeSteamConsumptionLbpS: if p.large_ejector_lb_s == 0. {
                per_second(650.)
            } else {
                p.large_ejector_lb_s
            },
            BurnCurve: burn,
        })
    }
    fn efficiency(&self, x: f32) -> f32 {
        curve(
            &self.source.boiler_efficiency,
            if self.EfficiencyTableSuperheated {
                SuperBoilerEfficiencyGrateAreaInterpolatorLbstoX
            } else {
                SatBoilerEfficiencyGrateAreaInterpolatorLbstoX
            },
            x,
        )
    }
    fn burn(&self, x: f32) -> f32 {
        curve(
            &self.BurnCurve,
            Table {
                x: &[0., 1.],
                y: &[0., 1.],
            },
            x,
        )
    }
    fn initial_drop(&self, rpm: f32) -> f32 {
        curve(
            &self.source.initial_pressure_drop,
            if self.InitialDropTableSuperheated {
                SuperInitialPressureDropRatioInterpolatorRpMtoX
            } else {
                SatInitialPressureDropRatioInterpolatorRpMtoX
            },
            rpm,
        )
    }
    fn backpressure(&self, ihp: f32) -> f32 {
        curve(
            &self.source.backpressure_psi,
            if self.HasSuperheater {
                BackpressureSuperIHPtoPSI
            } else {
                BackpressureSatIHPtoPSI
            },
            ihp,
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NativeSteamEngineState {
    pub DriveWheelRevRpS: f32,
    pub MeanEffectivePressurePSI: f32,
    pub CylinderSteamUsageLBpS: f32,
    pub CalculatedCylinderSteamUsageLBpS: f32,
    pub CylCockSteamUsageLBpS: f32,
    pub CylinderCocksPressureAtmPSI: f32,
    pub Pressure_a_AtmPSI: f32,
    pub Pressure_b_AtmPSI: f32,
    pub Pressure_c_AtmPSI: f32,
    pub Pressure_d_AtmPSI: f32,
    pub Pressure_e_AtmPSI: f32,
    pub Pressure_f_AtmPSI: f32,
    pub IndicatedHorsePowerHP: f32,
    pub RealTractiveForceN: f32,
    pub PistonSpeedFtpMin: f32,
    pub SteamChestPressurePSI: f32,
    pub CutoffPressureDropRatio: f32,
    pub CylinderWorksInLbf: [f32; 6],
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NativeSteamState {
    pub schema: u32,
    pub hardware: NativeSteamHardware,
    pub HotStart: bool,
    pub feedback: NativeSteamFeedback,
    /// Train.PropagateBrakePressure reads the controller state published by
    /// the preceding car.Update. The initial native wrapper state is Release,
    /// even when the authored controller handle starts in Apply.
    pub previous_train_brake_kind: NativeSteamBrakeKind,
    pub BoilerPressurePSI: f32,
    pub BoilerMassLB: f32,
    pub BoilerHeatBTU: f32,
    pub WaterFraction: f32,
    pub BoilerHeatSmooth: NativeSmooth,
    pub BurnRateCoalSmooth: NativeSmooth,
    pub BoilerHeatSmoothedBTU: f32,
    pub PreviousBoilerHeatSmoothedBTU: f32,
    pub BoilerSteamHeatBTUpLB: f32,
    pub BoilerWaterHeatBTUpLB: f32,
    pub BoilerSteamDensityLBpFT3: f32,
    pub BoilerWaterDensityLBpFT3: f32,
    pub BoilerWaterTempK: f32,
    pub WaterTempNewK: f32,
    pub BkW_Diff: f32,
    pub FlueTempK: f32,
    pub BoilerKW: f32,
    pub EvaporationLBpS: f32,
    pub BoilerHeatInBTUpS: f32,
    pub BoilerHeatOutBTUpS: f32,
    pub PreviousBoilerHeatOutBTUpS: f32,
    pub TotalSteamUsageLBpS: f32,
    pub PreviousTotalSteamUsageLBpS: f32,
    pub CylinderSteamUsageLBpS: f32,
    pub CylCockSteamUsageLBpS: f32,
    pub BlowerSteamUsageLBpS: f32,
    pub BlowerBurnEffect: f32,
    pub DamperBurnEffect: f32,
    pub RadiationSteamLossLBpS: f32,
    pub FireMassKG: f32,
    pub BurnRateRawKGpS: f32,
    pub FuelBurnRateSmoothedKGpS: f32,
    pub FuelFeedRateKGpS: f32,
    pub FireRatio: f32,
    pub FireHeatLossPercent: f32,
    pub TempFireHeatLossPercent: f32,
    pub FuelControllerValue: f32,
    pub WaterControllerValue: f32,
    pub CurrentLocoTenderWaterVolumeUKG: f32,
    pub WaterConsumptionLbpS: f32,
    pub BoilerWaterInputLB: f32,
    pub CumulativeWaterConsumptionLbs: f32,
    pub CumulativeCylinderSteamConsumptionLbs: f32,
    pub CumulativeTotalSteamConsumptionLbs: f32,
    pub CoalIsExhausted: bool,
    pub WaterIsExhausted: bool,
    pub FireIsExhausted: bool,
    pub FusiblePlugIsBlown: bool,
    pub BoilerIsPriming: bool,
    pub BlowerIsOn: bool,
    pub SafetyIsOn: bool,
    pub safety2IsOn: bool,
    pub safety3IsOn: bool,
    pub safety4IsOn: bool,
    pub SafetyValveUsageLBpS: f32,
    pub SafetyValveUsage: [f32; 4],
    pub BlowdownSteamUsageLBpS: f32,
    pub CurrentSuperheatTempF: f32,
    pub SuperheatVolumeRatio: f32,
    pub SuperheaterSteamUsageFactor: f32,
    pub IsSuperSet: bool,
    pub InjectorFraction: [f32; 2],
    pub InjectorWaterDeliveryF: [f32; 2],
    pub InjectorFlowRateLBpS: f32,
    pub MaxInjectorFlowRateLBpS: f32,
    pub InjectorSize: f32,
    pub InjectorLockedOut: bool,
    pub InjectorLockOutTimeS: f32,
    pub EjectorTotalSteamConsumptionLbpS: f32,
    pub SmallEjectorFeedFraction: f32,
    pub LargeEjectorFeedFraction: f32,
    pub SmallSteamEjectorIsOn: bool,
    pub LargeSteamEjectorIsOn: bool,
    pub SmallEjectorChargingInHgpS: f32,
    pub LargeEjectorChargingInHgpS: f32,
    pub VacuumPumpChargingInHgpS: f32,
    pub VacuumPumpOperating: bool,
    pub GeneratorSteamUsageLBpS: f32,
    pub SandingSteamUsageLBpS: f32,
    pub StokerSteamUsageLBpS: f32,
    pub WaterGlassLevelIN: f32,
    pub MaxForceN: f32,
    pub engine: NativeSteamEngineState,
}

/// Actual controller values, never measured force/pressure/temperature. The
/// brake query is the pipe after train propagation and before car.Update.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeSteamInput {
    pub regulator: f32,
    pub cutoff: f32,
    pub speed_mps: f32,
    pub signed_speed_mps: f32,
    pub firing: f32,
    pub damper: f32,
    pub blower: f32,
    pub injectors_on: [bool; 2],
    pub injectors: [f32; 2],
    pub cocks_open: bool,
    pub blowdown_open: bool,
    pub sander: bool,
    pub reverse: bool,
    pub outside_c: f32,
    pub small_ejector: f32,
    pub large_ejector: f32,
    pub brake_release: bool,
    pub brake_pipe_absolute_psi: f32,
    pub has_tender_coupled: bool,
    pub generator_steam_effects: bool,
}
/// Per-tick physical feedback, replaced by the train/brake model before the
/// next tick. It is never sourced from an oracle trajectory.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct NativeSteamFeedback {
    pub outside_c: f32,
    pub height_m: f32,
    pub signed_speed_mps: f32,
    pub has_tender_coupled: bool,
    pub generator_steam_effects: bool,
    pub brake_pipe_absolute_psi: f32,
    pub brake_release: bool,
    pub small_ejector_setting: f32,
    pub large_ejector_setting: f32,
}

impl NativeSteamState {
    pub fn new(p: &NativeSteamParams, hot: bool) -> Result<Self, String> {
        let h = NativeSteamHardware::new(p)?;
        let psi = if hot {
            p.maximum_pressure_psi - 5.
        } else {
            p.maximum_pressure_psi * 0.66
        };
        let temp = kelvin_from_f(PressureToTemperatureInterpolatorPSItoF.at(psi));
        let steam_heat = SteamHeatInterpolatorPSItoBTUpLB.at(psi);
        let water_heat = WaterHeatInterpolatorPSItoBTUpLB.at(psi);
        let water_density = WaterDensityInterpolatorPSItoLBpFT3.at(psi);
        let steam_density = SteamDensityInterpolatorPSItoLBpFT3.at(psi);
        let mass = 0.9 * h.BoilerVolumeFT3 * water_density
            + (1. - 0.9) * h.BoilerVolumeFT3 * steam_density;
        let heat = 0.9 * h.BoilerVolumeFT3 * water_density * water_heat
            + (1. - 0.9) * h.BoilerVolumeFT3 * steam_density * steam_heat;
        let start_kw = kg_from_lb((psi / p.maximum_pressure_psi) * h.TheoreticalMaxSteamOutputLBpS)
            * kw_from_btu_s(steam_heat);
        let flue = start_kw / ((45. * (1. / 1000.)) * h.EvaporationAreaM2 * 1.) + temp;
        let fire = h.IdealFireMassKG;
        let max_force = p.initial_maximum_force_n;
        let feedback = NativeSteamFeedback {
            outside_c: 20.,
            height_m: 0.,
            signed_speed_mps: 0.,
            has_tender_coupled: false,
            generator_steam_effects: p.generator_fitted,
            brake_pipe_absolute_psi: 14.5037738,
            brake_release: false,
            small_ejector_setting: 0.,
            large_ejector_setting: 0.,
        };
        Ok(Self {
            schema: 1,
            hardware: h,
            HotStart: hot,
            feedback,
            previous_train_brake_kind: NativeSteamBrakeKind::Release,
            BoilerPressurePSI: psi,
            BoilerMassLB: mass,
            BoilerHeatBTU: heat,
            WaterFraction: 0.9,
            BoilerHeatSmooth: NativeSmooth::default(),
            BurnRateCoalSmooth: NativeSmooth::default(),
            BoilerHeatSmoothedBTU: 0.,
            PreviousBoilerHeatSmoothedBTU: 0.,
            BoilerSteamHeatBTUpLB: steam_heat,
            BoilerWaterHeatBTUpLB: water_heat,
            BoilerSteamDensityLBpFT3: steam_density,
            BoilerWaterDensityLBpFT3: water_density,
            BoilerWaterTempK: 0.,
            WaterTempNewK: temp,
            BkW_Diff: 0.,
            FlueTempK: flue,
            BoilerKW: 0.,
            EvaporationLBpS: 0.,
            BoilerHeatInBTUpS: 0.,
            BoilerHeatOutBTUpS: 0.,
            PreviousBoilerHeatOutBTUpS: 0.,
            TotalSteamUsageLBpS: 0.,
            PreviousTotalSteamUsageLBpS: 0.,
            CylinderSteamUsageLBpS: 1.,
            CylCockSteamUsageLBpS: 0.,
            BlowerSteamUsageLBpS: 0.,
            BlowerBurnEffect: 0.,
            DamperBurnEffect: 0.,
            RadiationSteamLossLBpS: 2.5,
            FireMassKG: fire,
            BurnRateRawKGpS: 0.,
            FuelBurnRateSmoothedKGpS: 0.,
            FuelFeedRateKGpS: 0.,
            FireRatio: 0.,
            FireHeatLossPercent: 0.,
            TempFireHeatLossPercent: 0.,
            FuelControllerValue: 1.,
            WaterControllerValue: 1.,
            CurrentLocoTenderWaterVolumeUKG: 0.,
            WaterConsumptionLbpS: 0.,
            BoilerWaterInputLB: 0.,
            CumulativeWaterConsumptionLbs: 0.,
            CumulativeCylinderSteamConsumptionLbs: 0.,
            CumulativeTotalSteamConsumptionLbs: 0.,
            CoalIsExhausted: false,
            WaterIsExhausted: false,
            FireIsExhausted: false,
            FusiblePlugIsBlown: false,
            BoilerIsPriming: false,
            BlowerIsOn: false,
            SafetyIsOn: false,
            safety2IsOn: false,
            safety3IsOn: false,
            safety4IsOn: false,
            SafetyValveUsageLBpS: 0.,
            SafetyValveUsage: [0.; 4],
            BlowdownSteamUsageLBpS: 0.,
            CurrentSuperheatTempF: 0.,
            SuperheatVolumeRatio: 0.,
            SuperheaterSteamUsageFactor: 1.,
            IsSuperSet: false,
            InjectorFraction: [0.; 2],
            InjectorWaterDeliveryF: [65.; 2],
            InjectorFlowRateLBpS: 0.,
            MaxInjectorFlowRateLBpS: 0.,
            InjectorSize: 0.,
            InjectorLockedOut: false,
            InjectorLockOutTimeS: 0.,
            EjectorTotalSteamConsumptionLbpS: 0.,
            SmallEjectorFeedFraction: 0.,
            LargeEjectorFeedFraction: 0.,
            SmallSteamEjectorIsOn: false,
            LargeSteamEjectorIsOn: false,
            SmallEjectorChargingInHgpS: 0.,
            LargeEjectorChargingInHgpS: 0.,
            VacuumPumpChargingInHgpS: 0.,
            VacuumPumpOperating: false,
            GeneratorSteamUsageLBpS: 1.,
            SandingSteamUsageLBpS: 0.,
            StokerSteamUsageLBpS: 0.,
            WaterGlassLevelIN: 0.,
            MaxForceN: max_force,
            engine: NativeSteamEngineState::default(),
        })
    }
    pub fn tender_coal_kg(&self) -> f32 {
        self.FuelControllerValue * self.hardware.source.tender_coal_capacity_kg
    }
    pub fn maximum_tender_water_ukg(&self) -> f32 {
        lb_from_kg(self.hardware.source.tender_water_capacity_kg) / 10.021
    }
    pub fn tender_water_ukg(&self) -> f32 {
        self.WaterControllerValue * self.maximum_tender_water_ukg()
    }
    pub fn liquid_water_kg(&self) -> f32 {
        kg_from_lb(
            self.WaterFraction * self.hardware.BoilerVolumeFT3 * self.BoilerWaterDensityLBpFT3,
        )
    }
    pub fn advance(&mut self, input: NativeSteamInput, dt: f32) -> f32 {
        if !dt.is_finite() || dt <= 0. {
            return self.engine.RealTractiveForceN;
        }
        if [
            input.regulator,
            input.cutoff,
            input.speed_mps,
            input.signed_speed_mps,
            input.firing,
            input.damper,
            input.blower,
            input.injectors[0],
            input.injectors[1],
            input.outside_c,
            input.small_ejector,
            input.large_ejector,
            input.brake_pipe_absolute_psi,
        ]
        .iter()
        .any(|v| !v.is_finite())
        {
            return 0.;
        }
        let mut input = input;
        input.regulator = clamp(input.regulator, 0., 1.);
        input.cutoff = clamp(input.cutoff.abs(), 0., self.hardware.source.cutoff_maximum);
        // The supported manual mode does not run the ±100% AI reverser path.
        // Base.UpdateTrackSander computes its field from the OLD pressure and
        // includes dt; native UpdateAuxiliaries later multiplies by dt again.
        // `sander` is the effective steam-sander request after its type/speed
        // interlocks; this bounded bridge does not enable a sander itself.
        self.SandingSteamUsageLBpS = if input.sander {
            (self.BoilerPressurePSI / self.hardware.source.maximum_pressure_psi)
                * (if input.reverse {
                    self.hardware.source.sand_reverse_steam_lb_s
                } else {
                    self.hardware.source.sand_forward_steam_lb_s
                })
                * dt
        } else {
            0.
        };
        self.update_ejector_switches(input);
        self.update_tender(input, dt);
        self.update_firebox(input, dt);
        self.update_boiler(input, dt);
        self.update_auxiliaries(input, dt);
        self.update_superheat(input);
        self.CylinderSteamUsageLBpS = 0.;
        self.CylCockSteamUsageLBpS = 0.;
        self.update_cylinders(input);
        self.withdraw(self.engine.CylinderSteamUsageLBpS, dt);
        self.CumulativeCylinderSteamConsumptionLbs += self.engine.CylinderSteamUsageLBpS * dt;
        self.CylinderSteamUsageLBpS += self.engine.CylinderSteamUsageLBpS;
        self.CylCockSteamUsageLBpS += self.engine.CylCockSteamUsageLBpS;
        self.update_force(input);
        self.update_water_gauge();
        self.update_injectors(input, dt);
        self.update_firing(input);
        self.engine.RealTractiveForceN
    }
    fn withdraw(&mut self, lb_s: f32, dt: f32) {
        self.BoilerMassLB -= dt * lb_s;
        self.BoilerHeatBTU -= dt * lb_s * (self.BoilerSteamHeatBTUpLB - self.BoilerWaterHeatBTUpLB);
        self.BoilerHeatOutBTUpS += lb_s * (self.BoilerSteamHeatBTUpLB - self.BoilerWaterHeatBTUpLB);
        self.TotalSteamUsageLBpS += lb_s;
    }
    pub fn valid_for(&self, p: &NativeSteamParams) -> bool {
        self.schema == 1
            && self.HotStart == p.hot_start
            && NativeSteamHardware::new(p).is_ok_and(|h| h == self.hardware)
            && (matches!(
                self.previous_train_brake_kind,
                NativeSteamBrakeKind::Release | NativeSteamBrakeKind::Emergency
            ) || p
                .train_brake_notches
                .iter()
                .any(|n| n.kind == self.previous_train_brake_kind))
            && self.BoilerPressurePSI.is_finite()
            && (0. ..=p.maximum_pressure_psi + 7.).contains(&self.BoilerPressurePSI)
            && self.BoilerMassLB.is_finite()
            && self.BoilerMassLB >= 0.
            && self.BoilerHeatBTU.is_finite()
            && self.BoilerHeatBTU >= 0.
            && (0. ..=1.01).contains(&self.WaterFraction)
            && (0. ..=1.).contains(&self.FuelControllerValue)
            && (0. ..=1.).contains(&self.WaterControllerValue)
            && self.BoilerHeatSmooth.value.is_none_or(f32::is_finite)
            && self.BurnRateCoalSmooth.value.is_none_or(f32::is_finite)
            && self.all_state_finite()
    }
    fn all_state_finite(&self) -> bool {
        // JSON cannot represent nonfinite floats. This also traverses nested
        // engine/filter fields instead of forgetting new persisted state.
        serde_json::to_value(self).is_ok_and(|v| !contains_null_number(&v))
    }
}
fn contains_null_number(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Object(xs) => xs.iter().any(|(k, v)| {
            if k == "value" && v.is_null() {
                false
            } else {
                v.is_null() || contains_null_number(v)
            }
        }),
        serde_json::Value::Array(xs) => xs.iter().any(|v| v.is_null() || contains_null_number(v)),
        _ => false,
    }
}
