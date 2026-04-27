use serde::{Deserialize, Serialize};

use crate::ScenarioError;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScenarioFile {
    pub scenario: ScenarioMeta,
    pub route: RouteSection,
    pub train: TrainSection,
    pub gameplay: GameplaySection,
    pub simulation: SimulationSection,
    pub output: OutputSection,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScenarioMeta {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// Intermediate stop along the route with target arrival and departure times.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StopDef {
    pub node: String,
    pub arrive_s: f64,
    pub depart_s: f64,
    /// How long the train must dwell at this stop before departing (seconds, default 0).
    #[serde(default)]
    pub dwell_s: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RouteSection {
    pub path: String,
    pub start: String,
    pub destination: String,
    #[serde(default)]
    pub stops: Vec<StopDef>,
}

/// Optional Davis resistance override (falls back to consist defaults if absent).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DavisSection {
    pub a_n: f64,
    pub b_n_per_mps: f64,
    pub c_n_per_mps2: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TrainSection {
    pub consist: String,
    #[serde(default)]
    pub davis: Option<DavisSection>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ObjectiveKind {
    #[default]
    ArriveOnTime,
    Arrive,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    #[default]
    Normal,
    Easy,
    Hard,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GameplaySection {
    #[serde(default)]
    pub objective: ObjectiveKind,
    pub time_limit_seconds: Option<u64>,
    #[serde(default)]
    pub difficulty: Difficulty,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SimulationSection {
    pub duration: f64,
    pub time_step: f64,
    #[serde(default = "default_seed")]
    pub seed: u64,
}

fn default_seed() -> u64 {
    42
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OutputSection {
    pub csv: String,
    pub metadata: String,
}

impl ScenarioFile {
    pub fn validate(&self) -> Result<(), ScenarioError> {
        if self.scenario.name.trim().is_empty() {
            return Err(ScenarioError::Validation(
                "scenario.name must not be empty".into(),
            ));
        }
        if self.simulation.duration <= 0.0 {
            return Err(ScenarioError::Validation(
                "simulation.duration must be positive".into(),
            ));
        }
        if self.simulation.time_step <= 0.0 {
            return Err(ScenarioError::Validation(
                "simulation.time_step must be positive".into(),
            ));
        }
        if self.route.path.trim().is_empty() {
            return Err(ScenarioError::Validation("route.path is required".into()));
        }
        if self.output.csv.trim().is_empty() || self.output.metadata.trim().is_empty() {
            return Err(ScenarioError::Validation(
                "output.csv and output.metadata are required".into(),
            ));
        }
        for stop in &self.route.stops {
            if stop.arrive_s > stop.depart_s {
                return Err(ScenarioError::Validation(format!(
                    "stop '{}': arrive_s ({}) must be <= depart_s ({})",
                    stop.node, stop.arrive_s, stop.depart_s
                )));
            }
        }
        Ok(())
    }
}
