//! Authored MSTS/OR gearbox parameters. Unitless gear speeds are MPH in OR 1.6.1.
use super::{field_values, scalar_text};
use crate::{Ast, FormatError};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GearboxOperation {
    #[default]
    Manual,
    Automatic,
    Semiautomatic,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GearboxKind {
    #[default]
    Unknown,
    A,
    B,
    C,
    D,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GearboxClutch {
    #[default]
    Unknown,
    Friction,
    Fluid,
    Scoop,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NativeGearboxProfile {
    pub operation: GearboxOperation,
    pub kind: GearboxKind,
    pub clutch: GearboxClutch,
    pub mechanical: bool,
    pub reverse_indication: bool,
    pub freewheel: bool,
    pub direct_drive_gear: usize,
    pub max_speeds_mps: Vec<f64>,
    pub max_forces_n: Vec<f64>,
    pub forces_at_max_speed: bool,
    pub overspeed_percent: f64,
    pub back_load_force_n: f64,
    pub coasting_force_n: f64,
    pub up_proportion: f64,
    pub down_proportion: f64,
    pub wheel_radius_m: f64,
    /// Authored torque table, or OR's normalized default (only its ratio matters).
    pub torque_tab: Vec<(f64, f64)>,
}

fn invalid(key: &str) -> FormatError {
    FormatError::UnexpectedAtom {
        key: key.into(),
        context: "native diesel gearbox".into(),
        expected: "complete finite gearbox parameters and supported units".into(),
    }
}

fn values(ast: &Ast, key: &str) -> Vec<String> {
    field_values(ast, key)
        .last()
        .map(|v| v.iter().filter_map(scalar_text).collect())
        .unwrap_or_default()
}

fn text(ast: &Ast, key: &str) -> Option<String> {
    values(ast, key).first().cloned()
}

fn scalar(ast: &Ast, key: &str, default: f64) -> Result<f64, FormatError> {
    let Some(v) = text(ast, key) else {
        return Ok(default);
    };
    let number = v.parse::<f64>().map_err(|_| invalid(key))?;
    if number.is_finite() {
        Ok(number)
    } else {
        Err(invalid(key))
    }
}

fn quantity(raw: &str, speed: bool) -> Option<f64> {
    if let Ok(number) = raw.parse::<f64>() {
        return Some(if speed { number * 0.44704 } else { number });
    }
    let units: &[(&str, f64)] = if speed {
        &[
            ("mph", 0.44704),
            ("km/h", 1. / 3.6),
            ("kmh", 1. / 3.6),
            ("ft/s", 0.3048),
            ("m/s", 1.),
            ("mps", 1.),
        ]
    } else {
        &[("kn", 1000.), ("lbf", 4.448_221_615_260_5), ("n", 1.)]
    };
    let lower = raw.to_ascii_lowercase();
    units.iter().find_map(|(unit, scale)| {
        lower
            .strip_suffix(unit)?
            .trim()
            .parse::<f64>()
            .ok()
            .map(|v| v * scale)
    })
}

fn table(ast: &Ast, key: &str, count: usize, speed: bool) -> Result<Vec<f64>, FormatError> {
    let mut joined: Vec<String> = vec![];
    for token in values(ast, key) {
        if matches!(
            token.to_ascii_lowercase().as_str(),
            "mph" | "km/h" | "kmh" | "ft/s" | "m/s" | "mps" | "n" | "kn" | "lbf"
        ) && joined.last().is_some_and(|s| s.parse::<f64>().is_ok())
        {
            joined.last_mut().unwrap().push_str(&token);
        } else {
            joined.push(token);
        }
    }
    if joined.len() != count {
        return Err(invalid(key));
    }
    joined
        .iter()
        .map(|s| {
            let v = quantity(s, speed).ok_or_else(|| invalid(key))?;
            if v.is_finite() && v > 0. {
                Ok(v)
            } else {
                Err(invalid(key))
            }
        })
        .collect()
}

pub fn parse_native_gearbox(
    ast: &Ast,
    idle_rpm: f64,
    max_rpm: f64,
) -> Result<Option<NativeGearboxProfile>, FormatError> {
    let Some(raw_count) = text(ast, "GearBoxNumberOfGears") else {
        return Ok(None);
    };
    let count = raw_count
        .parse::<usize>()
        .map_err(|_| invalid("GearBoxNumberOfGears"))?;
    if !(1..=32).contains(&count)
        || !idle_rpm.is_finite()
        || !max_rpm.is_finite()
        || idle_rpm < 0.
        || max_rpm <= idle_rpm
    {
        return Err(invalid("GearBoxNumberOfGears / engine RPM"));
    }
    let operation = match text(ast, "GearBoxOperation")
        .unwrap_or_else(|| "manual".into())
        .to_ascii_lowercase()
        .as_str()
    {
        "manual" => GearboxOperation::Manual,
        "automatic" => GearboxOperation::Automatic,
        "semiautomatic" => GearboxOperation::Semiautomatic,
        _ => return Err(invalid("GearBoxOperation")),
    };
    let kind = match text(ast, "ORTSGearBoxType")
        .unwrap_or_else(|| "unknown".into())
        .to_ascii_lowercase()
        .as_str()
    {
        "unknown" => GearboxKind::Unknown,
        "a" => GearboxKind::A,
        "b" => GearboxKind::B,
        "c" => GearboxKind::C,
        "d" => GearboxKind::D,
        _ => return Err(invalid("ORTSGearBoxType")),
    };
    let clutch = match text(ast, "ORTSMainClutchType")
        .unwrap_or_else(|| "unknown".into())
        .to_ascii_lowercase()
        .as_str()
    {
        "unknown" => GearboxClutch::Unknown,
        "friction" => GearboxClutch::Friction,
        "fluid" => GearboxClutch::Fluid,
        "scoop" => GearboxClutch::Scoop,
        _ => return Err(invalid("ORTSMainClutchType")),
    };
    let mechanical =
        text(ast, "ORTSDieselTransmissionType").is_some_and(|s| s.eq_ignore_ascii_case("mechanic"));
    let direct = scalar(ast, "GearBoxDirectDriveGear", 1.)?;
    let up = scalar(ast, "GearBoxUpGearProportion", 0.85)?;
    let down = scalar(ast, "GearBoxDownGearProportion", 0.35)?;
    let overspeed = scalar(ast, "GearBoxOverspeedPercentageForFailure", 150.)?;
    if direct.fract() != 0.
        || !(1.0..=count as f64).contains(&direct)
        || !(0.0..1.0).contains(&down)
        || !(down..=1.0).contains(&up)
        || up <= down
        || overspeed < 100.
    {
        return Err(invalid("gear limits"));
    }
    let max_speeds_mps = table(ast, "GearBoxMaxSpeedForGears", count, true)?;
    if max_speeds_mps.windows(2).any(|p| p[0] >= p[1]) {
        return Err(invalid("GearBoxMaxSpeedForGears"));
    }
    let forces_at_max_speed = !values(ast, "ORTSGearBoxTractiveForceAtSpeed").is_empty();
    let max_forces_n = table(
        ast,
        if forces_at_max_speed {
            "ORTSGearBoxTractiveForceAtSpeed"
        } else {
            "GearBoxMaxTractiveForceForGears"
        },
        count,
        false,
    )?;
    let force = |key, default| -> Result<f64, FormatError> {
        let tokens = values(ast, key);
        let v = if tokens.is_empty() {
            Some(default)
        } else {
            quantity(&tokens.join(""), false)
        }
        .ok_or_else(|| invalid(key))?;
        if v.is_finite() && v >= 0. {
            Ok(v)
        } else {
            Err(invalid(key))
        }
    };
    let wheel_values = values(ast, "WheelRadius");
    let wheel_radius_m = if wheel_values.is_empty() {
        0.5
    } else {
        crate::parse_length_m(&wheel_values.join("")).ok_or_else(|| invalid("WheelRadius"))?
    };
    if !wheel_radius_m.is_finite() || wheel_radius_m <= 0. {
        return Err(invalid("WheelRadius"));
    }
    let authored_torque = values(ast, "DieselTorqueTab");
    let torque_tab = if authored_torque.is_empty() {
        let levels = [
            0.2, 0.4, 0.7, 0.95, 1., 1., 0.98, 0.95, 0.9, 0.86, 0.81, 0.2,
        ];
        levels
            .iter()
            .enumerate()
            .map(|(i, &torque)| {
                (
                    if i == 11 {
                        max_rpm * 1.5
                    } else {
                        idle_rpm + i as f64 * (max_rpm - idle_rpm) / 10.
                    },
                    torque,
                )
            })
            .collect()
    } else {
        if authored_torque.len() < 4 || !authored_torque.len().is_multiple_of(2) {
            return Err(invalid("DieselTorqueTab"));
        }
        let numbers: Vec<f64> = authored_torque
            .iter()
            .map(|s| s.parse().map_err(|_| invalid("DieselTorqueTab")))
            .collect::<Result<_, _>>()?;
        let points: Vec<_> = numbers.chunks_exact(2).map(|p| (p[0], p[1])).collect();
        if points
            .iter()
            .any(|p| !p.0.is_finite() || !p.1.is_finite() || p.0 < 0. || p.1 < 0.)
            || points.windows(2).any(|p| p[0].0 >= p[1].0)
            || points.iter().all(|p| p.1 == 0.)
        {
            return Err(invalid("DieselTorqueTab"));
        }
        points
    };
    if forces_at_max_speed {
        let pair = torque_tab
            .windows(2)
            .find(|p| max_rpm <= p[1].0)
            .unwrap_or(&torque_tab[torque_tab.len() - 2..]);
        let at_max =
            pair[0].1 + (max_rpm - pair[0].0) / (pair[1].0 - pair[0].0) * (pair[1].1 - pair[0].1);
        if !at_max.is_finite() || at_max <= 0. {
            return Err(invalid("ORTSGearBoxTractiveForceAtSpeed / DieselTorqueTab"));
        }
    }
    Ok(Some(NativeGearboxProfile {
        operation,
        kind,
        clutch,
        mechanical,
        reverse_indication: scalar(ast, "ORTSReverseGearBoxIndication", 0.)? == 1.,
        freewheel: scalar(ast, "ORTSGearBoxFreeWheel", 0.)? == 1.,
        direct_drive_gear: direct as usize,
        max_speeds_mps,
        max_forces_n,
        forces_at_max_speed,
        overspeed_percent: overspeed,
        back_load_force_n: force("GearBoxBackLoadForce", 1000.)?,
        coasting_force_n: force("GearBoxCoastingForce", 500.)?,
        up_proportion: up,
        down_proportion: down,
        wheel_radius_m,
        torque_tab,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_gear_speeds_use_mph_but_preserve_explicit_units_and_override_order() {
        let ast = crate::parse_vehicle_text("Engine ( e GearBoxNumberOfGears ( 2 ) GearBoxOperation ( Manual ) GearBoxMaxSpeedForGears ( 15 60km/h ) GearBoxMaxTractiveForceForGears ( 40kN 20000N ) GearBoxUpGearProportion ( 0.8 ) GearBoxUpGearProportion ( 0.9 ) )").unwrap();
        let p = parse_native_gearbox(&ast, 500., 1900.).unwrap().unwrap();
        assert!((p.max_speeds_mps[0] - 6.7056).abs() < 1e-9);
        assert!((p.max_speeds_mps[1] - 60. / 3.6).abs() < 1e-9);
        assert_eq!(p.max_forces_n, [40000., 20000.]);
        assert_eq!(p.up_proportion, 0.9);
    }
    #[test]
    fn incomplete_and_nonfinite_gears_are_diagnosed_instead_of_using_ungeared_power() {
        for fields in [
            "GearBoxNumberOfGears ( 2 )",
            "GearBoxNumberOfGears ( 2 ) GearBoxMaxSpeedForGears ( 20 10 ) GearBoxMaxTractiveForceForGears ( 1000 500 )",
            "GearBoxNumberOfGears ( 2 ) GearBoxMaxSpeedForGears ( 10 20 ) GearBoxMaxTractiveForceForGears ( 1000 NaN )",
        ] {
            let ast = crate::parse_vehicle_text(&format!("Engine ( e {fields} )")).unwrap();
            assert!(parse_native_gearbox(&ast, 500., 1900.).is_err());
        }
    }
    #[test]
    fn separated_units_and_zero_torque_at_a_force_rating_are_checked() {
        let fields = "GearBoxNumberOfGears ( 1 ) GearBoxMaxSpeedForGears ( 20 mph ) GearBoxMaxTractiveForceForGears ( 30 kN ) GearBoxBackLoadForce ( 1 kN ) WheelRadius ( 18 in )";
        let ast = crate::parse_vehicle_text(&format!("Engine ( e {fields} )")).unwrap();
        let profile = parse_native_gearbox(&ast, 500., 1900.).unwrap().unwrap();
        assert_eq!(profile.back_load_force_n, 1000.);
        assert!((profile.wheel_radius_m - 0.4572).abs() < 1e-8);
        assert!(parse_native_gearbox(&ast, 500., f64::NAN).is_err());
        let ast = crate::parse_vehicle_text(&format!("Engine ( e {fields} ORTSGearBoxTractiveForceAtSpeed ( 30kN ) DieselTorqueTab ( 500 1 1900 0 ) )")).unwrap();
        assert!(parse_native_gearbox(&ast, 500., 1900.).is_err());
    }
}
