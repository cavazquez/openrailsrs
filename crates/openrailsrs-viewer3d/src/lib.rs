//! Experimental 3D viewer for `openrailsrs` (Bevy).
//!
//! Loads a route directory (`track.toml`) and renders the logical graph as
//! 3D geometry: edges as orange cylinders, nodes as coloured spheres (white
//! Plain, cyan Switch, yellow Station). Optionally replays train position from
//! simulation CSV when launched with a `scenario.toml`.
//!
//! See `docs/OPEN_RAILS_VIEWER_3D.md` for the full roadmap (issue #8).

pub mod camera;
pub mod dyntrack;
pub mod hud;
pub mod scene;
pub mod shapes;
pub mod signals;
pub mod teleport;
pub mod terrain;
pub mod track;
pub mod train;
pub mod world;

#[cfg(test)]
mod app_smoke;

use bevy::prelude::*;

pub use hud::HudTitle;
pub use shapes::RouteAssets;
pub use terrain::TerrainScene;
pub use track::{TrackRenderMode, TrackScene};
pub use train::ReplayState;
pub use world::WorldScene;

/// Plugin that wires up the camera, scene and update systems for the
/// experimental 3D viewer. Add it on top of [`DefaultPlugins`].
///
/// Requires [`TrackScene`], [`ReplayState`], [`HudTitle`], [`WorldScene`],
/// [`TerrainScene`] and [`RouteAssets`] resources (insert before adding this plugin).
pub struct ViewerPlugin;

impl Plugin for ViewerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::srgb(0.04, 0.07, 0.10)))
            .init_resource::<camera::CameraMode>()
            .init_resource::<camera::CameraFollowMode>()
            .init_resource::<camera::OrbitDistanceLimit>()
            .init_resource::<teleport::TeleportDialog>()
            .add_systems(
                Startup,
                (
                    scene::spawn_ground_and_lights,
                    terrain::spawn_terrain_meshes,
                    track::spawn_track_meshes,
                    dyntrack::spawn_dyntrack_segments,
                    world::spawn_world_boxes,
                    signals::spawn_signal_markers,
                    camera::spawn_camera,
                    hud::spawn_hud,
                    teleport::spawn_teleport_ui,
                    track::frame_orbit_camera_on_track,
                    train::spawn_train_markers,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    teleport::toggle_teleport_dialog,
                    teleport::teleport_input_system,
                    teleport::teleport_button_system,
                    teleport::sync_teleport_ui,
                    camera::toggle_mode_system.run_if(teleport::teleport_closed),
                    camera::cycle_follow_mode.run_if(teleport::teleport_closed),
                    camera::update_primary_window_cursor,
                    train::replay_controls.run_if(teleport::teleport_closed),
                    train::advance_replay_time,
                    train::update_train_markers,
                    hud::update_hud,
                    (camera::follow_train_camera, camera::orbit_camera_system)
                        .chain()
                        .run_if(camera::in_orbit_mode)
                        .run_if(teleport::teleport_closed)
                        .after(train::update_train_markers),
                    camera::fly_camera_system
                        .run_if(camera::in_fly_mode)
                        .run_if(teleport::teleport_closed),
                    scene::draw_grid_and_axes,
                    track::draw_compact_edges,
                ),
            );
    }
}
