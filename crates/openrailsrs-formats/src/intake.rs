//! Native locomotive/tender intake points, in the authored vehicle coordinates.
use crate::{Ast, Atom, parse_length_m};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IntakePoint {
    pub offset_m: f64,
    pub width_m: f64,
    /// Open Rails PickupType: 5 water, 6 coal, 7 diesel.
    pub pickup_type: u32,
}

pub fn parse_intake_points(ast: &Ast) -> Vec<IntakePoint> {
    fn text(ast: &Ast) -> Option<String> {
        match ast {
            Ast::Atom(Atom::Symbol(s) | Atom::String(s)) => Some(s.clone()),
            Ast::Atom(Atom::Integer(n)) => Some(n.to_string()),
            Ast::Atom(Atom::Number(n)) => Some(n.to_string()),
            _ => None,
        }
    }
    fn visit(ast: &Ast, out: &mut Vec<IntakePoint>) {
        let Ast::List(items) = ast else { return };
        if matches!(items.first(), Some(Ast::Atom(Atom::Symbol(s))) if s.eq_ignore_ascii_case("IntakePoint"))
        {
            let values: Vec<_> = items.iter().skip(1).filter_map(text).collect();
            if values.len() >= 3 {
                let kind = match values[2].to_ascii_lowercase().as_str() {
                    "fuelwater" | "water" | "5" => 5,
                    "fuelcoal" | "coal" | "6" => 6,
                    "fueldiesel" | "diesel" | "7" => 7,
                    _ => return,
                };
                if let (Some(offset_m), Some(width_m)) =
                    (parse_length_m(&values[0]), parse_length_m(&values[1]))
                    && offset_m.is_finite()
                    && width_m.is_finite()
                    && width_m > 0.0
                {
                    out.push(IntakePoint {
                        offset_m,
                        width_m,
                        pickup_type: kind,
                    });
                }
            }
            return;
        }
        for item in items {
            visit(item, out);
        }
    }
    let mut points = Vec::new();
    visit(ast, &mut points);
    points
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_intakes_preserve_offset_units_and_ignore_invalid_and_freight_types() {
        let ast = crate::parse_vehicle_text("Wagon ( IntakePoint ( -2m 4ft FuelWater ) IntakePoint ( 3 1 FuelDiesel ) IntakePoint ( 0 0 FuelCoal ) IntakePoint ( 0 1 Grain ) )").unwrap();
        let points = parse_intake_points(&ast);
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].offset_m, -2.0);
        assert!((points[0].width_m - 1.2192).abs() < 1e-8);
        assert_eq!(points[1].pickup_type, 7);
    }
}
