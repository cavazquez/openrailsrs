//! Declared mechanical bogie overrides. Visual yaw/pivots remain presentation.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BogieDynamicsDef {
    pub vehicle: usize,
    /// Formation offsets, front then rear, in metres about the car centre.
    #[serde(default)]
    pub support_offsets_m: Option<[f64; 2]>,
    #[serde(default)]
    pub lateral_suspension: Option<LateralSuspensionDef>,
    /// Explicit mechanical shared support with an adjacent body. This is
    /// independent of presentation.jacobs and its mesh/matrix binding.
    #[serde(default)]
    pub shared_with: Option<usize>,
    #[serde(default)]
    pub vertical_suspension: Option<VerticalSuspensionDef>,
    #[serde(default)]
    pub derailment: Option<DerailmentLimitsDef>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct IrregularitySectionDef {
    pub start_chainage_m: f64,
    pub end_chainage_m: f64,
    pub transition_m: f64,
    pub seed: u64,
    pub spectrum: Vec<[f64; 2]>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VerticalSuspensionDef {
    pub enabled: bool,
    pub sprung_mass_fraction: f64,
    pub spring_n_m: f64,
    pub damper_ns_m: f64,
    pub contact_spring_n_m: f64,
    pub contact_damper_ns_m: f64,
    pub travel_m: f64,
    pub bump_stop_n_m: f64,
    pub sections: Vec<IrregularitySectionDef>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DerailmentLimitsDef {
    pub enabled: bool,
    pub minimum_speed_mps: f64,
    pub nadal_multiplier: f64,
    pub unloading_fraction: f64,
    pub persistence_s: f64,
    pub offrail_drag_mu: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LateralSuspensionDef {
    pub stiffness_npm: f64,
    pub damping_nspm: f64,
    pub travel_m: f64,
    pub maximum_attack_rad: f64,
}
impl BogieDynamicsDef {
    pub fn valid(&self) -> bool {
        self.vehicle < 256
            && self.support_offsets_m.is_none_or(|p| {
                p.iter().all(|x| x.is_finite() && x.abs() <= 50.) && p[0] > 0. && p[1] < 0.
            })
            && self
                .shared_with
                .is_none_or(|v| v < 256 && self.vehicle.abs_diff(v) == 1)
            && self.lateral_suspension.as_ref().is_none_or(|p| {
                (10_000. ..=100_000_000.).contains(&p.stiffness_npm)
                    && (0. ..=10_000_000.).contains(&p.damping_nspm)
                    && (0.001..=0.15).contains(&p.travel_m)
                    && (0.001..=0.3).contains(&p.maximum_attack_rad)
            })
            && self.derailment.as_ref().is_none_or(|p| {
                (0. ..=10.).contains(&p.minimum_speed_mps)
                    && (0.25..=4.).contains(&p.nadal_multiplier)
                    && (0. ..=0.5).contains(&p.unloading_fraction)
                    && (0.01..=30.).contains(&p.persistence_s)
                    && (0.01..=2.).contains(&p.offrail_drag_mu)
            })
            && self.vertical_suspension.as_ref().is_none_or(|p| {
                (0.5..=0.99).contains(&p.sprung_mass_fraction)
                    && (10_000. ..=100_000_000.).contains(&p.spring_n_m)
                    && (0. ..=10_000_000.).contains(&p.damper_ns_m)
                    && (100_000. ..=1_000_000_000.).contains(&p.contact_spring_n_m)
                    && (0. ..=10_000_000.).contains(&p.contact_damper_ns_m)
                    && (0.001..=0.5).contains(&p.travel_m)
                    && (100_000. ..=1_000_000_000.).contains(&p.bump_stop_n_m)
                    && p.sections.len() <= 4096
                    && p.sections.iter().all(|s| {
                        s.start_chainage_m.is_finite()
                            && s.start_chainage_m >= 0.
                            && s.end_chainage_m.is_finite()
                            && s.end_chainage_m > s.start_chainage_m
                            && (0.5..=(s.end_chainage_m - s.start_chainage_m) / 2.)
                                .contains(&s.transition_m)
                            && !s.spectrum.is_empty()
                            && s.spectrum.len() <= 32
                            && s.spectrum.iter().all(|v| {
                                (0.5..=1000.).contains(&v[0]) && (0. ..=0.1).contains(&v[1])
                            })
                    })
                    && p.sections
                        .windows(2)
                        .all(|s| s[0].end_chainage_m <= s[1].start_chainage_m)
            })
    }
}
