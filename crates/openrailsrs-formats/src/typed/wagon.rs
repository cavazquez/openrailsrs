use crate::ast::{Ast, Atom};
use crate::error::FormatError;
use crate::msts_units::{parse_force_n, parse_length_m, parse_mass_kg};

use super::brake_shoe::{BrakeShoeFrictionCurve, OrtsBrakeShoeType, parse_orts_brake_shoe};
use super::friction::parse_orts_friction_fields;
use super::{
    OrtsFrictionFields, atom_to_number, atom_to_string, find_list_value,
    find_optional_string_field, walk_lists_find,
};
#[derive(Clone, Debug, PartialEq)]
pub struct WagonFile {
    pub name: String,
    pub mass_kg: f64,
    pub max_brake_force_n: f64,
    pub length_m: f64,
    pub davis_a_n: f64,
    pub davis_b_n_per_mps: f64,
    pub davis_c_n_per_mps2: f64,
    pub wagon_shape: Option<String>,
    pub friction: OrtsFrictionFields,
    pub brake_shoe_type: OrtsBrakeShoeType,
    pub brake_shoe_friction: Option<BrakeShoeFrictionCurve>,
}

impl WagonFile {
    pub fn from_ast(ast: &Ast) -> Result<Self, FormatError> {
        let context = "Wagon";
        let mass_kg = find_mass_field(ast, &["Mass", "MassKG"], context)?;
        let name = find_optional_string_field(ast, &["Type", "Name"], context)?
            .unwrap_or_else(|| "wagon".to_string());
        let max_brake_force_n = find_optional_force_field(
            ast,
            &[
                "MaxBrakeForce",
                "Brake",
                "ORTSMaxBrakeShoeForce",
                "MaxBrakeShoeForce",
            ],
            context,
        )?
        .unwrap_or(80_000.0);
        let length_m = parse_length_from_ast(ast).unwrap_or(15.0);
        let wagon_shape = find_optional_string_field(ast, &["WagonShape", "Shape"], context)?;
        let (davis_a_n, davis_b_n_per_mps, davis_c_n_per_mps2) = parse_orts_davis(ast);
        let friction = parse_orts_friction_fields(ast, false, &name);
        let (brake_shoe_type, brake_shoe_friction) = parse_orts_brake_shoe(ast);
        Ok(Self {
            name,
            mass_kg,
            max_brake_force_n,
            length_m,
            davis_a_n,
            davis_b_n_per_mps,
            davis_c_n_per_mps2,
            wagon_shape,
            friction,
            brake_shoe_type,
            brake_shoe_friction,
        })
    }
}

fn parse_orts_davis(ast: &Ast) -> (f64, f64, f64) {
    let context = "ORTSDavis";
    (
        find_optional_scalar_field(ast, &["ORTSDavis_A"], context)
            .ok()
            .flatten()
            .unwrap_or(0.0),
        find_optional_scalar_field(ast, &["ORTSDavis_B"], context)
            .ok()
            .flatten()
            .unwrap_or(0.0),
        find_optional_scalar_field(ast, &["ORTSDavis_C"], context)
            .ok()
            .flatten()
            .unwrap_or(0.0),
    )
}

fn find_optional_scalar_field(
    root: &Ast,
    keys: &[&str],
    context: &str,
) -> Result<Option<f64>, FormatError> {
    for key in keys {
        if let Some(value) = find_list_value(root, key) {
            return parse_scalar_ast(value)
                .map(Some)
                .ok_or_else(|| FormatError::UnexpectedAtom {
                    key: (*key).to_string(),
                    context: context.to_string(),
                    expected: "numeric scalar".to_string(),
                });
        }
    }
    Ok(None)
}

fn parse_scalar_ast(value: &Ast) -> Option<f64> {
    match value {
        Ast::Atom(atom) => super::atom_to_number(atom)
            .or_else(|| atom_to_string(atom).and_then(|s| s.parse::<f64>().ok())),
        Ast::List(items) => items.iter().find_map(parse_scalar_ast),
    }
}

fn find_mass_field(root: &Ast, keys: &[&str], context: &str) -> Result<f64, FormatError> {
    for key in keys {
        if let Some(value) = find_list_value(root, key) {
            return parse_mass_ast(value).ok_or_else(|| FormatError::UnexpectedAtom {
                key: (*key).to_string(),
                context: context.to_string(),
                expected: "mass quantity".to_string(),
            });
        }
    }
    Err(FormatError::MissingField {
        key: keys.join("|"),
        context: context.to_string(),
    })
}

fn find_optional_force_field(
    root: &Ast,
    keys: &[&str],
    context: &str,
) -> Result<Option<f64>, FormatError> {
    for key in keys {
        if let Some(value) = find_list_value(root, key) {
            return parse_force_ast(value)
                .map(Some)
                .ok_or_else(|| FormatError::UnexpectedAtom {
                    key: (*key).to_string(),
                    context: context.to_string(),
                    expected: "force quantity".to_string(),
                });
        }
    }
    Ok(None)
}

fn parse_mass_ast(value: &Ast) -> Option<f64> {
    match value {
        Ast::Atom(atom) => super::atom_to_number(atom)
            .or_else(|| atom_to_string(atom).and_then(|s| parse_mass_kg(&s))),
        Ast::List(items) => items.first().and_then(parse_mass_ast),
    }
}

fn parse_force_ast(value: &Ast) -> Option<f64> {
    match value {
        Ast::Atom(atom) => {
            atom_to_number(atom).or_else(|| atom_to_string(atom).and_then(|s| parse_force_n(&s)))
        }
        Ast::List(items) => items.first().and_then(parse_force_ast),
    }
}

fn parse_length_ast(value: &Ast) -> Option<f64> {
    match value {
        Ast::Atom(atom) => {
            atom_to_number(atom).or_else(|| atom_to_string(atom).and_then(|s| parse_length_m(&s)))
        }
        Ast::List(items) => items.first().and_then(parse_length_ast),
    }
}

fn parse_length_from_ast(ast: &Ast) -> Option<f64> {
    for key in [
        "ORTSLengthCouplerFace",
        "ORTSLengthCarBody",
        "Length",
        "WagonLength",
    ] {
        if let Some(v) = find_list_value(ast, key) {
            if let Some(len) = parse_length_ast(v) {
                return Some(len);
            }
        }
    }
    let mut found = None;
    walk_lists_find::<(), _>(ast, &mut |items| {
        if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
            if head.eq_ignore_ascii_case("Size") {
                if items.len() >= 4 {
                    found = parse_length_ast(&items[3]);
                } else if let Some(Ast::List(dims)) = items.get(1) {
                    if dims.len() >= 3 {
                        found = parse_length_ast(&dims[2]);
                    }
                }
            }
        }
        None
    });
    found
}
