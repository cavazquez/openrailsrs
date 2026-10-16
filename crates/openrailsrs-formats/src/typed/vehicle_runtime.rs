//! Authored brake response and effect emitters used by simulation and Bevy.
use super::named_blocks;
use crate::{Ast, Atom};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct VehicleBrakeProfile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub air_circuit: Option<AirCircuitParams>,
    pub system: Option<String>,
    pub max_cylinder_bar: Option<f64>,
    pub application_bar_s: Option<f64>,
    pub release_bar_s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ep: Option<NativeEpBrakeProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_air: Option<NativeAirBrakeProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_vacuum: Option<NativeVacuumBrakeProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_ep: Option<LegacyEpBrakeProfile>,
}

/// Locomotive compressor and independent brake, in SI units. OR 1.6.1
/// MSTSLocomotive defaults apply only when the author omits a value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AirCircuitParams {
    pub maximum_bar: f64,
    pub restart_bar: f64,
    pub reservoir_m3: f64,
    pub charging_bar_s: f64,
    pub application_bar_s: f64,
    pub release_bar_s: f64,
}
impl Default for AirCircuitParams {
    fn default() -> Self {
        Self {
            maximum_bar: 130.0 * 0.0689475729,
            restart_bar: 110.0 * 0.0689475729,
            reservoir_m3: 0.3,
            charging_bar_s: 0.025 * 1.01325 / 0.3,
            application_bar_s: 1.0,
            release_bar_s: 1.0,
        }
    }
}
impl AirCircuitParams {
    pub fn valid(&self) -> bool {
        [
            self.maximum_bar,
            self.restart_bar,
            self.reservoir_m3,
            self.charging_bar_s,
            self.application_bar_s,
            self.release_bar_s,
        ]
        .iter()
        .all(|v| v.is_finite() && *v > 0.0)
            && self.restart_bar <= self.maximum_bar
            && self.maximum_bar <= 100.0
            && self.reservoir_m3 <= 100.0
    }
}

fn parse_air_circuit(ast: &Ast) -> Option<AirCircuitParams> {
    parse_air_circuit_inner(ast, false)
}
fn parse_air_circuit_inner(ast: &Ast, native_train: bool) -> Option<AirCircuitParams> {
    // Native STF normalization removes the outer Engine name but preserves
    // its Brake_Engine controller. Do not lose an explicitly fitted circuit
    // just because the input used native keyword-before-parenthesis syntax.
    let engine_body = |body: &Ast| {
        let Ast::List(fields) = body else {
            return false;
        };
        fields.iter().any(|field| {
            let Ast::List(values) = field else { return false };
            matches!(values.first(), Some(Ast::Atom(Atom::Symbol(key))) if key.eq_ignore_ascii_case("Type"))
                && matches!(values.get(1), Some(Ast::Atom(Atom::Symbol(kind) | Atom::String(kind)))
                    if matches!(kind.to_ascii_lowercase().as_str(), "engine" | "diesel" | "electric" | "steam"))
        })
    };
    let native_engine =
        engine_body(ast) || matches!(ast, Ast::List(roots) if roots.iter().any(engine_body));
    if (named_blocks(ast, "Engine").is_empty()
        && (!native_engine
            || (named_blocks(ast, "Brake_Engine").is_empty()
                && !(native_train
                    && scalar(ast, "BrakeSystemType")
                        .is_some_and(|s| s.eq_ignore_ascii_case("air_single_pipe"))
                    && !named_blocks(ast, "Brake_Train").is_empty()))))
        || scalar(ast, "BrakeSystemType").is_some_and(|s| s.to_ascii_lowercase().contains("vacuum"))
    {
        return None;
    }
    let mut p = AirCircuitParams::default();
    let pressure_field = |key| scalar(ast, key).and_then(pressure);
    let volume = |key| {
        scalar(ast, key).and_then(|s| {
            let raw = s.to_ascii_lowercase().replace(['*', '(', ')', ' '], "");
            let (number, factor) = raw
                .strip_suffix("m^3")
                .or_else(|| raw.strip_suffix("m3"))
                .map_or_else(
                    || {
                        (
                            raw.strip_suffix("ft^3")
                                .or_else(|| raw.strip_suffix("ft3"))
                                .unwrap_or(&raw),
                            0.0283168466,
                        )
                    },
                    |n| (n, 1.0),
                );
            number
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite() && *v > 0.0)
                .map(|v| v * factor)
        })
    };
    p.maximum_bar = pressure_field("AirBrakesMainMaxAirPressure").unwrap_or(p.maximum_bar);
    p.restart_bar = pressure_field("AirBrakesCompressorRestartPressure")
        .unwrap_or(p.restart_bar)
        .min(p.maximum_bar);
    p.reservoir_m3 = volume("ORTSAirBrakeMainResVolume")
        .or_else(|| volume("AirBrakesMainResVolume"))
        .unwrap_or(p.reservoir_m3);
    p.charging_bar_s = pressure_field("ORTSMainResChargingRate").unwrap_or_else(|| {
        volume("AirBrakesAirCompressorPowerRating")
            .unwrap_or(0.025)
            .max(0.025)
            * 1.01325
            / p.reservoir_m3
    });
    if native_train
        && !named_blocks(ast, "Brake_Train").is_empty()
        && scalar(ast, "BrakeSystemType").is_some_and(|s| s.eq_ignore_ascii_case("air_single_pipe"))
    {
        // MSTSLocomotive.OneAtmospherePSI is Bar.ToPSI(1), despite the name.
        // Native main-reservoir charging therefore uses one bar, not 1 atm.
        p.charging_bar_s = pressure_field("ORTSMainResChargingRate").unwrap_or_else(|| {
            volume("AirBrakesAirCompressorPowerRating")
                .unwrap_or(0.025)
                .max(0.025)
                / p.reservoir_m3
        });
    }
    p.application_bar_s =
        pressure_field("ORTSEngineBrakeApplicationRate").unwrap_or(p.application_bar_s);
    p.release_bar_s = pressure_field("ORTSEngineBrakeReleaseRate").unwrap_or(p.release_bar_s);
    if scalar(ast, "ORTSRSAdvancedEmergency").is_some_and(|v| v == "1") {
        p.application_bar_s = pressure_field("EngineBrakesControllerMaxApplicationRate")
            .unwrap_or(p.application_bar_s);
        p.release_bar_s =
            pressure_field("EngineBrakesControllerMaxReleaseRate").unwrap_or(p.release_bar_s);
    }
    Some(p)
}

/// EP cylinders without an authored diameter use OR's legacy pressure model.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LegacyEpBrakeProfile {
    pub charged_psi: f64,
    pub service_psi: f64,
    pub reference_psi: f64,
    pub application_psi_s: f64,
    pub charging_psi_s: f64,
    pub auxiliary_volume_m3: f64,
    pub pipe_volume_m3: Option<f64>,
    pub auxiliary_cylinder_ratio: f64,
    pub distributor: bool,
}

