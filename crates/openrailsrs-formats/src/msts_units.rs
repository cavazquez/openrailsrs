//! Parse MSTS / Open Rails quantity strings (`68t-uk`, `12000lbf`, `90mph`, …).

use crate::units::{kn_to_n, kw_to_w, lb_to_kg, mph_to_mps};

/// Parse a mass expression to kilograms.
pub fn parse_mass_kg(raw: &str) -> Option<f64> {
    let s = raw.trim();
    if let Some(v) = parse_leading_number(s) {
        let rest = s[v.1..].trim().to_ascii_lowercase();
        return Some(match rest.as_str() {
            "t" | "t-uk" | "ton" | "tons" | "tonne" | "tonnes" => v.0 * 1000.0,
            "kg" | "" => v.0,
            "lb" | "lbf" => lb_to_kg(v.0),
            "g-uk" | "g-us" => v.0 * 0.001,
            _ if rest.starts_with('t') => v.0 * 1000.0,
            _ if rest.starts_with("kg") => v.0,
            _ if rest.starts_with("lb") => lb_to_kg(v.0),
            _ => v.0,
        });
    }
    None
}

/// Parse a force expression to Newtons.
pub fn parse_force_n(raw: &str) -> Option<f64> {
    let s = raw.trim();
    if let Some(v) = parse_leading_number(s) {
        let rest = s[v.1..].trim().to_ascii_lowercase();
        return Some(match rest.as_str() {
            "n" | "" => v.0,
            "kn" | "kN" => kn_to_n(v.0),
            "lbf" | "lb" => v.0 * 4.448_221_615_260_5,
            "kips" => v.0 * 4_448.221_615_260_5,
            _ if rest.starts_with("kn") => kn_to_n(v.0),
            _ if rest.starts_with("lbf") || rest.starts_with("lb") => v.0 * 4.448_221_615_260_5,
            _ => v.0,
        });
    }
    None
}

/// Parse a length expression to metres (`68ft 6in`, `21.0in`, `20.602m`).
pub fn parse_length_m(raw: &str) -> Option<f64> {
    let s = raw.trim();
    if s.contains(' ') {
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.len() == 2 {
            if let (Some(feet), Some(inches)) =
                (parse_leading_number(parts[0]), parse_inches(parts[1]))
            {
                return Some(feet.0 * 0.3048 + inches);
            }
        }
    }
    if let Some(v) = parse_leading_number(s) {
        let rest = s[v.1..].trim().to_ascii_lowercase();
        return Some(match rest.as_str() {
            "m" | "" => v.0,
            "cm" => v.0 * 0.01,
            "mm" => v.0 * 0.001,
            "in" | "inch" | "inches" => v.0 * 0.0254,
            "ft" | "feet" => v.0 * 0.3048,
            _ if rest.starts_with('m') => v.0,
            _ if rest.starts_with("cm") => v.0 * 0.01,
            _ if rest.starts_with("in") => v.0 * 0.0254,
            _ if rest.starts_with("ft") => v.0 * 0.3048,
            _ => v.0,
        });
    }
    None
}

/// Parse a velocity expression to m/s.
pub fn parse_velocity_mps(raw: &str) -> Option<f64> {
    let s = raw.trim();
    if let Some(v) = parse_leading_number(s) {
        let rest = s[v.1..].trim().to_ascii_lowercase();
        return Some(match rest.as_str() {
            "mph" => mph_to_mps(v.0),
            "m/s" | "mps" => v.0,
            "km/h" | "kmh" => v.0 / 3.6,
            "" => v.0,
            _ if rest.starts_with("mph") => mph_to_mps(v.0),
            _ if rest.starts_with("km") => v.0 / 3.6,
            _ => v.0,
        });
    }
    None
}

/// Parse a power expression to watts (OR diesel tables use watts or hp).
pub fn parse_power_w(raw: &str) -> Option<f64> {
    let s = raw.trim();
    if let Some(v) = parse_leading_number(s) {
        let rest = s[v.1..].trim().to_ascii_lowercase();
        return Some(match rest.as_str() {
            "w" | "" => v.0,
            "kw" => kw_to_w(v.0),
            "hp" => v.0 * 745.699_871_582_27,
            "mw" => v.0 * 1_000_000.0,
            _ if rest.starts_with("kw") => kw_to_w(v.0),
            _ if rest.starts_with("hp") => v.0 * 745.699_871_582_27,
            _ if rest.starts_with('w') => v.0,
            _ => v.0,
        });
    }
    None
}

/// Parse a pressure expression to bar.
pub fn parse_pressure_bar(raw: &str) -> Option<f64> {
    let s = raw.trim();
    if let Some(v) = parse_leading_number(s) {
        let rest = s[v.1..].trim().to_ascii_lowercase();
        return Some(match rest.as_str() {
            "bar" | "" => v.0,
            "psi" => v.0 * 0.068_947_572_9,
            "kpa" => v.0 * 0.01,
            _ if rest.starts_with("psi") => v.0 * 0.068_947_572_9,
            _ if rest.starts_with("bar") => v.0,
            _ => v.0,
        });
    }
    None
}

fn parse_inches(part: &str) -> Option<f64> {
    let (value, end) = parse_leading_number(part)?;
    let rest = part[end..].trim().to_ascii_lowercase();
    if rest.starts_with("in") {
        Some(value * 0.0254)
    } else {
        Some(value)
    }
}

/// Returns `(value, byte_len_of_numeric_prefix)`.
fn parse_leading_number(s: &str) -> Option<(f64, usize)> {
    let mut end = 0usize;
    for (i, ch) in s.char_indices() {
        if ch.is_ascii_digit() || ch == '.' || ch == '-' || ch == '+' || ch == 'e' || ch == 'E' {
            end = i + ch.len_utf8();
        } else {
            break;
        }
    }
    if end == 0 {
        return None;
    }
    s[..end].parse::<f64>().ok().map(|v| (v, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mass_units() {
        assert!((parse_mass_kg("68t-uk").unwrap() - 68_000.0).abs() < 1.0);
        assert!((parse_mass_kg("50t").unwrap() - 50_000.0).abs() < 1.0);
    }

    #[test]
    fn force_units() {
        assert!((parse_force_n("12000lbf").unwrap() - 53_378.659).abs() < 1.0);
        assert!((parse_force_n("70kN").unwrap() - 70_000.0).abs() < 1.0);
    }

    #[test]
    fn length_units() {
        assert!((parse_length_m("20.602m").unwrap() - 20.602).abs() < 0.001);
        assert!((parse_length_m("21.0in").unwrap() - 0.5334).abs() < 0.001);
        assert!((parse_length_m("68ft 6in").unwrap() - 20.8788).abs() < 0.01);
    }
}
