//! Scenario file format (`scenario.toml`) and validation.

pub mod error;
pub mod model;
pub mod timetable;

pub use error::ScenarioError;
pub use model::{
    DavisSection, Difficulty, GameplaySection, ObjectiveKind, OutputSection, RouteSection,
    ScenarioFile, ScenarioMeta, SimulationSection, SoundRegionDef, StopDef, SwitchDef,
    SwitchPositionDef, TrainEntryDef, TrainSection, ValidateSection,
};
pub use timetable::{TimetableEntry, TimetableFile, TimetableMeta, load_timetable};

use std::path::Path;

/// Load and deserialize a scenario from disk.
pub fn load_scenario(path: impl AsRef<Path>) -> Result<ScenarioFile, ScenarioError> {
    let text = std::fs::read_to_string(path.as_ref()).map_err(|e| ScenarioError::Io {
        path: path.as_ref().display().to_string(),
        source: e,
    })?;
    let file: ScenarioFile = toml::from_str(&text).map_err(ScenarioError::Toml)?;
    file.validate()?;
    Ok(file)
}
