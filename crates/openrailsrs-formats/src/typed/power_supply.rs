use super::{field_values, scalar_text};
use crate::{Ast, FormatError};
use openrailsrs_core::power_supply::PowerSupplyParams;

/// Authored OR passenger power, in watts. Absent fields consume no power.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PassengerPowerParams {
    pub continuous_w: f64,
    pub heating_w: f64,
    pub cooling_w: f64,
}

pub fn parse_passenger_power(ast: &Ast) -> Result<PassengerPowerParams, FormatError> {
    let watts = |key: &str| -> Result<f64, FormatError> {
        let Some(items) = field_values(ast, key).last().cloned() else {
            return Ok(0.0);
        };
        let raw = items
            .iter()
            .filter_map(scalar_text)
            .collect::<String>()
            .to_ascii_lowercase();
        let raw = raw.replace([' ', '*', '(', ')'], "");
        let (value, scale) = if let Some(v) = raw.strip_suffix("kw") {
            (v, 1000.0)
        } else if let Some(v) = raw.strip_suffix("mw") {
            (v, 1_000_000.0)
        } else if let Some(v) = raw.strip_suffix("hp") {
            (v, 745.699872)
        } else {
            (raw.strip_suffix('w').unwrap_or(&raw), 1.0)
        };
        value
            .parse::<f64>()
            .ok()
            .map(|v| v * scale)
            .filter(|v| v.is_finite() && (0.0..=10_000_000.0).contains(v))
            .ok_or_else(|| FormatError::UnexpectedAtom {
                key: key.into(),
                context: "passenger power supply".into(),
                expected: "nonnegative power in W, kW, MW or hp".into(),
            })
    };
    Ok(PassengerPowerParams {
        continuous_w: watts("ORTSPowerSupplyContinuousPower")?,
        heating_w: watts("ORTSPowerSupplyHeatingPower")?,
        cooling_w: watts("ORTSPowerSupplyAirConditioningPower")?,
    })
}

pub fn parse_power_supply(ast: &Ast) -> Result<PowerSupplyParams, FormatError> {
    let number = |key: &str, time: bool| -> Result<f64, FormatError> {
        let Some(value) = field_values(ast, key).last().cloned() else {
            return Ok(0.);
        };
        let raw = value
            .iter()
            .filter_map(scalar_text)
            .collect::<String>()
            .to_ascii_lowercase();
        let (raw, scale) = if time {
            raw.strip_suffix("ms")
                .map_or((raw.strip_suffix('s').unwrap_or(&raw), 1.), |v| (v, 0.001))
        } else {
            (raw.as_str(), 1.)
        };
        let value = raw
            .parse::<f64>()
            .ok()
            .map(|v| v * scale)
            .filter(|v| v.is_finite() && *v >= 0.);
        value.ok_or_else(|| FormatError::UnexpectedAtom {
            key: key.into(),
            context: "default power supply".into(),
            expected: "finite nonnegative value in seconds or RPM".into(),
        })
    };
    let train_supply = field_values(ast, "ORTSElectricTrainSupply").last().cloned();
    let (fitted, manual, rpm) = if let Some(items) = train_supply {
        let block = Ast::List(items.to_vec());
        let mode = field_values(&block, "Mode")
            .last()
            .and_then(|v| v.first())
            .and_then(scalar_text);
        let rpm = field_values(&block, "DieselEngineMinRPM")
            .last()
            .and_then(|v| v.first())
            .and_then(scalar_text)
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v >= 0.)
            .unwrap_or(0.);
        (
            mode.as_ref()
                .is_none_or(|m| !m.eq_ignore_ascii_case("Unfitted")),
            mode.is_some_and(|m| m.eq_ignore_ascii_case("Switch")),
            rpm,
        )
    } else {
        (
            field_values(ast, "Type")
                .last()
                .and_then(|v| v.first())
                .and_then(scalar_text)
                .is_some_and(|s| s.eq_ignore_ascii_case("Electric")),
            false,
            0.,
        )
    };
    Ok(PowerSupplyParams {
        main_delay_s: number("ORTSPowerOnDelay", true)?,
        auxiliary_delay_s: number("ORTSAuxPowerOnDelay", true)?,
        relay_delay_s: number("ORTSTractionCutOffRelayClosingDelay", true)?,
        train_supply_fitted: fitted,
        manual_train_supply: manual,
        train_supply_min_rpm: rpm,
    })
}

#[cfg(test)]
mod passenger_tests {
    use super::*;
    #[test]
    fn passenger_power_units_and_invalid_demand() {
        let parse = |s: &str| parse_passenger_power(&crate::parse_vehicle_text(s).unwrap());
        let p = parse("Wagon ( Coach ORTSPowerSupplyContinuousPower ( 8kW ) ORTSPowerSupplyHeatingPower ( 0.06MW ) ORTSPowerSupplyAirConditioningPower ( 80000W ) )").unwrap();
        assert_eq!(p.continuous_w, 8000.0);
        assert_eq!(p.heating_w, 60000.0);
        assert_eq!(p.cooling_w, 80000.0);
        assert_eq!(
            parse("Wagon ( Coach )").unwrap(),
            PassengerPowerParams::default()
        );
        for invalid in ["-1kW", "NaN", "1e99W", "12bar"] {
            assert!(
                parse(&format!(
                    "Wagon ( Coach ORTSPowerSupplyHeatingPower ({invalid}) )"
                ))
                .is_err()
            );
        }
    }
}
