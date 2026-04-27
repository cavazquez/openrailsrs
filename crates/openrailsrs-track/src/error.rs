use thiserror::Error;

#[derive(Debug, Error)]
pub enum TrackError {
    #[error("unknown node: {0}")]
    UnknownNode(String),

    #[error("unknown edge: {0}")]
    UnknownEdge(String),

    #[error("duplicate id: {0}")]
    DuplicateId(String),

    #[error("duplicate signal id: {0}")]
    DuplicateSignalId(String),

    #[error("signal references unknown edge: {0}")]
    UnknownEdgeForSignal(String),

    #[error("node is not a switch: {0}")]
    NotASwitch(String),
}
