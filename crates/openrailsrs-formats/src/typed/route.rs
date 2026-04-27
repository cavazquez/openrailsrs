use crate::ast::Ast;
use crate::error::FormatError;

use super::find_optional_string_field;

#[derive(Clone, Debug, PartialEq)]
pub struct RouteFile {
    pub route_id: String,
    pub name: String,
}

impl RouteFile {
    pub fn from_ast(ast: &Ast) -> Result<Self, FormatError> {
        let route_id = find_optional_string_field(ast, &["RouteID"], "Tr_RouteFile")?
            .ok_or_else(|| FormatError::MissingField {
                key: "RouteID".to_string(),
                context: "Tr_RouteFile".to_string(),
            })?;
        let name = find_optional_string_field(ast, &["Name"], "Tr_RouteFile")?
            .unwrap_or_else(|| route_id.clone());
        Ok(Self { route_id, name })
    }
}
