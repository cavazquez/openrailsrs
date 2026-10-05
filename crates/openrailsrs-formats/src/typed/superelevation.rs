//! Vehicle cant/gauge parameters and the OR 1.6.1 comfort-speed formula.
//! These describe curve comfort, not a derailment or suspension model.
use crate::{Ast, Atom, msts_units::parse_length_m};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VehicleCurveParameters {
    /// None means inherit the route gauge (OR defaults to standard gauge).
    pub track_gauge_m: Option<f64>,
    pub max_unbalanced_m: f64,
}
impl Default for VehicleCurveParameters {
    fn default() -> Self {
        Self {
            track_gauge_m: None,
            max_unbalanced_m: 0.0762,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct CurveComfort {
    pub equilibrium_speed_mps: f64,
    pub comfortable_speed_mps: f64,
    pub lateral_acceleration_mps2: f64,
    pub cant_deficiency_m: f64,
    pub roll_rad: f64,
}
impl VehicleCurveParameters {
    /// R is positive, cant is the outer rail's elevation above the inner rail.
    /// Signed lateral acceleration: outward above balance, inward below it.
    pub fn evaluate(
        &self,
        radius_m: f64,
        cant_m: f64,
        speed_mps: f64,
        route_gauge_m: f64,
    ) -> Option<CurveComfort> {
        let gauge = self.track_gauge_m.unwrap_or(route_gauge_m);
        if ![radius_m, cant_m, speed_mps, gauge, self.max_unbalanced_m]
            .iter()
            .all(|v| v.is_finite())
            || radius_m <= 0.
            || gauge <= 0.
            || cant_m < 0.
            || cant_m >= gauge
            || self.max_unbalanced_m < 0.
        {
            return None;
        }
        let g = 9.80665;
        let deficiency = gauge * speed_mps.powi(2) / (g * radius_m) - cant_m;
        Some(CurveComfort {
            equilibrium_speed_mps: (cant_m * g * radius_m / gauge).sqrt(),
            comfortable_speed_mps: ((cant_m + self.max_unbalanced_m) * g * radius_m / gauge).sqrt(),
            lateral_acceleration_mps2: speed_mps.powi(2) / radius_m - g * cant_m / gauge,
            cant_deficiency_m: deficiency,
            roll_rad: (cant_m / gauge).asin(),
        })
    }
}

/// Both normalized fixture AST and native STF expose keyword/value lists.
pub(crate) fn field_values<'a>(ast: &'a Ast, key: &str) -> Vec<&'a [Ast]> {
    fn visit<'a>(ast: &'a Ast, key: &str, values: &mut Vec<&'a [Ast]>) {
        if let Ast::List(items) = ast {
            if matches!(items.first(), Some(Ast::Atom(Atom::Symbol(s))) if s.eq_ignore_ascii_case(key))
            {
                values.push(&items[1..]);
            }
            for child in items {
                visit(child, key, values);
            }
        }
    }
    let mut values = vec![];
    visit(ast, key, &mut values);
    values
}
pub(crate) fn scalar_text(ast: &Ast) -> Option<String> {
    match ast {
        Ast::Atom(Atom::String(s) | Atom::Symbol(s)) => Some(s.clone()),
        Ast::Atom(Atom::Number(n)) => Some(n.to_string()),
        Ast::Atom(Atom::Integer(n)) => Some(n.to_string()),
        _ => None,
    }
}
pub fn parse_vehicle_curve_parameters(ast: &Ast, is_engine: bool) -> VehicleCurveParameters {
    let quantity = |key: &str| {
        field_values(ast, key).first().and_then(|items| {
            let text = items
                .iter()
                .filter_map(scalar_text)
                .collect::<Vec<_>>()
                .join(" ");
            parse_length_m(&text).filter(|v| v.is_finite())
        })
    };
    let category = field_values(ast, "Type")
        .iter()
        .filter_map(|items| items.first().and_then(scalar_text))
        .find(|s| {
            ["Freight", "Passenger", "Carriage", "Engine", "Tender"]
                .iter()
                .any(|t| s.eq_ignore_ascii_case(t))
        });
    let default = match category.as_deref().map(str::to_ascii_lowercase).as_deref() {
        Some("engine" | "tender") => 0.1524,
        Some("passenger" | "carriage" | "freight") => 0.0762,
        _ if is_engine => 0.1524,
        _ => 0.000254,
    };
    VehicleCurveParameters {
        track_gauge_m: quantity("ORTSTrackGauge").filter(|g| *g > 0.),
        // MSTSWagon.Initialize uses a default for zero or > 0.5 m. A negative
        // authoring value is invalid geometry and is also rejected here.
        max_unbalanced_m: quantity("ORTSUnbalancedSuperElevation")
            .filter(|v| *v > 0. && *v <= 0.5)
            .unwrap_or(default),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imperial_gauge_and_explicit_cant_deficiency_are_preserved() {
        let ast = crate::parse_vehicle_text("Wagon ( x Type ( Passenger ) ORTSTrackGauge ( 4ft 8.5in ) ORTSUnbalancedSuperElevation ( 3in ) )").unwrap();
        let p = parse_vehicle_curve_parameters(&ast, false);
        assert!((p.track_gauge_m.unwrap() - 1.4351).abs() < 1e-8);
        assert!((p.max_unbalanced_m - 0.0762).abs() < 1e-8);
    }
    #[test]
    fn defaults_follow_pinned_code_including_freight_not_the_older_pdf() {
        for (category, expected) in [
            ("Freight", 0.0762),
            ("Passenger", 0.0762),
            ("Carriage", 0.0762),
            ("Engine", 0.1524),
            ("Tender", 0.1524),
        ] {
            let ast = crate::parse_vehicle_text(&format!(
                "Wagon ( x Type ( {category} ) ORTSUnbalancedSuperElevation ( 0m ) )"
            ))
            .unwrap();
            assert_eq!(
                parse_vehicle_curve_parameters(&ast, false).max_unbalanced_m,
                expected
            );
        }
    }
    #[test]
    fn balance_formula_and_zero_cant_turnout() {
        let p = VehicleCurveParameters {
            track_gauge_m: Some(1.435),
            max_unbalanced_m: 0.075,
        };
        let c = p.evaluate(500., 0.15, 0., 1.676).unwrap();
        assert!((c.comfortable_speed_mps * 3.6 - 99.81898494824298).abs() < 1e-6);
        let balanced = p
            .evaluate(500., 0.15, c.equilibrium_speed_mps, 1.676)
            .unwrap();
        assert!(balanced.lateral_acceleration_mps2.abs() < 1e-10);
        assert!(
            p.evaluate(200., 0., 20., 1.435)
                .unwrap()
                .comfortable_speed_mps
                > 0.
        );
        assert!(p.evaluate(0., 0.1, 20., 1.435).is_none());
        assert!(p.evaluate(100., 1.435, 20., 1.435).is_none());
    }
}
