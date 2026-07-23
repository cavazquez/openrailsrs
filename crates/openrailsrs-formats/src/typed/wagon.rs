use crate::ast::{Ast, Atom};
use crate::error::FormatError;
use crate::msts_units::{parse_force_n, parse_length_m, parse_mass_kg};

use super::brake_shoe::{BrakeShoeFrictionCurve, OrtsBrakeShoeType, parse_orts_brake_shoe};
use super::friction::parse_orts_friction_fields;
use super::{
    OrtsFrictionFields, atom_to_number, atom_to_string, find_list_value,
    find_optional_string_field, walk_lists_find, walk_lists_visit,
};

/// One passenger seat from `Inside` / `ORTSAlternatePassengerViewPoint` (OR camera 5).
#[derive(Clone, Debug, PartialEq)]
pub struct PassengerViewpoint {
    pub cabin_file: Option<String>,
    pub head_pos_m: [f64; 3],
    /// Degrees; X = pitch (look down), Y = yaw.
    pub start_direction_deg: [f64; 3],
    /// Degrees; typically (pitch, yaw, roll) limits about StartDirection.
    pub rotation_limit_deg: Option<[f64; 3]>,
}

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
    pub passenger_viewpoints: Vec<PassengerViewpoint>,
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
        let passenger_viewpoints = parse_passenger_viewpoints(ast);
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
            passenger_viewpoints,
        })
    }
}

/// Parse `Inside` and `ORTSAlternatePassengerViewPoint` blocks (Open Rails camera 5).
pub fn parse_passenger_viewpoints(ast: &Ast) -> Vec<PassengerViewpoint> {
    let mut views = Vec::new();
    walk_lists_visit(ast, &mut |items| {
        if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
            if head.eq_ignore_ascii_case("Inside")
                || head.eq_ignore_ascii_case("ORTSAlternatePassengerViewPoint")
            {
                if let Some(vp) = parse_passenger_viewpoint_block(items) {
                    views.push(vp);
                }
                return;
            }
        }
        // Bare Inside body: `parse_first` on `Inside (` yields content whose first
        // atom is `PassengerCabinFile` / `PassengerCabinHeadPos` (not `Inside`).
        if looks_like_inside_body(items) {
            if let Some(vp) = parse_passenger_viewpoint_fields(items) {
                views.push(vp);
            }
        }
    });
    views
}

fn looks_like_inside_body(items: &[Ast]) -> bool {
    items.iter().any(|item| match item {
        Ast::Atom(Atom::Symbol(s)) => {
            let k = s.to_ascii_lowercase();
            k == "passengercabinheadpos" || k == "passengercabinfile"
        }
        Ast::List(inner) => matches!(
            inner.first(),
            Some(Ast::Atom(Atom::Symbol(s)))
                if s.eq_ignore_ascii_case("PassengerCabinHeadPos")
                    || s.eq_ignore_ascii_case("PassengerCabinFile")
        ),
        _ => false,
    })
}

fn parse_passenger_viewpoint_block(items: &[Ast]) -> Option<PassengerViewpoint> {
    // Children after `Inside` / alternate head.
    let mut field_nodes: Vec<&[Ast]> = Vec::new();
    collect_passenger_field_nodes(items.iter().skip(1), &mut field_nodes);
    parse_passenger_viewpoint_from_field_nodes(&field_nodes)
}

fn parse_passenger_viewpoint_fields(items: &[Ast]) -> Option<PassengerViewpoint> {
    let mut field_nodes: Vec<&[Ast]> = Vec::new();
    let symbol_count = items
        .iter()
        .filter(|a| matches!(a, Ast::Atom(Atom::Symbol(_))))
        .count();
    if symbol_count >= 2 {
        // Flat Inside body: `[Sym(k1), val1, Sym(k2), val2, …]`.
        let mut i = 0;
        while i < items.len() {
            if matches!(&items[i], Ast::Atom(Atom::Symbol(_))) {
                let mut end = i + 1;
                while end < items.len() && !matches!(&items[end], Ast::Atom(Atom::Symbol(_))) {
                    end += 1;
                }
                field_nodes.push(&items[i..end]);
                i = end;
            } else {
                i += 1;
            }
        }
    } else {
        collect_passenger_field_nodes(items.iter(), &mut field_nodes);
    }
    parse_passenger_viewpoint_from_field_nodes(&field_nodes)
}

