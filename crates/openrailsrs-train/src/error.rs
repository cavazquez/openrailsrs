use thiserror::Error;

#[derive(Debug, Error)]
pub enum TrainError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("parse: {0}")]
    Parse(String),

    #[error("format: {0}")]
    Format(#[from] openrailsrs_formats::FormatError),

    #[error("missing field {field} in {context}")]
    MissingField { field: String, context: String },
}
