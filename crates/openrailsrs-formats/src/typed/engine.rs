use crate::ast::Ast;
use crate::error::FormatError;
use crate::units::kmh_to_mps;

use super::{find_numeric_field, find_optional_numeric_field, find_optional_string_field};

#[derive(Clone, Debug, PartialEq)]
pub struct EngineFile {
    pub name: String,
    pub mass_kg: f64,
    pub max_power_w: f64,
    pub max_velocity_mps: f64,
    pub max_tractive_effort_n: f64,
    pub max_brake_force_n: f64,
}

impl EngineFile {
    pub fn from_ast(ast: &Ast) -> Result<Self, FormatError> {
        let context = "Engine";
        let mass_kg = find_numeric_field(ast, &["Mass", "MassKG"], context)?;
        let max_power_w = find_optional_numeric_field(ast, &["MaxPower", "MaxForce"], context)?
            .unwrap_or(0.0);
        let max_velocity_mps =
            kmh_to_mps(find_optional_numeric_field(ast, &["MaxVelocity", "MaxSpeed"], context)?
                .unwrap_or(120.0));
        let max_tractive_effort_n =
            find_optional_numeric_field(ast, &["MaxTractiveEffort"], context)?.unwrap_or(350_000.0);
        let max_brake_force_n = find_optional_numeric_field(ast, &["MaxBrakeForce", "Brake"], context)?
            .unwrap_or(200_000.0);
        let name = find_optional_string_field(ast, &["Name"], context)?.unwrap_or_else(|| "engine".to_string());

        Ok(Self {
            name,
            mass_kg,
            max_power_w,
            max_velocity_mps,
            max_tractive_effort_n,
            max_brake_force_n,
        })
    }
}
