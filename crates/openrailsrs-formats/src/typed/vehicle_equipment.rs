//! Equipment declarations from the pinned native STF schema.
use super::{field_values, named_blocks, scalar_text};
use crate::{Ast, FormatError};
#[derive(Clone, Debug, PartialEq)]
pub struct NativeDynamicBrake {
    pub force_ramp_n_s: [f64; 3],
    pub power_ramp_w_s: [f64; 3],
    pub maximum_current_a: Option<f64>,
    pub force_curve: Vec<[f64; 2]>,
    pub notches: Vec<(f64, Vec<[f64; 2]>)>,
    pub delay_s: f64,
}
pub fn parse_native_dynamic_brake(ast: &Ast) -> Result<Option<NativeDynamicBrake>, FormatError> {
    let field = |key: &str| {
        field_values(ast, key).last().map(|v| {
            v.iter()
                .filter_map(scalar_text)
                .collect::<Vec<_>>()
                .join(" ")
        })
    };
    let force = field("DynamicBrakesMaximumForce")
        .and_then(|v| crate::msts_units::parse_force_n(&v))
        .unwrap_or(0.);
    let mut renamed = ast.clone();
    fn rename(ast: &mut Ast) {
        match ast {
            Ast::List(v) => {
                for a in v {
                    rename(a)
                }
            }
            Ast::Atom(crate::Atom::Symbol(s))
                if s.eq_ignore_ascii_case("ORTSMaxTractiveForceCurves") =>
            {
                *s = "IgnoredTractionCurve".into()
            }
            Ast::Atom(crate::Atom::Symbol(s))
                if s.eq_ignore_ascii_case("ORTSDynamicBrakeForceCurves") =>
            {
                *s = "ORTSMaxTractiveForceCurves".into()
            }
            _ => {}
        }
    }
    rename(&mut renamed);
    let tables = super::engine::parse_orts_notch_curves(&renamed);
    // The full-handle curve is preserved. Intermediate declared notches are
    // retained in the native parser, and require the notch extension to select.
    let mut curve = tables
        .last()
        .map(|(_, t)| t.iter().map(|(v, f)| [*v, *f]).collect::<Vec<_>>())
        .unwrap_or_default();
    if curve.is_empty() && force > 0. {
        let speed = |key: &str, default: f64| {
            field(key)
                .and_then(|v| {
                    v.parse::<f64>()
                        .ok()
                        .map(|v| v * 0.44704)
                        .or_else(|| crate::msts_units::parse_velocity_mps(&v))
                })
                .unwrap_or(default)
        };
        let mut v2 = speed("DynamicBrakesFadingSpeed", 30. / 3.6);
        let mut v3 = speed("DynamicBrakesMaximumEffectiveSpeed", 999. / 3.6);
        if v2 > v3 {
            std::mem::swap(&mut v2, &mut v3);
        }
        let v4 = speed("DynamicBrakesMaximumSpeedForFadeOut", 999. / 3.6);
        let ratio = field("DynamicBrakesEffectAtMaximumFadeOut")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.);
        curve = vec![
            [0., 0.],
            [speed("DynamicBrakesMinUsableSpeed", 5. / 3.6), 0.],
            [v2, force],
            [v3, force],
            [v4, force * ratio],
            [v4 + 0.5, 0.],
            [100., 0.],
        ];
        curve.sort_by(|a, b| a[0].total_cmp(&b[0]));
        // Native Interpolator replaces an existing equal abscissa.
        let mut unique: Vec<[f64; 2]> = Vec::new();
        for point in curve {
            if let Some(last) = unique.last_mut()
                && last[0] == point[0]
            {
                *last = point;
            } else {
                unique.push(point);
            }
        }
        curve = unique;
    }
    if curve.is_empty() {
        return Ok(None);
    }
    let delay = field("DynamicBrakesDelayTimeBeforeEngaging")
        .and_then(|v| v.trim_end_matches('s').trim().parse::<f64>().ok())
        .unwrap_or(0.);
    if curve.len() < 2
        || !curve
            .iter()
            .all(|p| p[0].is_finite() && p[0] >= 0. && p[1].is_finite() && p[1] >= 0.)
        || !curve.windows(2).all(|p| p[0][0] < p[1][0])
        || !delay.is_finite()
        || !(0. ..=60.).contains(&delay)
    {
        return Err(FormatError::UnexpectedAtom {
            key: "DynamicBrake".into(),
            context: "Engine".into(),
            expected: "finite ascending speed/force curve".into(),
        });
    }
    let force_rate = |key: &str| {
        field(key)
            .and_then(|s| crate::msts_units::parse_force_n(s.trim_end_matches("/s")))
            .unwrap_or(0.)
    };
    let power_rate = |key: &str| {
        field(key)
            .and_then(|s| crate::msts_units::parse_power_w(s.trim_end_matches("/s")))
            .unwrap_or(0.)
    };
    let up = force_rate("ORTSDynamicBrakeForceRampUpRate");
    let down = force_rate("ORTSDynamicBrakeForceRampDownRate");
    let zero = field("ORTSDynamicBrakeForceRampDownToZeroRate").map_or(down, |_| {
        force_rate("ORTSDynamicBrakeForceRampDownToZeroRate")
    });
    let pup = power_rate("ORTSDynamicBrakePowerRampUpRate");
    let pdown = power_rate("ORTSDynamicBrakePowerRampDownRate");
    let pzero = field("ORTSDynamicBrakePowerRampDownToZeroRate").map_or(pdown, |_| {
        power_rate("ORTSDynamicBrakePowerRampDownToZeroRate")
    });
    Ok(Some(NativeDynamicBrake {
        force_ramp_n_s: [
            up.max(0.),
            down.max(0.),
            if zero < 0. { down.max(0.) } else { zero },
        ],
        power_ramp_w_s: [
            pup.max(0.),
            pdown.max(0.),
            if pzero < 0. { pdown.max(0.) } else { pzero },
        ],
        maximum_current_a: field("DynamicBrakesResistorCurrentLimit")
            .and_then(|s| s.trim_end_matches('A').trim().parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v > 0.),
        force_curve: curve,
        notches: tables
            .iter()
            .map(|(h, t)| (*h, t.iter().map(|(v, f)| [*v, *f]).collect()))
            .collect(),
        delay_s: delay,
    }))
}
#[derive(Clone, Debug, PartialEq)]
pub struct NativeDoors {
    pub opening_s: f64,
    pub closing_s: f64,
}
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMotor {
    pub power_curve: Vec<[f64; 2]>,
    pub throttle_rpm_curve: Vec<[f64; 2]>,
    pub rate_up_rpm_s2: f64,
    pub rate_down_rpm_s2: f64,
    pub idle_rpm: f64,
    pub maximum_rpm: f64,
    pub rise_rpm_s: f64,
    pub fall_rpm_s: f64,
    pub power_w: f64,
}
/// Keep each declared Diesel block separate. Single-engine legacy profiles
/// continue through the existing compatibility model and its reference oracles.
pub fn parse_native_motors(ast: &Ast) -> Result<Vec<NativeMotor>, FormatError> {
    let mut result = Vec::new();
    for group in named_blocks(ast, "ORTSDieselEngines") {
        for block in named_blocks(group, "Diesel") {
            let number = |key: &str, default: f64| {
                field_values(block, key)
                    .last()
                    .and_then(|v| v.first())
                    .and_then(scalar_text)
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(default)
            };
            let idle = number("IdleRPM", 300.);
            let maximum = number("MaxRPM", 600.);
            let mut power = number("MaximalPower", 0.);
            for table in named_blocks(block, "DieselPowerTab") {
                if let Ast::List(items) = table {
                    let values = items
                        .iter()
                        .filter_map(scalar_text)
                        .filter_map(|v| v.parse::<f64>().ok())
                        .collect::<Vec<_>>();
                    for pair in values.chunks_exact(2) {
                        power = power.max(pair[1]);
                    }
                }
            }
            let rise = number("ChangeUpRPMpS", 40.);
            let fall = number("ChangeDownRPMpS", rise);
            if ![idle, maximum, rise, fall, power]
                .iter()
                .all(|v| v.is_finite() && *v >= 0.)
                || maximum <= idle
                || result.len() >= 8
            {
                return Err(FormatError::UnexpectedAtom {
                    key: "Diesel".into(),
                    context: "ORTSDieselEngines".into(),
                    expected: "up to eight finite independent motor profiles".into(),
                });
            }
            let native = match block {
                Ast::List(items) => super::engine::parse_diesel_block_fields(items),
                _ => Default::default(),
            };
            result.push(NativeMotor {
                power_curve: native.power_tab.iter().map(|(a, b)| [*a, *b]).collect(),
                throttle_rpm_curve: native
                    .throttle_rpm_tab
                    .iter()
                    .map(|(a, b)| [*a, *b])
                    .collect(),
                rate_up_rpm_s2: native.rate_of_change_up_rpm_pss.max(rise),
                rate_down_rpm_s2: native.rate_of_change_down_rpm_pss.max(fall),
                idle_rpm: idle,
                maximum_rpm: maximum,
                rise_rpm_s: rise.max(1.),
                fall_rpm_s: fall.max(1.),
                power_w: power,
            });
        }
    }
    Ok(result)
}
/// Authored matrix convention used by OR's wagon viewer. Native car coordinates
/// determine the side, independent of formation Flip and direction of travel.
pub fn door_matrix_side(name: &str) -> Option<usize> {
    let name = name.to_ascii_uppercase();
    if ["DOOR_D", "DOOR_E", "DOOR_F", "DOOR_LEFT"]
        .iter()
        .any(|p| name.starts_with(p))
    {
        Some(0)
    } else if ["DOOR_A", "DOOR_B", "DOOR_C", "DOOR_RIGHT"]
        .iter()
        .any(|p| name.starts_with(p))
    {
        Some(1)
    } else {
        None
    }
}

