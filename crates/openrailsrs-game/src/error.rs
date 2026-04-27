use thiserror::Error;

#[derive(Debug, Error)]
pub enum GameError {
    #[error("scenario file must have a parent directory")]
    InvalidScenarioPath,

    #[error("sim: {0}")]
    Sim(#[from] openrailsrs_sim::SimError),

    #[error("scenario: {0}")]
    Scenario(#[from] openrailsrs_scenarios::ScenarioError),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("toml: {0}")]
    TomlSer(#[from] toml::ser::Error),
}
