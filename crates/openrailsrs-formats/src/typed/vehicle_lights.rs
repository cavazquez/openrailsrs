//! First authored state of MSTS vehicle lights (OR 1.6.1 LightCollection).
use crate::{Ast, Atom};
fn atom_to_number(ast: &Ast) -> Option<f64> {
    if let Ast::Atom(a) = ast {
        super::atom_to_number(a)
    } else {
        None
    }
}
fn atom_to_string(ast: &Ast) -> Option<String> {
    if let Ast::Atom(a) = ast {
        super::atom_to_string(a)
    } else {
        None
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct VehicleLight {
    pub cone: bool,
    pub headlight: u8,
    pub unit: u8,
    pub position_m: [f32; 3],
    pub color_rgba: [f32; 4],
    pub radius_m: f32,
    pub angle_deg: f32,
}
fn block<'a>(items: &'a [Ast], name: &str) -> Option<&'a [Ast]> {
    items.iter().find_map(|a| match a {
        Ast::List(v)
            if v.first()
                .and_then(atom_to_string)
                .is_some_and(|s| s.eq_ignore_ascii_case(name)) =>
        {
            Some(&v[1..])
        }
        _ => None,
    })
}
fn number(items: &[Ast], name: &str, default: f64) -> f64 {
    block(items, name)
        .and_then(|v| v.first())
        .and_then(atom_to_number)
        .unwrap_or(default)
}

pub fn parse_vehicle_lights(ast: &Ast) -> Vec<VehicleLight> {
    let mut result = vec![];
    fn visit(a: &Ast, result: &mut Vec<VehicleLight>) {
        let Ast::List(items) = a else { return };
        if items
            .first()
            .and_then(atom_to_string)
            .is_some_and(|s| s.eq_ignore_ascii_case("Light"))
        {
            let Some(state) = block(items, "States").and_then(|s| block(s, "State")) else {
                return;
            };
            let Some(position) = block(state, "Position") else {
                return;
            };
            let values = position
                .iter()
                .filter_map(atom_to_number)
                .collect::<Vec<_>>();
            if values.len() < 3 || values.iter().any(|v| !v.is_finite()) {
                return;
            }
            let condition = block(items, "Conditions").unwrap_or(&[]);
            let raw = block(state, "LightColour")
                .and_then(|s| s.first())
                .map(|a| match a {
                    Ast::Atom(Atom::Symbol(s) | Atom::String(s)) => s.clone(),
                    Ast::Atom(Atom::Integer(n)) => n.to_string(),
                    _ => String::new(),
                })
                .unwrap_or_else(|| "ffffffff".into());
            let color =
                u32::from_str_radix(raw.trim_start_matches("0x"), 16).unwrap_or(0xffff_ffff);
            result.push(VehicleLight {
                cone: number(items, "Type", 0.0) == 1.0,
                headlight: number(condition, "Headlight", 0.0) as u8,
                unit: number(condition, "Unit", 0.0) as u8,
                position_m: [values[0] as f32, values[1] as f32, values[2] as f32],
                color_rgba: [
                    ((color >> 16) & 255) as f32 / 255.0,
                    ((color >> 8) & 255) as f32 / 255.0,
                    (color & 255) as f32 / 255.0,
                    ((color >> 24) & 255) as f32 / 255.0,
                ],
                radius_m: number(state, "Radius", 0.5) as f32,
                angle_deg: number(state, "Angle", 15.0) as f32,
            });
            return;
        }
        for child in items {
            visit(child, result);
        }
    }
    visit(ast, &mut result);
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_glow_position_and_conditions_are_preserved() {
        let ast = crate::parse_from_first_paren("(Wagon (Lights 1 (Light (Type 0) (Conditions (Unit 2) (Headlight 3)) (States 1 (State (Position -0.875 1.598 10.031) (Radius 0.5) (LightColour 80ffffff))))))").unwrap();
        let lights = parse_vehicle_lights(&ast);
        assert_eq!(lights.len(), 1);
        assert!(!lights[0].cone);
        assert_eq!(lights[0].headlight, 3);
        assert_eq!(lights[0].position_m, [-0.875, 1.598, 10.031]);
        assert!(lights[0].color_rgba[3] > 0.5);
    }
}
