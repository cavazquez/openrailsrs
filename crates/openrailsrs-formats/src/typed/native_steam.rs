//! Explicit OR1.6.1 coal/simple/injector cycle. All native values are Single.
//! Parsing a candidate does not select this cycle for legacy scenarios.
#![allow(clippy::excessive_precision)] // Preserve OR Single conversion constants and rounding order.
use crate::{Ast, Atom, FormatError};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeSteamBoilerType {
    #[default]
    LegacyInferred,
    Saturated,
    Superheated,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSteamParams {
    pub hot_start: bool,
    pub cylinders: u32,
    pub cylinder_diameter_m: f32,
    pub cylinder_stroke_m: f32,
    pub driver_radius_m: f32,
    pub maximum_pressure_psi: f32,
    pub boiler_volume_ft3: f32,
    pub grate_area_m2: f32,
    pub evaporation_area_m2: f32,
    pub boiler_evaporation_lb_ft2_h: f32,
    pub boiler_type: NativeSteamBoilerType,
    pub legacy_superheater: f32,
    pub superheater_area_m2: f32,
    pub maximum_superheat_f: f32,
    pub superheat_cutoff_pressure_factor: f32,
    pub cylinder_port_opening: f32,
    pub cylinder_efficiency: f32,
    pub tractive_effort_factor: f32,
    pub maximum_ihp: f32,
    /// MSTSLocomotive.MaxForceN before UpdateTractiveForce replaces it.
    pub initial_maximum_force_n: f32,
    pub maximum_fire_mass_kg: f32,
    pub maximum_firing_rate_kg_s: f32,
    pub orts_maximum_firing_rate_kg_s: f32,
    pub coal_heat_kj_kg: f32,
    pub tender_coal_capacity_kg: f32,
    pub tender_water_capacity_kg: f32,
    pub tender_required: bool,
    pub boiler_surface_ft2: f32,
    pub insulated_fraction: f32,
    pub insulation_kc: f32,
    pub small_ejector_lb_s: f32,
    pub large_ejector_lb_s: f32,
    pub vacuum_maximum_boiler_psi: f32,
    pub vacuum_charging_inhg_s: f32,
    pub small_ejector_controller_fitted: bool,
    pub large_ejector_controller_fitted: bool,
    pub vacuum_pump_fitted: bool,
    pub generator_fitted: bool,
    pub mechanical_stoker: bool,
    pub sand_forward_steam_lb_s: f32,
    pub sand_reverse_steam_lb_s: f32,
    pub cylinder_cock_diameter_in: f32,
    pub piston_shaft_diameter_in: f32,
    pub cutoff_maximum: f32,
    pub initial_cutoff: f32,
    pub initial_blower: f32,
    pub initial_damper: f32,
    pub initial_firing: f32,
    pub initial_train_brake: f32,
    pub maximum_vacuum_psi: f32,
    pub pipe_time_factor_s: f32,
    pub service_time_factor_s: f32,
    pub emergency_time_factor_s: f32,
    pub pipe_discharge_multiplier: f32,
    pub train_pipe_leak_psi_s: f32,
    pub train_brake_notches: Vec<NativeSteamBrakeNotch>,
    /// Native two-column authored overrides. Empty selects source defaults.
    pub boiler_efficiency: Vec<(f32, f32)>,
    pub burn_rate_lb_h: Vec<(f32, f32)>,
    pub initial_pressure_drop: Vec<(f32, f32)>,
    pub backpressure_psi: Vec<(f32, f32)>,
    pub exhaust_event: Vec<(f32, f32)>,
    pub compression_event: Vec<(f32, f32)>,
    pub admission_event: Vec<(f32, f32)>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeSteamBrakeKind {
    Release,
    FullQuickRelease,
    Running,
    Lap,
    Apply,
    Emergency,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NativeSteamBrakeNotch {
    pub value: f32,
    pub smooth: bool,
    pub kind: NativeSteamBrakeKind,
}
impl NativeSteamParams {
    pub fn valid(&self) -> bool {
        (1..=8).contains(&self.cylinders)
            && (0.05..=2.).contains(&self.cylinder_diameter_m)
            && (0.05..=3.).contains(&self.cylinder_stroke_m)
            && (0.1..=2.).contains(&self.driver_radius_m)
            && (10. ..=293.).contains(&self.maximum_pressure_psi)
            && self.boiler_volume_ft3.is_finite() && self.boiler_volume_ft3 > 0. && self.boiler_volume_ft3 <= 1.0e9
            && (1000. ..=50000.).contains(&self.coal_heat_kj_kg)
            && self.tender_coal_capacity_kg > 0. && self.tender_coal_capacity_kg <= 1.0e9
            && self.tender_water_capacity_kg > 0. && self.tender_water_capacity_kg <= 1.0e9
            && [self.grate_area_m2, self.evaporation_area_m2, self.boiler_evaporation_lb_ft2_h,
                self.legacy_superheater, self.superheater_area_m2, self.maximum_superheat_f,
                self.superheat_cutoff_pressure_factor, self.cylinder_port_opening,
                self.cylinder_efficiency, self.tractive_effort_factor, self.maximum_ihp,self.initial_maximum_force_n,
                self.maximum_fire_mass_kg, self.maximum_firing_rate_kg_s,
                self.orts_maximum_firing_rate_kg_s, self.boiler_surface_ft2,
                self.insulated_fraction, self.insulation_kc, self.small_ejector_lb_s,
                self.large_ejector_lb_s, self.vacuum_maximum_boiler_psi,
                self.vacuum_charging_inhg_s, self.sand_forward_steam_lb_s,
                self.sand_reverse_steam_lb_s, self.cylinder_cock_diameter_in,
                self.piston_shaft_diameter_in, self.cutoff_maximum,self.initial_blower,self.initial_damper,
                self.initial_firing,self.initial_train_brake,self.maximum_vacuum_psi,self.pipe_time_factor_s,
                self.service_time_factor_s,self.emergency_time_factor_s,self.pipe_discharge_multiplier,self.train_pipe_leak_psi_s]
                .iter().all(|v| v.is_finite() && *v >= 0. && *v <= 1.0e9)
            && self.insulated_fraction <= 1.
            && self.cutoff_maximum > 0. && self.cutoff_maximum <= 0.9
            && self.initial_cutoff.is_finite() && self.initial_cutoff.abs()<=self.cutoff_maximum
            && [self.initial_blower,self.initial_damper,self.initial_firing,self.initial_train_brake]
                .iter().all(|v|(0. ..=1.).contains(v))
            && self.maximum_vacuum_psi>=7. && self.maximum_vacuum_psi<=13.
            && self.pipe_time_factor_s>0. && self.service_time_factor_s>0. && self.emergency_time_factor_s>0.
            && self.train_brake_notches.len()>=2
            && self.train_brake_notches.iter().all(|n|(0. ..=1.).contains(&n.value))
            && self.train_brake_notches.windows(2).all(|n|n[0].value<n[1].value)
            && [&self.boiler_efficiency, &self.burn_rate_lb_h, &self.initial_pressure_drop,
                &self.backpressure_psi, &self.exhaust_event, &self.compression_event,
                &self.admission_event].iter().all(|t| valid_table(t))
            // The native grate-limit initialization divides by this slope.
            // Reject degenerate authored hardware rather than creating Inf or
            // silently substituting the default curve.
            && (self.boiler_efficiency.is_empty() || {
                let zero=table_at(&self.boiler_efficiency,0.);
                let hundred=table_at(&self.boiler_efficiency,100.);
                zero>0.0001 && zero<=1. && hundred>=0. && hundred<zero
                    && (zero-hundred)>=0.00001
            })
    }
}
fn valid_table(v: &[(f32, f32)]) -> bool {
    v.is_empty()
        || (v.len() >= 2
            && v.len() <= 4096
            && v.iter()
                .all(|(x, y)| x.is_finite() && y.is_finite() && *x >= 0. && *y >= 0.)
            && v.windows(2).all(|w| w[0].0 < w[1].0))
}
fn table_at(v: &[(f32, f32)], x: f32) -> f32 {
    let i = v.windows(2).position(|w| x < w[1].0).unwrap_or(v.len() - 2);
    let a = (v[i + 1].0 - x) / (v[i + 1].0 - v[i].0);
    let b = (x - v[i].0) / (v[i + 1].0 - v[i].0);
    a * v[i].1 + b * v[i + 1].1
}
fn invalid(name: &str) -> FormatError {
    FormatError::UnexpectedAtom {
        key: name.into(),
        context: "bounded OR1.6.1 steam cycle".into(),
        expected:
            "coal, one simple engine, injector feed, vacuum brake and supported native quantities"
                .into(),
    }
}
fn scalar_strings(values: &[Ast]) -> Vec<String> {
    let mut out = Vec::new();
    for v in values {
        if let Some(s) = super::scalar_text(v) {
            out.push(s);
        } else if let Ast::List(xs) = v {
            out.extend(scalar_strings(xs));
        }
    }
    out
}
fn body(ast: &Ast) -> Option<&Ast> {
    let Ast::List(items) = ast else {
        return None;
    };
    // parse_vehicle_text normalizes native sibling roots and drops the outer
    // Engine keyword; its direct Type(Steam) still identifies this body.
    // parse_named_stf and parenthesized fixtures retain Engine and work too.
    if items.iter().any(|v| {
        matches!(v,Ast::List(xs)
        if matches!(xs.first(),Some(Ast::Atom(Atom::Symbol(k))) if k.eq_ignore_ascii_case("Type"))
        && xs.get(1).and_then(super::scalar_text).is_some_and(|s|s.eq_ignore_ascii_case("Steam")))
    }) {
        return Some(ast);
    }
    items.iter().find_map(body)
}
fn values(ast: &Ast, key: &str) -> Option<Vec<String>> {
    let Ast::List(items) = ast else {
        return None;
    };
    items
        .iter()
        .filter_map(|v| {
            let Ast::List(xs) = v else {
                return None;
            };
            matches!(xs.first(), Some(Ast::Atom(Atom::Symbol(k))) if k.eq_ignore_ascii_case(key))
                .then(|| scalar_strings(&xs[1..]))
        })
        .next_back()
}
#[derive(Clone, Copy)]
enum Unit {
    Scalar,
    Length,
    Mass,
    VolumeFt3,
    AreaM2,
    PressurePsi,
    LbHour,
    EnergyDensity,
    Power,
    Force,
    Time,
}
/// STFReader.ReadFloat parses the numeric token as Single, multiplies by a
/// Double unit factor and then casts back to Single. Keep those two roundings.
fn q(ast: &Ast, key: &str, unit: Unit, default: f32) -> Result<f32, FormatError> {
    let Some(parts) = values(ast, key) else {
        return Ok(default);
    };
    if parts.is_empty() || parts.len() > 2 {
        return Err(invalid(key));
    }
    let raw = parts.concat().to_ascii_lowercase();
    let split = raw
        .find(|c: char| !c.is_ascii_digit() && !matches!(c, '.' | '-' | '+' | 'e'))
        .unwrap_or(raw.len());
    let n = raw[..split].parse::<f32>().map_err(|_| invalid(key))?;
    let suffix = raw[split..].replace(['*', '(', ')', '^'], "");
    let scale: f64 = match (unit, suffix.as_str()) {
        (Unit::Scalar, "")
        | (Unit::Length, "" | "m")
        | (Unit::Mass, "" | "kg")
        | (Unit::VolumeFt3, "" | "ft3")
        | (Unit::PressurePsi, "" | "psi")
        | (Unit::LbHour, "" | "lb/h")
        | (Unit::AreaM2, "m2")
        | (Unit::EnergyDensity, "" | "kj/kg" | "j/g")
        | (Unit::Power, "" | "w")
        | (Unit::Force, "" | "n")
        | (Unit::Time, "" | "s") => 1.,
        (Unit::Length, "cm") => 0.01,
        (Unit::Length, "mm") => 0.001,
        (Unit::Length, "in") => 0.0254,
        (Unit::Length, "ft") => 0.3048,
        (Unit::Mass, "lb") => 0.45359237,
        (Unit::Mass, "t") => 1000.,
        (Unit::VolumeFt3, "m3") => 35.3146667,
        (Unit::VolumeFt3, "l") => 0.0353146667,
        (Unit::VolumeFt3, "in3") => 0.000578703704,
        (Unit::AreaM2, "" | "ft2") => 0.09290304_f32 as f64,
        (Unit::PressurePsi, "bar") => 14.5037738,
        (Unit::PressurePsi, "kpa") => 0.145037738,
        (Unit::PressurePsi, "inhg") => 0.4911542,
        (Unit::PressurePsi, "cmhg") => 0.1933672,
        (Unit::LbHour, "kg/h") => 2.20462,
        (Unit::LbHour, "g/h") => 0.00220462,
        (Unit::EnergyDensity, "btu/lb") => 2.326_f32 as f64,
        (Unit::Power, "kw") => 1000.,
        (Unit::Power, "hp") => 745.699872,
        (Unit::Force, "kn") => 1000.,
        (Unit::Force, "lbf" | "lb") => 4.44822162,
        (Unit::Time, "ms") => 0.001,
        _ => return Err(invalid(key)),
    };
    let result = if scale == 1. {
        n
    } else {
        (n as f64 * scale) as f32
    };
    if !result.is_finite() || result < 0. {
        return Err(invalid(key));
    }
    Ok(result)
}
fn table(ast: &Ast, key: &str) -> Result<Vec<(f32, f32)>, FormatError> {
    let Some(v) = values(ast, key) else {
        return Ok(Vec::new());
    };
    if v.len() < 4 || v.len() % 2 != 0 {
        return Err(invalid(key));
    }
    let nums = v
        .iter()
        .map(|v| v.parse::<f32>().map_err(|_| invalid(key)))
        .collect::<Result<Vec<_>, _>>()?;
    let pairs = nums
        .chunks_exact(2)
        .map(|p| (p[0], p[1]))
        .collect::<Vec<_>>();
    if !valid_table(&pairs) {
        return Err(invalid(key));
    }
    Ok(pairs)
}
/// Call only when selecting the explicit native cycle. No filename/profile
/// identity affects interpretation; unsupported authored equipment is rejected.
pub fn parse_native_steam161(ast: &Ast) -> Result<NativeSteamParams, FormatError> {
    let a = body(ast).ok_or_else(|| invalid("Engine Type Steam"))?;
    for (key, allowed) in [
        ("ORTSSteamLocomotiveType", &["unknown", "simple"][..]),
        ("ORTSSteamLocomotiveFuelType", &["unknown", "coal"][..]),
        (
            "ORTSSteamLocomotiveFeedWaterSystemType",
            &["injectors", "injector", "unknown"][..],
        ),
    ] {
        if values(a, key)
            .is_some_and(|v| !allowed.contains(&v.join("").to_ascii_lowercase().as_str()))
        {
            return Err(invalid(key));
        }
    }
    for key in [
        "ORTSSteamEngines",
        "ORTSSteamGearType",
        "ORTSSteamGearRatio",
        "LPNumCylinders",
    ] {
        if values(a, key).is_some() {
            return Err(invalid(key));
        }
    }
    if !super::field_values(ast, "BrakeSystemType")
        .last()
        .is_some_and(|v| {
            scalar_strings(v)
                .join("")
                .eq_ignore_ascii_case("vacuum_single_pipe")
        })
    {
        return Err(invalid("BrakeSystemType"));
    }
    if values(a, "BrakesTrainBrakeType")
        .is_some_and(|v| v.join("").to_ascii_lowercase().contains("_eq"))
    {
        return Err(invalid(
            "equalizing vacuum reservoir outside bounded manual cycle",
        ));
    }
    let scalar = |key, default| q(a, key, Unit::Scalar, default);
    let size = |key, default| q(a, key, Unit::Length, default);
    let mass = |key, default| q(a, key, Unit::Mass, default);
    let count = scalar("NumCylinders", scalar("NumberOfCylinders", 2.)?)?;
    if count.fract() != 0. || !(1. ..=8.).contains(&count) {
        return Err(invalid("NumCylinders"));
    }
    let boiler_type = match values(a, "ORTSSteamBoilerType")
        .map(|v| v.join("").to_ascii_lowercase())
        .as_deref()
    {
        None => NativeSteamBoilerType::LegacyInferred,
        Some("saturated") => NativeSteamBoilerType::Saturated,
        Some("superheated") => NativeSteamBoilerType::Superheated,
        _ => return Err(invalid("ORTSSteamBoilerType")),
    };
    let controllers = super::field_values(a, "EngineControllers");
    let controller_ast = Ast::List(controllers.last().map_or_else(Vec::new, |v| v.to_vec()));
    let control = |name: &str, index: usize, default: f32| -> Result<f32, FormatError> {
        let Some(v) = values(&controller_ast, name) else {
            return Ok(default);
        };
        v.get(index)
            .ok_or_else(|| invalid(name))?
            .parse::<f32>()
            .map_err(|_| invalid(name))
    };
    let train_ast = super::field_values(&controller_ast, "Brake_Train");
    let train_ast = Ast::List(
        train_ast
            .last()
            .ok_or_else(|| invalid("Brake_Train"))?
            .to_vec(),
    );
    let mut train_brake_notches = Vec::new();
    for values in super::field_values(&train_ast, "Notch") {
        let v = scalar_strings(values);
        if v.len() != 3 {
            return Err(invalid("Brake_Train Notch"));
        }
        let value = v[0]
            .parse::<f32>()
            .map_err(|_| invalid("Brake_Train Notch"))?;
        let smooth = v[1]
            .parse::<u32>()
            .map_err(|_| invalid("Brake_Train Notch"))?
            != 0;
        let kind = match v[2]
            .to_ascii_lowercase()
            .trim_start_matches("trainbrakescontroller")
            .trim_end_matches("start")
        {
            "release" => NativeSteamBrakeKind::Release,
            "fullquickrelease" => NativeSteamBrakeKind::FullQuickRelease,
            "running" => NativeSteamBrakeKind::Running,
            "lap" => NativeSteamBrakeKind::Lap,
            "apply" => NativeSteamBrakeKind::Apply,
            "emergency" => NativeSteamBrakeKind::Emergency,
            _ => return Err(invalid("Brake_Train unsupported notch")),
        };
        train_brake_notches.push(NativeSteamBrakeNotch {
            value,
            smooth,
            kind,
        });
    }
    let requested_vacuum = q(
        a,
        "TrainBrakesControllerMaxSystemPressure",
        Unit::PressurePsi,
        21.,
    )?;
    let maximum_vacuum = if requested_vacuum == 21. || requested_vacuum == 25. {
        (requested_vacuum * 0.03386389) * 14.5037738
    } else if !(7. ..=13.).contains(&requested_vacuum) {
        (21. * 0.03386389) * 14.5037738
    } else {
        requested_vacuum
    };
    let default_if_zero = |value: f32, default: f32| if value == 0. { default } else { value };
    let p = NativeSteamParams {
        hot_start: match values(a, "ORTSRSNativeSteamStartup")
            .map(|v| v.join("").to_ascii_lowercase())
            .as_deref()
        {
            None | Some("or_hot") => true,
            Some("or_cold") => false,
            _ => return Err(invalid("ORTSRSNativeSteamStartup")),
        },
        cylinders: count as u32,
        cylinder_diameter_m: size("CylinderDiameter", 0.)?,
        cylinder_stroke_m: size("CylinderStroke", 0.)?,
        driver_radius_m: size("WheelRadius", 0.)?,
        maximum_pressure_psi: q(a, "MaxBoilerPressure", Unit::PressurePsi, 0.)?,
        boiler_volume_ft3: q(a, "BoilerVolume", Unit::VolumeFt3, 1.)?,
        grate_area_m2: q(a, "ORTSGrateArea", Unit::AreaM2, 0.)?,
        evaporation_area_m2: q(a, "ORTSEvaporationArea", Unit::AreaM2, 0.)?,
        boiler_evaporation_lb_ft2_h: scalar("ORTSBoilerEvaporationRate", 0.)?,
        boiler_type,
        legacy_superheater: scalar("SuperHeater", 1.)?,
        superheater_area_m2: q(a, "ORTSSuperheatArea", Unit::AreaM2, 0.)?,
        maximum_superheat_f: scalar("ORTSMaxSuperheatTemperature", 0.)?,
        superheat_cutoff_pressure_factor: scalar("ORTSSuperheatCutoffPressureFactor", 0.)?,
        cylinder_port_opening: scalar("ORTSCylinderPortOpening", 0.)?,
        cylinder_efficiency: scalar("ORTSCylinderEfficiencyRate", 1.)?,
        tractive_effort_factor: scalar("ORTSTractiveEffortFactor", 0.85)?,
        maximum_ihp: q(a, "ORTSMaxIndicatedHorsepower", Unit::Power, 0.)? * (1. / 745.699872),
        initial_maximum_force_n: q(a, "MaxForce", Unit::Force, 0.)?,
        // MSTSSteamLocomotive does not parse the MSTS MaxFireMass/IdealFireMass
        // tokens; initialization derives both from grate area. Do not reuse
        // the own vessel's interpretation of those legacy tokens here.
        maximum_fire_mass_kg: 0.,
        maximum_firing_rate_kg_s: q(a, "SteamFiremanMaxPossibleFiringRate", Unit::LbHour, 0.)?
            / 2.2046
            / 3600.,
        orts_maximum_firing_rate_kg_s: q(
            a,
            "ORTSSteamFiremanMaxPossibleFiringRate",
            Unit::LbHour,
            0.,
        )? / 2.2046
            / 3600.,
        coal_heat_kj_kg: q(a, "ORTSFuelCalorific", Unit::EnergyDensity, 33400.)?,
        tender_coal_capacity_kg: mass("MaxTenderCoalMass", 1.)?,
        tender_water_capacity_kg: mass("MaxTenderWaterMass", 1.)?,
        tender_required: scalar("IsTenderRequired", 1.)? == 1.,
        boiler_surface_ft2: q(a, "ORTSBoilerSurfaceArea", Unit::AreaM2, 0.)?,
        insulated_fraction: scalar("ORTSFractionBoilerInsulated", 0.)?,
        insulation_kc: scalar("ORTSHeatCoefficientInsulation", 0.)?,
        small_ejector_lb_s: q(a, "VacuumBrakesSmallEjectorUsageRate", Unit::LbHour, 0.)?
            * (1. / 3600.),
        large_ejector_lb_s: q(a, "VacuumBrakesLargeEjectorUsageRate", Unit::LbHour, 0.)?
            * (1. / 3600.),
        vacuum_maximum_boiler_psi: q(
            a,
            "VacuumBrakesMinBoilerPressureMaxVacuum",
            Unit::PressurePsi,
            110.,
        )?,
        vacuum_charging_inhg_s: default_if_zero(scalar("ORTSBrakePipeChargingRate", 0.32)?, 0.32),
        small_ejector_controller_fitted: values(&controller_ast, "ORTSSmallEjector").is_some(),
        large_ejector_controller_fitted: values(&controller_ast, "ORTSLargeEjector").is_some(),
        vacuum_pump_fitted: scalar("VacuumBrakesHasVacuumPump", 0.)? != 0.,
        // Simulator-only native capture does not construct a steam viewer;
        // GeneratorSteamEffects stays false until that separate hook runs.
        generator_fitted: false,
        mechanical_stoker: scalar("SteamFiremanIsMechanicalStoker", 0.)? == 1.,
        sand_forward_steam_lb_s: mass("ORTSMaxTrackSanderSteamConsumptionForward", 0.)?,
        sand_reverse_steam_lb_s: mass("ORTSMaxTrackSanderSteamConsumptionReverse", 0.)?,
        cylinder_cock_diameter_in: 0.5,
        piston_shaft_diameter_in: 3.5,
        cutoff_maximum: control("Cutoff", 1, 0.9)?,
        initial_cutoff: control("Cutoff", 3, 0.)?,
        initial_blower: control("Blower", 3, 0.)?,
        initial_damper: control("DampersFront", 3, 0.)?,
        initial_firing: control("Shovel", 3, 0.)?,
        initial_train_brake: control("Brake_Train", 3, 0.)?,
        maximum_vacuum_psi: maximum_vacuum,
        pipe_time_factor_s: default_if_zero(
            q(a, "ORTSBrakePipeTimeFactor", Unit::Time, 0.0015)?,
            0.02,
        ),
        service_time_factor_s: default_if_zero(
            q(a, "ORTSBrakeServiceTimeFactor", Unit::Time, 10.)?,
            10.,
        ),
        emergency_time_factor_s: default_if_zero(
            q(a, "ORTSBrakeEmergencyTimeFactor", Unit::Time, 1.)?,
            1.,
        ),
        pipe_discharge_multiplier: default_if_zero(
            scalar("ORTSBrakePipeDischargeTimeMult", 1.5)?,
            1.5,
        ),
        train_pipe_leak_psi_s: scalar("TrainPipeLeakRate", 0.)?,
        train_brake_notches,
        boiler_efficiency: table(a, "ORTSBoilerEfficiency")?,
        burn_rate_lb_h: table(a, "ORTSBurnRate")?,
        initial_pressure_drop: table(a, "ORTSCylinderInitialPressureDrop")?,
        backpressure_psi: table(a, "ORTSCylinderBackPressure")?,
        exhaust_event: table(a, "ORTSCylinderEventExhaust")?,
        compression_event: table(a, "ORTSCylinderEventCompression")?,
        admission_event: table(a, "ORTSCylinderEventAdmission")?,
    };
    if !p.valid() {
        return Err(invalid("steam hardware"));
    }
    Ok(p)
}
