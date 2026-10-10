use super::{field_values, scalar_text};
use crate::{Ast, FormatError};
use openrailsrs_core::power_supply::PowerSupplyParams;

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
