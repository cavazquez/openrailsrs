//! Render 3D validation against Open Rails (tile stream, VSM, OR shaders).
//!
//! See `docs/RENDER3D.md`.

pub mod activity;
pub mod consist;
pub mod debug_hud;
pub mod dyntrack;
pub mod lighting;
pub mod loading;
pub mod objects;
pub mod or_cascade;
pub mod or_scenery_material;
pub mod or_terrain_material;
pub mod or_vsm;
pub mod or_vsm_debug;
pub mod or_vsm_moments;
pub mod or_vsm_render;
pub mod player_spawn;
pub mod runtime;
pub mod scenery;
pub mod shape_descriptor;
pub mod shapes;
pub mod sky;
pub mod stream;
pub mod tdb_track;
pub mod terrain;
pub mod textures;
pub mod tile_bundle;
pub mod tile_parse;
pub mod track;
pub mod transfer;
pub mod world_spawn;

pub use activity::{ActivitySession, build_texture_environment, load_activity_session};
pub use consist::{StaticConsistPlan, load_consist_at_path, resolve_player_consist_path};
pub use debug_hud::{
    DebugHudEnabled, FlySpeed, SceneDebugContext, toggle_debug_hud, update_debug_hud,
    update_window_title,
};
pub use loading::AppState;
pub use or_vsm_debug::OrVsmDebugPlugin;
pub use or_vsm_moments::OrVsmPlugin;
pub use player_spawn::{
    PlayerStartPoseResource, default_track_camera_pose, default_trackobj_camera_pose,
    resolve_pat_start_pose, resolve_player_start_pose,
};
pub use runtime::{
    MstsRootDir, RouteDir, SceneExtent, TdbTrackResource, TileEntry, TilesToRender, fly_camera,
    quit_on_esc,
};
pub use stream::{TileCatalog, TileStreamConfig, catalog_entries_for_initial_load};
pub use terrain::TileGeometry;
pub use tile_bundle::{
    TileBundleHandles, materialize_loaded_tile_bundles, request_tile_bundle,
    try_materialize_tile_entry,
};
pub use tile_parse::{
    ParsedTiles, TileParseRequest, parse_tiles_for_load, tile_entry_from_snapshot,
    tile_geometry_from_snapshot,
};
pub use track::{load_graph, load_tdb_context};

use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::prelude::*;
use bevy::state::condition::in_state;

use crate::loading::{
    begin_load_stage, finish_world_load, progressive_world_load, setup_loading_screen,
    update_loading_ui,
};
use openrailsrs_bevy_scenery::vsm::OrVsmRenderPlugin;

/// Core plugins and loading/playing systems for render3d.
pub struct Render3dPlugin;

impl Plugin for Render3dPlugin {
    fn build(&self, app: &mut App) {
        use openrailsrs_bevy_scenery::ScenerySpawnSet;

        app.add_plugins(openrailsrs_bevy_scenery::OrSceneryPlugins)
            .add_plugins(OrVsmPlugin)
            .add_plugins(OrVsmRenderPlugin)
            .add_plugins(OrVsmDebugPlugin)
            .add_plugins(FrameTimeDiagnosticsPlugin::default())
            .init_resource::<openrailsrs_bevy_scenery::MstsLoadDiagnostics>()
            .init_resource::<crate::stream::StreamHeightIndexCache>()
            .init_resource::<crate::tile_bundle::TileBundleHandles>()
            .init_state::<AppState>()
            .add_systems(
                Startup,
                (
                    setup_loading_screen,
                    begin_load_stage.in_set(ScenerySpawnSet::Catalog),
                    lighting::spawn_scene_sun
                        .run_if(resource_exists::<crate::SceneExtent>),
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    update_loading_ui.run_if(in_state(AppState::Loading)),
                    // Internal LoadStage still advances Catalog→Terrain→Track→Objects;
                    // the shared set places the progressive pump before Ready/finish.
                    progressive_world_load
                        .run_if(in_state(AppState::Loading))
                        .in_set(ScenerySpawnSet::Objects),
                    finish_world_load
                        .run_if(in_state(AppState::Loading))
                        .in_set(ScenerySpawnSet::Ready),
                ),
            );
    }
}

#[cfg(test)]
pub mod test_harness {
    use bevy::prelude::*;
    use openrailsrs_bevy_scenery::OrSceneryPlugins;

    pub fn minimal_render_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(openrailsrs_bevy_scenery::shared_asset_plugin())
            .add_plugins(OrSceneryPlugins);
        app
    }

    #[test]
    fn both_viewers_share_scenery_spawn_plugin_via_or_scenery() {
        let app = minimal_render_app();
        assert!(
            app.world()
                .contains_resource::<openrailsrs_bevy_scenery::ScenerySpawnCycle>(),
            "OrSceneryPlugins must register ScenerySpawnPlugin (#52)"
        );
        assert!(
            app.world()
                .contains_resource::<openrailsrs_bevy_scenery::ScenerySpawnBudgets>()
        );
    }
}
