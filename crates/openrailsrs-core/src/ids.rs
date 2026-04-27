use serde::{Deserialize, Serialize};

/// Stable node identifier in a track graph.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct NodeId(pub String);

/// Edge between two nodes (directed rail segment).
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct EdgeId(pub String);
