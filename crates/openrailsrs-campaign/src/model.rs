use serde::{Deserialize, Serialize};

// ── Campaign definition (campaign.toml) ───────────────────────────────────────

/// Top-level campaign file.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CampaignFile {
    pub campaign: CampaignMeta,
    #[serde(default)]
    pub missions: Vec<MissionDef>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CampaignMeta {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Version string for save-compatibility checks.
    #[serde(default = "default_version")]
    pub version: String,
}

fn default_version() -> String {
    "1.0".into()
}

/// A single mission entry inside `campaign.toml`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MissionDef {
    /// Unique identifier, used in `progress.json` and CLI.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Path to the `scenario.toml` (relative to the campaign file).
    pub scenario: String,
    /// Mission IDs that must be completed before this one is available.
    #[serde(default)]
    pub requires: Vec<String>,
    /// Minimum score (0–100) needed to satisfy the `requires` condition.
    #[serde(default = "default_min_pass")]
    pub min_pass_score: u32,
    /// Score threshold for the "bonus star" display.
    #[serde(default = "default_bonus")]
    pub bonus_threshold: u32,
    /// Suggested simulation speed multiplier for `cab` / `dispatch`.
    #[serde(default = "default_speed")]
    pub sim_speed: f64,
    /// Difficulty label shown in the UI.
    #[serde(default)]
    pub difficulty: Difficulty,
}

fn default_min_pass() -> u32 {
    60
}
fn default_bonus() -> u32 {
    90
}
fn default_speed() -> f64 {
    10.0
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    #[default]
    Easy,
    Medium,
    Hard,
}

// ── Progress (progress.json) ──────────────────────────────────────────────────

/// Persisted progress for the whole campaign.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Progress {
    /// Map from mission id → best result achieved.
    #[serde(default)]
    pub completed: std::collections::HashMap<String, MissionResult>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MissionResult {
    /// Best score (0–100).
    pub score: u32,
    /// ISO-8601 timestamp string of the last play.
    pub timestamp: String,
    /// Whether the bonus threshold was reached.
    pub bonus: bool,
}

// ── Resolved view (computed at runtime) ──────────────────────────────────────

#[derive(Debug, Clone)]
pub struct MissionStatus<'c> {
    pub def: &'c MissionDef,
    pub state: MissionState,
    /// Best score so far, or None if never played.
    pub best_score: Option<u32>,
    pub bonus: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MissionState {
    /// Prerequisites not met.
    Locked,
    /// Available to play.
    Available,
    /// Completed (score ≥ min_pass_score).
    Completed,
}
