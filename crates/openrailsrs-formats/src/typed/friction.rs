//! Open Rails rolling-resistance metadata from `.wag` / `.eng` files.

use crate::ast::{Ast, Atom};
use crate::msts_units::parse_length_m;

use super::{atom_to_string, find_list_value, find_optional_string_field, walk_lists_find};

/// OR `ORTSBearingType` (see `MSTSWagon.BearingTypes`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OrtsBearingType {
    #[default]
    Default,
    Grease,
    Friction,
    Roller,
    Low,
}

/// OR wagon/car category for Davis B estimation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OrtsWagonType {
    #[default]
    Freight,
    Passenger,
    Engine,
    Tender,
}

/// Parsed friction-related fields for auto Davis calculation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OrtsFrictionFields {
    pub bearing_type: OrtsBearingType,
    pub wagon_type: OrtsWagonType,
    /// `ORTSNumberAxles` or inferred from `NumWheels`.
    pub num_axles: Option<u32>,
    /// Locomotive driven axles (`ORTSNumberDriveAxles`).
    pub num_drive_axles: Option<u32>,
    /// MSTS `NumWheels` (legacy axle hint).
    pub num_wheels: Option<u32>,
    pub frontal_area_m2: Option<f64>,
    pub drag_constant: Option<f64>,
    pub car_width_m: Option<f64>,
    pub car_height_m: Option<f64>,
}

impl OrtsFrictionFields {
    /// Bearing type used for auto-friction when `Default` (matches OR runtime path).
    pub fn effective_bearing_type(&self) -> OrtsBearingType {
        if self.bearing_type == OrtsBearingType::Default {
            OrtsBearingType::Friction
        } else {
            self.bearing_type
        }
    }

    /// Total axles for Davis A/B (wagon axles + loco drive axles when applicable).
    pub fn total_axles(&self, is_loco: bool) -> u32 {
        let drive = self.num_drive_axles.unwrap_or(0);
        let wagon_axles = self
            .num_axles
            .or_else(|| self.num_wheels.filter(|&w| w > 0 && w < 6))
            .unwrap_or(if is_loco { 0 } else { 4 });
        let base = if is_loco {
            wagon_axles.max(drive)
        } else {
            wagon_axles
        };
        base + if is_loco { drive } else { 0 }
    }

    pub fn frontal_area_m2(&self) -> f64 {
        if let Some(a) = self.frontal_area_m2.filter(|a| *a > 0.0) {
            return a;
        }
        let w = self.car_width_m.unwrap_or(0.0);
        let h = self.car_height_m.unwrap_or(0.0);
        if w > 0.0 && h > 0.0 { w * h } else { 0.0 }
    }
}

pub fn parse_orts_friction_fields(ast: &Ast, is_loco: bool, name: &str) -> OrtsFrictionFields {
    let mut fields = OrtsFrictionFields {
        wagon_type: if is_loco {
            OrtsWagonType::Engine
        } else {
            infer_wagon_type_from_name(name)
        },
        ..Default::default()
    };

    if let Some(bt) = find_optional_string_field(ast, &["ORTSBearingType"], "ORTSBearingType")
        .ok()
        .flatten()
    {
        fields.bearing_type = parse_bearing_type(&bt);
    }

    if let Some(wt) = find_optional_string_field(ast, &["WagonType", "EngineType"], "WagonType")
        .ok()
        .flatten()
    {
        fields.wagon_type = parse_wagon_type(&wt);
    }

    fields.num_axles = find_optional_u32(ast, &["ORTSNumberAxles"]);
    fields.num_drive_axles = find_optional_u32(ast, &["ORTSNumberDriveAxles"]);
    fields.num_wheels = find_optional_u32(ast, &["NumWheels"]);

    fields.frontal_area_m2 = find_optional_f64(ast, &["ORTSWagonFrontalArea"]);
    fields.drag_constant = find_optional_f64(ast, &["ORTSDavisDragConstant"]);

    parse_size_dimensions(ast, &mut fields);

    fields
}

