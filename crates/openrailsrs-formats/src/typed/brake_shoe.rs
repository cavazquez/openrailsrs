//! OR brake-shoe friction vs speed (ORTSBrakeShoeFriction / Karwatzki 1D curves).

use crate::ast::Ast;

use super::{atom_to_number, find_list_value, find_optional_string_field};

/// Built-in OR brake shoe types (`ORTSBrakeShoeType`).
#[derive(Clone, Debug, PartialEq, Default)]
pub enum OrtsBrakeShoeType {
    #[default]
    CastIronP6,
    CastIronP10,
    HighFrictionComposite,
    DiscPads,
    /// Explicit `ORTSBrakeShoeFriction ( kph mu … )` curve.
    UserDefined,
}

/// Speed (km/h) vs coefficient of friction μ. OR normalizes to μ at standstill.
#[derive(Clone, Debug, PartialEq)]
pub struct BrakeShoeFrictionCurve {
    points_kph: Vec<(f64, f64)>,
    mu_at_zero_kph: f64,
}

impl BrakeShoeFrictionCurve {
    /// No speed dependency (legacy MSTS / P6a off).
    pub fn identity() -> Self {
        Self {
            points_kph: vec![(0.0, 1.0)],
            mu_at_zero_kph: 1.0,
        }
    }

    /// OR default cast-iron 1D curve (Karwatzki / Elvas Tower reference).
    pub fn cast_iron_default() -> Self {
        Self::from_kph_mu_pairs(CAST_IRON_KPH_MU)
    }

    /// High-friction composite (COBRA-style reference curve).
    pub fn high_friction_composite_default() -> Self {
        Self::from_kph_mu_pairs(HFC_KPH_MU)
    }

    pub fn from_kph_mu_pairs(pairs: &[(f64, f64)]) -> Self {
        let mut points_kph: Vec<(f64, f64)> = pairs
            .iter()
            .copied()
            .filter(|(_, mu)| mu.is_finite() && *mu > 0.0)
            .collect();
        points_kph.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        if points_kph.is_empty() {
            return Self::identity();
        }
        let mu_at_zero_kph = points_kph
            .iter()
            .find(|(kph, _)| kph.abs() < 1e-6)
            .map(|(_, mu)| *mu)
            .unwrap_or(points_kph[0].1)
            .max(1e-6);
        Self {
            points_kph,
            mu_at_zero_kph,
        }
    }

    /// Multiplier on nominal `MaxBrakeForce`: μ(v) / μ(0).
    pub fn speed_factor(&self, speed_mps: f64) -> f64 {
        if self.points_kph.len() <= 1 && (self.mu_at_zero_kph - 1.0).abs() < 1e-9 {
            return 1.0;
        }
        let kph = (speed_mps * 3.6).max(0.0);
        let mu = self.interpolate_mu_kph(kph);
        (mu / self.mu_at_zero_kph).clamp(0.05, 1.0)
    }

    fn interpolate_mu_kph(&self, kph: f64) -> f64 {
        let pts = &self.points_kph;
        if pts.is_empty() {
            return 1.0;
        }
        if kph <= pts[0].0 {
            return pts[0].1;
        }
        if kph >= pts.last().unwrap().0 {
            return pts.last().unwrap().1;
        }
        let idx = pts.partition_point(|(k, _)| *k <= kph).saturating_sub(1);
        let (k0, mu0) = pts[idx];
        let (k1, mu1) = pts[(idx + 1).min(pts.len() - 1)];
        if (k1 - k0).abs() < 1e-9 {
            return mu0;
        }
        let t = (kph - k0) / (k1 - k0);
        mu0 + t * (mu1 - mu0)
    }
}

impl OrtsBrakeShoeType {
    pub fn default_curve(&self) -> BrakeShoeFrictionCurve {
        match self {
            Self::HighFrictionComposite => {
                BrakeShoeFrictionCurve::high_friction_composite_default()
            }
            Self::DiscPads => {
                // Disc pads hold μ more constant; mild falloff vs cast iron.
                BrakeShoeFrictionCurve::from_kph_mu_pairs(&[
                    (0.0, 0.38),
                    (80.0, 0.32),
                    (120.0, 0.30),
                ])
            }
            Self::CastIronP6 | Self::CastIronP10 | Self::UserDefined => {
                BrakeShoeFrictionCurve::cast_iron_default()
            }
        }
    }
}

