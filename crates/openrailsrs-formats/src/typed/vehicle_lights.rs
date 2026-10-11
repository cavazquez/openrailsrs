//! Authored MSTS vehicle light states (OR 1.6.1 LightCollection).
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
    pub states: Vec<VehicleLightState>,
    pub cycle: bool,
    pub conditions: Vec<(String, u8)>,
    /// Each Conditions block is an alternative (OR combines blocks with OR).
    pub condition_sets: Vec<Vec<(String, u8)>>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct VehicleLightState {
    pub position_m: [f32; 3],
    pub color_rgba: [f32; 4],
    pub radius_m: f32,
    pub angle_deg: f32,
    pub azimuth_deg: f32,
    pub elevation_deg: f32,
    pub duration_s: f32,
    pub transition: bool,
}
fn vector(items: &[Ast], name: &str) -> [f32; 3] {
    let values = block(items, name)
        .unwrap_or(&[])
        .iter()
        .filter_map(atom_to_number)
        .take(3)
        .collect::<Vec<_>>();
    std::array::from_fn(|i| {
        values
            .get(i)
            .copied()
            .filter(|n| n.is_finite())
            .unwrap_or(0.) as f32
    })
}
fn state(items: &[Ast]) -> Option<VehicleLightState> {
    block(items, "Position")?;
    let raw = block(items, "LightColour")
        .and_then(|s| s.first())
        .map(|a| match a {
            Ast::Atom(Atom::Symbol(s) | Atom::String(s)) => s.clone(),
            Ast::Atom(Atom::Integer(n)) => n.to_string(),
            _ => String::new(),
        })
        .unwrap_or_else(|| "ffffffff".into());
    let color = u32::from_str_radix(raw.trim_start_matches("0x"), 16).unwrap_or(u32::MAX);
    Some(VehicleLightState {
        position_m: vector(items, "Position"),
        color_rgba: [
            ((color >> 16) & 255) as f32 / 255.,
            ((color >> 8) & 255) as f32 / 255.,
            (color & 255) as f32 / 255.,
            ((color >> 24) & 255) as f32 / 255.,
        ],
        radius_m: number(items, "Radius", 0.5).clamp(0.01, 100.) as f32,
        angle_deg: number(items, "Angle", 15.).clamp(1., 120.) as f32,
        azimuth_deg: vector(items, "Azimuth")[1],
        elevation_deg: vector(items, "Elevation")[1],
        duration_s: number(items, "Duration", 1.).clamp(0.001, 86400.) as f32,
        transition: number(items, "Transition", 0.) != 0.,
    })
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
        .filter(|v| v.is_finite())
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
            let states = block(items, "States")
                .unwrap_or(&[])
                .iter()
                .filter_map(|a| match a {
                    Ast::List(v)
                        if v.first()
                            .and_then(atom_to_string)
                            .is_some_and(|n| n.eq_ignore_ascii_case("State")) =>
                    {
                        state(&v[1..])
                    }
                    _ => None,
                })
                .take(128)
                .collect::<Vec<_>>();
            let Some(first) = states.first() else {
                return;
            };
            let condition = block(items, "Conditions").unwrap_or(&[]);
            let parse_conditions = |values: &[Ast]| {
                values
                    .iter()
                    .filter_map(|a| match a {
                        Ast::List(v) if v.len() >= 2 => Some((
                            atom_to_string(&v[0])?.to_ascii_lowercase(),
                            atom_to_number(&v[1])? as u8,
                        )),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            };
            let condition_sets = items
                .iter()
                .filter_map(|a| match a {
                    Ast::List(v)
                        if v.first()
                            .and_then(atom_to_string)
                            .is_some_and(|n| n.eq_ignore_ascii_case("Conditions")) =>
                    {
                        Some(parse_conditions(&v[1..]))
                    }
                    _ => None,
                })
                .collect();
            result.push(VehicleLight {
                cone: number(items, "Type", 0.0) == 1.0,
                headlight: number(condition, "Headlight", 0.0) as u8,
                unit: number(condition, "Unit", 0.0) as u8,
                position_m: first.position_m,
                color_rgba: first.color_rgba,
                radius_m: first.radius_m,
                angle_deg: first.angle_deg,
                states,
                cycle: number(items, "Cycle", 0.) != 0.,
                condition_sets,
                conditions: condition
                    .iter()
                    .filter_map(|a| match a {
                        Ast::List(v) if v.len() >= 2 => Some((
                            atom_to_string(&v[0])?.to_ascii_lowercase(),
                            atom_to_number(&v[1])? as u8,
                        )),
                        _ => None,
                    })
                    .collect(),
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
