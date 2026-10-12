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
    #[serde(default)]
    pub size_m: Option<[f64; 3]>,
    pub engine_type: Option<String>,
    #[serde(default)]
    pub electric: Option<openrailsrs_core::electrification::ElectricVehicleParams>,
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
    let engine_type = is_engine.then(|| vehicle_engine_type(ast)).flatten();
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
    let size_m = field_values(ast, "Size").first().and_then(|v| {
        let dims = v
            .iter()
            .filter_map(scalar_text)
            .filter_map(|v| crate::msts_units::parse_length_m(&v))
            .collect::<Vec<_>>();
        (dims.len() == 3 && dims.iter().all(|v| v.is_finite() && *v > 0.))
            .then(|| [dims[0], dims[1], dims[2]])
    });
    VehicleContentMetadata {
        size_m,
        engine_type,
        electric: is_engine
            .then(|| super::parse_electric_vehicle(ast).ok().flatten())
            .flatten(),
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

pub(crate) fn vehicle_engine_type(ast: &Ast) -> Option<String> {
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
}
