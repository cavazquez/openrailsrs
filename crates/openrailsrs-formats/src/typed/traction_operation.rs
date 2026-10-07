//! Authored consumables and operating controls, separate from traction curves.
use super::{field_values, scalar_text};
use crate::{Ast, FormatError};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DieselOperatingParams {
    pub capacity_l: f64,
    pub starting_rpm: f64,
    pub confirmation_rpm: f64,
    /// RPM → litres/hour. Native OR tables are not g/kWh.
    pub consumption_lph: Vec<(f64, f64)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SteamOperatingParams {
    pub boiler_water_capacity_kg: f64,
    pub max_fire_mass_kg: f64,
    pub max_firing_rate_kg_s: f64,
    pub injector_rate_kg_s: f64,
}
impl Default for SteamOperatingParams {
    fn default() -> Self {
        Self {
            boiler_water_capacity_kg: 4000.,
            max_fire_mass_kg: 300.,
            max_firing_rate_kg_s: 1.,
            injector_rate_kg_s: 5.,
        }
    }
}

fn invalid(key: &str) -> FormatError {
    FormatError::UnexpectedAtom {
        key: key.into(),
        context: "traction operating parameters".into(),
        expected: "finite nonnegative quantity with supported units".into(),
    }
}

fn scalar_values(values: &[Ast]) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        if let Some(text) = scalar_text(value) {
            out.push(text);
        } else if let Ast::List(items) = value {
            out.extend(scalar_values(items));
        }
    }
    out
}

/// Strict quantities used for resources; unknown suffixes must not become litres.
pub(super) fn quantity(
    ast: &Ast,
    key: &str,
    default_unit: &str,
) -> Result<Option<f64>, FormatError> {
    let fields = field_values(ast, key);
    // Native STF processes repeated declarations in order; the final value
    // overrides earlier stock defaults (also after Include expansion).
    let Some(values) = fields.last() else {
        return Ok(None);
    };
    let scalars = scalar_values(values);
    if scalars.is_empty() || scalars.len() > 2 {
        return Err(invalid(key));
    }
    let raw = scalars.concat().to_ascii_lowercase();
    let split = raw.find(|c: char| !c.is_ascii_digit() && !matches!(c, '.' | '-' | '+' | 'e'));
    let (number, unit) = split.map_or((raw.as_str(), default_unit), |i| (&raw[..i], &raw[i..]));
    // Native STF also writes quoted volume units such as "225*(ft^3)".
    // Normalize only the suffix; no expression is evaluated.
    let unit = unit.replace(['*', '(', ')', '^'], "");
    let scale = match (default_unit, unit.as_str()) {
        ("l" | "ft3", "l") | ("kg", "kg") | ("rpm", "rpm") => 1.,
        ("l" | "ft3", "g-uk" | "gal-uk") => 4.54609,
        ("l" | "ft3", "g-us" | "gal-us" | "gal" | "gals" | "gallon" | "gallons") => 3.785411784,
        ("l" | "ft3", "m3") => 1000.,
        ("l" | "ft3", "ft3") => 28.316846592,
        ("l" | "ft3", "in3") => 0.016387064,
        ("kg", "lb") => 0.45359237,
        ("kg", "t") => 1000.,
        ("kg", "t-uk") => 1016.05,
        ("kg", "t-us") => 907.18474,
        ("psi", "bar") => 1.,
        ("psi", "psi") => 0.06894757293168,
        ("psi", "kpa") => 0.01,
        ("psi", "inhg") => 0.0338639,
        ("psi", "cmhg") => 0.0133322,
        ("lb/h", "kg/s") => 1.,
        ("lb/h", "lb/h") => 0.45359237 / 3600.,
        ("lb/h", "kg/h") => 1. / 3600.,
        ("lb/h", "g/h") => 0.001 / 3600.,
        ("m/s", "m/s" | "mps") => 1.,
        ("m/s", "km/h" | "kmh") => 1. / 3.6,
        ("m/s", "mph") => 0.44704,
        ("ft3/s", "ft3" | "ft3/s") => 28.316846592,
        ("ft3/s", "ft3/h") => 28.316846592 / 3600.,
        ("ft3/s", "m3" | "m3/s") => 1000.,
        ("ft3/s", "m3/h") => 1000. / 3600.,
        ("ft3/s", "l" | "l/s") => 1.,
        ("ft3/s", "l/h") => 1. / 3600.,
        ("ft3/s", "in3" | "in3/s") => 0.016387064,
        ("ft3/s", "g-uk" | "g-uk/s") => 4.54609,
        ("ft3/s", "g-us" | "g-us/s") => 3.785411784,
        _ => return Err(invalid(key)),
    };
    let value = number.parse::<f64>().map_err(|_| invalid(key))? * scale;
    if !value.is_finite() || value < 0. {
        return Err(invalid(key));
    }
    Ok(Some(value))
}