/// Automatic vacuum cylinders/reservoirs from OR 1.6.1 VacuumSinglePipe.
/// Vacuum twin-pipe stock uses the same cylinder implementation in that client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NativeVacuumBrakeProfile {
    pub max_force_psi: f64,
    pub application_psi_s: f64,
    pub release_psi_s: f64,
    pub cylinder_volume_m3: f64,
    pub reservoir_volume_m3: f64,
    pub pipe_volume_m3: Option<f64>,
    pub direct_admission: bool,
    pub charged_vacuum_psi: f64,
    /// Native pipe 3 serves the leading locomotive group, plus a steam tender.
    #[serde(default)]
    pub engine: bool,
    #[serde(default)]
    pub steam: bool,
    #[serde(default)]
    pub tender: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub independent: Option<NativeVacuumController>,
}

/// OR 1.6.1 engine brake controller rates use PSI/s, including vacuum stock.
/// Notches retain their native type; unhandled types hold pipe 3 in OR too.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NativeVacuumController {
    pub application_psi_s: f64,
    pub release_psi_s: f64,
    pub emergency_psi_s: f64,
    /// Position, smooth flag, native state token.
    pub notches: Vec<(f64, bool, String)>,
}
impl NativeVacuumController {
    /// Same floor-notch selection as MSTSNotchController.SetValue. A discrete
    /// notch fixes the valve position; a smooth notch retains the handle value.
    pub fn setting(&self, requested: f64) -> (f64, &str) {
        let requested = requested.clamp(0., 1.) as f32;
        let Some((value, smooth, state)) = self
            .notches
            .iter()
            .rev()
            .find(|(value, _, _)| *value as f32 <= requested)
            .or_else(|| self.notches.first())
        else {
            return (0., "dummy");
        };
        (
            f64::from(if *smooth { requested } else { *value as f32 }),
            state,
        )
    }
}

