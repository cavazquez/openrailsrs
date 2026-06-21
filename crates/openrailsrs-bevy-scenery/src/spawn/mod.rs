//! Progressive WORLD / track scenery spawn (shared between viewer3d and render3d).

pub mod dyntrack;
pub mod tdb_track;

use bevy::prelude::*;

/// How shared scenery spawn systems batch GPU work.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScenerySpawnMode {
    /// Tile/stream batches (default for large routes).
    #[default]
    Progressive,
    /// Spawn everything in one startup pass (small routes / tests).
    Eager,
}

/// Registers shared scenery spawn resources/systems.
///
/// Phase 3 stub: progressive WORLD + terrain wiring remains in viewer3d for now.
pub struct ScenerySpawnPlugin;

impl Plugin for ScenerySpawnPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScenerySpawnMode>();
    }
}