pub fn collector_matrix_index(name: &str) -> Option<usize> {
    let name = name.trim().to_ascii_uppercase();
    let suffix = name
        .strip_prefix("PANTOGRAPH")
        .or_else(|| name.strip_prefix("PANTO"))?;
    let digits = suffix
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    digits
        .parse::<usize>()
        .ok()
        .filter(|i| (1..=4).contains(i))
        .map(|i| i - 1)
}
pub fn parse_native_doors(ast: &Ast) -> Result<Option<NativeDoors>, FormatError> {
    let blocks = named_blocks(ast, "ORTSDoors");
    let Some(block) = blocks.last() else {
        return Ok(None);
    };
    let seconds = |key: &str| -> Result<f64, FormatError> {
        let values = field_values(block, key);
        let Some(values) = values.last() else {
            return Ok(0.);
        };
        let raw = values
            .iter()
            .filter_map(scalar_text)
            .collect::<String>()
            .to_ascii_lowercase();
        let (raw, scale) = raw.strip_suffix("ms").map_or_else(
            || (raw.strip_suffix('s').unwrap_or(&raw), 1.),
            |s| (s, 0.001),
        );
        let value = raw
            .parse::<f64>()
            .ok()
            .map(|v| v * scale)
            .filter(|v| v.is_finite() && (0. ..=60.).contains(v));
        value.ok_or_else(|| FormatError::UnexpectedAtom {
            key: key.into(),
            context: "ORTSDoors".into(),
            expected: "finite delay from 0 to 60 seconds".into(),
        })
    };
    Ok(Some(NativeDoors {
        opening_s: seconds("OpeningDelay")?,
        closing_s: seconds("ClosingDelay")?,
    }))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_delays_and_zero_defaults_are_preserved_without_guessing_doors() {
        let a = crate::parse_vehicle_text(
            "Wagon ( c ORTSDoors ( OpeningDelay ( 250ms ) ClosingDelay ( 3s ) ) )",
        )
        .unwrap();
        assert_eq!(
            parse_native_doors(&a).unwrap(),
            Some(NativeDoors {
                opening_s: 0.25,
                closing_s: 3.
            })
        );
        let a = crate::parse_vehicle_text("Wagon ( c ORTSDoors ( ) )").unwrap();
        assert_eq!(
            parse_native_doors(&a).unwrap(),
            Some(NativeDoors {
                opening_s: 0.,
                closing_s: 0.
            })
        );
        let a = crate::parse_vehicle_text("Wagon ( c )").unwrap();
        assert!(parse_native_doors(&a).unwrap().is_none());
        let a =
            crate::parse_vehicle_text("Wagon ( c ORTSDoors ( OpeningDelay ( -1s ) ) )").unwrap();
        assert!(parse_native_doors(&a).is_err());
    }
}
