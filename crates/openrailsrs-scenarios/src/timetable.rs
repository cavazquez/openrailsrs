//! Timetable file format (`timetable.toml`) — Fase 18.
//!
//! A timetable describes multiple train services over a shared route network.
//! Each entry specifies which consist to use, where it starts, where it goes,
//! and when it departs (relative to t = 0 s).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::ScenarioError;

/// Root of a `timetable.toml` file.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TimetableFile {
    pub timetable: TimetableMeta,
    /// Individual train service entries.
    #[serde(rename = "trains")]
    pub trains: Vec<TimetableEntry>,
}

/// Header metadata for the timetable.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TimetableMeta {
    pub name: String,
    /// Path to the route directory (contains `track.toml`).
    pub route: String,
    /// Simulation duration in seconds.
    #[serde(default = "default_duration")]
    pub duration_s: f64,
    /// Simulation time step in seconds.
    #[serde(default = "default_time_step")]
    pub time_step_s: f64,
}

fn default_duration() -> f64 {
    7200.0
}

fn default_time_step() -> f64 {
    1.0
}

/// A single train service within the timetable.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TimetableEntry {
    /// Unique service identifier (e.g. "S-401").
    pub id: String,
    /// Path to consist file, relative to the route directory.
    pub consist: String,
    /// Start node id in the route graph.
    pub start: String,
    /// Destination node id in the route graph.
    pub destination: String,
    /// Scheduled departure time in seconds from t = 0.
    #[serde(default)]
    pub depart_s: f64,
}

/// Load a [`TimetableFile`] from disk.
pub fn load_timetable(path: impl AsRef<Path>) -> Result<TimetableFile, ScenarioError> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path).map_err(|e| ScenarioError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    toml::from_str(&text).map_err(ScenarioError::Toml)
}
