//! Authored consumables and operating controls, separate from traction curves.
use super::{field_values, scalar_text};
use crate::{Ast, FormatError};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SteamStartup {
    #[default]
    WorkingPressure,
    OrHot,
    OrCold,
}
impl SteamStartup {
    fn is_working_pressure(&self) -> bool {
        *self == Self::WorkingPressure
    }
}

/// Saturated two-phase vessel using the pinned SteamTable properties. The
/// vessel, heat balance and cylinder cycle are an explicit Rust extension.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SteamThermodynamicParams {
    /// Explicit source-compatible startup. Absence retains the previous own
    /// vessel initialization and does not change a legacy boiler.
    #[serde(default, skip_serializing_if = "SteamStartup::is_working_pressure")]
    pub startup: SteamStartup,
    pub boiler_volume_m3: f64,
    pub coal_heat_j_kg: f64,
    pub boiler_efficiency: f64,
    pub heat_loss_w: f64,
    pub feedwater_c: f64,
    pub cylinder_efficiency: f64,
    pub idle_steam_kg_s: f64,
}
impl SteamThermodynamicParams {
    pub fn initial_pressure_bar(&self, maximum_bar: f64) -> f64 {
        let psi = (maximum_bar * 14.503773773) as f32;
        match self.startup {
            SteamStartup::WorkingPressure => maximum_bar,
            SteamStartup::OrHot => (psi - 5.).max(0.) as f64 / 14.503773773,
            SteamStartup::OrCold => (psi * 0.66) as f64 / 14.503773773,
        }
    }
    pub fn valid(&self) -> bool {
        (0.1..=100.).contains(&self.boiler_volume_m3)
            && (1_000_000. ..=50_000_000.).contains(&self.coal_heat_j_kg)
            && (0.01..=1.).contains(&self.boiler_efficiency)
            && (0. ..=10_000_000.).contains(&self.heat_loss_w)
            && (0. ..=99.).contains(&self.feedwater_c)
            && (0.01..=1.).contains(&self.cylinder_efficiency)
            && (0. ..=10.).contains(&self.idle_steam_kg_s)
    }
}

/// Declared compound cycle, independently controlled on each locomotive.
/// Receiver storage and LP expansion are not claimed as OR cylinder parity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompoundSteamParams {
    pub hp_cylinders: u32,
    pub lp_cylinders: u32,
    pub lp_bore_m: f64,
    pub receiver_volume_m3: f64,
    pub receiver_pressure_ratio: f64,
    pub starting_valve: bool,
}
impl CompoundSteamParams {
    pub fn valid(&self) -> bool {
        (1..=8).contains(&self.hp_cylinders)
            && (1..=8).contains(&self.lp_cylinders)
            && (0.05..=2.).contains(&self.lp_bore_m)
            && (0.01..=20.).contains(&self.receiver_volume_m3)
            && (0.05..=0.95).contains(&self.receiver_pressure_ratio)
    }
}

