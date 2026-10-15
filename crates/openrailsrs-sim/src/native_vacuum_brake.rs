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
    /// Absolute pipe 3 pressure. Absent in saves made before independent vacuum.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub independent_pipe_psi: Option<f64>,
    /// Piston displacement, refreshed during native train-pipe propagation.
    /// Update-only legacy references leave the original default at zero.
    #[serde(default)]
    pub brake_cyl_fraction: f64,
}

impl NativeVacuumState {
    pub fn new(profile: NativeVacuumBrakeProfile, length_m: f64) -> Self {
        let charged = ATMOSPHERE_PSI - profile.charged_vacuum_psi;
        let pipe_volume_m3 = profile
            .pipe_volume_m3
            .unwrap_or(0.05f64.powi(2) * std::f64::consts::PI / 4. * (length_m + 1.).max(5.));
        Self {
            brake_cyl_fraction: 0.,
            independent_pipe_psi: profile.independent.as_ref().map(|_| charged),
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
        if self.profile.independent.is_some() {
            self.independent_pipe_psi = Some(self.pipe_psi);
        }
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

    pub fn prepare_pipe(&mut self) {
        let vacuum = ATMOSPHERE_PSI as f32 - self.cylinder_psi as f32;
        self.brake_cyl_fraction =
            f64::from((1. - vacuum / self.profile.charged_vacuum_psi as f32).clamp(0.01, 1.));
    }

    /// The original independent controller admits air to pipe 3, or restores
    /// vacuum. It never changes the train pipe or a hauled coach's reservoir.
    pub fn step_independent_pipe(&mut self, command: f64, dt: f64, lead_pipe: Option<f64>) {
        let Some(controller) = &self.profile.independent else {
            return;
        };
        let (command, mode) = controller.setting(command);
        let mode = mode
            .strip_prefix("enginebrakescontroller")
            .or_else(|| mode.strip_prefix("trainbrakescontroller"))
            .unwrap_or(mode);
        // OR stores these controller thresholds in Single. Retaining that
        // precision also retains its one-step continuous-service oscillation.
        let atmosphere = ATMOSPHERE_PSI as f32;
        let charged = atmosphere - self.profile.charged_vacuum_psi as f32;
        let mut pressure = self.independent_pipe_psi.unwrap_or(f64::from(charged)) as f32;
        let dt = dt as f32;
        match mode {
            "applystart" => {
                pressure = (pressure + dt * controller.application_psi_s as f32).min(atmosphere);
            }
            "emergencystart" => {
                pressure = (pressure + dt * controller.emergency_psi_s as f32).min(atmosphere);
            }
            "releasestart" => {
                pressure = (pressure - dt * controller.release_psi_s as f32).max(charged);
            }
            "vacuumcontinuousservicestart" | "brakenotchstart" => {
                let desired = command as f32 * (atmosphere - charged) + charged;
                let controlling_pressure = lead_pipe.unwrap_or(f64::from(pressure)) as f32;
                if controlling_pressure < desired {
                    pressure =
                        (pressure + dt * controller.application_psi_s as f32).min(atmosphere);
                } else if controlling_pressure > desired {
                    pressure = (pressure - dt * controller.release_psi_s as f32).max(charged);
                }
            }
            // Lap, running, dummy and other types have no pipe 3 operation in
            // VacuumSinglePipe.PropagateBrakeLinePressures in the pinned OR.
            _ => {}
        }
        self.independent_pipe_psi = Some(f64::from(pressure));
    }

    /// Engine/tender branch of VacuumSinglePipe.Update. Train braking cannot
    /// be bailed off with the independent handle; both use this same cylinder.
    pub fn step_pipe_independent(&mut self, train_command: f64, independent_command: f64, dt: f64) {
        let Some(independent) = self.independent_pipe_psi else {
            self.step_pipe(dt, false);
            return;
        };
        let independent_command = self
            .profile
            .independent
            .as_ref()
            .map_or(independent_command, |c| c.setting(independent_command).0);
        self.cylinder_psi = if train_command == 0. && independent_command > 0. {
            independent
        } else if train_command > 0. {
            self.pipe_psi.max(independent)
        } else {
            self.pipe_psi
        };
        if self.pipe_psi < self.reservoir_psi {
            let p = &self.profile;
            let ratio = p.reservoir_volume_m3 / self.pipe_volume_m3;
            let dp = (dt * p.application_psi_s * p.cylinder_volume_m3 / p.reservoir_volume_m3)
                .min((self.reservoir_psi - self.pipe_psi) / (1. + ratio));
            self.reservoir_psi -= dp;
        }
    }

    pub fn valid(&self) -> bool {
        let p = &self.profile;
        [self.pipe_psi, self.cylinder_psi, self.reservoir_psi]
            .iter()
            .all(|v| v.is_finite() && (0.0..=ATMOSPHERE_PSI + 0.001).contains(v))
            && self.pipe_volume_m3.is_finite()
            && self.pipe_volume_m3 > 0.
            && self.brake_cyl_fraction.is_finite()
            && (0.0..=1.0).contains(&self.brake_cyl_fraction)
            && [
                p.max_force_psi,
                p.application_psi_s,
                p.release_psi_s,
                p.cylinder_volume_m3,
                p.reservoir_volume_m3,
                p.charged_vacuum_psi,
            ]
            .iter()
            .all(|v| v.is_finite() && *v > 0.)
            && self
                .independent_pipe_psi
                .is_none_or(|v| v.is_finite() && (0.0..=ATMOSPHERE_PSI + 0.001).contains(&v))
            && self.independent_pipe_psi.is_some() == p.independent.is_some()
            && p.independent.as_ref().is_none_or(|c| {
                [c.application_psi_s, c.release_psi_s, c.emergency_psi_s]
                    .iter()
                    .all(|v| v.is_finite() && *v > 0.)
                    && !c.notches.is_empty()
                    && c.notches
                        .iter()
                        .all(|(v, _, _)| v.is_finite() && (0.0..=1.0).contains(v))
                    && c.notches.windows(2).all(|w| w[0].0 < w[1].0)
            })
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
            let p = &self.profile;
            let piston_change = p.reservoir_volume_m3
                / (p.reservoir_volume_m3 - self.brake_cyl_fraction * p.cylinder_volume_m3);
            (self.reservoir_psi + piston_change).min(self.cylinder_psi)
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
    fn independent_vacuum_matches_real_pinned_or_pipe_reservoir_and_force() {
        let oracle: Value = serde_json::from_str(include_str!(
            "../../../oracles/openrails-vacuum-independent.json"
        ))
        .unwrap();
        let config: Value = serde_json::from_str(include_str!(
            "../../../examples/vacuum_independent/reference-input.json"
        ))
        .unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/vacuum_independent");
        let pressure_tolerance = config["tolerances_declared_before_capture"]["pressure_psi_abs"]
            .as_f64()
            .unwrap();
        let force_tolerance = config["tolerances_declared_before_capture"]["shoe_force_n_abs"]
            .as_f64()
            .unwrap();
        for (case, spec) in oracle["cases"]
            .as_array()
            .unwrap()
            .iter()
            .zip(config["cases"].as_array().unwrap())
        {
            assert_eq!(case["name"], spec["name"]);
            let mut states: Vec<_> = spec["vehicles"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| {
                    let ast = openrailsrs_formats::read_vehicle_ast(
                        root.join(v["fixture"].as_str().unwrap()),
                    )
                    .unwrap();
                    NativeVacuumState::new(
                        openrailsrs_formats::parse_vehicle_brake_profile(&ast)
                            .native_vacuum
                            .unwrap(),
                        v["length_m"].as_f64().unwrap(),
                    )
                })
                .collect();
            let lead = states[0].profile.clone();
            let controller = lead.independent.clone().unwrap();
            assert!(
                (lead.charged_vacuum_psi - case["max_vacuum_psi"].as_f64().unwrap()).abs()
                    < 0.00003
            );
            for (key, actual) in [
                ("application_psi_s", controller.application_psi_s),
                ("release_psi_s", controller.release_psi_s),
                ("emergency_psi_s", controller.emergency_psi_s),
            ] {
                assert!((actual - case["controller"][key].as_f64().unwrap()).abs() < 0.00003);
            }
            for (i, state) in states.iter_mut().enumerate() {
                state.profile.charged_vacuum_psi = lead.charged_vacuum_psi;
                if state.profile.tender && lead.steam {
                    state.profile.independent = Some(controller.clone());
                    state.independent_pipe_psi = Some(ATMOSPHERE_PSI - lead.charged_vacuum_psi);
                }
                // Initialization is a captured input condition. Keep native
                // conversion rounding rather than prescribing later outputs.
                let initial = &case["initial"][i];
                state.pipe_psi = initial["pipe_psi"].as_f64().unwrap();
                state.cylinder_psi = initial["cylinder_psi"].as_f64().unwrap();
                state.reservoir_psi = initial["reservoir_psi"].as_f64().unwrap();
                state.brake_cyl_fraction = initial["brake_cyl_fraction"].as_f64().unwrap();
                if state.profile.independent.is_some() {
                    state.independent_pipe_psi =
                        Some(initial["independent_pipe_psi"].as_f64().unwrap());
                }
                let h = &case["hardware"][i];
                for (key, actual) in [
                    ("max_force_psi", state.profile.max_force_psi),
                    ("application_psi_s", state.profile.application_psi_s),
                    ("release_psi_s", state.profile.release_psi_s),
                    ("cylinder_volume_m3", state.profile.cylinder_volume_m3),
                    ("reservoir_volume_m3", state.profile.reservoir_volume_m3),
                    ("pipe_volume_m3", state.pipe_volume_m3),
                ] {
                    assert!(
                        (actual - h[key].as_f64().unwrap()).abs() < 0.00003,
                        "{} hardware {i} {key}",
                        case["name"]
                    );
                }
            }
            for point in case["points"].as_array().unwrap() {
                let train = point["train_command"].as_f64().unwrap();
                let independent = point["independent_command"].as_f64().unwrap();
                let requested = spec["phases"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|p| p["name"] == point["phase"])
                    .unwrap()["independent"]
                    .as_f64()
                    .unwrap();
                assert!((controller.setting(requested).0 - independent).abs() < 0.000001);
                let dt = case["dt_s"].as_f64().unwrap();
                let mut lead_pipe = None;
                for (i, state) in states.iter_mut().enumerate() {
                    let expected = &point["vehicles"][i];
                    state.prepare_pipe();
                    assert!(
                        (state.brake_cyl_fraction
                            - expected["brake_cyl_fraction"].as_f64().unwrap())
                        .abs()
                            < 0.00005
                    );
                    state.pipe_psi = expected["pipe_input_psi"].as_f64().unwrap();
                    if state.profile.independent.is_some() {
                        state.step_independent_pipe(independent, dt, lead_pipe);
                        lead_pipe = lead_pipe.or(state.independent_pipe_psi);
                        state.step_pipe_independent(train, independent, dt);
                        let err = (state.independent_pipe_psi.unwrap()
                            - expected["independent_pipe_psi"].as_f64().unwrap())
                        .abs();
                        assert!(
                            err <= pressure_tolerance,
                            "{} tick {} vehicle {i}: pipe 3 error {err}",
                            case["name"],
                            point["tick"]
                        );
                    } else {
                        state.step_pipe(dt, false);
                    }
                    for (key, actual) in [
                        ("pipe_psi", state.pipe_psi),
                        ("cylinder_psi", state.cylinder_psi),
                        ("reservoir_psi", state.reservoir_psi),
                        ("adjusted_reservoir_psi", state.adjusted_reservoir_psi()),
                        ("cylinder_vacuum_psi", ATMOSPHERE_PSI - state.cylinder_psi),
                    ] {
                        let err = (actual - expected[key].as_f64().unwrap()).abs();
                        assert!(
                            err <= pressure_tolerance,
                            "{} tick {} vehicle {i} {key}: {actual} vs {}, error {err}",
                            case["name"],
                            point["tick"],
                            expected[key]
                        );
                    }
                    let force =
                        state.shoe_force_n(case["hardware"][i]["max_force_n"].as_f64().unwrap());
                    let err = (force - expected["shoe_force_n"].as_f64().unwrap()).abs();
                    assert!(
                        err <= force_tolerance,
                        "{} tick {} vehicle {i}: force error {err}",
                        case["name"],
                        point["tick"]
                    );
                }
            }
        }
    }

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
