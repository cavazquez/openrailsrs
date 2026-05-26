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
}

#[derive(Debug, Default, Deserialize)]
pub struct SimulationOverlay {
    pub duration: Option<f64>,
    pub time_step: Option<f64>,
    pub seed: Option<u64>,
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