/// A converter operating point: turbine/pump speed ratio, torque ratio and
/// pump torque per squared rad/s. Loss follows Ppump-Pturbine, never a free
/// multiplier on an existing electric tractive-effort curve.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConverterPoint {
    pub speed_ratio: f64,
    pub torque_ratio: f64,
    pub capacity_nm_per_rad_s2: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HydraulicProfile {
    pub converter: Vec<ConverterPoint>,
    /// Ordered overall turbine/wheel ratios, low gear first.
    pub gear_ratios: Vec<f64>,
    pub wheel_radius_m: f64,
    pub final_efficiency: f64,
    pub engine_inertia_kg_m2: f64,
    pub shift_up_ratio: f64,
    pub shift_down_ratio: f64,
    pub shift_time_s: f64,
    pub lock_up_ratio: Option<f64>,
    pub lock_up_release_ratio: f64,
    pub cooling_w_per_k: f64,
    pub heat_capacity_j_per_k: f64,
    pub max_temperature_c: f64,
    pub fuel_sfc_g_kwh: f64,
}
impl HydraulicProfile {
    pub fn valid(&self) -> bool {
        self.converter.len() >= 2
            && self.converter.len() <= 128
            && self.converter.first().is_some_and(|p| p.speed_ratio == 0.)
            && self.converter.last().is_some_and(|p| p.speed_ratio == 1.)
            && self.converter.iter().all(|p| {
                (0. ..=1.).contains(&p.speed_ratio)
                    && (0. ..=5.).contains(&p.torque_ratio)
                    && (0.00001..=10.).contains(&p.capacity_nm_per_rad_s2)
                    && p.speed_ratio * p.torque_ratio <= 1. + 1e-10
            })
            && self.converter.windows(2).all(|p| {
                let a = &p[0];
                let b = &p[1];
                let dr = b.speed_ratio - a.speed_ratio;
                if dr <= 0. {
                    return false;
                }
                // Linear torque interpolation makes efficiency r*TR a
                // quadratic. Endpoints alone miss an interior maximum.
                // Parameterize by t in [0,1] to avoid a slope divided by
                // arbitrarily small authored speed-ratio intervals.
                let dt = b.torque_ratio - a.torque_ratio;
                let quadratic = dr * dt;
                if quadratic >= 0. {
                    return true;
                }
                let linear = a.speed_ratio * dt + dr * a.torque_ratio;
                let t = -linear / (2. * quadratic);
                if !(0. ..1.).contains(&t) {
                    return true;
                }
                let ratio = a.speed_ratio + t * dr;
                let torque = a.torque_ratio + t * dt;
                ratio * torque <= 1. + 1e-10
            })
            && !self.gear_ratios.is_empty()
            && self.gear_ratios.len() <= 16
            && self.gear_ratios.iter().all(|v| (0.1..=200.).contains(v))
            && self.gear_ratios.windows(2).all(|v| v[0] > v[1])
            && (0.1..=2.).contains(&self.wheel_radius_m)
            && (0.1..=1.).contains(&self.final_efficiency)
            && (0.1..=10000.).contains(&self.engine_inertia_kg_m2)
            && (0.1..=1.).contains(&self.shift_up_ratio)
            && (0. ..self.shift_up_ratio).contains(&self.shift_down_ratio)
            && (0. ..=10.).contains(&self.shift_time_s)
            && self
                .lock_up_ratio
                .is_none_or(|v| (0.1..=1.).contains(&v) && v > self.lock_up_release_ratio)
            && (0. ..=1.).contains(&self.lock_up_release_ratio)
            && (0. ..=100000.).contains(&self.cooling_w_per_k)
            && (1000. ..=100_000_000.).contains(&self.heat_capacity_j_per_k)
            && (40. ..=300.).contains(&self.max_temperature_c)
            && (0. ..=1000.).contains(&self.fuel_sfc_g_kwh)
    }
}

