/// Errors produced by the MSTS importer.
#[derive(Debug, thiserror::Error)]
pub enum MstsError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("parse error: {0}")]
    Parse(#[from] openrailsrs_formats::FormatError),

    #[error("TOML serialization error: {0}")]
    Toml(#[from] toml::ser::Error),

    #[error("{0}")]
    Msg(String),
}

impl MstsError {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Msg(s.into())
    }
}