fn collect_passenger_field_nodes<'a, I>(nodes: I, out: &mut Vec<&'a [Ast]>)
where
    I: IntoIterator<Item = &'a Ast>,
{
    for item in nodes {
        let Ast::List(inner) = item else {
            continue;
        };
        let symbol_count = inner
            .iter()
            .filter(|a| matches!(a, Ast::Atom(Atom::Symbol(_))))
            .count();
        if symbol_count >= 2 {
            let mut i = 0;
            while i < inner.len() {
                if matches!(&inner[i], Ast::Atom(Atom::Symbol(_))) {
                    let mut end = i + 1;
                    while end < inner.len() && !matches!(&inner[end], Ast::Atom(Atom::Symbol(_))) {
                        end += 1;
                    }
                    out.push(&inner[i..end]);
                    i = end;
                } else {
                    i += 1;
                }
            }
        } else if symbol_count == 1 {
            out.push(inner.as_slice());
        }
    }
}

fn parse_passenger_viewpoint_from_field_nodes(
    field_nodes: &[&[Ast]],
) -> Option<PassengerViewpoint> {
    let mut cabin_file = None;
    let mut head = None;
    let mut start = [0.0, 0.0, 0.0];
    let mut limit = None;
    for pair in field_nodes {
        let Some(Ast::Atom(Atom::Symbol(key))) = pair.first() else {
            continue;
        };
        match key.to_ascii_lowercase().as_str() {
            "passengercabinfile" => {
                cabin_file = pair.get(1).and_then(atom_or_string);
            }
            "passengercabinheadpos" => {
                head = f64_triplet_from_field_list(pair);
            }
            "startdirection" => {
                if let Some(v) = f64_triplet_from_field_list(pair) {
                    start = v;
                }
            }
            "rotationlimit" => {
                limit = f64_triplet_from_field_list(pair);
            }
            _ => {}
        }
    }
    let head_pos_m = head?;
    Some(PassengerViewpoint {
        cabin_file,
        head_pos_m,
        start_direction_deg: start,
        rotation_limit_deg: limit,
    })
}

/// Accept `Key ( x y z )` as nested list or flat numeric siblings.
fn f64_triplet_from_field_list(pair: &[Ast]) -> Option<[f64; 3]> {
    if let Some(v) = pair.get(1).and_then(f64_triplet_from_ast_value) {
        return Some(v);
    }
    let values: Vec<f64> = pair
        .iter()
        .skip(1)
        .filter_map(|item| match item {
            Ast::Atom(atom) => atom_to_number(atom),
            _ => None,
        })
        .collect();
    (values.len() >= 3).then_some([values[0], values[1], values[2]])
}

fn atom_or_string(ast: &Ast) -> Option<String> {
    match ast {
        Ast::Atom(atom) => atom_to_string(atom),
        Ast::List(items) => items.iter().find_map(atom_or_string),
    }
}

fn f64_triplet_from_ast_value(ast: &Ast) -> Option<[f64; 3]> {
    match ast {
        Ast::List(items) => {
            let values: Vec<f64> = items
                .iter()
                .filter_map(|item| match item {
                    Ast::Atom(atom) => atom_to_number(atom),
                    _ => None,
                })
                .collect();
            (values.len() >= 3).then_some([values[0], values[1], values[2]])
        }
        Ast::Atom(atom) => {
            // Bare "x y z" is unusual; require a list.
            let _ = atom;
            None
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_from_first_paren;

    #[test]
    fn parse_inside_passenger_viewpoint() {
        // Lisp-style nesting (same as engine fixtures); MSTS `Key (` form is also
        // handled by `parse_passenger_viewpoint_block` flat-list path.
        let text = r#"
(Wagon
  (Type "PSG")
  (Mass 40000)
  (Inside
    (PassengerCabinFile ( RF_WP_PFC.s ))
    (PassengerCabinHeadPos ( -1.0 2.46 -6.44 ))
    (RotationLimit ( 30 70 0 ))
    (StartDirection ( 0 180 0 ))
  )
)
"#;
        let ast = parse_from_first_paren(text).expect("ast");
        let views = parse_passenger_viewpoints(&ast);
        assert_eq!(views.len(), 1);
        let vp = &views[0];
        assert_eq!(vp.cabin_file.as_deref(), Some("RF_WP_PFC.s"));
        assert!((vp.head_pos_m[0] - -1.0).abs() < 1e-9);
        assert!((vp.head_pos_m[1] - 2.46).abs() < 1e-9);
        assert!((vp.head_pos_m[2] - -6.44).abs() < 1e-9);
        assert_eq!(vp.start_direction_deg, [0.0, 180.0, 0.0]);
        assert_eq!(vp.rotation_limit_deg, Some([30.0, 70.0, 0.0]));
    }
}