fn parse_hydraulic(ast: &Ast) -> Result<Option<HydraulicProfile>, FormatError> {
    let fields = field_values(ast, "ORTSRSHydraulicConverter");
    let Some(values) = fields.last() else {
        return Ok(None);
    };
    let nums = scalar_values(values)
        .into_iter()
        .map(|v| {
            v.parse::<f64>()
                .map_err(|_| invalid("ORTSRSHydraulicConverter"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if nums.len() < 6 || nums.len() % 3 != 0 {
        return Err(invalid("ORTSRSHydraulicConverter"));
    }
    let ratios = field_values(ast, "ORTSRSHydraulicGearRatios");
    let gear_ratios = ratios
        .last()
        .map(|v| scalar_values(v))
        .unwrap_or_default()
        .into_iter()
        .map(|v| {
            v.parse::<f64>()
                .map_err(|_| invalid("ORTSRSHydraulicGearRatios"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let q = |name, default| quantity(ast, name, "rpm").map(|v| v.unwrap_or(default));
    let p = HydraulicProfile {
        converter: nums
            .chunks_exact(3)
            .map(|v| ConverterPoint {
                speed_ratio: v[0],
                torque_ratio: v[1],
                capacity_nm_per_rad_s2: v[2],
            })
            .collect(),
        gear_ratios,
        wheel_radius_m: q("ORTSRSHydraulicWheelRadiusM", 0.5)?,
        final_efficiency: q("ORTSRSHydraulicFinalEfficiency", 0.95)?,
        engine_inertia_kg_m2: q("ORTSRSHydraulicEngineInertia", 12.)?,
        shift_up_ratio: q("ORTSRSHydraulicShiftUpRatio", 0.85)?,
        shift_down_ratio: q("ORTSRSHydraulicShiftDownRatio", 0.35)?,
        shift_time_s: quantity(ast, "ORTSRSHydraulicShiftTime", "s")?.unwrap_or(0.5),
        lock_up_ratio: quantity(ast, "ORTSRSHydraulicLockUpRatio", "rpm")?,
        lock_up_release_ratio: q("ORTSRSHydraulicLockUpReleaseRatio", 0.6)?,
        cooling_w_per_k: q("ORTSRSHydraulicCoolingWPerK", 300.)?,
        heat_capacity_j_per_k: q("ORTSRSHydraulicHeatCapacityJPerK", 150000.)?,
        max_temperature_c: q("ORTSRSHydraulicMaxTemperatureC", 130.)?,
        fuel_sfc_g_kwh: q("ORTSRSHydraulicFuelSfcGPerKwh", 210.)?,
    };
    if !p.valid() || super::gearbox::parse_native_gearbox(ast, 300., 900.)?.is_some() {
        return Err(invalid("conflicting or invalid hydraulic transmission"));
    }
    Ok(Some(p))
}

pub(super) fn parse_steam_extensions(
    ast: &Ast,
    operation: &mut SteamOperatingParams,
    hp_cylinders: u32,
) -> Result<Option<u32>, FormatError> {
    let enabled = quantity(ast, "ORTSRSUseSteamTables", "rpm")?.unwrap_or(0.) > 0.;
    if !enabled {
        return Ok(None);
    }
    let q = |name, default| quantity(ast, name, "rpm").map(|v| v.unwrap_or(default));
    let p = SteamThermodynamicParams {
        startup: match field_values(ast, "ORTSRSBoilerStartup")
            .last()
            .map(|v| scalar_values(v).join("").to_ascii_lowercase())
            .as_deref()
        {
            None | Some("working_pressure") => SteamStartup::WorkingPressure,
            Some("or_hot") => SteamStartup::OrHot,
            Some("or_cold") => SteamStartup::OrCold,
            _ => return Err(invalid("ORTSRSBoilerStartup")),
        },
        boiler_volume_m3: operation.boiler_water_capacity_kg / 1000.,
        coal_heat_j_kg: q("ORTSRSCoalHeatJPerKg", 28_000_000.)?,
        boiler_efficiency: q("ORTSRSBoilerEfficiency", 0.7)?,
        heat_loss_w: q("ORTSRSBoilerHeatLossW", 15000.)?,
        feedwater_c: quantity(ast, "ORTSRSFeedwaterTemperature", "c")?.unwrap_or(20.),
        cylinder_efficiency: q("ORTSRSCylinderEfficiency", 0.12)?,
        idle_steam_kg_s: q("ORTSRSIdleSteamKgPerS", 0.05)?,
    };
    if !p.valid() {
        return Err(invalid("steam thermodynamics"));
    }
    operation.thermodynamics = Some(p);
    let lp = quantity(ast, "LPNumCylinders", "rpm")?.unwrap_or(0.);
    if lp == 0. {
        return Ok(None);
    }
    if lp.fract() != 0. || !(1. ..=8.).contains(&lp) {
        return Err(invalid("LPNumCylinders"));
    }
    let raw_bore = field_values(ast, "LPCylinderDiameter")
        .last()
        .map(|v| scalar_values(v).join(" "));
    let bore = raw_bore
        .as_deref()
        .and_then(crate::msts_units::parse_length_m)
        .ok_or_else(|| invalid("LPCylinderDiameter"))?;
    let compound = CompoundSteamParams {
        hp_cylinders,
        lp_cylinders: lp as u32,
        lp_bore_m: bore,
        receiver_volume_m3: q("ORTSRSReceiverVolumeM3", 0.3)?,
        receiver_pressure_ratio: q("ORTSRSReceiverPressureRatio", 0.4)?,
        starting_valve: q("ORTSRSCompoundStartingValve", 0.)? > 0.,
    };
    if !compound.valid() {
        return Err(invalid("compound cycle"));
    }
    operation.compound = Some(compound);
    Ok(Some(hp_cylinders + lp as u32))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DieselOperatingParams {
    /// Authored Rust converter model. OR 1.6.1's `Hydraulic` label alone does
    /// not supply converter characteristics and never enables this extension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hydraulic: Option<HydraulicProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gearbox: Option<super::gearbox::NativeGearboxProfile>,
    pub thermal: DieselThermalParams,
    pub capacity_l: f64,
    pub starting_rpm: f64,
    pub confirmation_rpm: f64,
    /// RPM → litres/hour. Native OR tables are not g/kWh.
    pub consumption_lph: Vec<(f64, f64)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SteamOperatingParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_or161: Option<super::native_steam::NativeSteamParams>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thermodynamics: Option<SteamThermodynamicParams>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compound: Option<CompoundSteamParams>,
    pub boiler_water_capacity_kg: f64,
    pub max_fire_mass_kg: f64,
    pub max_firing_rate_kg_s: f64,
    pub injector_rate_kg_s: f64,
}
impl Default for SteamOperatingParams {
    fn default() -> Self {
        Self {
            native_or161: None,
            thermodynamics: None,
            compound: None,
            boiler_water_capacity_kg: 4000.,
            max_fire_mass_kg: 300.,
            max_firing_rate_kg_s: 1.,
            injector_rate_kg_s: 5.,
        }
    }
}

fn invalid(key: &str) -> FormatError {
    FormatError::UnexpectedAtom {
        key: key.into(),
        context: "traction operating parameters".into(),
        expected: "finite nonnegative quantity with supported units".into(),
    }
}

fn scalar_values(values: &[Ast]) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        if let Some(text) = scalar_text(value) {
            out.push(text);
        } else if let Ast::List(items) = value {
            out.extend(scalar_values(items));
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DieselThermalParams {
    pub maximum_c: f64,
    pub time_constant_s: f64,
    pub cooling: u8,
    /// Explicit Rust extension; native OR temperature is normally diagnostic.
    pub protection: bool,
}
impl Default for DieselThermalParams {
    fn default() -> Self {
        Self {
            maximum_c: 100.0,
            time_constant_s: 720.0,
            cooling: 3,
            protection: false,
        }
    }
}

/// Strict quantities used for resources; unknown suffixes must not become litres.
pub(super) fn quantity(
    ast: &Ast,
    key: &str,
    default_unit: &str,
) -> Result<Option<f64>, FormatError> {
    let fields = field_values(ast, key);
    // Native STF processes repeated declarations in order; the final value
    // overrides earlier stock defaults (also after Include expansion).
    let Some(values) = fields.last() else {
        return Ok(None);
    };
    let scalars = scalar_values(values);
    if scalars.is_empty() || scalars.len() > 2 {
        return Err(invalid(key));
    }
    let raw = scalars.concat().to_ascii_lowercase();
    let split = raw.find(|c: char| !c.is_ascii_digit() && !matches!(c, '.' | '-' | '+' | 'e'));
    let (number, unit) = split.map_or((raw.as_str(), default_unit), |i| (&raw[..i], &raw[i..]));
    // Native STF also writes quoted volume units such as "225*(ft^3)".
    // Normalize only the suffix; no expression is evaluated.
    let unit = unit.replace(['*', '(', ')', '^'], "");
    let per_minute = default_unit == "psi" && unit.ends_with("/min");
    let unit = if default_unit == "psi" {
        unit.trim_end_matches("/s").trim_end_matches("/min")
    } else {
        unit.as_str()
    };
    let scale = match (default_unit, unit) {
        ("c", "c" | "degc") | ("s", "s") => 1.,
        ("c", "degf") => 5. / 9.,
        ("s", "ms") => 0.001,
        ("rpm", "") => 1.,
        ("l" | "ft3", "l") | ("kg", "kg") | ("rpm", "rpm") => 1.,
        ("l" | "ft3", "g-uk" | "gal-uk") => 4.54609,
        ("l" | "ft3", "g-us" | "gal-us" | "gal" | "gals" | "gallon" | "gallons") => 3.785411784,
        ("l" | "ft3", "m3") => 1000.,
        ("l" | "ft3", "ft3") => 28.316846592,
        ("l" | "ft3", "in3") => 0.016387064,
        ("kg", "lb") => 0.45359237,
        ("kg", "t") => 1000.,
        ("kg", "t-uk") => 1016.05,
        ("kg", "t-us") => 907.18474,
        ("psi", "bar") => 1.,
        ("psi", "psi") => 0.06894757293168,
        ("psi", "kpa") => 0.01,
        ("psi", "inhg") => 0.0338639,
        ("psi", "cmhg") => 0.0133322,
        ("lb/h", "kg/s") => 1.,
        ("lb/h", "lb/h") => 0.45359237 / 3600.,
        ("lb/h", "kg/h") => 1. / 3600.,
        ("lb/h", "g/h") => 0.001 / 3600.,
        ("m/s", "m/s" | "mps") => 1.,
        ("m/s", "km/h" | "kmh") => 1. / 3.6,
        ("m/s", "mph") => 0.44704,
        ("ft3/s", "ft3" | "ft3/s") => 28.316846592,
        ("ft3/s", "ft3/h") => 28.316846592 / 3600.,
        ("ft3/s", "m3" | "m3/s") => 1000.,
        ("ft3/s", "m3/h") => 1000. / 3600.,
        ("ft3/s", "l" | "l/s") => 1.,
        ("ft3/s", "l/h") => 1. / 3600.,
        ("ft3/s", "in3" | "in3/s") => 0.016387064,
        ("ft3/s", "g-uk" | "g-uk/s") => 4.54609,
        ("ft3/s", "g-us" | "g-us/s") => 3.785411784,
        _ => return Err(invalid(key)),
    };
    let mut value = number.parse::<f64>().map_err(|_| invalid(key))? * scale
        / if per_minute { 60. } else { 1. };
    if default_unit == "c" && unit == "degf" {
        // STFReader.UNITS.Temperature uses an absolute Fahrenheit offset;
        // BNSF's original diesel files declare MaxTemperature (120degF).
        value -= 32. * scale;
    }
    if !value.is_finite() || value < 0. {
        return Err(invalid(key));
    }
    Ok(Some(value))
}

pub(super) fn parse_diesel(
    ast: &Ast,
    idle: f64,
    max: f64,
) -> Result<Option<DieselOperatingParams>, FormatError> {
    if !super::parse_vehicle_content_metadata(ast, true)
        .engine_type
        .is_some_and(|v| v.eq_ignore_ascii_case("diesel"))
    {
        return Ok(None);
    }
    let idle = if idle > 0. { idle } else { 300. };
    let max = if max > idle {
        max
    } else {
        600_f64.max(idle * 1.5)
    };
    let fields = field_values(ast, "DieselConsumptionTab");
    let mut consumption = Vec::new();
    if let Some(values) = fields.first() {
        let numbers: Vec<_> = scalar_values(values)
            .into_iter()
            .map(|s| {
                s.parse::<f64>()
                    .map_err(|_| invalid("DieselConsumptionTab"))
            })
            .collect::<Result<_, _>>()?;
        if numbers.len() < 4 || numbers.len() % 2 != 0 {
            return Err(invalid("DieselConsumptionTab"));
        }
        for pair in numbers.as_chunks::<2>().0 {
            if pair.iter().any(|v| !v.is_finite() || *v < 0.)
                || consumption.last().is_some_and(|(r, _)| *r >= pair[0])
            {
                return Err(invalid("DieselConsumptionTab"));
            }
            consumption.push((pair[0], pair[1]));
        }
    } else {
        consumption = vec![
            (
                idle,
                quantity(ast, "DieselUsedPerHourAtIdle", "l")?.unwrap_or(1.),
            ),
            (
                max,
                quantity(ast, "DieselUsedPerHourAtMaxPower", "l")?.unwrap_or(1.),
            ),
        ];
    }
    let starting_rpm = quantity(ast, "StartingRPM", "rpm")?.unwrap_or(idle * 2. / 3.);
    let authored_confirmation_rpm = quantity(ast, "StartingConfirmRPM", "rpm")?;
    // OR 1.6.1 DieselEngine.Initialize copies IdleRPM * 1.1 when absent, even
    // for an auxiliary engine whose authored MaxRPM is only slightly above idle.
    // Keep that default rather than rejecting or changing the author's RPMs.
    let confirmation_rpm = authored_confirmation_rpm.unwrap_or(idle * 1.1);
    if starting_rpm <= 0.
        || confirmation_rpm <= starting_rpm
        || authored_confirmation_rpm.is_some_and(|rpm| rpm >= max)
    {
        return Err(invalid("StartingRPM / StartingConfirmRPM"));
    }
    let maximum_c = quantity(ast, "MaxTemperature", "c")?
        .or(quantity(ast, "DieselMaxTemperature", "c")?)
        .unwrap_or(100.0);
    let time_constant_s = quantity(ast, "TempTimeConstant", "s")?.unwrap_or(720.0);
    let cooling = quantity(ast, "Cooling", "rpm")?.unwrap_or(3.0);
    if !(40.0..=300.0).contains(&maximum_c)
        || time_constant_s <= 0.0
        || cooling > 3.0
        || cooling.fract() != 0.0
    {
        return Err(invalid("diesel thermal parameters"));
    }
    Ok(Some(DieselOperatingParams {
        hydraulic: parse_hydraulic(ast)?,
        gearbox: super::gearbox::parse_native_gearbox(ast, idle, max)?,
        thermal: DieselThermalParams {
            maximum_c,
            time_constant_s,
            cooling: cooling as u8,
            protection: quantity(ast, "ORTSRSOverheatProtection", "rpm")?.unwrap_or(0.0) > 0.0,
        },
        capacity_l: quantity(ast, "MaxDieselLevel", "l")?.unwrap_or(5000.),
        starting_rpm,
        confirmation_rpm,
        consumption_lph: consumption,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EngineFile, parse_vehicle_text};

    #[test]
    fn hydraulic_rejects_interior_energy_creation_between_valid_endpoints() {
        let ast = parse_vehicle_text(include_str!(
            "../../../../examples/physics_lab/vehicles/hydraulic.eng"
        ))
        .unwrap();
        let mut p = parse_hydraulic(&ast).unwrap().unwrap();
        assert!(p.valid(), "existing authored converter remains unchanged");
        p.converter = [(0., 2.2), (0.5, 2.), (1., 1.)]
            .into_iter()
            .map(|(speed_ratio, torque_ratio)| ConverterPoint {
                speed_ratio,
                torque_ratio,
                capacity_nm_per_rad_s2: 0.1,
            })
            .collect();
        assert!(
            p.converter
                .iter()
                .all(|c| c.speed_ratio * c.torque_ratio <= 1.)
        );
        assert_eq!(0.75 * 1.5, 1.125);
        assert!(!p.valid(), "interpolated converter efficiency exceeds one");
        // A concave segment whose interior maximum is exactly one remains
        // valid; do not reject every negative torque slope.
        p.converter[1].torque_ratio = 1.875;
        p.converter[2].torque_ratio = 0.75;
        assert!(p.valid());
        let bad = parse_vehicle_text("Engine ( own Type ( Diesel ) ORTSRSHydraulicConverter ( 0 2.2 0.1 0.5 2 0.1 1 1 0.1 ) ORTSRSHydraulicGearRatios ( 6 3.5 2 ) )").unwrap();
        assert!(
            parse_hydraulic(&bad).is_err(),
            "reject before runtime clipping"
        );
    }

    #[test]
    fn native_resources_use_correct_units_and_distinct_boiler_and_tender() {
        let ast = parse_vehicle_text(
            r#"
Wagon ( engine Mass ( 82000 ) WheelRadius ( 0.75m ) )
Engine ( engine Type ( Steam ) NumberOfCylinders ( 2 )
    CylinderDiameter ( 16in ) CylinderStroke ( 26in ) WheelRadius ( 0.5m )
    MaxBoilerPressure ( 225 ) BoilerVolume ( "225*(ft^3)" )
    MaxTenderWaterMass ( 40000lb ) MaxTenderCoalMass ( 13440lb )
    MaxFireMass ( 2400lb ) SteamFiremanMaxPossibleFiringRate ( 3600lb/h ) )
"#,
        )
        .unwrap();
        let s = EngineFile::from_ast(&ast).unwrap().steam.unwrap();
        assert_eq!(s.cylinder_count, 2);
        assert_eq!(
            s.driving_wheel_radius_m, 0.5,
            "engine wheels must not inherit wagon wheel radius"
        );
        assert!((s.cylinder_bore_m - 0.4064).abs() < 1e-8);
        assert!((s.working_pressure_bar - 15.51320391).abs() < 1e-7);
        assert!((s.operation.boiler_water_capacity_kg - 6371.2904832).abs() < 1e-7);
        assert!((s.initial_water_kg - 18143.6948).abs() < 1e-7);
        assert!((s.initial_coal_kg - 6096.2814528).abs() < 1e-7);
        assert!((s.operation.max_firing_rate_kg_s - 0.45359237).abs() < 1e-9);
    }

    #[test]
    fn a_driving_wheel_diameter_below_two_metres_is_still_a_diameter() {
        let ast = parse_vehicle_text("(Engine (Mass 82000) (NumberOfCylinders 2) (CylinderDiameter 0.47m) (CylinderStroke 0.66m) (DrivingWheelDiameter 1.94m) (MaxBoilerPressure (16bar)))").unwrap();
        let s = EngineFile::from_ast(&ast).unwrap().steam.unwrap();
        assert_eq!(s.driving_wheel_radius_m, 0.97);
        assert_eq!(s.working_pressure_bar, 16.);
    }

    #[test]
    fn diesel_native_gallons_are_us_and_tables_and_start_thresholds_are_authored() {
        let ast = parse_vehicle_text("Engine ( e Type ( Diesel ) MaxDieselLevel ( 500gal ) StartingRPM ( 180 ) StartingConfirmRPM ( 350 ) DieselConsumptionTab ( 0 0 300 18 900 180 ) )").unwrap();
        let p = parse_diesel(&ast, 300., 900.).unwrap().unwrap();
        assert!((p.capacity_l - 1892.705892).abs() < 1e-6);
        assert_eq!(p.starting_rpm, 180.);
        assert_eq!(p.confirmation_rpm, 350.);
        assert_eq!(p.consumption_lph, vec![(0., 0.), (300., 18.), (900., 180.)]);
        let legacy = parse_vehicle_text("Engine ( e Type ( Diesel ) MaxDieselLevel ( 500g-uk ) DieselUsedPerHourAtIdle ( 2gal ) DieselUsedPerHourAtMaxPower ( 15gal ) )").unwrap();
        let p = parse_diesel(&legacy, 150., 1800.).unwrap().unwrap();
        assert!((p.capacity_l - 2273.045).abs() < 1e-6);
        assert!((p.consumption_lph[0].1 - 7.570823568).abs() < 1e-8);
        assert_eq!(p.starting_rpm, 100.);
        assert_eq!(p.confirmation_rpm, 165.);
    }

    #[test]
    fn narrow_auxiliary_rpm_range_retains_open_rails_startup_defaults() {
        let ast = parse_vehicle_text(
            "Engine ( e Type ( Diesel ) DieselEngineIdleRPM ( 1229 ) DieselEngineMaxRPM ( 1230 ) )",
        )
        .unwrap();
        let p = parse_diesel(&ast, 1229., 1230.).unwrap().unwrap();
        assert_eq!(p.starting_rpm, 1229. * 2. / 3.);
        assert_eq!(p.confirmation_rpm, 1229. * 1.1);
        assert_eq!(p.consumption_lph[0].0, 1229.);
        assert_eq!(p.consumption_lph[1].0, 1230.);
        for field in ["StartingRPM ( NaN )", "StartingConfirmRPM ( 1351.9 )"] {
            let ast = parse_vehicle_text(&format!("Engine ( e Type ( Diesel ) {field} )")).unwrap();
            assert!(
                parse_diesel(&ast, 1229., 1230.).is_err(),
                "accepted {field}"
            );
        }
    }

    #[test]
    fn invalid_resource_units_tables_and_geometry_are_rejected() {
        for field in [
            "MaxDieselLevel ( 2kg )",
            "MaxDieselLevel ( -2l )",
            "MaxDieselLevel ( 5bananas )",
            "StartingRPM ( 0 )",
            "StartingConfirmRPM ( 100 )",
            "DieselConsumptionTab ( 300 18 300 180 )",
            "DieselConsumptionTab ( 300 -18 900 180 )",
        ] {
            let ast = parse_vehicle_text(&format!("Engine ( e Type ( Diesel ) {field} )")).unwrap();
            assert!(parse_diesel(&ast, 300., 900.).is_err(), "accepted {field}");
        }
        let ast = parse_vehicle_text("Engine ( e Type ( Steam ) Mass ( 82000 ) NumCylinders ( 2 ) CylinderDiameter ( 0.47m ) CylinderStroke ( 0.66m ) WheelRadius ( 0m ) MaxBoilerPressure ( 225 ) )").unwrap();
        assert!(EngineFile::from_ast(&ast).is_err());
        for count in ["0", "-2", "2.5"] {
            let ast = parse_vehicle_text(&format!("Engine ( e Type ( Steam ) Mass ( 82000 ) NumCylinders ( {count} ) CylinderDiameter ( 0.47m ) CylinderStroke ( 0.66m ) WheelRadius ( 0.97m ) MaxBoilerPressure ( 225 ) )")).unwrap();
            assert!(
                EngineFile::from_ast(&ast).is_err(),
                "accepted cylinder count {count}"
            );
        }
    }

    #[test]
    fn repeated_resource_declarations_keep_the_final_native_override() {
        let ast = parse_vehicle_text(
            "Engine ( e Type ( Diesel ) MaxDieselLevel ( 872gal ) MaxDieselLevel ( 830gal ) )",
        )
        .unwrap();
        let p = parse_diesel(&ast, 315., 1400.).unwrap().unwrap();
        assert!((p.capacity_l - 830. * 3.785411784).abs() < 1e-8);
    }

    #[test]
    fn native_fahrenheit_maximum_uses_absolute_temperature_conversion() {
        let ast = parse_vehicle_text(
            "Engine ( e Type ( Diesel ) MaxTemperature ( 120 ) MaxTemperature ( 120degF ) )",
        )
        .unwrap();
        let profile = parse_diesel(&ast, 315., 1400.).unwrap().unwrap();
        assert!((profile.thermal.maximum_c - (120. - 32.) * 5. / 9.).abs() < 1e-10);
        let invalid =
            parse_vehicle_text("Engine ( e Type ( Diesel ) MaxTemperature ( 120kelvin-typo ) )")
                .unwrap();
        assert!(parse_diesel(&invalid, 315., 1400.).is_err());
    }
}