fn infer_wagon_type_from_name(name: &str) -> OrtsWagonType {
    let upper = name.to_ascii_uppercase();
    if upper.contains("MK2")
        || upper.contains("MKII")
        || upper.contains("COACH")
        || upper.contains("TSO")
        || upper.contains("BSO")
        || upper.contains("FK")
        || upper.contains("PCF")
        || upper.contains("PSG")
        || upper.contains("CARRIAGE")
    {
        OrtsWagonType::Passenger
    } else {
        OrtsWagonType::Freight
    }
}

fn parse_bearing_type(s: &str) -> OrtsBearingType {
    match s.trim().to_ascii_lowercase().as_str() {
        "grease" => OrtsBearingType::Grease,
        "friction" => OrtsBearingType::Friction,
        "roller" => OrtsBearingType::Roller,
        "low" => OrtsBearingType::Low,
        _ => OrtsBearingType::Default,
    }
}

fn parse_wagon_type(s: &str) -> OrtsWagonType {
    let normalized = s.trim().replace("Carriage", "Passenger");
    match normalized.to_ascii_lowercase().as_str() {
        "passenger" => OrtsWagonType::Passenger,
        "engine" | "locomotive" => OrtsWagonType::Engine,
        "tender" => OrtsWagonType::Tender,
        _ => OrtsWagonType::Freight,
    }
}

fn parse_size_dimensions(ast: &Ast, fields: &mut OrtsFrictionFields) {
    walk_lists_find::<(), _>(ast, &mut |items| {
        if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
            if head.eq_ignore_ascii_case("Size") {
                if items.len() >= 4 {
                    fields.car_width_m = parse_length(items.get(1));
                    fields.car_height_m = parse_length(items.get(3));
                } else if let Some(crate::ast::Ast::List(dims)) = items.get(1) {
                    if dims.len() >= 3 {
                        fields.car_width_m = parse_length(dims.get(1));
                        fields.car_height_m = parse_length(dims.get(2));
                    }
                }
            }
        }
        None
    });
}

fn parse_length(node: Option<&Ast>) -> Option<f64> {
    node.and_then(|v| match v {
        Ast::Atom(atom) => super::atom_to_number(atom)
            .or_else(|| atom_to_string(atom).and_then(|s| parse_length_m(&s))),
        Ast::List(items) => items.first().and_then(|n| parse_length(Some(n))),
    })
}

fn find_optional_u32(ast: &Ast, keys: &[&str]) -> Option<u32> {
    for key in keys {
        if let Some(v) = find_list_value(ast, key) {
            if let Some(n) = parse_scalar(v) {
                if n >= 0.0 {
                    return Some(n.round() as u32);
                }
            }
        }
    }
    None
}

fn find_optional_f64(ast: &Ast, keys: &[&str]) -> Option<f64> {
    for key in keys {
        if let Some(v) = find_list_value(ast, key) {
            if let Some(n) = parse_scalar(v) {
                return Some(n);
            }
        }
    }
    None
}

fn parse_scalar(value: &Ast) -> Option<f64> {
    match value {
        Ast::Atom(atom) => super::atom_to_number(atom)
            .or_else(|| atom_to_string(atom).and_then(|s| s.parse().ok())),
        Ast::List(items) => items.first().and_then(parse_scalar),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_from_first_paren;

    #[test]
    fn parses_bearing_type_and_axles() {
        let ast = parse_from_first_paren(
            r#"(Wagon
  (Name "coach")
  (Mass 34000)
  (ORTSBearingType Roller)
  (ORTSNumberAxles 4)
)"#,
        )
        .unwrap();
        let f = parse_orts_friction_fields(&ast, false, "coach");
        assert_eq!(f.bearing_type, OrtsBearingType::Roller);
        assert_eq!(f.num_axles, Some(4));
        assert_eq!(f.total_axles(false), 4);
    }

    #[test]
    fn infers_passenger_mk2_name() {
        let ast = parse_from_first_paren(r#"(Wagon (Name "MK2_TSO") (Mass 34000))"#).unwrap();
        let f = parse_orts_friction_fields(&ast, false, "MK2_TSO");
        assert_eq!(f.wagon_type, OrtsWagonType::Passenger);
        assert_eq!(f.effective_bearing_type(), OrtsBearingType::Friction);
    }
}
