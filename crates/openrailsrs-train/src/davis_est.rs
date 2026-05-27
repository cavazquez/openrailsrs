//! Estimate Davis coefficients when ORTSDavis is absent (Open Rails auto-friction style).

use openrailsrs_formats::OrtsFrictionFields;

use crate::auto_friction::auto_davis_coefficients;
use crate::model::DavisCoefficients;

/// True when the asset file omitted all ORTSDavis / friction fields.
pub fn is_unspecified_davis(d: &DavisCoefficients) -> bool {
    d.a_n == 0.0 && d.b_n_per_mps == 0.0 && d.c_n_per_mps2 == 0.0
}

/// Use parsed ORTSDavis when present; otherwise OR CN/Davis auto-friction.
pub fn resolve_davis_coefficients(
    parsed: DavisCoefficients,
    mass_kg: f64,
    is_loco: bool,
    meta: &OrtsFrictionFields,
) -> DavisCoefficients {
    if !is_unspecified_davis(&parsed)
        && parsed.a_n > 0.0
        && parsed.b_n_per_mps > 0.0
        && parsed.c_n_per_mps2 > 0.0
    {
        return parsed;
    }
    auto_davis_coefficients(parsed, mass_kg, is_loco, meta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::{OrtsBearingType, OrtsWagonType};

    #[test]
    fn mk2_passenger_auto_resistance() {
        let meta = OrtsFrictionFields {
            bearing_type: OrtsBearingType::Default,
            wagon_type: OrtsWagonType::Passenger,
            num_axles: Some(4),
            ..Default::default()
        };
        let parsed = DavisCoefficients {
            a_n: 0.0,
            b_n_per_mps: 0.0,
            c_n_per_mps2: 0.0,
        };
        let d = resolve_davis_coefficients(parsed, 34_000.0, false, &meta);
        assert!((d.a_n - 732.7).abs() < 5.0, "a={}", d.a_n);
        assert!((d.b_n_per_mps - 11.2).abs() < 1.0);
    }
}
