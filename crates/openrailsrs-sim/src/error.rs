use thiserror::Error;

#[derive(Debug, Error)]
pub enum SimError {
    #[error("scenario: {0}")]
    Scenario(#[from] openrailsrs_scenarios::ScenarioError),

    #[error("route: {0}")]
    Route(#[from] openrailsrs_route::RouteError),

    #[error("train: {0}")]
    Train(#[from] openrailsrs_train::TrainError),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("csv: {0}")]
    Csv(#[from] csv::Error),

    #[error("toml serialize: {0}")]
    TomlSer(#[from] toml::ser::Error),

    #[error("{0}")]
    Msg(String),
}