/// Parse shoe type + optional user curve from a vehicle AST.
pub fn parse_orts_brake_shoe(ast: &Ast) -> (OrtsBrakeShoeType, Option<BrakeShoeFrictionCurve>) {
    let user_curve = parse_brake_shoe_friction_list(ast);
    let shoe_type = find_optional_string_field(ast, &["ORTSBrakeShoeType"], "ORTSBrakeShoeType")
        .ok()
        .flatten()
        .map(|s| parse_shoe_type_name(&s))
        .unwrap_or(if user_curve.is_some() {
            OrtsBrakeShoeType::UserDefined
        } else {
            OrtsBrakeShoeType::CastIronP6
        });
    (shoe_type, user_curve)
}

/// Resolve the curve to use when OR-P6b speed factor is enabled.
pub fn resolve_brake_shoe_curve(
    shoe_type: &OrtsBrakeShoeType,
    user_curve: &Option<BrakeShoeFrictionCurve>,
) -> BrakeShoeFrictionCurve {
    if let Some(c) = user_curve {
        return c.clone();
    }
    shoe_type.default_curve()
}

fn parse_shoe_type_name(s: &str) -> OrtsBrakeShoeType {
    let u = s.to_ascii_lowercase().replace(' ', "_");
    if u.contains("high_friction") || u.contains("composite") || u.contains("hfc") {
        OrtsBrakeShoeType::HighFrictionComposite
    } else if u.contains("disc") {
        OrtsBrakeShoeType::DiscPads
    } else if u.contains("p10") {
        OrtsBrakeShoeType::CastIronP10
    } else if u.contains("user") {
        OrtsBrakeShoeType::UserDefined
    } else {
        OrtsBrakeShoeType::CastIronP6
    }
}

fn flatten_numbers(ast: &Ast, out: &mut Vec<f64>) {
    match ast {
        Ast::Atom(atom) => {
            if let Some(n) = atom_to_number(atom) {
                out.push(n);
            }
        }
        Ast::List(items) => {
            for item in items {
                flatten_numbers(item, out);
            }
        }
    }
}

fn parse_brake_shoe_friction_list(ast: &Ast) -> Option<BrakeShoeFrictionCurve> {
    let values = find_list_value(ast, "ORTSBrakeShoeFriction")?;
    let mut nums = Vec::new();
    flatten_numbers(values, &mut nums);
    if nums.len() < 4 || nums.len() % 2 != 0 {
        return None;
    }
    let mut pairs = Vec::new();
    for chunk in nums.chunks(2) {
        pairs.push((chunk[0], chunk[1]));
    }
    Some(BrakeShoeFrictionCurve::from_kph_mu_pairs(&pairs))
}

/// Cast-iron μ(v) — speeds in km/h (OR default / Karwatzki).
const CAST_IRON_KPH_MU: &[(f64, f64)] = &[
    (0.0, 0.50),
    (8.0, 0.288),
    (16.1, 0.241),
    (24.1, 0.211),
    (32.2, 0.187),
    (40.2, 0.173),
    (48.3, 0.161),
    (56.3, 0.150),
    (64.4, 0.142),
    (72.2, 0.139),
    (80.5, 0.134),
    (88.5, 0.129),
    (96.6, 0.125),
    (104.6, 0.123),
    (112.7, 0.121),
];

#[allow(clippy::approx_constant)]
const HFC_KPH_MU: &[(f64, f64)] = &[
    (0.0, 0.49),
    (8.0, 0.436),
    (16.1, 0.400),
    (24.1, 0.371),
    (32.2, 0.350),
    (40.2, 0.336),
    (48.3, 0.325),
    (56.3, 0.318),
    (64.4, 0.309),
    (72.2, 0.304),
    (80.5, 0.298),
    (88.5, 0.295),
    (96.6, 0.289),
    (104.6, 0.288),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_from_first_paren;
    use crate::units::kmh_to_mps;

    #[test]
    fn cast_iron_factor_falls_with_speed() {
        let c = BrakeShoeFrictionCurve::cast_iron_default();
        assert!((c.speed_factor(0.0) - 1.0).abs() < 1e-6);
        let f80 = c.speed_factor(kmh_to_mps(80.5));
        assert!(f80 > 0.25 && f80 < 0.30, "f80={f80}");
        assert!(c.speed_factor(kmh_to_mps(100.0)) < f80);
    }

    #[test]
    fn parses_orts_brake_shoe_friction_list() {
        let text = r#"(Wagon (ORTSBrakeShoeFriction ( 0.0 0.49 80.5 0.298 )))"#;
        let ast = parse_from_first_paren(text).unwrap();
        let curve = parse_brake_shoe_friction_list(&ast).unwrap();
        assert!((curve.speed_factor(0.0) - 1.0).abs() < 1e-6);
        let f = curve.speed_factor(kmh_to_mps(80.5));
        assert!((f - 0.298 / 0.49).abs() < 0.02, "f={f}");
    }
}
