use crate::ast::Ast;
use crate::error::FormatError;

use super::{find_numeric_field, find_optional_numeric_field, find_optional_string_field};

#[derive(Clone, Debug, PartialEq)]
pub struct WagonFile {
    pub name: String,
    pub mass_kg: f64,
    pub max_brake_force_n: f64,
}

impl WagonFile {
    pub fn from_ast(ast: &Ast) -> Result<Self, FormatError> {
        let context = "Wagon";
        let mass_kg = find_numeric_field(ast, &["Mass", "MassKG"], context)?;
        let name = find_optional_string_field(ast, &["Type", "Name"], context)?
            .unwrap_or_else(|| "wagon".to_string());
        let max_brake_force_n = find_optional_numeric_field(ast, &["MaxBrakeForce", "Brake"], context)?
            .unwrap_or(80_000.0);
        Ok(Self {
            name,
            mass_kg,
            max_brake_force_n,
        })
    }
}
