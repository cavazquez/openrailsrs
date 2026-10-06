use super::{field_values, named_blocks, scalar_text};
use crate::{Ast, FormatError};
use openrailsrs_core::electrification::{ElectricPickup, ElectricVehicleParams};

pub fn parse_electric_vehicle(ast: &Ast) -> Result<Option<ElectricVehicleParams>, FormatError> {
    if !super::parse_vehicle_content_metadata(ast, true)
        .engine_type
        .is_some_and(|v| v.eq_ignore_ascii_case("electric"))
    {
        return Ok(None);
    }
    let mut params = ElectricVehicleParams::default();
    if let Some(value) = field_values(ast, "ORTSRSPickup")
        .first()
        .and_then(|v| v.first())
        .and_then(scalar_text)
    {
        params.pickup = match value.to_ascii_lowercase().as_str() {
            "overhead" => ElectricPickup::Overhead,
            "third_rail" => ElectricPickup::ThirdRail,
            "fourth_rail" => ElectricPickup::FourthRail,
            _ => return Err(invalid("ORTSRSPickup")),
        };
    }
    params.breaker_delay_s = number(ast, "ORTSCircuitBreakerClosingDelay", true)?.unwrap_or(0.);
    params.power_on_delay_s = number(ast, "ORTSPowerOnDelay", true)?.unwrap_or(0.);
    if let Some(pantographs) = named_blocks(ast, "ORTSPantographs").first()
        && let Some(pantograph) = named_blocks(pantographs, "Pantograph").first()
    {
        params.pantograph_delay_s = number(pantograph, "Delay", true)?.unwrap_or(0.);
    }
    params.minimum_voltage_v = number(ast, "ORTSRSMinimumVoltage", false)?.unwrap_or(1.);
    params.maximum_voltage_v = number(ast, "ORTSRSMaximumVoltage", false)?;
    if params
        .maximum_voltage_v
        .is_some_and(|v| v < params.minimum_voltage_v)
    {
        return Err(invalid("ORTSRSMaximumVoltage"));
    }
    Ok(Some(params))
}

fn invalid(key: &str) -> FormatError {
    FormatError::UnexpectedAtom {
        key: key.into(),
        context: "electric supply".into(),
        expected: "a finite nonnegative value with supported units".into(),
    }
}

fn number(ast: &Ast, key: &str, time: bool) -> Result<Option<f64>, FormatError> {
    let fields = field_values(ast, key);
    let Some(field) = fields.first() else {
        return Ok(None);
    };
    let raw = field
        .iter()
        .filter_map(scalar_text)
        .collect::<String>()
        .to_ascii_lowercase();
    let (raw, scale) = if time {
        if let Some(v) = raw.strip_suffix("ms") {
            (v, 0.001)
        } else {
            (raw.strip_suffix('s').unwrap_or(&raw), 1.)
        }
    } else if let Some(v) = raw.strip_suffix("kv") {
        (v, 1000.)
    } else {
        (raw.strip_suffix('v').unwrap_or(&raw), 1.)
    };
    let value = raw.parse::<f64>().map_err(|_| invalid(key))? * scale;
    if !value.is_finite() || value < 0. {
        return Err(invalid(key));
    }
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn electric_type_and_native_delays_are_retained() {
        let ast = crate::parse_vehicle_text("Engine ( e Type ( Electric ) ORTSPowerOnDelay ( 200ms ) ORTSCircuitBreakerClosingDelay ( 1 s ) ORTSPantographs ( Pantograph ( Delay ( 3s ) ) ) )").unwrap();
        let p = parse_electric_vehicle(&ast).unwrap().unwrap();
        assert_eq!(p.pickup, ElectricPickup::Overhead);
        assert_eq!(p.power_on_delay_s, 0.2);
        assert_eq!(p.breaker_delay_s, 1.);
        assert_eq!(p.pantograph_delay_s, 3.);
        let diesel = crate::parse_vehicle_text("Engine ( e Type ( Diesel ) )").unwrap();
        assert!(parse_electric_vehicle(&diesel).unwrap().is_none());
    }
    #[test]
    fn pickup_voltage_and_invalid_input() {
        let ast = crate::parse_vehicle_text("Engine ( e Type ( Electric ) ORTSRSPickup ( fourth_rail ) ORTSRSMinimumVoltage ( 400V ) ORTSRSMaximumVoltage ( 0.8kV ) )").unwrap();
        let p = parse_electric_vehicle(&ast).unwrap().unwrap();
        assert_eq!(p.pickup, ElectricPickup::FourthRail);
        assert_eq!(p.minimum_voltage_v, 400.);
        assert_eq!(p.maximum_voltage_v, Some(800.));
        let invalid =
            crate::parse_vehicle_text("Engine ( e Type ( Electric ) ORTSPowerOnDelay ( -1s ) )")
                .unwrap();
        assert!(parse_electric_vehicle(&invalid).is_err());
    }
}
