//! Existing BoilerState integration. Native inventory is authoritative; mirrors
//! serve the common cab/save API and are checked before a restore mutation.
use super::*;
use crate::steam::{BoilerState, SteamControls};
use openrailsrs_train::SteamParams;

fn mirror(b: &mut BoilerState) {
    let n = b
        .native_or161
        .as_ref()
        .expect("selected native steam state");
    b.pressure_bar = n.BoilerPressurePSI as f64 / 14.503773773;
    b.water_kg = n.liquid_water_kg() as f64;
    b.coal_kg = n.tender_coal_kg() as f64;
    b.initial_water_kg = kg_from_lb(
        n.hardware.BoilerVolumeFT3
            * WaterDensityInterpolatorPSItoLBpFT3.at(n.hardware.source.maximum_pressure_psi),
    ) as f64;
    b.initial_tender_water_kg = n.hardware.source.tender_water_capacity_kg as f64;
    b.initial_coal_kg = n.hardware.source.tender_coal_capacity_kg as f64;
    b.tender_water_kg = kg_from_lb(n.tender_water_ukg() * 10.021) as f64;
    b.fire_mass_kg = n.FireMassKG as f64;
    b.fire_capacity_kg = n.hardware.MaxFireMassKG as f64;
    b.low_water_failure = n.FusiblePlugIsBlown;
    b.safety_valve = n.SafetyIsOn;
    b.steam_usage_kg_s = kg_from_lb(n.PreviousTotalSteamUsageLBpS) as f64;
    b.evaporation_kg_s = kg_from_lb(n.EvaporationLBpS) as f64;
    b.injection_kg_s = kg_from_lb(n.WaterConsumptionLbpS) as f64;
    b.coal_burn_kg_s = n.FuelBurnRateSmoothedKGpS as f64;
    b.tractive_force_n = n.engine.RealTractiveForceN as f64;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeSteamSupply {
    Water,
    Coal,
}
/// Resource-installation bridge, separate from the native cycle's evolution.
/// Quantize delivery down to a representable controller inventory, so an
/// installation never supplies more mass than it owns. No boiler/failure state
/// is reset, and the native next-tick exhaustion checks retain their lag.
pub fn replenish_boiler(
    b: &mut BoilerState,
    supply: NativeSteamSupply,
    requested_kg: f64,
) -> Result<f64, String> {
    if !requested_kg.is_finite() || requested_kg < 0. {
        return Err("Invalid native steam supply quantity".into());
    }
    let source = b
        .native_or161
        .as_ref()
        .ok_or("Native steam state is absent")?
        .hardware
        .source
        .clone();
    if !boiler_valid(b, &source) {
        return Err("Native steam inventory/mirror is invalid".into());
    }
    if requested_kg == 0. {
        return Ok(0.);
    }
    let n = b.native_or161.as_mut().expect("validated native boiler");
    let (previous, capacity) = match supply {
        NativeSteamSupply::Coal => (n.FuelControllerValue, source.tender_coal_capacity_kg),
        NativeSteamSupply::Water => (n.WaterControllerValue, n.maximum_tender_water_ukg()),
    };
    let quantity = |ratio: f32| match supply {
        NativeSteamSupply::Coal => ratio * capacity,
        NativeSteamSupply::Water => kg_from_lb((ratio * capacity) * 10.021),
    };
    let before = quantity(previous) as f64;
    let maximum = quantity(1.) as f64;
    let target = (before + requested_kg).min(maximum);
    let mut ratio = ((target / maximum) as f32).clamp(previous, 1.);
    while quantity(ratio) as f64 > target && ratio > previous {
        ratio = f32::from_bits(ratio.to_bits() - 1).max(previous);
    }
    let accepted = (quantity(ratio) as f64 - before).max(0.);
    match supply {
        NativeSteamSupply::Coal => n.FuelControllerValue = ratio,
        NativeSteamSupply::Water => n.WaterControllerValue = ratio,
    }
    mirror(b);
    Ok(accepted)
}
pub fn initialize_boiler(b: &mut BoilerState, p: &NativeSteamParams) -> Result<(), String> {
    b.native_or161 = Some(NativeSteamState::new(p, p.hot_start)?);
    b.thermodynamics = None;
    b.controls = SteamControls {
        automatic_fireman: false,
        cutoff: p.initial_cutoff.abs() as f64,
        firing: p.initial_firing as f64,
        damper: p.initial_damper as f64,
        blower: p.initial_blower > 0.,
        injector1: false,
        injector2: false,
        cylinder_cocks: false,
        compound_bypass: false,
    };
    mirror(b);
    Ok(())
}
pub fn boiler_valid(b: &BoilerState, p: &NativeSteamParams) -> bool {
    let Some(n) = b.native_or161.as_ref() else {
        return false;
    };
    if b.thermodynamics.is_some()
        || !n.valid_for(p)
        || b.controls.automatic_fireman
        || b.controls.compound_bypass
    {
        return false;
    }
    if !b.controls.cutoff.is_finite()
        || !(0. ..=p.cutoff_maximum as f64).contains(&b.controls.cutoff)
        || ![b.controls.firing, b.controls.damper]
            .iter()
            .all(|v| v.is_finite() && (0. ..=1.).contains(v))
    {
        return false;
    }
    let mut expected = b.clone();
    mirror(&mut expected);
    b.pressure_bar == expected.pressure_bar
        && b.water_kg == expected.water_kg
        && b.coal_kg == expected.coal_kg
        && b.initial_water_kg == expected.initial_water_kg
        && b.initial_tender_water_kg == expected.initial_tender_water_kg
        && b.initial_coal_kg == expected.initial_coal_kg
        && b.tender_water_kg == expected.tender_water_kg
        && b.fire_mass_kg == expected.fire_mass_kg
        && b.fire_capacity_kg == expected.fire_capacity_kg
        && b.low_water_failure == expected.low_water_failure
        && b.safety_valve == expected.safety_valve
        && b.steam_usage_kg_s == expected.steam_usage_kg_s
        && b.evaporation_kg_s == expected.evaporation_kg_s
        && b.injection_kg_s == expected.injection_kg_s
        && b.coal_burn_kg_s == expected.coal_burn_kg_s
        && b.tractive_force_n == expected.tractive_force_n
}
pub fn advance_boiler(
    b: &mut BoilerState,
    p: &SteamParams,
    regulator: f64,
    speed_mps: f64,
    dt: f64,
) -> f64 {
    let Some(profile) = p.operation.native_or161.as_ref() else {
        return 0.;
    };
    let Some(n) = b.native_or161.as_mut() else {
        return 0.;
    };
    if !dt.is_finite() || dt <= 0. {
        return b.tractive_force_n;
    }
    let f = n.feedback;
    let input = NativeSteamInput {
        regulator: regulator as f32,
        cutoff: b.controls.cutoff as f32,
        speed_mps: speed_mps.abs() as f32,
        signed_speed_mps: f.signed_speed_mps,
        firing: b.controls.firing as f32,
        damper: b.controls.damper as f32,
        blower: if b.controls.blower {
            if profile.initial_blower > 0. {
                profile.initial_blower
            } else {
                1.
            }
        } else {
            0.
        },
        injectors_on: [b.controls.injector1, b.controls.injector2],
        injectors: [1., 1.],
        cocks_open: b.controls.cylinder_cocks,
        blowdown_open: false,
        sander: false,
        reverse: f.signed_speed_mps < 0.,
        outside_c: f.outside_c,
        small_ejector: f.small_ejector_setting,
        large_ejector: f.large_ejector_setting,
        brake_release: f.brake_release,
        brake_pipe_absolute_psi: f.brake_pipe_absolute_psi,
        has_tender_coupled: f.has_tender_coupled,
        generator_steam_effects: f.generator_steam_effects,
    };
    n.advance(input, dt as f32);
    mirror(b);
    b.tractive_force_n
}
