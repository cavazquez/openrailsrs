use thiserror::Error;

#[derive(Debug, Error)]
pub enum TrackError {
    #[error("unknown node: {0}")]
    UnknownNode(String),

    #[error("unknown edge: {0}")]
    UnknownEdge(String),

    #[error("duplicate id: {0}")]
    DuplicateId(String),
}
