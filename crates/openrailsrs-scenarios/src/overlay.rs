//! Optional `scenario.overlay.toml` merged after MSTS activity import.

use std::path::Path;

use serde::Deserialize;

use crate::{ScenarioError, ScenarioFile};

/// Filename searched in the import output directory.
pub const SCENARIO_OVERLAY_FILENAME: &str = "scenario.overlay.toml";

/// Partial scenario overrides applied on top of an imported activity.
#[derive(Debug, Default, Deserialize)]
pub struct ScenarioOverlay {
    #[serde(default)]
    pub simulation: Option<SimulationOverlay>,
    #[serde(default)]
    pub train: Option<TrainOverlay>,
    #[serde(default)]
    pub validate: Option<super::ValidateSection>,
    #[serde(default)]
    pub gameplay: Option<super::GameplaySection>,
    #[serde(default)]
    pub output: Option<super::OutputSection>,
    #[serde(default)]
    pub route: Option<RouteOverlay>,
}

#[derive(Debug, Default, Deserialize)]
pub struct RouteOverlay {
    pub assume_signals_clear: Option<bool>,
    #[serde(default)]
    pub edge_speed_limits: Vec<super::EdgeSpeedLimitDef>,
}

#[derive(Debug, Default, Deserialize)]
pub struct SimulationOverlay {
    pub duration: Option<f64>,
    pub time_step: Option<f64>,
    pub seed: Option<u64>,
    pub driver_brake_full_scale_psi: Option<f64>,
    pub brake_cylinder_full_scale_psi: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
pub struct TrainOverlay {
    pub consist: Option<String>,
    pub davis: Option<super::DavisSection>,
    pub max_capacity: Option<u32>,
}

/// Load `scenario.overlay.toml` from `dir` if present and merge into `scenario`.
pub fn apply_scenario_overlay_dir(
    scenario: &mut ScenarioFile,
    dir: &Path,
) -> Result<bool, ScenarioError> {
    apply_scenario_overlay_file(scenario, &dir.join(SCENARIO_OVERLAY_FILENAME))
}

/// Merge overlay file into `scenario`. Returns `Ok(false)` when the file is absent.
pub fn apply_scenario_overlay_file(
    scenario: &mut ScenarioFile,
    path: &Path,
) -> Result<bool, ScenarioError> {
    if !path.is_file() {
        return Ok(false);
    }
    let text = std::fs::read_to_string(path).map_err(|e| ScenarioError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    let overlay: ScenarioOverlay = toml::from_str(&text).map_err(ScenarioError::Toml)?;
    apply_scenario_overlay(scenario, &overlay);
    Ok(true)
}

/// Merge route/train overrides for a simulation run.
///
/// Unlike [`apply_scenario_overlay`], leaves `[simulation].duration` and `[validate]` from
/// `scenario.toml` intact so `scenario.overlay.toml` can hold import-time defaults without
/// shortening OR evaluation runs or replacing validation thresholds.
pub fn apply_scenario_runtime_overlay(scenario: &mut ScenarioFile, overlay: &ScenarioOverlay) {
    if let Some(route) = &overlay.route {
        if let Some(clear) = route.assume_signals_clear {
            scenario.route.assume_signals_clear = clear;
        }
        if !route.edge_speed_limits.is_empty() {
            scenario.route.edge_speed_limits = route.edge_speed_limits.clone();
        }
    }
    if let Some(train) = &overlay.train {
        if let Some(consist) = &train.consist {
            scenario.train.consist = consist.clone();
        }
        if let Some(davis) = &train.davis {
            scenario.train.davis = Some(davis.clone());
        }
        if let Some(max_capacity) = train.max_capacity {
            scenario.train.max_capacity = Some(max_capacity);
        }
    }
    if let Some(sim) = &overlay.simulation {
        if let Some(v) = sim.driver_brake_full_scale_psi {
            scenario.simulation.driver_brake_full_scale_psi = Some(v);
        }
        if let Some(v) = sim.brake_cylinder_full_scale_psi {
            scenario.simulation.brake_cylinder_full_scale_psi = Some(v);
        }
    }
}

/// Load and merge `scenario.overlay.toml` for simulation (route/train only).
pub fn apply_scenario_runtime_overlay_dir(
    scenario: &mut ScenarioFile,
    dir: &Path,
) -> Result<bool, ScenarioError> {
    let path = dir.join(SCENARIO_OVERLAY_FILENAME);
    if !path.is_file() {
        return Ok(false);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| ScenarioError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    let overlay: ScenarioOverlay = toml::from_str(&text).map_err(ScenarioError::Toml)?;
    apply_scenario_runtime_overlay(scenario, &overlay);
    Ok(true)
}

/// Merge parsed overlay fields into an imported scenario (route/placement untouched).
pub fn apply_scenario_overlay(scenario: &mut ScenarioFile, overlay: &ScenarioOverlay) {
    if let Some(sim) = &overlay.simulation {
        if let Some(duration) = sim.duration {
            scenario.simulation.duration = duration;
        }
        if let Some(time_step) = sim.time_step {
            scenario.simulation.time_step = time_step;
        }
        if let Some(seed) = sim.seed {
            scenario.simulation.seed = seed;
        }
        if let Some(v) = sim.driver_brake_full_scale_psi {
            scenario.simulation.driver_brake_full_scale_psi = Some(v);
        }
        if let Some(v) = sim.brake_cylinder_full_scale_psi {
            scenario.simulation.brake_cylinder_full_scale_psi = Some(v);
        }
    }
    if let Some(train) = &overlay.train {
        if let Some(consist) = &train.consist {
            scenario.train.consist = consist.clone();
        }
        if let Some(davis) = &train.davis {
            scenario.train.davis = Some(davis.clone());
        }
        if let Some(max_capacity) = train.max_capacity {
            scenario.train.max_capacity = Some(max_capacity);
        }
    }
    if overlay.validate.is_some() {
        scenario.validate = overlay.validate.clone();
    }
    if let Some(gameplay) = &overlay.gameplay {
        scenario.gameplay = gameplay.clone();
    }
    if let Some(output) = &overlay.output {
        scenario.output = output.clone();
    }
    apply_scenario_runtime_overlay(scenario, overlay);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        GameplaySection, ObjectiveKind, OutputSection, RouteSection, ScenarioMeta,
        SimulationSection, TrainSection,
    };

    fn minimal_scenario() -> ScenarioFile {
        ScenarioFile {
            scenario: ScenarioMeta {
                name: "test".into(),
                description: String::new(),
                start_time_s: None,
                season: None,
            },
            route: RouteSection {
                path: ".".into(),
                start: "n1".into(),
                destination: "n2".into(),
                start_offset_m: None,
                stops: vec![],
                switches: vec![],
                assume_signals_clear: false,
                edge_speed_limits: vec![],
            },
            train: TrainSection {
                consist: "from_act.con".into(),
                davis: None,
                max_capacity: None,
            },
            gameplay: GameplaySection {
                objective: ObjectiveKind::Arrive,
                time_limit_seconds: None,
                difficulty: crate::Difficulty::Normal,
                penalty_per_second_late: 0.0,
            },
            simulation: SimulationSection {
                duration: 3600.0,
                time_step: 1.0,
                seed: 42,
                driver_brake_full_scale_psi: None,
                brake_cylinder_full_scale_psi: None,
                legacy_power_cap: true,
                train_air_lap_hold: false,
            },
            output: OutputSection {
                csv: "run.csv".into(),
                metadata: "run.json".into(),
            },
            extra_trains: vec![],
            sound_regions: vec![],
            validate: None,
        }
    }

    #[test]
    fn runtime_overlay_keeps_duration_and_validate() {
        let overlay: ScenarioOverlay = toml::from_str(
            r#"
[simulation]
duration = 65.0

[route]
assume_signals_clear = true

[[route.edge_speed_limits]]
edge = "e10777"
speed_limit_kmh = 80.467

[validate]
max_velocity_rms = 4.5
"#,
        )
        .expect("parse overlay");

        let mut scenario = minimal_scenario();
        scenario.validate = Some(crate::ValidateSection {
            baseline_or: Some("../baselines/or.csv".into()),
            thresholds: openrailsrs_validate::ValidationConfig {
                max_velocity_rms: Some(0.5),
                ..Default::default()
            },
            phase_bounds: None,
            phase_max_velocity_rms: None,
        });
        apply_scenario_runtime_overlay(&mut scenario, &overlay);

        assert_eq!(scenario.simulation.duration, 3600.0);
        assert_eq!(
            scenario
                .validate
                .as_ref()
                .unwrap()
                .thresholds
                .max_velocity_rms,
            Some(0.5)
        );
        assert!(scenario.route.assume_signals_clear);
        assert_eq!(scenario.route.edge_speed_limits.len(), 1);
    }

    #[test]
    fn runtime_overlay_merges_brake_mapping() {
        let overlay: ScenarioOverlay = toml::from_str(
            r#"
[simulation]
brake_cylinder_full_scale_psi = 35.0
"#,
        )
        .expect("parse overlay");

        let mut scenario = minimal_scenario();
        apply_scenario_runtime_overlay(&mut scenario, &overlay);
        assert_eq!(
            scenario.simulation.brake_cylinder_full_scale_psi,
            Some(35.0)
        );
        assert!((scenario.brake_mapping().cylinder_full_scale_psi - 35.0).abs() < 1e-9);
    }

    #[test]
    fn overlay_sets_duration_validate_and_consist() {
        let overlay: ScenarioOverlay = toml::from_str(
            r#"
[simulation]
duration = 65.0

[train]
consist = "consists/custom.con"

[validate]
baseline_or = "../baselines/or.csv"
max_velocity_rms = 4.5
"#,
        )
        .expect("parse overlay");

        let mut scenario = minimal_scenario();
        apply_scenario_overlay(&mut scenario, &overlay);

        assert_eq!(scenario.simulation.duration, 65.0);
        assert_eq!(scenario.train.consist, "consists/custom.con");
        assert_eq!(scenario.route.start, "n1");
        let validate = scenario.validate.expect("validate");
        assert_eq!(validate.baseline_or.as_deref(), Some("../baselines/or.csv"));
        assert_eq!(validate.thresholds.max_velocity_rms, Some(4.5));
    }
}