/// Authored advanced EP hardware, expressed in the native solver's PSI units.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeEpBrakeProfile {
    pub diameter_m: f64,
    pub stroke_m: f64,
    pub spring_psi: f64,
    pub reference_psi: f64,
    pub max_psi: f64,
    pub service_psi: f64,
    pub application_psi_s: f64,
    pub release_psi_s: f64,
    pub auxiliary_volume_m3: f64,
    pub charging_psi_s: f64,
    pub shoe_count: f64,
    pub shoe_type: String,
    pub main_reservoir: bool,
    pub low_stage_psi: Option<f64>,
    pub stage_up_mps: f64,
    pub stage_down_mps: f64,
}
/// UIC distributor with authored cylinders and optional relay valve.
/// This covers normal service/release; emergency equipment remains separate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeAirBrakeProfile {
    /// Original plain-valve stock is enabled as a formation only when its
    /// leading locomotive supplies a supported authored Brake_Train control.
    #[serde(default)]
    pub native_plain: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_cylinder_volume_m3: Option<f64>,
    /// Opt-in supported AirSinglePipe emergency valve/reservoir subset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emergency: Option<NativeEmergencyBrakeProfile>,
    pub cylinder: NativeEpBrakeProfile,
    pub triple_ratio: f64,
    pub relay_ratio: f64,
    pub cylinder_count: f64,
    pub pipe_volume_ratio: f64,
    pub controller: NativeAirController,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeAirController {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notched: Option<NativeTrainBrakeNotches>,
    pub charged_psi: f64,
    pub full_service_drop_psi: f64,
    pub application_psi_s: f64,
    pub release_psi_s: f64,
    #[serde(default = "default_emergency_rate")]
    pub emergency_psi_s: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NativeTrainBrakeNotches {
    pub initial_value: f64,
    pub minimum_reduction_psi: f64,
    pub pipe_charging_psi_s: f64,
    pub pipe_quick_charging_psi_s: f64,
    pub quick_release_psi_s: f64,
    pub overcharge_elimination_psi_s: f64,
    pub slow_application_psi_s: f64,
    pub mechanical_compressor_max_rpm: Option<f64>,
    pub main_reservoir: Option<AirCircuitParams>,
    pub independent_fitted: bool,
    pub notches: Vec<(f64, bool, String)>,
}
impl NativeTrainBrakeNotches {
    pub fn setting(&self, value: f64) -> (f32, &str) {
        let value = value.clamp(0., 1.) as f32;
        let i = self
            .notches
            .iter()
            .rposition(|(v, _, _)| *v as f32 <= value)
            .unwrap_or(0);
        let (start, smooth, name) = &self.notches[i];
        let fraction = if *smooth {
            let end = self.notches.get(i + 1).map_or(1., |p| p.0 as f32);
            (value - *start as f32) / (end - *start as f32).max(f32::EPSILON)
        } else {
            1.
        };
        (
            if *smooth && name == "release" {
                1. - fraction
            } else {
                fraction
            },
            name,
        )
    }
}
fn parse_native_train_brake_notches(ast: &Ast) -> Option<NativeTrainBrakeNotches> {
    let block = named_blocks(ast, "Brake_Train").into_iter().last()?;
    let Ast::List(items) = block else { return None };
    let numbers = items
        .iter()
        .filter_map(|a| match a {
            Ast::Atom(a) => {
                super::atom_to_number(a).or_else(|| super::atom_to_string(a)?.parse::<f64>().ok())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut notches = vec![];
    for block in named_blocks(block, "Notch") {
        let Ast::List(items) = block else { continue };
        let fields = items
            .iter()
            .filter_map(|a| match a {
                Ast::Atom(a) => super::atom_to_string(a)
                    .or_else(|| super::atom_to_number(a).map(|v| v.to_string())),
                _ => None,
            })
            .filter(|s| !s.eq_ignore_ascii_case("Notch"))
            .collect::<Vec<_>>();
        if fields.len() < 3 {
            return None;
        }
        let name = fields[2].to_ascii_lowercase();
        let kind = name
            .strip_prefix("trainbrakescontroller")
            .unwrap_or(&name)
            .trim_end_matches("start");
        let kind = match kind {
            "holdlapped" => "lap",
            "neutralhandleoff" => "neutral",
            v => v,
        };
        if !matches!(
            kind,
            "release"
                | "fullquickrelease"
                | "running"
                | "neutral"
                | "hold"
                | "lap"
                | "minimalreduction"
                | "apply"
                | "fullservice"
                | "continuousservice"
                | "graduatedselflaplimitedholding"
                | "graduatedselflaplimited"
                | "suppression"
                | "emergency"
                | "dummy"
        ) {
            return None;
        }
        notches.push((
            fields[0].parse::<f64>().ok()?,
            fields[1].parse::<i32>().ok()? != 0,
            kind.to_string(),
        ));
    }
    let initial_value = *numbers.get(3)?;
    if notches.is_empty()
        || !initial_value.is_finite()
        || !(0. ..=1.).contains(&initial_value)
        || notches
            .iter()
            .any(|n| !n.0.is_finite() || !(0. ..=1.).contains(&n.0))
        || notches.windows(2).any(|p| p[0].0 >= p[1].0)
    {
        return None;
    }
    let psi = |key, default| {
        super::traction_operation::quantity(ast, key, "psi")
            .ok()
            .flatten()
            .map_or(default, |v| v / 0.0689475729)
    };
    // An arbitrary scripted controller cannot be replaced by the MSTS kernel.
    if scalar(ast, "ORTSTrainBrakeController").is_some_and(|s| !s.eq_ignore_ascii_case("MSTS")) {
        return None;
    }
    Some(NativeTrainBrakeNotches {
        initial_value,
        minimum_reduction_psi: psi("TrainBrakesControllerMinPressureReduction", 6.),
        pipe_charging_psi_s: psi("ORTSBrakePipeChargingRate", 21.),
        pipe_quick_charging_psi_s: psi(
            "ORTSBrakePipeQuickChargingRate",
            psi("ORTSBrakePipeChargingRate", 21.),
        ),
        quick_release_psi_s: psi("TrainBrakesControllerMaxQuickReleaseRate", 10.),
        overcharge_elimination_psi_s: psi(
            "ORTSTrainBrakesControllerOverchargeEliminationRate",
            0.036,
        ),
        slow_application_psi_s: psi("ORTSTrainBrakesControllerSlowApplicationRate", 1.),
        mechanical_compressor_max_rpm: (scalar(ast, "AirBrakesIsCompressorElectricOrMechanical")
            .is_some_and(|v| v == "1")
            && scalar(ast, "Type").is_some_and(|v| v.eq_ignore_ascii_case("Diesel")))
        .then(|| {
            scalar(ast, "DieselEngineMaxRPM")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(600.)
        })
        .filter(|v| v.is_finite() && *v > 0.),
        main_reservoir: parse_air_circuit_inner(ast, true),
        independent_fitted: !named_blocks(ast, "Brake_Engine").is_empty(),
        notches,
    })
}
fn default_emergency_rate() -> f64 {
    20.
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NativeEmergencyBrakeProfile {
    pub distributor: bool,
    pub reservoir_present: bool,
    pub reservoir_volume_m3: f64,
    pub reservoir_charging_psi_s: f64,
    pub maximum_application_psi_s: f64,
    pub actuation_psi_s: f64,
    pub legacy_actuation: bool,
    pub dump_psi_s: f64,
    pub dump_duration_s: f64,
    pub quick_action: bool,
    pub pipe_volume_m3: f64,
    pub pipe_volume_authored: bool,
    pub pipe_time_factor_s: f64,
    pub service_time_factor_s: f64,
    pub emergency_time_factor_s: f64,
    pub sensitivity_psi: f64,
    pub initial_application_psi: f64,
    pub insensitivity_psi_s: f64,
    pub independent_max_psi: f64,
}
impl NativeEmergencyBrakeProfile {
    pub fn valid(&self) -> bool {
        (0.0001..=20.).contains(&self.reservoir_volume_m3)
            && (0. ..=100.).contains(&self.reservoir_charging_psi_s)
            && (0.01..=200.).contains(&self.maximum_application_psi_s)
            && (0. ..=200.).contains(&self.actuation_psi_s)
            && (0. ..=1000.).contains(&self.dump_psi_s)
            && (0. ..=1000.).contains(&self.dump_duration_s)
            && (0.0001..=10.).contains(&self.pipe_volume_m3)
            && (0.0001..=1.).contains(&self.pipe_time_factor_s)
            && (0.0001..=10.).contains(&self.service_time_factor_s)
            && (0.0001..=10.).contains(&self.emergency_time_factor_s)
            && (0. ..=10.).contains(&self.sensitivity_psi)
            && (0. ..=10.).contains(&self.initial_application_psi)
            && (0. ..=10.).contains(&self.insensitivity_psi_s)
            && (0. ..=200.).contains(&self.independent_max_psi)
    }
}
/// Explicit requests must fail before simulation if outside the supported
/// subset, rather than degrading to the old service-only pressure model.
pub fn validate_advanced_emergency(
    ast: &Ast,
    p: &VehicleBrakeProfile,
) -> Result<(), crate::FormatError> {
    if super::traction_operation::quantity(ast, "ORTSRSAdvancedEmergency", "rpm")?.unwrap_or(0.)
        == 0.
    {
        return Ok(());
    }
    let unsupported = [
        "ORTSQuickServiceLimit",
        "ORTSQuickServiceVentRate",
        "ORTSQuickServiceApplicationRate",
        "ORTSAcceleratedApplicationFactor",
        "ORTSEmergencyResQuickRelease",
        "ORTSUniformChargingRatio",
        "ORTSUniformReleaseRatio",
        "ORTSBrakeRelayValveInshot",
    ];
    let equipment = scalar(ast, "BrakeEquipmentType")
        .unwrap_or_default()
        .to_ascii_lowercase();
    for key in [
        "EmergencyResCapacity",
        "BrakePipeVolume",
        "ORTSAuxiliaryResCapacity",
        "ORTSAuxilaryResCapacity",
    ] {
        super::traction_operation::quantity(ast, key, "ft3")?;
    }
    for key in [
        "MaxApplicationRate",
        "EmergencyResChargingRate",
        "ORTSEmergencyValveActuationRate",
        "ORTSEmergencyDumpValveRate",
        "TripleValveSensitivity",
        "ORTSInitialApplicationThreshold",
    ] {
        super::traction_operation::quantity(ast, key, "psi")?;
    }
    if unsupported.iter().any(|k| scalar(ast, k).is_some())
        || equipment.contains("supply_reservoir")
        || p.system
            .as_deref()
            .is_none_or(|s| !s.eq_ignore_ascii_case("air_single_pipe"))
        || p.native_air
            .as_ref()
            .is_none_or(|a| a.emergency.as_ref().is_none_or(|e| !e.valid()) || a.relay_ratio != 1.)
    {
        return Err(crate::FormatError::UnexpectedAtom { key:"ORTSRSAdvancedEmergency".into(),
            context:"pneumatic emergency".into(), expected:"single pipe, authored cylinder, plain triple/distributor, no unsupported auxiliary valves".into() });
    }
    Ok(())
}
impl VehicleBrakeProfile {
    pub fn electro_pneumatic(&self) -> Option<bool> {
        self.system
            .as_ref()
            .map(|s| matches!(s.to_ascii_lowercase().as_str(), "ep" | "epdisc" | "ep_disc"))
    }
}

pub(super) fn scalar(ast: &Ast, key: &str) -> Option<String> {
    let block = named_blocks(ast, key).into_iter().last()?;
    let Ast::List(items) = block else { return None };
    items
        .iter()
        .filter_map(|a| match a {
            Ast::Atom(Atom::String(s) | Atom::Symbol(s)) if !s.eq_ignore_ascii_case(key) => {
                Some(s.clone())
            }
            Ast::Atom(Atom::Number(n)) => Some(n.to_string()),
            Ast::Atom(Atom::Integer(n)) => Some(n.to_string()),
            _ => None,
        })
        .next()
}
pub(super) fn pressure(value: String) -> Option<f64> {
    let value = value.trim().to_ascii_lowercase();
    let end = value
        .find(|c: char| !c.is_ascii_digit() && !matches!(c, '.' | '-' | '+' | 'e'))
        .unwrap_or(value.len());
    let n: f64 = value[..end].parse().ok()?;
    let unit = &value[end..];
    let factor = if unit.starts_with("bar") {
        1.0
    } else if unit.starts_with("kpa") {
        0.01
    } else if unit.starts_with("pa") {
        0.00001
    } else {
        0.0689475729
    };
    (n.is_finite() && n > 0.0).then_some(n * factor)
}
pub fn parse_vehicle_brake_profile(ast: &Ast) -> VehicleBrakeProfile {
    VehicleBrakeProfile {
        air_circuit: parse_air_circuit(ast),
        system: scalar(ast, "BrakeSystemType"),
        max_cylinder_bar: [
            "MaxCylinderPressure",
            "MaxBrakeCylinderPressure",
            "BrakeCylinderPressureForMaxBrakeBrakeForce",
        ]
        .iter()
        .find_map(|k| scalar(ast, k).and_then(pressure)),
        application_bar_s: scalar(ast, "MaxApplicationRate").and_then(pressure),
        release_bar_s: scalar(ast, "MaxReleaseRate").and_then(pressure),
        native_ep: parse_native_ep(ast),
        legacy_ep: parse_legacy_ep(ast),
        native_air: parse_native_air(ast),
        native_vacuum: parse_native_vacuum(ast),
    }
}

fn parse_native_vacuum(ast: &Ast) -> Option<NativeVacuumBrakeProfile> {
    let system = scalar(ast, "BrakeSystemType")?.trim().to_ascii_lowercase();
    if !matches!(system.as_str(), "vacuum_single_pipe" | "vacuum_twin_pipe") {
        return None;
    }
    let psi = |key: &str, default: f64, vacuum_unit: bool| {
        scalar(ast, key)
            .and_then(|s| {
                let raw = s.trim().to_ascii_lowercase();
                let per_minute = raw.ends_with("/min");
                let raw = raw.trim_end_matches("/s").trim_end_matches("/min");
                let end = raw
                    .find(|c: char| !c.is_ascii_digit() && !matches!(c, '.' | '-' | '+' | 'e'))
                    .unwrap_or(raw.len());
                let number: f64 = raw[..end].parse().ok()?;
                let factor = match raw[end..].trim() {
                    "" if vacuum_unit => 0.4911542,
                    "" | "psi" => 1.,
                    "inhg" => 0.4911542,
                    "bar" => 14.5037738,
                    "kpa" if key.ends_with("Rate") => 0.145,
                    "kpa" => 0.145037738,
                    _ => return None,
                };
                let scale = if per_minute { factor / 60. } else { factor };
                (number.is_finite() && number > 0.)
                    .then_some(f64::from((f64::from(number as f32) * scale) as f32))
            })
            .unwrap_or(default)
    };
    let length = |keys: &[&str], default| {
        keys.iter()
            .find_map(|k| scalar(ast, k).and_then(|s| crate::msts_units::parse_length_m(&s)))
            .unwrap_or(default)
    };
    let volume = |key| {
        super::traction_operation::quantity(ast, key, "ft3")
            .ok()
            .flatten()
            .filter(|v| *v > 0.)
            .map(|v| v / 1000.)
    };
    let diameter = length(
        &["ORTSBrakeCylinderSize", "ORTSBrakeCylinderDiameter"],
        0.4572,
    );
    let stroke = length(&["ORTSBrakeCylinderPistonTravel"], 0.1143);
    let count = scalar(ast, "ORTSNumberBrakeCylinders")
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(2);
    let direct_type = |block: &Ast| {
        let Ast::List(items) = block else {
            return None;
        };
        let is_type =
            |a: &Ast| matches!(a, Ast::Atom(Atom::Symbol(s)) if s.eq_ignore_ascii_case("Type"));
        let field = items
            .windows(2)
            .find_map(|pair| {
                (is_type(&pair[0]) && matches!(&pair[1], Ast::List(_))).then_some(&pair[1])
            })
            .or_else(|| {
                items
                    .iter()
                    .find(|a| matches!(a, Ast::List(values) if values.first().is_some_and(is_type)))
            })?;
        let Ast::List(values) = field else {
            return None;
        };
        values.iter().find_map(|a| match a {
            Ast::Atom(Atom::Symbol(s) | Atom::String(s)) if !s.eq_ignore_ascii_case("Type") => {
                Some(s.clone())
            }
            _ => None,
        })
    };
    // Native STF normalization retains the root body, while parenthesized
    // fixtures also retain its keyword. Coupling(Type(Chain)) is not the
    // vehicle's direct Type(Tender).
    let Ast::List(items) = ast else { return None };
    let roots: Vec<_> = if items.first().is_some_and(|a| matches!(a, Ast::Atom(_))) {
        vec![ast]
    } else {
        items.iter().collect()
    };
    let root_types: Vec<_> = roots.into_iter().filter_map(direct_type).collect();
    let engine = root_types.iter().any(|s| {
        matches!(
            s.to_ascii_lowercase().as_str(),
            "steam" | "diesel" | "electric" | "engine"
        )
    }) || !named_blocks(ast, "Brake_Engine").is_empty();
    let independent = engine
        .then(|| {
            let controller = named_blocks(ast, "Brake_Engine").into_iter().last()?;
            let mut notches = Vec::new();
            for notch in named_blocks(controller, "Notch") {
                let Ast::List(items) = notch else { continue };
                let atoms: Vec<_> = items
                    .iter()
                    .filter_map(|a| match a {
                        Ast::Atom(Atom::String(s) | Atom::Symbol(s)) => Some(s.clone()),
                        Ast::Atom(Atom::Number(n)) => Some(n.to_string()),
                        Ast::Atom(Atom::Integer(n)) => Some(n.to_string()),
                        _ => None,
                    })
                    .collect();
                let first = usize::from(
                    atoms
                        .first()
                        .is_some_and(|s| s.eq_ignore_ascii_case("Notch")),
                );
                let value = atoms.get(first)?.parse::<f64>().ok()?;
                if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                    return None;
                }
                let smooth = atoms.get(first + 1)?.parse::<u32>().ok()? != 0;
                notches.push((value, smooth, atoms.get(first + 2)?.to_ascii_lowercase()));
            }
            if notches.is_empty() || !notches.windows(2).all(|w| w[0].0 < w[1].0) {
                return None;
            }
            Some(NativeVacuumController {
                application_psi_s: psi("EngineBrakesControllerMaxApplicationRate", 2., false),
                release_psi_s: psi("EngineBrakesControllerMaxReleaseRate", 5., false),
                emergency_psi_s: psi("EngineBrakesControllerEmergencyApplicationRate", 10., false),
                notches,
            })
        })
        .flatten();
    // MSTSLocomotive.Initialize repairs the legacy mixed units for vacuum:
    // 21/25 mean inHg; values outside 7..13 PSI fall back to 21 inHg.
    let requested_vacuum = psi("TrainBrakesControllerMaxSystemPressure", 90., false) as f32;
    let charged_vacuum_psi = if requested_vacuum == 21. || requested_vacuum == 25. {
        f64::from(requested_vacuum * 0.03386389_f32 * 14.503_774_f32)
    } else if !(7.0..=13.0).contains(&requested_vacuum) {
        f64::from(21_f32 * 0.03386389_f32 * 14.503_774_f32)
    } else {
        f64::from(requested_vacuum)
    };
    Some(NativeVacuumBrakeProfile {
        max_force_psi: psi(
            "BrakeCylinderPressureForMaxBrakeBrakeForce",
            21. * 0.491154152,
            true,
        ),
        application_psi_s: psi("MaxApplicationRate", 2.5, true),
        release_psi_s: psi("MaxReleaseRate", 2.5, true),
        cylinder_volume_m3: std::f64::consts::PI * diameter.powi(2) * stroke * f64::from(count)
            / 4.,
        reservoir_volume_m3: volume("ORTSAuxiliaryResCapacity")
            .or_else(|| volume("ORTSAuxilaryResCapacity"))
            .unwrap_or(std::f64::consts::PI * 12f64.powi(2) * 16. * 0.000016387064),
        pipe_volume_m3: volume("BrakePipeVolume"),
        direct_admission: scalar(ast, "ORTSDirectAdmissionValve").is_some_and(|s| s == "1"),
        charged_vacuum_psi,
        engine,
        steam: engine && root_types.iter().any(|s| s.eq_ignore_ascii_case("Steam")),
        tender: !engine && root_types.iter().any(|s| s.eq_ignore_ascii_case("Tender")),
        independent,
    })
}

fn parse_legacy_ep(ast: &Ast) -> Option<LegacyEpBrakeProfile> {
    if !scalar(ast, "BrakeSystemType")?.eq_ignore_ascii_case("EP")
        || scalar(ast, "ORTSBrakeCylinderDiameter").is_some()
        || scalar(ast, "ORTSEPBrakeControlsBrakePipe").is_some_and(|v| v != "0")
    {
        return None;
    }
    let psi = |key| {
        scalar(ast, key)
            .and_then(pressure)
            .map(|b| b / 0.0689475729)
    };
    let volume = |key| {
        super::traction_operation::quantity(ast, key, "ft3")
            .ok()
            .flatten()
            .filter(|v| *v > 0.)
            .map(|v| v / 1000.)
    };
    let number = |key| {
        scalar(ast, key)
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v > 0.)
    };
    let reference_psi = psi("ORTSBrakeForceReferencePressure")
        .or_else(|| psi("BrakeCylinderPressureForMaxBrakeBrakeForce"))
        .unwrap_or(64.);
    Some(LegacyEpBrakeProfile {
        charged_psi: psi("TrainBrakesControllerMaxSystemPressure").unwrap_or(90.),
        service_psi: psi("ORTSMaxServiceCylinderPressure")
            .or_else(|| psi("BrakeCylinderPressureForMaxBrakeBrakeForce"))
            .unwrap_or(64.),
        reference_psi,
        application_psi_s: psi("ORTSMaxServiceApplicationRate")
            .or_else(|| psi("MaxApplicationRate"))
            .unwrap_or(0.9),
        charging_psi_s: psi("MaxAuxilaryChargingRate")
            .or_else(|| psi("MaxAuxiliaryChargingRate"))
            .unwrap_or(1.684),
        auxiliary_volume_m3: volume("ORTSAuxiliaryResCapacity")
            .unwrap_or(0.07 / number("EmergencyResVolumeMultiplier").unwrap_or(1.4)),
        pipe_volume_m3: volume("BrakePipeVolume"),
        auxiliary_cylinder_ratio: number("TripleValveRatio").unwrap_or(2.5),
        distributor: scalar(ast, "BrakeEquipmentType").is_some_and(|v| {
            v.to_ascii_lowercase().contains("distributor")
                || v.to_ascii_lowercase()
                    .contains("graduated_release_triple_valve")
        }),
    })
}

fn parse_native_ep(ast: &Ast) -> Option<NativeEpBrakeProfile> {
    if !scalar(ast, "BrakeSystemType")?.eq_ignore_ascii_case("EP")
        || scalar(ast, "ORTSEPBrakeControlsBrakePipe")?
            .parse::<u32>()
            .ok()?
            != 0
    {
        return None;
    }
    parse_native_cylinder(ast)
}

fn parse_native_cylinder(ast: &Ast) -> Option<NativeEpBrakeProfile> {
    let psi = |key: &str| {
        scalar(ast, key)
            .and_then(pressure)
            .map(|bar| bar / 0.0689475729)
    };
    let length = |key: &str| scalar(ast, key).and_then(|s| crate::msts_units::parse_length_m(&s));
    let speed =
        |key: &str| scalar(ast, key).and_then(|s| crate::msts_units::parse_velocity_mps(&s));
    let diameter_m = length("ORTSBrakeCylinderDiameter")?;
    let stroke_m = length("ORTSBrakeCylinderPistonTravel")?;
    let max_psi = psi("BrakeCylinderPressureForMaxBrakeBrakeForce")?;
    let reference_psi = psi("ORTSBrakeForceReferencePressure").unwrap_or(max_psi);
    let spring_psi = psi("ORTSCylinderSpringPressure").unwrap_or(5.);
    if ![diameter_m, stroke_m, max_psi, reference_psi, spring_psi]
        .iter()
        .all(|v| v.is_finite() && *v > 0.)
        || diameter_m > 2.
        || stroke_m > 2.
        || spring_psi >= reference_psi
        || spring_psi >= 45.
    {
        return None;
    }
    let auxiliary_volume_m3 = scalar(ast, "ORTSAuxiliaryResCapacity")
        .and_then(|s| {
            let end = s
                .find(|c: char| !c.is_ascii_digit() && !matches!(c, '.' | '+' | '-' | 'e'))
                .unwrap_or(s.len());
            let value: f64 = s[..end].parse().ok()?;
            let unit = s[end..].trim().to_ascii_lowercase();
            let factor = if unit.starts_with("in") {
                0.0254f64.powi(3)
            } else if unit.starts_with("ft") {
                0.3048f64.powi(3)
            } else {
                1.
            };
            (value.is_finite() && value > 0.).then_some(value * factor)
        })
        .unwrap_or(0.07);
    Some(NativeEpBrakeProfile {
        diameter_m,
        stroke_m,
        max_psi,
        reference_psi,
        spring_psi,
        service_psi: psi("ORTSMaxServiceCylinderPressure").unwrap_or(max_psi),
        application_psi_s: psi("ORTSMaxServiceApplicationRate")
            .or_else(|| psi("MaxApplicationRate"))
            .unwrap_or(3.),
        release_psi_s: psi("MaxReleaseRate").unwrap_or(10.),
        auxiliary_volume_m3,
        charging_psi_s: psi("MaxAuxiliaryChargingRate").unwrap_or(2.),
        shoe_count: scalar(ast, "ORTSNumberCarBrakeShoes")
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v > 0.)
            .unwrap_or_else(|| {
                let axles = |key| {
                    scalar(ast, key)
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.)
                };
                (4. * (axles("ORTSNumberAxles") + axles("ORTSNumberDriveAxles"))).max(8.)
            }),
        shoe_type: scalar(ast, "ORTSBrakeShoeType").unwrap_or_else(|| "Cast_Iron_P6".into()),
        main_reservoir: !named_blocks(ast, "Engine").is_empty(),
        low_stage_psi: psi("ORTSTwoStageLowPressure"),
        stage_up_mps: speed("ORTSTwoStageIncreasingSpeed").unwrap_or(0.),
        stage_down_mps: speed("ORTSTwoStageDecreasingSpeed").unwrap_or(0.),
    })
}

fn parse_native_air(ast: &Ast) -> Option<NativeAirBrakeProfile> {
    let system = scalar(ast, "BrakeSystemType")?.to_ascii_lowercase();
    let advanced = scalar(ast, "ORTSRSAdvancedEmergency")
        .is_some_and(|s| s.parse::<f64>().is_ok_and(|v| v > 0.));
    let equipment = scalar(ast, "BrakeEquipmentType")?.to_ascii_lowercase();
    let tokens = equipment.split(',').map(str::trim).collect::<Vec<_>>();
    let native_plain = !advanced
        && system == "air_single_pipe"
        && tokens.contains(&"triple_valve")
        && tokens.iter().all(|t| {
            matches!(
                *t,
                "triple_valve"
                    | "handbrake"
                    | "auxilary_reservoir"
                    | "auxiliary_reservoir"
                    | "emergency_brake_reservoir"
            )
        })
        && [
            "ORTSQuickServiceLimit",
            "ORTSQuickServiceVentRate",
            "ORTSQuickServiceApplicationRate",
            "ORTSAcceleratedApplicationFactor",
            "ORTSEmergencyResQuickRelease",
            "ORTSUniformChargingRatio",
            "ORTSUniformReleaseRatio",
            "ORTSBrakeRelayValveInshot",
        ]
        .iter()
        .all(|k| scalar(ast, k).is_none());
    if !matches!(system.as_str(), "air_twin_pipe" | "air_single_pipe")
        || !(equipment.contains("distributor")
            || (advanced || native_plain) && equipment.contains("triple_valve"))
    {
        return None;
    }
    let authored_cylinder = parse_native_cylinder(ast);
    let number = |key: &str, default: f64| {
        scalar(ast, key)
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v > 0.)
            .unwrap_or(default)
    };
    let psi = |key: &str, default: f64| {
        scalar(ast, key)
            .and_then(pressure)
            .map(|bar| bar / 0.0689475729)
            .unwrap_or(default)
    };
    let linear = (advanced || native_plain) && authored_cylinder.is_none();
    let legacy_aux_volume =
        super::traction_operation::quantity(ast, "ORTSAuxiliaryResCapacity", "ft3")
            .ok()
            .flatten()
            .map(|v| v / 1000.)
            .unwrap_or_else(|| {
                super::traction_operation::quantity(ast, "EmergencyResCapacity", "ft3")
                    .ok()
                    .flatten()
                    .map(|v| v / 1000.)
                    .unwrap_or(0.07)
                    / number("EmergencyResVolumeMultiplier", 1.4)
            });
    let mut cylinder = authored_cylinder.or_else(|| {
        linear.then(|| NativeEpBrakeProfile {
            diameter_m: 0.,
            stroke_m: 0.,
            spring_psi: 0.,
            reference_psi: psi(
                "ORTSBrakeForceReferencePressure",
                psi("BrakeCylinderPressureForMaxBrakeBrakeForce", 64.),
            ),
            max_psi: psi("BrakeCylinderPressureForMaxBrakeBrakeForce", 64.),
            service_psi: psi(
                "ORTSMaxServiceCylinderPressure",
                psi("BrakeCylinderPressureForMaxBrakeBrakeForce", 64.),
            ),
            application_psi_s: psi(
                "ORTSMaxServiceApplicationRate",
                psi("MaxApplicationRate", 0.9),
            ),
            release_psi_s: psi("MaxReleaseRate", 1.86),
            auxiliary_volume_m3: legacy_aux_volume,
            charging_psi_s: psi("MaxAuxilaryChargingRate", 1.684),
            shoe_count: number("ORTSNumberCarBrakeShoes", 8.),
            shoe_type: scalar(ast, "ORTSBrakeShoeType").unwrap_or_default(),
            main_reservoir: false,
            low_stage_psi: None,
            stage_up_mps: 0.,
            stage_down_mps: 0.,
        })
    })?;
    if advanced || native_plain {
        let q = |key, default| {
            super::traction_operation::quantity(ast, key, "psi")
                .ok()
                .flatten()
                .map(|v| v / 0.0689475729)
                .unwrap_or(default)
        };
        cylinder.application_psi_s = q(
            "ORTSMaxServiceApplicationRate",
            q("MaxApplicationRate", 0.9),
        );
        cylinder.release_psi_s = q("MaxReleaseRate", 1.86);
        cylinder.charging_psi_s = q(
            "MaxAuxiliaryChargingRate",
            q("MaxAuxilaryChargingRate", 1.684),
        );
    }
    let triple_ratio = number("TripleValveRatio", 2.5);
    let relay_ratio = number("ORTSBrakeRelayValveRatio", 1.);
    let cylinder_count = number("ORTSNumberBrakeCylinders", 1.);
    let fixed_cylinder_volume_m3 =
        linear.then_some(cylinder.auxiliary_volume_m3 / triple_ratio / cylinder_count);
    let cylinder_volume = fixed_cylinder_volume_m3
        .unwrap_or(std::f64::consts::PI * cylinder.diameter_m.powi(2) / 4. * cylinder.stroke_m);
    let nominal_cylinder = 70. * triple_ratio / (triple_ratio + 1.);
    let piping_volume = if linear {
        0.
    } else if cylinder.main_reservoir || relay_ratio != 1. {
        cylinder_volume * 0.2
    } else {
        ((-cylinder_volume * 14.696
            - nominal_cylinder * (cylinder_volume + cylinder.auxiliary_volume_m3)
            + 70. * cylinder.auxiliary_volume_m3)
            / (cylinder_count * nominal_cylinder))
            .max(cylinder_volume * 0.05)
    };
    // Air cylinders are fed from the auxiliary reservoir or a pneumatic relay;
    // the EP-only wire and its MainRes release path do not apply.
    cylinder.main_reservoir = relay_ratio != 1.;
    let emergency = (advanced || native_plain).then(|| {
        let volume = |key: &str, default| {
            super::traction_operation::quantity(ast, key, "ft3")
                .ok()
                .flatten()
                .map(|v| v / 1000.)
                .unwrap_or(default)
        };
        let present = equipment.contains("emergency_brake_reservoir");
        let explicit_rate = scalar(ast, "ORTSEmergencyValveActuationRate").is_some();
        let psi = |key: &str, default| {
            super::traction_operation::quantity(ast, key, "psi")
                .ok()
                .flatten()
                .map(|v| v / 0.0689475729)
                .unwrap_or(default)
        };
        NativeEmergencyBrakeProfile {
            distributor: equipment.contains("distributor"),
            reservoir_present: present,
            reservoir_volume_m3: volume("EmergencyResCapacity", 0.07),
            reservoir_charging_psi_s: psi("EmergencyResChargingRate", 1.684),
            maximum_application_psi_s: psi("MaxApplicationRate", 0.9),
            actuation_psi_s: psi(
                "ORTSEmergencyValveActuationRate",
                if present { 15. } else { 0. },
            ),
            legacy_actuation: present && !explicit_rate,
            dump_psi_s: psi("ORTSEmergencyDumpValveRate", 0.),
            dump_duration_s: scalar(ast, "ORTSEmergencyDumpValveTimer")
                .and_then(|v| v.trim_end_matches('s').parse().ok())
                .unwrap_or(120.),
            quick_action: scalar(ast, "ORTSEmergencyQuickAction")
                .is_some_and(|v| v.eq_ignore_ascii_case("true") || v == "1"),
            pipe_volume_m3: volume(
                "BrakePipeVolume",
                0.032f64.powi(2) * std::f64::consts::PI / 4.
                    * scalar(ast, "ORTSLengthCouplerFace")
                        .and_then(|v| crate::msts_units::parse_length_m(&v))
                        .unwrap_or(21.)
                        .max(5.),
            ),
            pipe_volume_authored: scalar(ast, "BrakePipeVolume").is_some(),
            pipe_time_factor_s: scalar(ast, "ORTSBrakePipeTimeFactor")
                .and_then(|v| v.trim_end_matches('s').parse().ok())
                .unwrap_or(0.0015),
            service_time_factor_s: scalar(ast, "ORTSBrakeServiceTimeFactor")
                .and_then(|v| v.trim_end_matches('s').parse().ok())
                .unwrap_or(1.009),
            emergency_time_factor_s: scalar(ast, "ORTSBrakeEmergencyTimeFactor")
                .and_then(|v| v.trim_end_matches('s').parse().ok())
                .unwrap_or(0.1),
            sensitivity_psi: psi(
                "TripleValveSensitivity",
                if equipment.contains("distributor") {
                    1.4
                } else {
                    1.
                },
            ),
            initial_application_psi: psi(
                "ORTSInitialApplicationThreshold",
                if equipment.contains("distributor") {
                    2.2
                } else {
                    1.
                },
            ),
            insensitivity_psi_s: psi("ORTSBrakeInsensitivity", 0.07),
            independent_max_psi: psi("EngineBrakesControllerMaxSystemPressure", cylinder.max_psi),
        }
    });
    Some(NativeAirBrakeProfile {
        native_plain,
        fixed_cylinder_volume_m3,
        emergency,
        cylinder,
        triple_ratio,
        relay_ratio,
        cylinder_count,
        pipe_volume_ratio: piping_volume / cylinder_volume,
        controller: NativeAirController {
            notched: native_plain
                .then(|| parse_native_train_brake_notches(ast))
                .flatten(),
            charged_psi: psi(
                "TrainBrakesControllerMaxSystemPressure",
                if native_plain { 90. } else { 72.51887 },
            ),
            full_service_drop_psi: psi(
                "TrainBrakesControllerFullServicePressureDrop",
                if native_plain { 26. } else { 21.755661 },
            ),
            application_psi_s: psi(
                "TrainBrakesControllerMaxApplicationRate",
                if native_plain { 2. } else { 5.80151 },
            ),
            release_psi_s: psi(
                "TrainBrakesControllerMaxReleaseRate",
                if native_plain { 5. } else { 7.251887 },
            ),
            emergency_psi_s: psi(
                "TrainBrakesControllerEmergencyApplicationRate",
                if native_plain { 10. } else { 20. },
            ),
        },
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct VehicleEmitter {
    pub name: String,
    /// Original MSTS vehicle coordinates and direction (converted by the viewer).
    pub position: [f32; 3],
    pub direction: [f32; 3],
    pub radius_m: f32,
}
pub fn parse_vehicle_emitters(ast: &Ast) -> Vec<VehicleEmitter> {
    let mut out = Vec::new();
    for family in [
        "DieselSpecialEffects",
        "SteamSpecialEffects",
        "SanderSpecialEffects",
    ] {
        for block in named_blocks(ast, family) {
            let Ast::List(items) = block else { continue };
            let items = if matches!(items.first(),Some(Ast::Atom(Atom::Symbol(name))) if name.eq_ignore_ascii_case(family))
            {
                &items[1..]
            } else {
                items.as_slice()
            };
            let mut emit = |name: &str, payload: &[Ast]| {
                let nums: Vec<f32> = payload
                    .iter()
                    .filter_map(|a| match a {
                        Ast::Atom(Atom::Number(n)) => Some(*n as f32),
                        Ast::Atom(Atom::Integer(n)) => Some(*n as f32),
                        _ => None,
                    })
                    .collect();
                if nums.len() == 7 && nums.iter().all(|n| n.is_finite()) && nums[6] > 0.0 {
                    out.push(VehicleEmitter {
                        name: name.into(),
                        position: [nums[0], nums[1], nums[2]],
                        direction: [nums[3], nums[4], nums[5]],
                        radius_m: nums[6],
                    });
                }
            };
            // A native STF family containing one emitter normalizes to its
            // headed child payload. Keep that child's name and numeric body.
            if let Some(Ast::Atom(Atom::Symbol(name))) = items.first() {
                emit(name, &items[1..]);
            }
            for pair in items.windows(2) {
                if let (Ast::Atom(Atom::Symbol(name)), Ast::List(payload)) = (&pair[0], &pair[1]) {
                    emit(name, payload);
                }
            }
            for item in items {
                if let Ast::List(payload) = item
                    && let Some(Ast::Atom(Atom::Symbol(name))) = payload.first()
                {
                    emit(name, &payload[1..]);
                }
            }
        }
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plain_train_controller_preserves_authored_native_handles_and_main_reservoir() {
        let source =
            include_str!("../../../../examples/physics_lab/vehicles/plain_train_control.eng");
        let ast = crate::parse_vehicle_text(source).unwrap();
        let p = parse_vehicle_brake_profile(&ast);
        // Adding a train pipe feed must not invent an independent controller
        // or change the historical generic air-circuit metadata.
        assert!(p.air_circuit.is_none());
        let a = p.native_air.unwrap();
        assert!(a.native_plain && a.emergency.is_some());
        let n = a.controller.notched.unwrap();
        assert_eq!(n.initial_value, 0.7);
        assert!(!n.independent_fitted);
        assert_eq!(n.mechanical_compressor_max_rpm, Some(900.));
        assert_eq!(n.notches.len(), 4);
        assert_eq!(n.setting(0.552).1, "graduatedselflaplimitedholding");
        assert!((n.setting(0.552).0 - 0.704).abs() < 0.000001);
        let mr = n.main_reservoir.unwrap();
        assert!((mr.maximum_bar / 0.0689475729 - 130.).abs() < 1e-8);
        assert!((mr.charging_bar_s / 0.0689475729 - 3.082052).abs() < 0.00001);
        let unsupported = source.replace(
            "GraduatedSelfLapLimitedHoldingStart",
            "ArbitraryScriptStart",
        );
        let a = parse_vehicle_brake_profile(&crate::parse_vehicle_text(&unsupported).unwrap())
            .native_air
            .unwrap();
        assert!(a.controller.notched.is_none());
        let unsafe_valve = source.replace("Triple_valve,", "Triple_valve, Supply_reservoir,");
        assert!(
            parse_vehicle_brake_profile(&crate::parse_vehicle_text(&unsafe_valve).unwrap())
                .native_air
                .is_none()
        );
    }
    #[test]
    fn native_air_independent_controller_survives_normalized_engine_roots() {
        let ast = crate::parse_vehicle_text(include_str!(
            "../../../../examples/physics_lab/vehicles/hydraulic.eng"
        ))
        .unwrap();
        let p = parse_vehicle_brake_profile(&ast);
        let circuit = p
            .air_circuit
            .expect("Explicit native Brake_Engine was lost");
        assert!(circuit.valid());
        assert!((circuit.application_bar_s - 20. * 0.0689475729).abs() < 1e-8);
        assert!((circuit.release_bar_s - 20. * 0.0689475729).abs() < 1e-8);
        let wagon = crate::parse_vehicle_text(
            "Wagon ( own Type ( Freight ) BrakeSystemType ( Air_single_pipe ) Brake_Engine ( 0 1 0.1 0 ) )"
        ).unwrap();
        assert!(parse_vehicle_brake_profile(&wagon).air_circuit.is_none());
    }
    #[test]
    fn native_vacuum_independent_preserves_native_rates_notches_and_roles() {
        let ast = crate::parse_vehicle_text("Wagon ( hall Type ( Engine ) BrakeSystemType ( Vacuum_single_pipe ) Coupling ( Type ( Chain ) ) ) Engine ( hall Type ( Steam ) EngineBrakesControllerMaxApplicationRate ( 2 ) EngineBrakesControllerMaxReleaseRate ( 1bar/s ) EngineControllers ( Brake_Engine ( 0 1 0.1 0 NumNotches ( 2 Notch ( 0 0 EngineBrakesControllerReleaseStart ) Notch ( 0.2 1 EngineBrakesControllerVacuumContinuousServiceStart ) ) ) ) )").unwrap();
        let profile = parse_vehicle_brake_profile(&ast).native_vacuum.unwrap();
        assert!(profile.engine && profile.steam && !profile.tender);
        let controller = profile.independent.unwrap();
        assert_eq!(controller.application_psi_s, 2.);
        assert!((controller.release_psi_s - 1. / 0.0689475729).abs() < 0.000003);
        assert_eq!(controller.emergency_psi_s, 10.);
        assert_eq!(controller.notches.len(), 2);
        assert_eq!(controller.notches[1].0, 0.2);
        assert_eq!(
            controller.notches[1].2,
            "enginebrakescontrollervacuumcontinuousservicestart"
        );
        let tender = crate::parse_vehicle_text("Wagon ( tender Type ( Tender ) BrakeSystemType ( Vacuum_single_pipe ) Coupling ( Type ( Chain ) ) )").unwrap();
        let profile = parse_vehicle_brake_profile(&tender).native_vacuum.unwrap();
        assert!(profile.tender && !profile.engine && !profile.steam);
        assert!(profile.independent.is_none());
    }

    #[test]
    fn native_vacuum_independent_does_not_invent_a_controller_or_rewrite_dummy() {
        for definition in [
            "",
            "EngineBrakesControllerMaxApplicationRate ( 3 )",
            "EngineControllers ( Brake_Engine ( 0 1 0.1 0 NumNotches ( 2 Notch ( 0.5 0 Dummy ) Notch ( 0.2 0 Dummy ) ) ) )",
        ] {
            let ast = crate::parse_vehicle_text(&format!(
                "Engine ( x Type ( Diesel ) BrakeSystemType ( Vacuum_single_pipe ) {definition} )"
            ))
            .unwrap();
            assert!(
                parse_vehicle_brake_profile(&ast)
                    .native_vacuum
                    .unwrap()
                    .independent
                    .is_none()
            );
        }
        let ast = crate::parse_vehicle_text("Engine ( x Type ( Diesel ) BrakeSystemType ( Vacuum_single_pipe ) EngineControllers ( Brake_Engine ( 0 1 0.1 0 NumNotches ( 1 Notch ( 0 1 Dummy ) ) ) ) )").unwrap();
        let controller = parse_vehicle_brake_profile(&ast)
            .native_vacuum
            .unwrap()
            .independent
            .unwrap();
        assert_eq!(controller.notches, vec![(0., true, "dummy".into())]);
    }

    #[test]
    fn native_vacuum_independent_matches_original_mixed_unit_normalization() {
        for (field, expected) in [
            ("", 21. * 0.491154152),
            ("21", 21. * 0.491154152),
            ("25", 25. * 0.491154152),
            ("90", 21. * 0.491154152),
            ("6psi", 21. * 0.491154152),
            ("7psi", 7.),
            ("13psi", 13.),
            ("13.1psi", 21. * 0.491154152),
            ("10.5psi", 10.5),
            ("25InHg", 25. * 0.491154152),
        ] {
            let field = if field.is_empty() {
                String::new()
            } else {
                format!("TrainBrakesControllerMaxSystemPressure ( {field} )")
            };
            let ast = crate::parse_vehicle_text(&format!(
                "Engine ( x Type ( Diesel ) BrakeSystemType ( Vacuum_twin_pipe ) {field} )"
            ))
            .unwrap();
            let p = parse_vehicle_brake_profile(&ast).native_vacuum.unwrap();
            assert!(
                (p.charged_vacuum_psi - expected).abs() < 0.000003,
                "{field}"
            );
        }
    }

    #[test]
    fn native_stf_exhaust_keeps_its_named_family_and_seven_values() {
        let ast = crate::parse_vehicle_text("SIMISA@@@@@@@@@@JINX0D0t______\nWagon ( power Mass ( 40t ) ) Engine ( power Effects ( DieselSpecialEffects ( Exhaust1 ( 0.54 3.81 4.19 0 1 0 0.24 ) ) ) )").unwrap();
        let emitters = parse_vehicle_emitters(&ast);
        assert_eq!(emitters.len(), 1, "{ast:?}");
        assert_eq!(emitters[0].name, "Exhaust1");
        assert_eq!(emitters[0].position, [0.54, 3.81, 4.19]);
    }
    #[test]
    fn native_rates_and_repeated_emitters_keep_units_and_positions() {
        let a=crate::parse_vehicle_text("Wagon ( x BrakeSystemType ( EP ) BrakeCylinderPressureForMaxBrakeBrakeForce ( 45psi ) MaxApplicationRate ( 30psi/s ) MaxReleaseRate ( 10psi/s ) Effects ( SteamSpecialEffects ( CylindersFX ( 1 .7 4 0 0 1 .1 ) CylindersFX ( -1 .7 4 0 0 1 .1 ) ) ) )").unwrap();
        let p = parse_vehicle_brake_profile(&a);
        assert_eq!(p.electro_pneumatic(), Some(true));
        assert!((p.max_cylinder_bar.unwrap() - 3.10264078).abs() < 1e-6);
        assert!((p.application_bar_s.unwrap() / p.release_bar_s.unwrap() - 3.0).abs() < 1e-6);
        let e = parse_vehicle_emitters(&a);
        assert_eq!(e.len(), 2);
        assert_eq!(e[1].position, [-1.0, 0.7, 4.0]);
    }
}
