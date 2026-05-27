//! Estimate Davis coefficients when ORTSDavis is absent (Open Rails auto-friction style).

use crate::model::DavisCoefficients;

/// True when the asset file omitted all ORTSDavis / friction fields.
pub fn is_unspecified_davis(d: &DavisCoefficients) -> bool {
    d.a_n == 0.0 && d.b_n_per_mps == 0.0 && d.c_n_per_mps2 == 0.0
}

/// Per-vehicle rolling resistance when ORTSDavis is absent.
///
/// Uses Open Rails documentation defaults (502.8 N, 1.5465 N/(m/s), 1.43 N/(m/s)²)
/// scaled by mass relative to a ~34 t coach.
pub fn estimate_davis_coefficients(mass_kg: f64, is_loco: bool) -> DavisCoefficients {
    if mass_kg <= 0.0 {
        return DavisCoefficients::default();
    }
    let ref_mass = if is_loco { 118_674.0 } else { 34_000.0 };
    let scale = (mass_kg / ref_mass).clamp(0.25, 4.0);
    DavisCoefficients {
        a_n: 502.8 * scale,
        b_n_per_mps: 1.5465 * scale,
        c_n_per_mps2: 1.43 * scale,
    }
}

/// Use parsed ORTSDavis when present; otherwise estimate from vehicle mass.
pub fn resolve_davis_coefficients(
    parsed: DavisCoefficients,
    mass_kg: f64,
    is_loco: bool,
) -> DavisCoefficients {
    if is_unspecified_davis(&parsed) {
        estimate_davis_coefficients(mass_kg, is_loco)
    } else {
        parsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mk2_coach_estimate_scales_with_mass() {
        let d = estimate_davis_coefficients(34_000.0, false);
        assert!((d.a_n - 502.8).abs() < 1.0);
        assert!((d.b_n_per_mps - 1.5465).abs() < 0.01);
    }

    #[test]
    fn class47_estimate_exceeds_single_default() {
        let d = estimate_davis_coefficients(118_674.0, true);
        assert!((d.a_n - 502.8).abs() < 1.0);
    }
}
