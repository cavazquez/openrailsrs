use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RouteError {
    #[error("io error on {path}: {source}")]
    Io { path: String, source: io::Error },

    #[error("toml: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("track: {0}")]
    Track(#[from] openrailsrs_track::TrackError),

    #[error("{0}")]
    Msg(String),
}
