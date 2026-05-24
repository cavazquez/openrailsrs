//! Experimental 3D viewer for `openrailsrs` (Bevy).
//!
//! Loads a route directory (`track.toml`) and renders the logical graph as
//! 3D geometry: edges as orange cylinders, nodes as coloured spheres (white
//! Plain, cyan Switch, yellow Station). Optionally replays train position from
//! simulation CSV when launched with a `scenario.toml`.
//!
//! See `docs/OPEN_RAILS_VIEWER_3D.md` for the full roadmap (issue #8).

pub mod camera;
pub mod scene;
pub mod signals;
pub mod track;
pub mod train;

#[cfg(test)]
mod app_smoke;

use bevy::prelude::*;

pub use track::{TrackRenderMode, TrackScene};
pub use train::ReplayState;

/// Plugin that wires up the camera, scene and update systems for the
/// experimental 3D viewer. Add it on top of [`DefaultPlugins`].
///
/// Requires [`TrackScene`] and [`ReplayState`] resources (insert before adding
/// this plugin).
pub struct ViewerPlugin;

impl Plugin for ViewerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::srgb(0.04, 0.07, 0.10)))
            .init_resource::<camera::CameraMode>()
            .init_resource::<camera::CameraFollowMode>()
            .init_resource::<camera::OrbitDistanceLimit>()
            .add_systems(
                Startup,
                (
                    scene::spawn_ground_and_lights,
                    track::spawn_track_meshes,
                    signals::spawn_signal_markers,
                    camera::spawn_camera,
                    track::frame_orbit_camera_on_track,
                    train::spawn_train_markers,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    camera::toggle_mode_system,
                    camera::cycle_follow_mode,
                    camera::update_primary_window_cursor,
                    train::replay_controls,
                    train::advance_replay_time,
                    train::update_train_markers,
                    (camera::follow_train_camera, camera::orbit_camera_system)
                        .chain()
                        .run_if(camera::in_orbit_mode)
                        .after(train::update_train_markers),
                    camera::fly_camera_system.run_if(camera::in_fly_mode),
                    scene::draw_grid_and_axes,
                    track::draw_compact_edges,
                    train::update_window_hud,
                ),
            );
    }
}