pub(super) fn parse_diesel(
    ast: &Ast,
    idle: f64,
    max: f64,
) -> Result<Option<DieselOperatingParams>, FormatError> {
    if !super::parse_vehicle_content_metadata(ast, true)
        .engine_type
        .is_some_and(|v| v.eq_ignore_ascii_case("diesel"))
    {
        return Ok(None);
    }
    let idle = if idle > 0. { idle } else { 300. };
    let max = if max > idle {
        max
    } else {
        600_f64.max(idle * 1.5)
    };
    let fields = field_values(ast, "DieselConsumptionTab");
    let mut consumption = Vec::new();
    if let Some(values) = fields.first() {
        let numbers: Vec<_> = scalar_values(values)
            .into_iter()
            .map(|s| {
                s.parse::<f64>()
                    .map_err(|_| invalid("DieselConsumptionTab"))
            })
            .collect::<Result<_, _>>()?;
        if numbers.len() < 4 || numbers.len() % 2 != 0 {
            return Err(invalid("DieselConsumptionTab"));
        }
        for pair in numbers.as_chunks::<2>().0 {
            if pair.iter().any(|v| !v.is_finite() || *v < 0.)
                || consumption.last().is_some_and(|(r, _)| *r >= pair[0])
            {
                return Err(invalid("DieselConsumptionTab"));
            }
            consumption.push((pair[0], pair[1]));
        }
    } else {
        consumption = vec![
            (
                idle,
                quantity(ast, "DieselUsedPerHourAtIdle", "l")?.unwrap_or(1.),
            ),
            (
                max,
                quantity(ast, "DieselUsedPerHourAtMaxPower", "l")?.unwrap_or(1.),
            ),
        ];
    }
    let starting_rpm = quantity(ast, "StartingRPM", "rpm")?.unwrap_or(idle * 2. / 3.);
    let confirmation_rpm = quantity(ast, "StartingConfirmRPM", "rpm")?.unwrap_or(idle * 1.1);
    if starting_rpm <= 0. || confirmation_rpm <= starting_rpm || confirmation_rpm >= max {
        return Err(invalid("StartingRPM / StartingConfirmRPM"));
    }
    Ok(Some(DieselOperatingParams {
        capacity_l: quantity(ast, "MaxDieselLevel", "l")?.unwrap_or(5000.),
        starting_rpm,
        confirmation_rpm,
        consumption_lph: consumption,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EngineFile, parse_vehicle_text};

    #[test]
    fn native_resources_use_correct_units_and_distinct_boiler_and_tender() {
        let ast = parse_vehicle_text(
            r#"
Wagon ( engine Mass ( 82000 ) WheelRadius ( 0.75m ) )
Engine ( engine Type ( Steam ) NumberOfCylinders ( 2 )
    CylinderDiameter ( 16in ) CylinderStroke ( 26in ) WheelRadius ( 0.5m )
    MaxBoilerPressure ( 225 ) BoilerVolume ( "225*(ft^3)" )
    MaxTenderWaterMass ( 40000lb ) MaxTenderCoalMass ( 13440lb )
    MaxFireMass ( 2400lb ) SteamFiremanMaxPossibleFiringRate ( 3600lb/h ) )
"#,
        )
        .unwrap();
        let s = EngineFile::from_ast(&ast).unwrap().steam.unwrap();
        assert_eq!(s.cylinder_count, 2);
        assert_eq!(
            s.driving_wheel_radius_m, 0.5,
            "engine wheels must not inherit wagon wheel radius"
        );
        assert!((s.cylinder_bore_m - 0.4064).abs() < 1e-8);
        assert!((s.working_pressure_bar - 15.51320391).abs() < 1e-7);
        assert!((s.operation.boiler_water_capacity_kg - 6371.2904832).abs() < 1e-7);
        assert!((s.initial_water_kg - 18143.6948).abs() < 1e-7);
        assert!((s.initial_coal_kg - 6096.2814528).abs() < 1e-7);
        assert!((s.operation.max_firing_rate_kg_s - 0.45359237).abs() < 1e-9);
    }

    #[test]
    fn a_driving_wheel_diameter_below_two_metres_is_still_a_diameter() {
        let ast = parse_vehicle_text("(Engine (Mass 82000) (NumberOfCylinders 2) (CylinderDiameter 0.47m) (CylinderStroke 0.66m) (DrivingWheelDiameter 1.94m) (MaxBoilerPressure (16bar)))").unwrap();
        let s = EngineFile::from_ast(&ast).unwrap().steam.unwrap();
        assert_eq!(s.driving_wheel_radius_m, 0.97);
        assert_eq!(s.working_pressure_bar, 16.);
    }

    #[test]
    fn diesel_native_gallons_are_us_and_tables_and_start_thresholds_are_authored() {
        let ast = parse_vehicle_text("Engine ( e Type ( Diesel ) MaxDieselLevel ( 500gal ) StartingRPM ( 180 ) StartingConfirmRPM ( 350 ) DieselConsumptionTab ( 0 0 300 18 900 180 ) )").unwrap();
        let p = parse_diesel(&ast, 300., 900.).unwrap().unwrap();
        assert!((p.capacity_l - 1892.705892).abs() < 1e-6);
        assert_eq!(p.starting_rpm, 180.);
        assert_eq!(p.confirmation_rpm, 350.);
        assert_eq!(p.consumption_lph, vec![(0., 0.), (300., 18.), (900., 180.)]);
        let legacy = parse_vehicle_text("Engine ( e Type ( Diesel ) MaxDieselLevel ( 500g-uk ) DieselUsedPerHourAtIdle ( 2gal ) DieselUsedPerHourAtMaxPower ( 15gal ) )").unwrap();
        let p = parse_diesel(&legacy, 150., 1800.).unwrap().unwrap();
        assert!((p.capacity_l - 2273.045).abs() < 1e-6);
        assert!((p.consumption_lph[0].1 - 7.570823568).abs() < 1e-8);
        assert_eq!(p.starting_rpm, 100.);
        assert_eq!(p.confirmation_rpm, 165.);
    }

    #[test]
    fn invalid_resource_units_tables_and_geometry_are_rejected() {
        for field in [
            "MaxDieselLevel ( 2kg )",
            "MaxDieselLevel ( -2l )",
            "MaxDieselLevel ( 5bananas )",
            "StartingRPM ( 0 )",
            "StartingConfirmRPM ( 100 )",
            "DieselConsumptionTab ( 300 18 300 180 )",
            "DieselConsumptionTab ( 300 -18 900 180 )",
        ] {
            let ast = parse_vehicle_text(&format!("Engine ( e Type ( Diesel ) {field} )")).unwrap();
            assert!(parse_diesel(&ast, 300., 900.).is_err(), "accepted {field}");
        }
        let ast = parse_vehicle_text("Engine ( e Type ( Steam ) Mass ( 82000 ) NumCylinders ( 2 ) CylinderDiameter ( 0.47m ) CylinderStroke ( 0.66m ) WheelRadius ( 0m ) MaxBoilerPressure ( 225 ) )").unwrap();
        assert!(EngineFile::from_ast(&ast).is_err());
        for count in ["0", "-2", "2.5"] {
            let ast = parse_vehicle_text(&format!("Engine ( e Type ( Steam ) Mass ( 82000 ) NumCylinders ( {count} ) CylinderDiameter ( 0.47m ) CylinderStroke ( 0.66m ) WheelRadius ( 0.97m ) MaxBoilerPressure ( 225 ) )")).unwrap();
            assert!(
                EngineFile::from_ast(&ast).is_err(),
                "accepted cylinder count {count}"
            );
        }
    }

    #[test]
    fn repeated_resource_declarations_keep_the_final_native_override() {
        let ast = parse_vehicle_text(
            "Engine ( e Type ( Diesel ) MaxDieselLevel ( 872gal ) MaxDieselLevel ( 830gal ) )",
        )
        .unwrap();
        let p = parse_diesel(&ast, 315., 1400.).unwrap().unwrap();
        assert!((p.capacity_l - 830. * 3.785411784).abs() < 1e-8);
    }
}
