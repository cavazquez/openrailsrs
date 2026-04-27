//! Experimental 3D viewer for `openrailsrs` (Bevy).
//!
//! This is a standalone visual sandbox: it does **not** depend on
//! `openrailsrs-sim`, `openrailsrs-track` or any other crate of the
//! workspace. It only opens a window with a ground plane, a world-space
//! grid and a free camera that can be toggled between orbit (`F1`) and
//! fly (`F2`) modes.
//!
//! See `docs/OPEN_RAILS_VIEWER_3D.md` for the full roadmap of the 3D
//! viewer (issue #8).
//!
//! ```no_run
//! use bevy::prelude::*;
//! use openrailsrs_viewer3d::ViewerPlugin;
//!
//! App::new()
//!     .add_plugins(DefaultPlugins)
//!     .add_plugins(ViewerPlugin)
//!     .run();
//! ```

pub mod camera;
pub mod scene;

use bevy::prelude::*;

/// Plugin that wires up the camera, scene and update systems for the
/// experimental 3D viewer. Add it on top of [`DefaultPlugins`].
pub struct ViewerPlugin;

impl Plugin for ViewerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::srgb(0.04, 0.07, 0.10)))
            .init_resource::<camera::CameraMode>()
            .add_systems(
                Startup,
                (scene::spawn_ground_and_lights, camera::spawn_camera),
            )
            .add_systems(
                Update,
                (
                    camera::toggle_mode_system,
                    camera::update_primary_window_cursor,
                    camera::orbit_camera_system.run_if(camera::in_orbit_mode),
                    camera::fly_camera_system.run_if(camera::in_fly_mode),
                    scene::draw_grid_and_axes,
                ),
            );
    }
}
