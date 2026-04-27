use thiserror::Error;

#[derive(Debug, Error)]
pub enum ValidateError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("csv: {0}")]
    Csv(#[from] csv::Error),

    #[error("{0}")]
    Msg(String),
}
