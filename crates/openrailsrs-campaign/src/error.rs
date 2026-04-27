use thiserror::Error;

#[derive(Debug, Error)]
pub enum CampaignError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("TOML serialize error: {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("mission not found: {0}")]
    MissionNotFound(String),
    #[error("mission locked: {0} (requires completing: {1:?})")]
    MissionLocked(String, Vec<String>),
}
