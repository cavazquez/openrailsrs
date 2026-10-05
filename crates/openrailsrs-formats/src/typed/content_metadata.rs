//! Authored subsystems are reported separately from the presence of meshes.
use super::{VehicleCurveParameters, field_values, parse_vehicle_curve_parameters, scalar_text};
use crate::Ast;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptSystem {
    TrainControl,
    TrainBrake,
    EngineBrake,
    PowerSupply,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VehicleScriptReference {
    pub system: ScriptSystem,
    pub name: String,
    pub built_in: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VehicleContentMetadata {
    pub engine_type: Option<String>,
    pub brake_system: Option<String>,
    pub scripts: Vec<VehicleScriptReference>,
    pub sounds: Vec<String>,
    pub curve: VehicleCurveParameters,
}
pub fn parse_vehicle_content_metadata(ast: &Ast, is_engine: bool) -> VehicleContentMetadata {
    let value = |key: &str| {
        field_values(ast, key)
            .first()
            .and_then(|v| v.first())
            .and_then(scalar_text)
    };
    let engine_type = if is_engine {
        field_values(ast, "Engine")
            .first()
            .and_then(|body| {
                let engine = Ast::List(body.to_vec());
                field_values(&engine, "Type")
                    .first()
                    .and_then(|v| v.first())
                    .and_then(scalar_text)
            })
            .or_else(|| {
                // Native parse_vehicle_text deliberately drops the separate root
                // headers. Traction categories still disambiguate Engine.Type from
                // Wagon.Type (Engine/Passenger/Freight/Tender).
                field_values(ast, "Type")
                    .iter()
                    .filter_map(|v| v.first().and_then(scalar_text))
                    .find(|v| {
                        ["Diesel", "Electric", "Steam"]
                            .iter()
                            .any(|t| v.eq_ignore_ascii_case(t))
                    })
            })
    } else {
        None
    };
    let mut scripts = vec![];
    for (key, system) in [
        ("ORTSTrainControlSystem", ScriptSystem::TrainControl),
        ("ORTSTrainBrakeController", ScriptSystem::TrainBrake),
        ("ORTSEngineBrakeController", ScriptSystem::EngineBrake),
        ("ORTSPowerSupply", ScriptSystem::PowerSupply),
    ] {
        for items in field_values(ast, key) {
            if let Some(name) = items.first().and_then(scalar_text) {
                let built_in =
                    name.eq_ignore_ascii_case("MSTS") || name.eq_ignore_ascii_case("Default");
                scripts.push(VehicleScriptReference {
                    system: system.clone(),
                    name,
                    built_in,
                });
            }
        }
    }
    let sounds = ["Sound", "ORTSTrainControlSystemSound"]
        .iter()
        .flat_map(|key| field_values(ast, key))
        .filter_map(|v| v.first().and_then(scalar_text))
        .filter(|v| !v.is_empty() && !v.eq_ignore_ascii_case("none"))
        .collect();
    VehicleContentMetadata {
        engine_type,
        brake_system: value("BrakeSystemType"),
        scripts,
        sounds,
        curve: parse_vehicle_curve_parameters(ast, is_engine),
    }
}
/// WAV references from a parsed SMS without decoding or starting audio devices.
pub fn sms_wave_references(ast: &Ast) -> Vec<String> {
    let mut paths: Vec<_> = field_values(ast, "File")
        .iter()
        .filter_map(|v| v.first().and_then(scalar_text))
        .collect();
    paths.sort();
    paths.dedup();
    paths
}
