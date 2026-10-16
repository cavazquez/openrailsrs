//! Mechanical support geometry, independent of shape matrices and animation.
//!
//! Native fields retain metres/radians. Missing shape-only information is not
//! inferred from a rendered yaw. Optional lateral suspension is a Rust model,
//! deliberately separate from OR's quasi-static derailment indication.
use std::path::Path;

use openrailsrs_formats::{Ast, Atom, ConsistEntry, ConsistFile};
use serde::{Deserialize, Serialize};

use crate::{TrainError, from_ast::resolve_consist_entry_path};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct IrregularitySection {
    pub start_chainage_m: f64,
    pub end_chainage_m: f64,
    pub transition_m: f64,
    pub seed: u64,
    /// Wavelength and peak rail-height amplitude, in metres. Both rails share
    /// this spectrum but receive separate deterministic phases.
    pub spectrum: Vec<[f64; 2]>,
}
impl IrregularitySection {
    pub fn valid(&self) -> bool {
        self.start_chainage_m.is_finite()
            && self.start_chainage_m >= 0.
            && self.end_chainage_m.is_finite()
            && self.end_chainage_m > self.start_chainage_m
            && (0.5..=(self.end_chainage_m - self.start_chainage_m) / 2.)
                .contains(&self.transition_m)
            && !self.spectrum.is_empty()
            && self.spectrum.len() <= 32
            && self
                .spectrum
                .iter()
                .all(|v| (0.5..=1000.).contains(&v[0]) && (0. ..=0.1).contains(&v[1]))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VerticalSuspension {
    pub enabled: bool,
    pub sprung_mass_fraction: f64,
    pub spring_n_m: f64,
    pub damper_ns_m: f64,
    pub contact_spring_n_m: f64,
    pub contact_damper_ns_m: f64,
    pub travel_m: f64,
    pub bump_stop_n_m: f64,
    pub sections: Vec<IrregularitySection>,
}
impl VerticalSuspension {
    pub fn valid(&self) -> bool {
        (0.5..=0.99).contains(&self.sprung_mass_fraction)
            && (10_000. ..=100_000_000.).contains(&self.spring_n_m)
            && (0. ..=10_000_000.).contains(&self.damper_ns_m)
            && (100_000. ..=1_000_000_000.).contains(&self.contact_spring_n_m)
            && (0. ..=10_000_000.).contains(&self.contact_damper_ns_m)
            && (0.001..=0.5).contains(&self.travel_m)
            && (100_000. ..=1_000_000_000.).contains(&self.bump_stop_n_m)
            && self.sections.len() <= 4096
            && self.sections.iter().all(IrregularitySection::valid)
            && self
                .sections
                .windows(2)
                .all(|s| s[0].end_chainage_m <= s[1].start_chainage_m)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DerailmentLimits {
    pub enabled: bool,
    pub minimum_speed_mps: f64,
    pub nadal_multiplier: f64,
    pub unloading_fraction: f64,
    pub persistence_s: f64,
    pub offrail_drag_mu: f64,
}
impl DerailmentLimits {
    pub fn valid(&self) -> bool {
        (0. ..=10.).contains(&self.minimum_speed_mps)
            && (0.25..=4.).contains(&self.nadal_multiplier)
            && (0. ..=0.5).contains(&self.unloading_fraction)
            && (0.01..=30.).contains(&self.persistence_s)
            && (0.01..=2.).contains(&self.offrail_drag_mu)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LateralSuspension {
    pub stiffness_npm: f64,
    pub damping_nspm: f64,
    pub travel_m: f64,
    pub maximum_attack_rad: f64,
}
impl LateralSuspension {
    pub fn valid(&self) -> bool {
        (10_000. ..=100_000_000.).contains(&self.stiffness_npm)
            && (0. ..=10_000_000.).contains(&self.damping_nspm)
            && (0.001..=0.15).contains(&self.travel_m)
            && (0.001..=0.3).contains(&self.maximum_attack_rad)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct VehicleBogieModel {
    pub mass_kg: f64,
    pub body_length_m: f64,
    /// Formation coordinates: support 0 is toward increasing path chainage.
    pub support_offsets_m: [f64; 2],
    pub gauge_m: f64,
    /// Native CentreOfGravity.Y may be zero; it removes height-induced load
    /// transfer without changing the supported mass or inventing a fallback.
    pub cog_height_m: f64,
    pub axles: u32,
    pub bogies: u32,
    pub rigid_wheelbase_m: f64,
    pub flange_angle_rad: f64,
    pub flange_height_m: f64,
    pub suspension: Option<LateralSuspension>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertical_suspension: Option<VerticalSuspension>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derailment: Option<DerailmentLimits>,
}
impl VehicleBogieModel {
    pub fn for_length(length_m: f64) -> Self {
        // TrainCar.Initialize fallback; shape support pivots remain a separate
        // presentation concern. A short fixed wheelbase uses the same .65 rule.
        let span = if length_m > 4.3 {
            length_m - 4.3
        } else {
            length_m * 0.65
        };
        Self {
            mass_kg: 40_000.,
            body_length_m: length_m,
            support_offsets_m: [span / 2., -span / 2.],
            gauge_m: 1.435,
            cog_height_m: 1.8,
            axles: 4,
            bogies: 2,
            rigid_wheelbase_m: 1.6764,
            flange_angle_rad: 1.22173,
            flange_height_m: 0.0254,
            suspension: None,
            vertical_suspension: None,
            derailment: None,
        }
    }
    pub fn valid(&self) -> bool {
        (100. ..=1_000_000.).contains(&self.mass_kg)
            && (0.5..=100.).contains(&self.body_length_m)
            && self
                .support_offsets_m
                .iter()
                .all(|x| x.is_finite() && x.abs() <= 50.)
            && self.support_offsets_m[0] > self.support_offsets_m[1]
            && self.support_offsets_m[0] > 0.
            && self.support_offsets_m[1] < 0.
            && (0.3..=2.5).contains(&self.gauge_m)
            && (0. ..=10.).contains(&self.cog_height_m)
            && (1..=32).contains(&self.axles)
            && (1..=8).contains(&self.bogies)
            && (0.1..=30.).contains(&self.rigid_wheelbase_m)
            && (0.5..=1.5).contains(&self.flange_angle_rad)
            && (0.005..=0.1).contains(&self.flange_height_m)
            && self
                .suspension
                .as_ref()
                .is_none_or(LateralSuspension::valid)
            && self
                .vertical_suspension
                .as_ref()
                .is_none_or(VerticalSuspension::valid)
            && self.derailment.as_ref().is_none_or(DerailmentLimits::valid)
    }
    pub fn reverse(&mut self) {
        self.support_offsets_m = [-self.support_offsets_m[1], -self.support_offsets_m[0]];
    }
}

fn values(ast: &Ast, key: &str) -> Option<Vec<String>> {
    let Ast::List(items) = ast else { return None };
    if matches!(items.first(), Some(Ast::Atom(Atom::Symbol(s))) if s.eq_ignore_ascii_case(key)) {
        return Some(
            items
                .iter()
                .skip(1)
                .filter_map(|v| match v {
                    Ast::Atom(Atom::Symbol(s) | Atom::String(s)) => Some(s.clone()),
                    Ast::Atom(Atom::Number(n)) => Some(n.to_string()),
                    Ast::Atom(Atom::Integer(n)) => Some(n.to_string()),
                    _ => None,
                })
                .collect(),
        );
    }
    items.iter().find_map(|v| values(v, key))
}
fn length(ast: &Ast, key: &str) -> Option<f64> {
    let v = openrailsrs_formats::msts_units::parse_length_m(&values(ast, key)?.join(" "))?;
    (v.is_finite() && v > 0.).then_some(v)
}
fn count(ast: &Ast, key: &str) -> Option<u32> {
    values(ast, key)?
        .first()?
        .parse::<u32>()
        .ok()
        .filter(|v| (1..=32).contains(v))
}

/// Numeric authored metadata and documented native defaults. Shape wheel/part
/// counts are unavailable here: an explicit axle/bogie declaration wins, then
/// the legacy NumWheels hint, then the four-axle/two-bogie fallback.
pub fn parse_bogie_model(ast: &Ast, engine: bool, length_m: f64) -> VehicleBogieModel {
    let mut m = VehicleBogieModel::for_length(length_m);
    if let Some(span) = length(ast, "ORTSLengthBogieCentre") {
        m.support_offsets_m = [span / 2., -span / 2.];
    }
    m.gauge_m = openrailsrs_formats::parse_vehicle_curve_parameters(ast, engine)
        .track_gauge_m
        .unwrap_or(1.435);
    m.axles = count(ast, "ORTSNumberAxles")
        .or_else(|| engine.then(|| count(ast, "ORTSNumberDriveAxles")).flatten())
        .or_else(|| count(ast, "NumWheels").filter(|v| *v < 7))
        .unwrap_or(4);
    m.bogies = count(ast, "ORTSNumberBogies").unwrap_or(if m.axles < 4 { 1 } else { 2 });
    let passenger = values(ast, "Type").is_some_and(|v| {
        v.iter()
            .any(|s| s.eq_ignore_ascii_case("Passenger") || s.eq_ignore_ascii_case("Carriage"))
    });
    m.rigid_wheelbase_m = length(ast, "ORTSRigidWheelBase").unwrap_or_else(|| {
        match (m.bogies, m.axles / m.bogies, engine, passenger) {
            (1, 2, _, _) => 3.5052,
            (1, 3, _, _) => 3.6576,
            (_, 3, true, _) => 3.5052,
            (_, 3, false, _) => 3.6576,
            (_, 2, false, true) => 2.4384,
            _ => 1.6764,
        }
    });
    if let Some(v) = values(ast, "CentreOfGravity")
        .and_then(|v| v.get(1).cloned())
        .and_then(|v| openrailsrs_formats::msts_units::parse_length_m(&v))
    {
        m.cog_height_m = v;
    }
    m.flange_height_m = length(ast, "ORTSWheelFlangeLength").unwrap_or(0.0254);
    if let Some(raw) = values(ast, "ORTSMaximumWheelFlangeAngle").map(|v| v.join(" ")) {
        let raw = raw.trim().to_ascii_lowercase();
        let angle = if let Some(v) = raw.strip_suffix("deg") {
            v.trim().parse::<f64>().ok().map(|v| v * 0.0174533)
        } else {
            raw.trim_end_matches("rad").trim().parse::<f64>().ok()
        };
        if let Some(v) = angle {
            m.flange_angle_rad = v;
        }
    }
    m
}

pub fn load_consist_bogie_models(
    consist: &Path,
    base: &Path,
    lengths: &[f64],
    masses: &[f64],
) -> Result<Vec<VehicleBogieModel>, TrainError> {
    let ast = openrailsrs_formats::read_vehicle_ast(consist)?;
    let entries = ConsistFile::from_ast(&ast)?.entries;
    if entries.len() != lengths.len() || entries.len() != masses.len() {
        return Err(TrainError::Parse(
            "Bogie geometry does not match formation".into(),
        ));
    }
    entries
        .iter()
        .zip(lengths)
        .zip(masses)
        .map(|((entry, &length_m), &mass)| {
            let (relative, engine, flipped) = match entry {
                ConsistEntry::Engine { path, flipped, .. } => (path, true, *flipped),
                ConsistEntry::Wagon { path, flipped, .. } => (path, false, *flipped),
            };
            let path = resolve_consist_entry_path(base, relative);
            let ast = if engine && crate::steam_loader::is_toml_eng(&path).unwrap_or(false) {
                Ast::List(vec![])
            } else {
                openrailsrs_formats::read_vehicle_ast(path)?
            };
            let mut m = parse_bogie_model(&ast, engine, length_m);
            m.mass_kg = mass;
            if flipped {
                m.reverse();
            }
            if !m.valid() {
                return Err(TrainError::Parse(format!(
                    "Invalid bogie geometry: {relative}"
                )));
            }
            Ok(m)
        })
        .collect()
}

pub fn load_models_for_consist(
    path: &Path,
    asset_root: &Path,
    consist: &crate::Consist,
) -> Result<Vec<VehicleBogieModel>, TrainError> {
    let masses = consist
        .vehicles
        .iter()
        .map(|v| match v {
            crate::Vehicle::Loco(v) => v.mass_kg,
            crate::Vehicle::Wagon(v) => v.mass_kg,
        })
        .collect::<Vec<_>>();
    load_consist_bogie_models(path, asset_root, &consist.vehicle_lengths_m(), &masses)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_geometry_and_units_are_independent_of_shape_yaw() {
        let ast = openrailsrs_formats::parse_vehicle_text("Wagon ( own Type ( Passenger ) NumWheels ( 4 ) ORTSNumberBogies ( 2 ) ORTSLengthBogieCentre ( 40ft ) ORTSRigidWheelBase ( 8ft ) ORTSTrackGauge ( 1000mm ) CentreOfGravity ( 0 1.6m 0 ) ORTSMaximumWheelFlangeAngle ( 70deg ) ORTSWheelFlangeLength ( 1in ) )").unwrap();
        let p = parse_bogie_model(&ast, false, 20.);
        assert!(p.valid());
        assert!((p.support_offsets_m[0] - 6.096).abs() < 1e-8);
        assert!((p.rigid_wheelbase_m - 2.4384).abs() < 1e-8);
        assert_eq!(p.gauge_m, 1.);
        assert_eq!(p.cog_height_m, 1.6);
        assert!((p.flange_angle_rad - 1.221731).abs() < 1e-8);
    }
    #[test]
    fn native_zero_centre_of_gravity_height_is_preserved() {
        let ast = openrailsrs_formats::parse_vehicle_text(
            "Wagon ( legacy CentreOfGravity ( 0m 0m 0m ) )",
        )
        .unwrap();
        let mut model = parse_bogie_model(&ast, false, 20.);
        assert_eq!(model.cog_height_m, 0.);
        assert!(model.valid());
        model.cog_height_m = -0.01;
        assert!(!model.valid());
        model.cog_height_m = f64::NAN;
        assert!(!model.valid());
    }
    #[test]
    fn reversal_preserves_asymmetric_support_geometry() {
        let mut p = VehicleBogieModel::for_length(20.);
        p.support_offsets_m = [7., -5.];
        p.reverse();
        assert_eq!(p.support_offsets_m, [5., -7.]);
        p.reverse();
        assert_eq!(p.support_offsets_m, [7., -5.]);
    }
}
