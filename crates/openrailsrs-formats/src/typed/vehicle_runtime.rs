//! Authored brake response and effect emitters used by simulation and Bevy.
use super::named_blocks;
use crate::{Ast, Atom};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct VehicleBrakeProfile {
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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeVacuumBrakeProfile {
    pub max_force_psi: f64,
    pub application_psi_s: f64,
    pub release_psi_s: f64,
    pub cylinder_volume_m3: f64,
    pub reservoir_volume_m3: f64,
    pub pipe_volume_m3: Option<f64>,
    pub direct_admission: bool,
    pub charged_vacuum_psi: f64,
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
    pub cylinder: NativeEpBrakeProfile,
    pub triple_ratio: f64,
    pub relay_ratio: f64,
    pub cylinder_count: f64,
    pub pipe_volume_ratio: f64,
    pub controller: NativeAirController,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeAirController {
    pub charged_psi: f64,
    pub full_service_drop_psi: f64,
    pub application_psi_s: f64,
    pub release_psi_s: f64,
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
                let raw = s.trim().trim_end_matches("/s").to_ascii_lowercase();
                let end = raw
                    .find(|c: char| !c.is_ascii_digit() && !matches!(c, '.' | '-' | '+' | 'e'))
                    .unwrap_or(raw.len());
                let number: f64 = raw[..end].parse().ok()?;
                let factor = match raw[end..].trim() {
                    "" if vacuum_unit => 0.491154152,
                    "" | "psi" => 1.,
                    "inhg" => 0.491154152,
                    "bar" => 1. / 0.0689475729,
                    "kpa" => 0.1450377377,
                    _ => return None,
                };
                (number.is_finite() && number > 0.).then_some(number * factor)
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
    let atmosphere = 1. / 0.0689475729;
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
        charged_vacuum_psi: psi(
            "TrainBrakesControllerMaxSystemPressure",
            21. * 0.491154152,
            false,
        )
        .min(atmosphere),
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
    if !matches!(system.as_str(), "air_twin_pipe" | "air_single_pipe")
        || !scalar(ast, "BrakeEquipmentType")?
            .to_ascii_lowercase()
            .contains("distributor")
    {
        return None;
    }
    let mut cylinder = parse_native_cylinder(ast)?;
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
    let triple_ratio = number("TripleValveRatio", 2.5);
    let relay_ratio = number("ORTSBrakeRelayValveRatio", 1.);
    let cylinder_count = number("ORTSNumberBrakeCylinders", 1.);
    let cylinder_volume =
        std::f64::consts::PI * cylinder.diameter_m.powi(2) / 4. * cylinder.stroke_m;
    let nominal_cylinder = 70. * triple_ratio / (triple_ratio + 1.);
    let piping_volume = if cylinder.main_reservoir || relay_ratio != 1. {
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
    Some(NativeAirBrakeProfile {
        cylinder,
        triple_ratio,
        relay_ratio,
        cylinder_count,
        pipe_volume_ratio: piping_volume / cylinder_volume,
        controller: NativeAirController {
            charged_psi: psi("TrainBrakesControllerMaxSystemPressure", 72.51887),
            full_service_drop_psi: psi("TrainBrakesControllerFullServicePressureDrop", 21.755661),
            application_psi_s: psi("TrainBrakesControllerMaxApplicationRate", 5.80151),
            release_psi_s: psi("TrainBrakesControllerMaxReleaseRate", 7.251887),
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
    for family in ["DieselSpecialEffects", "SteamSpecialEffects"] {
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
