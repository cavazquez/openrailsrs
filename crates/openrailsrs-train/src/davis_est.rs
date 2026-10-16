//! Estimate Davis coefficients when ORTSDavis is absent (Open Rails auto-friction style).

use openrailsrs_formats::OrtsFrictionFields;

use crate::auto_friction::auto_davis_coefficients;
use crate::model::DavisCoefficients;

/// True when the asset file omitted all ORTSDavis / friction fields.
pub fn is_unspecified_davis(d: &DavisCoefficients) -> bool {
    d.a_n == 0.0 && d.b_n_per_mps == 0.0 && d.c_n_per_mps2 == 0.0
}

/// Use complete positive ORTSDavis when present, otherwise recover authored
/// MSTS FCalc friction before considering OR CN/Davis auto-friction.
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
    if let Some(friction) = meta.legacy_friction {
        return recover_msts_davis(friction, mass_kg);
    }
    if meta
        .legacy_friction_speed_mps
        .is_some_and(|v| !(0.0..=4.4407).contains(&v))
    {
        return legacy_default_davis(mass_kg);
    }
    auto_davis_coefficients(parsed, mass_kg, is_loco, meta)
}

fn legacy_default_davis(mass_kg: f64) -> DavisCoefficients {
    let mass = mass_kg as f32;
    DavisCoefficients {
        a_n: f64::from(6.3743_f32 * mass / 1000. + 128.998_f32 * 4.),
        b_n_per_mps: f64::from(0.49358_f32 * mass / 1000.),
        c_n_per_mps2: f64::from(0.11979_f32 * 100. / 10.76_f32),
    }
}

/// Literal Single-order recovery in OR1.6.1 MSTSWagon.cs2383–2436. Math.Pow
/// evaluates its Single arguments as Double, then casts the result to Single
/// before any multiplication. Do not clamp or fit the recovered coefficients.
fn recover_msts_davis(values: [f64; 5], mass_kg: f64) -> DavisCoefficients {
    let [c1, e1, v2, c2, e2] = values.map(|v| v as f32);
    if !(0. ..=4.4407_f32).contains(&v2) {
        return legacy_default_davis(mass_kg);
    }
    let mps1 = v2;
    let mps2 = 80. * 0.44704_f32;
    let s = mps2 - mps1;
    let mut x1 = mps1 * mps1;
    let mut x2 = mps2 * mps2;
    let sx = (x2 - x1) / 2.;
    let y0 = c1 * (f64::from(mps1).powf(f64::from(e1)) as f32) + c2 * mps1;
    let mut y1 = c2 * (f64::from(mps1).powf(f64::from(e2)) as f32) * mps1;
    let mut y2 = c2 * (f64::from(mps2).powf(f64::from(e2)) as f32) * mps2;
    let sy = y0 * (mps2 - mps1) + (y2 - y1) / (1. + e2);
    y1 *= mps1;
    y2 *= mps2;
    let syx = y0 * (x2 - x1) / 2. + (y2 - y1) / (2. + e2);
    x1 *= mps1;
    x2 *= mps2;
    let sx2 = (x2 - x1) / 3.;
    y1 *= mps1;
    y2 *= mps2;
    let syx2 = y0 * (x2 - x1) / 3. + (y2 - y1) / (3. + e2);
    x1 *= mps1;
    x2 *= mps2;
    let sx3 = (x2 - x1) / 4.;
    x1 *= mps1;
    x2 *= mps2;
    let sx4 = (x2 - x1) / 5.;
    let s1 = syx - sy * sx / s;
    let s2 = sx * sx2 / s - sx3;
    let s3 = sx2 - sx * sx / s;
    let s4 = syx2 - sy * sx2 / s;
    let s5 = sx2 * sx2 / s - sx4;
    let s6 = sx3 - sx * sx2 / s;
    let c = (s1 * s6 - s3 * s4) / (s3 * s5 - s2 * s6);
    let b = (s1 + c * s2) / s3;
    let a = (sy - b * sx - c * sx2) / s;
    DavisCoefficients {
        a_n: f64::from(a),
        b_n_per_mps: f64::from(b),
        c_n_per_mps2: f64::from(c),
    }
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

    fn simple_tank_meta(speed: f64) -> OrtsFrictionFields {
        OrtsFrictionFields {
            legacy_friction: Some([707., 0., speed, f64::from(13.76_f32), f64::from(1.8_f32)]),
            legacy_friction_speed_mps: Some(speed),
            ..Default::default()
        }
    }

    #[test]
    fn original_simple_tank_fcalc_recovers_all_three_single_coefficients() {
        let meta = simple_tank_meta(f64::from(0.44704_f32));
        let zero = DavisCoefficients {
            a_n: 0.,
            b_n_per_mps: 0.,
            c_n_per_mps2: 0.,
        };
        let d = resolve_davis_coefficients(zero, 50_000., true, &meta);
        // Source-order regression for the identified707/0/1mph/13.76/1.8
        // inputs; the separate native-DLL capture validates this boundary.
        assert_eq!(d.a_n, f64::from(623.3469_f32));
        assert_eq!(d.b_n_per_mps, f64::from(41.69932_f32));
        assert_eq!(d.c_n_per_mps2, f64::from(5.668605_f32));
        assert_eq!(meta.legacy_start_force_n(50_000.), Some(707.));
    }

    #[test]
    fn invalid_fcalc_speed_uses_native_default_and_start_force_twice_a() {
        for speed in [-1., 4.5] {
            let meta = simple_tank_meta(speed);
            let zero = DavisCoefficients {
                a_n: 0.,
                b_n_per_mps: 0.,
                c_n_per_mps2: 0.,
            };
            let d = resolve_davis_coefficients(zero, 50_000., true, &meta);
            assert_eq!(d, legacy_default_davis(50_000.));
            assert_eq!(meta.legacy_start_force_n(50_000.), Some(d.a_n * 2.));
        }
    }

    #[test]
    fn authored_positive_davis_keeps_priority_over_fcalc() {
        let meta = simple_tank_meta(f64::from(0.44704_f32));
        let authored = DavisCoefficients {
            a_n: 371.,
            b_n_per_mps: 20.,
            c_n_per_mps2: 0.86,
        };
        assert_eq!(
            resolve_davis_coefficients(authored.clone(), 50_000., true, &meta),
            authored
        );
    }

    #[test]
    fn absent_msts_friction_keeps_existing_auto_davis_defaults() {
        let meta = OrtsFrictionFields::default();
        let zero = DavisCoefficients {
            a_n: 0.,
            b_n_per_mps: 0.,
            c_n_per_mps2: 0.,
        };
        assert_eq!(
            resolve_davis_coefficients(zero.clone(), 50_000., true, &meta),
            auto_davis_coefficients(zero, 50_000., true, &meta)
        );
        assert_eq!(meta.legacy_start_force_n(50_000.), None);
    }
}
