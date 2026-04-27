use thiserror::Error;

#[derive(Debug, Error)]
pub enum ImportError {
    #[error("I/O error reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML serialise error: {0}")]
    Toml(#[from] toml::ser::Error),

    #[error("no railway ways found in input")]
    NoRailwayWays,

    #[error("node {0} referenced by a way is missing from the element list")]
    MissingNode(i64),
}
