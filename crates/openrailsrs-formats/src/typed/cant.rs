//! Automatic cant profiles, following OR 1.6.1 `SuperElevation.MarkSections`.
//! Profiles use single precision like the reference, with angles interpolated
//! separately from physical cant. Turnout eligibility belongs to the caller.
use super::superelevation::{field_values, scalar_text};
use crate::{
    Ast,
    msts_units::{parse_length_m, parse_velocity_mps},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CantStandard {
    pub min_cant_m: f32,
    pub max_cant_m: f32,
    pub min_speed_mps: f32,
    pub max_speed_mps: f32,
    pub precision_m: f32,
    pub runoff_slope: f32,
    pub runoff_speed_mps: f32,
    pub passenger_deficiency_m: f32,
    pub freight_deficiency_m: f32,
}
impl CantStandard {
    pub fn native_default(metric: bool, high_speed: bool) -> Self {
        if metric {
            Self {
                min_cant_m: 0.010,
                max_cant_m: 0.180,
                min_speed_mps: 25.5 * (1. / 3.6),
                max_speed_mps: f32::MAX,
                precision_m: 0.005,
                runoff_slope: 0.003,
                runoff_speed_mps: if high_speed { 0.080 } else { 0.055 },
                passenger_deficiency_m: if high_speed { 0.150 } else { 0.130 },
                freight_deficiency_m: if high_speed { 0.110 } else { 0.100 },
            }
        } else {
            Self {
                min_cant_m: 0.5 * 0.0254,
                max_cant_m: 6.0 * 0.0254,
                min_speed_mps: 15.5 * (1. / 2.236_936_3),
                max_speed_mps: f32::MAX,
                precision_m: 0.25 * 0.0254,
                runoff_slope: 0.003,
                runoff_speed_mps: if high_speed {
                    0.1136 * (1. / 2.236_936_3)
                } else {
                    0.0852 * (1. / 2.236_936_3)
                },
                passenger_deficiency_m: if high_speed {
                    5.0 * 0.0254
                } else {
                    3.0 * 0.0254
                },
                freight_deficiency_m: if high_speed {
                    3.0 * 0.0254
                } else {
                    2.0 * 0.0254
                },
            }
        }
    }
    pub fn valid(&self) -> bool {
        [
            self.min_cant_m,
            self.max_cant_m,
            self.min_speed_mps,
            self.max_speed_mps,
            self.precision_m,
            self.runoff_slope,
            self.runoff_speed_mps,
            self.passenger_deficiency_m,
            self.freight_deficiency_m,
        ]
        .iter()
        .all(|v| v.is_finite() && *v >= 0.)
            && self.max_cant_m >= self.min_cant_m
            && self.max_speed_mps >= self.min_speed_mps
            && self.precision_m > 0.
            && self.runoff_slope > 0.
            && self.runoff_speed_mps > 0.
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RouteCantSettings {
    pub gauge_m: f32,
    pub design_speed_mps: f32,
    pub standards: Vec<CantStandard>,
    /// Older radius/cant tables are preserved by disabling generation until
    /// their separate legacy algorithm has a matching reference test.
    pub legacy_table_present: bool,
}
impl Default for RouteCantSettings {
    fn default() -> Self {
        Self {
            gauge_m: 1.435,
            design_speed_mps: 40.,
            standards: vec![CantStandard::native_default(true, false)],
            legacy_table_present: false,
        }
    }
}
fn quantity(ast: &Ast, key: &str, speed: bool) -> Option<f32> {
    let text = field_values(ast, key)
        .first()?
        .iter()
        .filter_map(scalar_text)
        .collect::<Vec<_>>()
        .join(" ");
    (if speed {
        parse_velocity_mps(&text)
    } else {
        parse_length_m(&text)
    })
    .filter(|v| v.is_finite() && *v >= 0.)
    .map(|v| v as f32)
}
impl RouteCantSettings {
    pub fn from_ast(ast: &Ast) -> Self {
        let speed = quantity(ast, "SpeedLimit", true).unwrap_or(40.);
        let metric = field_values(ast, "MilepostUnitsMiles").is_empty();
        let mut settings = Self {
            gauge_m: quantity(ast, "ORTSTrackGauge", false)
                .filter(|g| *g > 0.1)
                .unwrap_or(1.435),
            design_speed_mps: speed,
            standards: vec![CantStandard::native_default(metric, speed > 45.)],
            legacy_table_present: !field_values(ast, "ORTSTrackSuperElevation").is_empty(),
        };
        let mut standards = vec![];
        for values in field_values(ast, "ORTSSuperElevation") {
            let block = Ast::List(values.to_vec());
            // Explicit standards start with OR's metric field defaults.
            let mut s = CantStandard::native_default(true, false);
            s.passenger_deficiency_m =
                quantity(&block, "MaxPassengerUnderbalance", false).unwrap_or(f32::MAX);
            s.freight_deficiency_m =
                quantity(&block, "MaxFreightUnderbalance", false).unwrap_or(f32::MAX);
            if s.passenger_deficiency_m > 10. && s.freight_deficiency_m > 10. {
                s.passenger_deficiency_m = 0.075;
                s.freight_deficiency_m = 0.05;
            } else if s.passenger_deficiency_m > 10. {
                s.passenger_deficiency_m = s.freight_deficiency_m;
            } else if s.freight_deficiency_m > 10. {
                s.freight_deficiency_m = s.passenger_deficiency_m;
            }
            for (key, target, speed) in [
                ("MinimumCant", &mut s.min_cant_m, false),
                ("MaximumCant", &mut s.max_cant_m, false),
                ("MinimumSpeed", &mut s.min_speed_mps, true),
                ("MaximumSpeed", &mut s.max_speed_mps, true),
                ("Precision", &mut s.precision_m, false),
                ("MaxRunoffSlope", &mut s.runoff_slope, false),
                ("MaxRunoffSpeed", &mut s.runoff_speed_mps, true),
            ] {
                if let Some(v) = quantity(&block, key, speed) {
                    *target = v;
                }
            }
            if s.valid() {
                standards.push(s);
            }
        }
        if !standards.is_empty() {
            settings.standards = standards;
        }
        settings
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct CantSection {
    pub length_m: f32,
    /// Zero denotes straight track.
    pub radius_m: f32,
    pub passenger_speed_mps: f32,
    pub freight_speed_mps: f32,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CantProfile {
    pub positions: Vec<f32>,
    pub elevations_m: Vec<f32>,
    pub angles_rad: Vec<f32>,
}
impl CantProfile {
    fn interpolate(&self, values: &[f32], fraction: f32) -> f32 {
        if values.is_empty() {
            return 0.;
        }
        let x = fraction.clamp(0., 1.);
        let i = self
            .positions
            .partition_point(|p| *p < x)
            .clamp(1, values.len() - 1);
        let t = (x - self.positions[i - 1]) / (self.positions[i] - self.positions[i - 1]);
        values[i - 1] + (values[i] - values[i - 1]) * t
    }
    pub fn cant_m(&self, fraction: f32) -> f32 {
        self.interpolate(&self.elevations_m, fraction)
    }
    pub fn roll_rad(&self, fraction: f32) -> f32 {
        self.interpolate(&self.angles_rad, fraction)
    }
}

/// One contiguous group of the same curve direction, including its transitions.
pub fn generate_cant_profiles(
    sections: &[CantSection],
    gauge: f32,
    standards: &[CantStandard],
    direction: f32,
) -> Vec<CantProfile> {
    let empty = || vec![CantProfile::default(); sections.len()];
    if sections.is_empty()
        || !gauge.is_finite()
        || gauge <= 0.
        || direction.abs() != 1.
        || sections.iter().any(|s| {
            ![
                s.length_m,
                s.radius_m,
                s.passenger_speed_mps,
                s.freight_speed_mps,
            ]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.)
                || s.length_m <= 0.
        })
    {
        return empty();
    }
    let speed = sections
        .iter()
        .map(|s| s.passenger_speed_mps.max(s.freight_speed_mps))
        .fold(0., f32::max);
    let Some(std) = standards
        .iter()
        .find(|s| s.valid() && speed < s.max_speed_mps + 0.05 && speed > s.min_speed_mps - 0.05)
    else {
        return empty();
    };
    let slope = std.runoff_slope.min(std.runoff_speed_mps / speed);
    let total: f32 = sections.iter().map(|s| s.length_m).sum();
    let max_cant = std
        .max_cant_m
        .clamp(0., gauge)
        .min(total / (2. * 1.3) * slope);
    if max_cant < std.min_cant_m {
        return empty();
    }
    let mut nominal: Vec<f32> = sections
        .iter()
        .map(|s| {
            if s.radius_m == 0. {
                return 0.;
            }
            let factor = gauge / (9.81 * s.radius_m);
            let cant = (factor * (s.passenger_speed_mps * s.passenger_speed_mps)
                - std.passenger_deficiency_m)
                .max(
                    factor * (s.freight_speed_mps * s.freight_speed_mps) - std.freight_deficiency_m,
                );
            ((cant / std.precision_m).round() * std.precision_m).clamp(std.min_cant_m, max_cant)
        })
        .collect();
    let mut result = vec![];
    let (mut before, mut start) = (0., 0.);
    for (i, s) in sections.iter().enumerate() {
        let end = if i + 1 == sections.len() {
            0.
        } else {
            if i + 2 < sections.len()
                && nominal[i + 1] == 0.
                && sections[i + 1].length_m < nominal[i] / slope * 1.5
            {
                nominal[i + 1] = (nominal[i] + nominal[i + 2]) / 2.;
            }
            ((nominal[i] + nominal[i + 1]) / 2.)
                .min((before + s.length_m).min(total - (before + s.length_m)) * slope)
        };
        let n = nominal[i];
        let a = (n - start).abs() / slope;
        let b = (end - n).abs() / slope;
        let run = (end - start).abs() / slope;
        let (positions, elevations) = if a + b >= s.length_m && run < s.length_m * 0.75 {
            let (mid, elev) = if n > start && n > end {
                let m = s.length_m - ((start + slope * s.length_m - end) / slope) / 2.;
                (m, start + slope * m)
            } else if n == 0. {
                (s.length_m / 2., (start + end) / 2.)
            } else {
                let m = s.length_m - ((end - (start - slope * s.length_m)) / slope) / 2.;
                (m, start + slope * m)
            };
            (
                vec![0., mid / s.length_m, 1.],
                vec![start, elev.max(0.), end],
            )
        } else if a + b < s.length_m && !(start == n && n == end) {
            if start != n && end != n {
                (
                    vec![0., a / s.length_m, (s.length_m - b) / s.length_m, 1.],
                    vec![start, n, n, end],
                )
            } else {
                let mid = if start == n { s.length_m - b } else { a };
                (vec![0., mid / s.length_m, 1.], vec![start, n, end])
            }
        } else {
            (vec![0., 1.], vec![start, end])
        };
        let angles = elevations
            .iter()
            .map(|e| (*e / gauge).asin() * -direction)
            .collect();
        result.push(CantProfile {
            positions,
            elevations_m: elevations,
            angles_rad: angles,
        });
        start = end;
        before += s.length_m;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transitions_are_continuous_and_reverse_direction_changes_only_roll() {
        let ss = [
            CantSection {
                length_m: 40.,
                radius_m: 0.,
                passenger_speed_mps: 30.,
                freight_speed_mps: 25.,
            },
            CantSection {
                length_m: 250.,
                radius_m: 700.,
                passenger_speed_mps: 30.,
                freight_speed_mps: 25.,
            },
            CantSection {
                length_m: 40.,
                radius_m: 0.,
                passenger_speed_mps: 30.,
                freight_speed_mps: 25.,
            },
        ];
        let std = [CantStandard::native_default(false, false)];
        let a = generate_cant_profiles(&ss, 1.435, &std, 1.);
        let b = generate_cant_profiles(&ss, 1.435, &std, -1.);
        assert_eq!(a[0].cant_m(0.), 0.);
        assert_eq!(a[2].cant_m(1.), 0.);
        for i in 0..2 {
            assert!((a[i].cant_m(1.) - a[i + 1].cant_m(0.)).abs() < 1e-6);
        }
        assert!(a[1].cant_m(0.5) > 0.05);
        for i in 0..3 {
            assert_eq!(a[i].cant_m(0.5), b[i].cant_m(0.5));
            assert_eq!(a[i].roll_rad(0.5), -b[i].roll_rad(0.5));
        }
    }
    #[test]
    fn native_route_units_and_explicit_standard_are_used() {
        let ast=crate::parse_named_stf("Tr_RouteFile ( RouteID ( test ) SpeedLimit ( 40.2336 ) MilepostUnitsMiles ( ) ORTSTrackGauge ( 4ft 8.5in ) )").unwrap();
        let route = RouteCantSettings::from_ast(&ast);
        assert!((route.design_speed_mps - 40.2336).abs() < 1e-5);
        assert!((route.gauge_m - 1.4351).abs() < 1e-6);
        assert_eq!(route.standards[0].min_cant_m, 0.5 * 0.0254);
        let ast=crate::parse_named_stf("Tr_RouteFile ( RouteID ( test ) ORTSSuperElevation ( MaximumCant ( 4in ) Precision ( 0.25in ) MinimumSpeed ( 10mph ) MaxFreightUnderbalance ( 2in ) MaxRunoffSpeed ( 0.04m/s ) ) )").unwrap();
        let route = RouteCantSettings::from_ast(&ast);
        assert!((route.standards[0].max_cant_m - 0.1016).abs() < 1e-6);
        assert!((route.standards[0].passenger_deficiency_m - 0.0508).abs() < 1e-6);
        let ast = crate::parse_named_stf(
            "Tr_RouteFile ( RouteID ( legacy ) ORTSTrackSuperElevation ( 2 300 0.15 1000 0.05 ) )",
        )
        .unwrap();
        assert!(RouteCantSettings::from_ast(&ast).legacy_table_present);
    }
    #[test]
    fn too_short_or_slow_curves_do_not_bank() {
        let s = CantSection {
            length_m: 2.,
            radius_m: 100.,
            passenger_speed_mps: 20.,
            freight_speed_mps: 20.,
        };
        assert_eq!(
            generate_cant_profiles(
                &[s],
                1.435,
                &[CantStandard::native_default(true, false)],
                1.
            )[0]
            .cant_m(0.5),
            0.
        );
        assert!(
            generate_cant_profiles(
                &[CantSection {
                    length_m: 200.,
                    passenger_speed_mps: 1.,
                    freight_speed_mps: 1.,
                    ..s
                }],
                1.435,
                &[CantStandard::native_default(true, false)],
                1.
            )[0]
            .positions
            .is_empty()
        );
    }
}
