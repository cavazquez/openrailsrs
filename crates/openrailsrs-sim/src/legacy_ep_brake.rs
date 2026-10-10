//! OR 1.6.1 EP pressure model for stock without cylinder geometry.
use openrailsrs_formats::LegacyEpBrakeProfile;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct LegacyEpState {
    pub profile: LegacyEpBrakeProfile,
    pub pressure_psi: f64,
    pub auto_psi: f64,
    pub auxiliary_psi: f64,
    pipe_ratio: f64,
}
impl LegacyEpState {
    pub fn new(profile: LegacyEpBrakeProfile, length_m: f64) -> Self {
        let pipe = profile
            .pipe_volume_m3
            .unwrap_or(0.032f64.powi(2) * std::f64::consts::PI / 4. * (length_m + 1.).max(5.));
        let pipe_ratio = profile.auxiliary_volume_m3 / pipe;
        Self {
            auxiliary_psi: profile.charged_psi,
            profile,
            pressure_psi: 0.,
            auto_psi: 0.,
            pipe_ratio,
        }
    }
    pub fn precharge(&mut self, command: f64) {
        self.auto_psi = command.clamp(0., 1.) * self.profile.service_psi;
        self.pressure_psi = self.auto_psi;
        self.auxiliary_psi = self.profile.charged_psi;
    }
    pub fn step(&mut self, command: f64, dt: f64) {
        let target = command.clamp(0., 1.) * self.profile.service_psi;
        // Legacy EP updates automatic cylinder pressure electrically without
        // adding to AutoCylAir. OR's pneumatic release consequently empties
        // that cylinder when its holding valve releases (unlike advanced EP).
        if command <= 0. || self.auto_psi > target {
            self.auto_psi = 0.;
        }
        self.pressure_psi = self.auto_psi;
        let mut pipe = self.profile.charged_psi;
        if self.auxiliary_psi > pipe {
            let dp = (dt * 0.07).min((self.auxiliary_psi - pipe) / (1. + self.pipe_ratio));
            self.auxiliary_psi -= dp;
            pipe += dp * self.pipe_ratio;
        }
        if self.auxiliary_psi < pipe {
            let mut dp = dt * self.profile.charging_psi_s;
            if self.profile.distributor {
                // Main-reservoir pipe supply is 120 PSI; control reservoir
                // closes charging on the next update after its setpoint.
                dp = dp.min((120. - self.auxiliary_psi) / (1. + self.pipe_ratio));
            } else {
                let difference = pipe - self.auxiliary_psi;
                if difference < 1. {
                    dp *= 0.1 + 0.9 * difference;
                }
                dp = dp.min(difference / (1. + self.pipe_ratio));
            }
            self.auxiliary_psi += dp.max(0.);
        }
        if self.auto_psi < target {
            let ratio = self.profile.auxiliary_cylinder_ratio;
            let dp = (dt * self.profile.application_psi_s)
                .min(target - self.auto_psi)
                .min((self.auxiliary_psi - self.auto_psi) * ratio / (1. + ratio))
                .max(0.);
            self.auxiliary_psi -= dp / ratio;
            self.auto_psi += dp;
        }
    }
    pub fn shoe_force_n(&self, max_force: f64) -> f64 {
        max_force * (self.pressure_psi / self.profile.reference_psi).clamp(0., 1.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_legacy_ep_profiles_match_pinned_or161_dlls() {
        let oracle: serde_json::Value =
            serde_json::from_str(include_str!("../../../oracles/openrails-brake-power.json"))
                .unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/brake_supply_native");
        for case in oracle["legacy_ep"].as_array().unwrap() {
            let ast =
                openrailsrs_formats::read_vehicle_ast(root.join(case["file"].as_str().unwrap()))
                    .unwrap();
            let profile = openrailsrs_formats::parse_vehicle_brake_profile(&ast)
                .legacy_ep
                .unwrap();
            let mut state = LegacyEpState::new(profile, 20.);
            let mut tick = 0;
            for point in case["points"].as_array().unwrap() {
                while tick < point["tick"].as_u64().unwrap() {
                    let command = if tick < 100 {
                        0.
                    } else if tick < 400 {
                        0.6
                    } else if tick < 600 {
                        0.
                    } else if tick < 800 {
                        1.
                    } else {
                        0.
                    };
                    state.step(command, 0.05);
                    tick += 1;
                }
                for (key, actual) in [
                    ("pressure_psi", state.pressure_psi),
                    ("auto_psi", state.auto_psi),
                    ("auxiliary_psi", state.auxiliary_psi),
                ] {
                    let error = (actual - point[key].as_f64().unwrap()).abs();
                    assert!(
                        error <= 0.001,
                        "{} tick {tick} {key}: {actual} vs {}",
                        case["source"],
                        point[key]
                    );
                }
                let error = (state.shoe_force_n(case["max_force_n"].as_f64().unwrap())
                    - point["shoe_force_n"].as_f64().unwrap())
                .abs();
                assert!(
                    error <= 5.,
                    "{} tick {tick}: force error {error}",
                    case["source"]
                );
            }
        }
    }
}
