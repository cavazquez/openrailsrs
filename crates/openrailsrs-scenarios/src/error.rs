use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ScenarioError {
    #[error("failed to read {path}: {source}")]
    Io { path: String, source: io::Error },

    #[error("invalid TOML: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("scenario validation: {0}")]
    Validation(String),
}
