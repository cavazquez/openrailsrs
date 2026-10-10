//! Port of the pinned OR 1.6.1 VacuumSinglePipe cylinder/reservoir update.
//! Pressures inside the solver are absolute PSI; a destroyed vacuum applies
//! brakes, unlike a charged air pipe. Units exposed to the cab are vacuum PSI.
use openrailsrs_formats::NativeVacuumBrakeProfile;
use serde::{Deserialize, Serialize};

pub(crate) const ATMOSPHERE_PSI: f64 = 1. / super::native_ep_brake::PSI_TO_BAR;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct NativeVacuumState {
    pub profile: NativeVacuumBrakeProfile,
    pub pipe_psi: f64,
    pub cylinder_psi: f64,
    pub reservoir_psi: f64,
    pub pipe_volume_m3: f64,
}

impl NativeVacuumState {
    pub fn new(profile: NativeVacuumBrakeProfile, length_m: f64) -> Self {
        let charged = ATMOSPHERE_PSI - profile.charged_vacuum_psi;
        let pipe_volume_m3 = profile
            .pipe_volume_m3
            .unwrap_or(0.05f64.powi(2) * std::f64::consts::PI / 4. * (length_m + 1.).max(5.));
        Self {
            profile,
            pipe_psi: charged,
            cylinder_psi: charged,
            reservoir_psi: charged,
            pipe_volume_m3,
        }
    }

    pub fn precharge(&mut self, command: f64) {
        self.reservoir_psi = ATMOSPHERE_PSI - self.profile.charged_vacuum_psi;
        self.pipe_psi =
            self.reservoir_psi + command.clamp(0., 1.) * self.profile.charged_vacuum_psi;
        self.cylinder_psi = self.pipe_psi;
    }

    pub fn step(&mut self, command: f64, dt: f64, vented: bool) {
        // The controller establishes pipe vacuum; automatic cylinder valves
        // keep their own air, rather than converting the handle to force.
        self.pipe_psi = if vented {
            ATMOSPHERE_PSI
        } else {
            ATMOSPHERE_PSI - self.profile.charged_vacuum_psi * (1. - command.clamp(0., 1.))
        };
        self.step_pipe(dt, false);
    }

    /// Original valve update with an externally propagated pipe pressure.
    pub fn step_pipe(&mut self, dt: f64, bleed: bool) {
        let p = &self.profile;
        if bleed {
            let dp = dt * p.application_psi_s;
            self.reservoir_psi = (self.reservoir_psi + dp).min(ATMOSPHERE_PSI);
            self.cylinder_psi = (self.cylinder_psi + dp).min(ATMOSPHERE_PSI);
        } else if self.pipe_psi < self.reservoir_psi {
            let ratio = p.reservoir_volume_m3 / self.pipe_volume_m3;
            let dp = (dt * p.application_psi_s * p.cylinder_volume_m3 / p.reservoir_volume_m3)
                .min((self.reservoir_psi - self.pipe_psi) / (1. + ratio));
            self.reservoir_psi -= dp;
            self.cylinder_psi = self.reservoir_psi;
        } else if self.pipe_psi < self.cylinder_psi {
            let ratio = p.cylinder_volume_m3 / self.pipe_volume_m3;
            let dp = (dt * p.release_psi_s).min((self.cylinder_psi - self.pipe_psi) / (1. + ratio));
            self.cylinder_psi -= dp;
        } else if self.pipe_psi > self.cylinder_psi {
            let ratio = p.cylinder_volume_m3 / self.pipe_volume_m3;
            let dp =
                (dt * p.application_psi_s).min((self.pipe_psi - self.cylinder_psi) / (1. + ratio));
            self.cylinder_psi += dp;
            if !p.direct_admission {
                self.pipe_psi -= dp * ratio;
            }
        }
    }

    pub fn adjusted_reservoir_psi(&self) -> f64 {
        if self.reservoir_psi >= self.cylinder_psi {
            self.reservoir_psi
        } else {
            (self.reservoir_psi + 1.).min(self.cylinder_psi)
        }
    }

    pub fn shoe_force_n(&self, nominal: f64) -> f64 {
        nominal
            * ((self.cylinder_psi - self.adjusted_reservoir_psi()) / self.profile.max_force_psi)
                .clamp(0., 1.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    #[test]
    fn vacuum_native_original_profiles_match_pinned_dll_valves_and_reservoirs() {
        let oracle: Value =
            serde_json::from_str(include_str!("../../../oracles/openrails-brake-power.json"))
                .unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/brake_supply_native");
        for case in oracle["vacuum"].as_array().unwrap() {
            let source = case["source"].as_str().unwrap();
            let ast =
                openrailsrs_formats::read_vehicle_ast(root.join(case["file"].as_str().unwrap()))
                    .unwrap();
            let profile = openrailsrs_formats::parse_vehicle_brake_profile(&ast)
                .native_vacuum
                .unwrap();
            let n = |key: &str| case[key].as_f64().unwrap();
            assert!(
                (profile.max_force_psi - n("max_force_psi")).abs() < 0.00003,
                "{source}: pressure unit conversion"
            );
            assert!(
                (profile.application_psi_s - n("apply_psi_s")).abs() < 0.00003,
                "{source}: application units"
            );
            assert!(
                (profile.cylinder_volume_m3 - n("cylinder_volume_m3")).abs() < 0.000001,
                "{source}: cylinder volume"
            );
            let mut state = NativeVacuumState::new(profile, 20.);
            state.precharge(0.);
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
                    state.pipe_psi =
                        ATMOSPHERE_PSI - state.profile.charged_vacuum_psi * (1. - command);
                    state.step_pipe(0.05, (1100..1140).contains(&tick));
                    tick += 1;
                }
                for (key, actual) in [
                    ("pipe_psi", state.pipe_psi),
                    ("cylinder_psi", state.cylinder_psi),
                    ("reservoir_psi", state.reservoir_psi),
                    ("adjusted_reservoir_psi", state.adjusted_reservoir_psi()),
                ] {
                    let error = (actual - point[key].as_f64().unwrap()).abs();
                    assert!(
                        error <= 0.0005,
                        "{source} tick {tick} {key}: {actual} vs {} (error {error})",
                        point[key]
                    );
                }
                let error = (state.shoe_force_n(n("max_force_n"))
                    - point["shoe_force_n"].as_f64().unwrap())
                .abs();
                assert!(
                    error <= 5.,
                    "{source} tick {tick}: shoe force error {error}"
                );
            }
        }
    }
}
