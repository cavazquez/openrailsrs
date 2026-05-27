//! Open Rails auto-friction: Davis 1926 / CN 1992 coefficient estimation.
//!
//! Port of `MSTSWagon.CalcDavisAValue`, `CalcDavisBValue`, and C from drag constant
//! + frontal area (see Open Rails `MSTSWagon.cs`).

use openrailsrs_formats::{OrtsBearingType, OrtsFrictionFields, OrtsWagonType};

use crate::model::DavisCoefficients;

/// US short ton (2000 lb).
const KG_PER_US_TON: f64 = 907.18474;
const MPS_PER_MPH: f64 = 0.44704;
const M2_PER_FT2: f64 = 10.763910416709722;

fn kg_to_us_tons(kg: f64) -> f64 {
    kg / KG_PER_US_TON
}

fn lbf_to_n(lbf: f64) -> f64 {
    lbf * 4.4482216152605
}

fn lbf_per_mph_to_n_per_mps(lbf_per_mph: f64) -> f64 {
    lbf_to_n(lbf_per_mph) / MPS_PER_MPH
}

fn lbf_per_mph2_to_n_per_mps2(lbf_per_mph2: f64) -> f64 {
    lbf_to_n(lbf_per_mph2) / (MPS_PER_MPH * MPS_PER_MPH)
}

/// OR `CalcDavisAValue` — journal + mechanical friction (N).
pub fn calc_davis_a_n(bearing: OrtsBearingType, mass_kg: f64, axles: u32) -> f64 {
    if mass_kg <= 0.0 || axles == 0 {
        return 0.0;
    }
    let axles_f = axles as f64;
    let tons = kg_to_us_tons(mass_kg);
    let (c_t, c_n) = match bearing {
        OrtsBearingType::Grease | OrtsBearingType::Friction => {
            if tons / axles_f < 5.0 {
                (9.4 * tons.sqrt(), 12.5)
            } else {
                (1.3, 29.0)
            }
        }
        OrtsBearingType::Roller => (1.5, 18.0),
        OrtsBearingType::Low => (1.5, 11.0),
        OrtsBearingType::Default => (1.3, 29.0),
    };
    lbf_to_n(c_t * tons + c_n * axles_f)
}

/// OR `CalcDavisBValue` — flange friction (N/(m/s)).
pub fn calc_davis_b_n_per_mps(
    bearing: OrtsBearingType,
    mass_kg: f64,
    axles: u32,
    wagon_type: OrtsWagonType,
) -> f64 {
    if mass_kg <= 0.0 {
        return 0.0;
    }
    let axles_f = axles.max(1) as f64;
    let tons = kg_to_us_tons(mass_kg);
    let c_t = match bearing {
        OrtsBearingType::Grease | OrtsBearingType::Friction => {
            if tons / axles_f < 5.0 {
                0.009
            } else {
                match wagon_type {
                    OrtsWagonType::Tender | OrtsWagonType::Freight => 0.045,
                    OrtsWagonType::Engine | OrtsWagonType::Passenger => 0.03,
                }
            }
        }
        OrtsBearingType::Roller => match wagon_type {
            OrtsWagonType::Tender | OrtsWagonType::Freight => 0.03,
            OrtsWagonType::Engine | OrtsWagonType::Passenger => 0.02,
        },
        OrtsBearingType::Low => match wagon_type {
            OrtsWagonType::Tender | OrtsWagonType::Freight => 0.02,
            OrtsWagonType::Engine | OrtsWagonType::Passenger => 0.015,
        },
        OrtsBearingType::Default => 0.045,
    };
    lbf_per_mph_to_n_per_mps(c_t * tons)
}

/// Default OR drag constant by wagon type when `ORTSDavisDragConstant` is absent.
pub fn default_drag_constant(wagon_type: OrtsWagonType) -> f64 {
    match wagon_type {
        OrtsWagonType::Engine => 0.0024,
        OrtsWagonType::Passenger => 0.00034,
        OrtsWagonType::Tender | OrtsWagonType::Freight => 0.0005,
    }
}

/// OR Davis C from frontal area (ft²) × drag constant.
pub fn calc_davis_c_n_per_mps2(frontal_area_m2: f64, drag_constant: f64) -> f64 {
    if frontal_area_m2 <= 0.0 || drag_constant <= 0.0 {
        return 0.0;
    }
    lbf_per_mph2_to_n_per_mps2(frontal_area_m2 * M2_PER_FT2 * drag_constant)
}

/// Fill missing Davis components using OR auto-friction rules.
pub fn auto_davis_coefficients(
    parsed: DavisCoefficients,
    mass_kg: f64,
    is_loco: bool,
    meta: &OrtsFrictionFields,
) -> DavisCoefficients {
    let bearing = meta.effective_bearing_type();
    let axles = meta.total_axles(is_loco).max(1);
    let drag = meta
        .drag_constant
        .filter(|d| *d > 0.0)
        .unwrap_or_else(|| default_drag_constant(meta.wagon_type));
    let area = meta.frontal_area_m2();

    let mut a = parsed.a_n;
    let mut b = parsed.b_n_per_mps;
    let mut c = parsed.c_n_per_mps2;

    if a <= 0.0 {
        a = calc_davis_a_n(bearing, mass_kg, axles);
    }
    if b <= 0.0 {
        b = calc_davis_b_n_per_mps(bearing, mass_kg, axles, meta.wagon_type);
    }
    if c <= 0.0 {
        c = calc_davis_c_n_per_mps2(area, drag);
    }

    DavisCoefficients {
        a_n: a,
        b_n_per_mps: b,
        c_n_per_mps2: c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mk2_friction_passenger_matches_or_formula() {
        // 34000 kg, 4 axles, Friction, Passenger — OR path for SCE MK2 without ORTS fields.
        let a = calc_davis_a_n(OrtsBearingType::Friction, 34_000.0, 4);
        let b = calc_davis_b_n_per_mps(
            OrtsBearingType::Friction,
            34_000.0,
            4,
            OrtsWagonType::Passenger,
        );
        assert!((a - 732.7).abs() < 5.0, "a={a}");
        assert!((b - 11.2).abs() < 1.0, "b={b}");
    }

    #[test]
    fn roller_freight_differs_from_legacy_scale() {
        let a = calc_davis_a_n(OrtsBearingType::Roller, 34_000.0, 4);
        assert!((a - 570.0).abs() < 5.0, "a={a}");
    }

    #[test]
    fn explicit_davis_preserved() {
        let parsed = DavisCoefficients {
            a_n: 371.0,
            b_n_per_mps: 20.0,
            c_n_per_mps2: 0.86,
        };
        let meta = OrtsFrictionFields::default();
        let out = auto_davis_coefficients(parsed.clone(), 34_000.0, false, &meta);
        assert_eq!(out, parsed);
    }

    #[test]
    fn c_from_area_and_drag() {
        let c = calc_davis_c_n_per_mps2(10.0, 0.00034);
        assert!(c > 0.0);
    }
}
