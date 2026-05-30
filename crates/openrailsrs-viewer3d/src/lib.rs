//! Experimental 3D viewer for `openrailsrs` (Bevy).
//!
//! Loads a route directory (`track.toml`) and renders the logical graph as
//! 3D geometry: edges as orange cylinders, nodes as coloured spheres (white
//! Plain, cyan Switch, yellow Station). Optionally replays train position from
//! simulation CSV when launched with a `scenario.toml`.
//!
//! See `docs/OPEN_RAILS_VIEWER_3D.md` for the full roadmap (issue #8).

pub mod cab_panel;
pub mod cab_view;
pub mod camera;
pub mod dyntrack;
pub mod floating_origin;
pub mod forest;
pub mod gameplay;
pub mod hud;
pub mod launch;
pub mod live;
pub mod log;
pub mod overspeed_flash;
pub mod precipitation;
pub mod rolling_stock;
pub mod scene;
pub mod shapes;
pub mod signals;
pub mod sky;
pub mod teleport;
pub mod terrain;
pub mod terrain_assets;
pub mod terrain_material;
pub mod track;
pub mod train;
pub mod water;
pub mod world;

#[cfg(test)]
mod app_floating;
#[cfg(test)]
mod app_gameplay;
#[cfg(test)]
mod app_live;
#[cfg(test)]
mod app_smoke;
#[cfg(test)]
mod app_spawn;
#[cfg(test)]
mod test_harness;

use bevy::prelude::*;

pub use hud::HudTitle;
pub use launch::ViewerLaunchOpts;
pub use live::LiveDrive;
pub use log::init as init_viewer_log;
pub use log::log_step;
pub use rolling_stock::TrainConsistScene;
pub use shapes::RouteAssets;
pub use terrain::{TerrainElevation, TerrainScene};
pub use track::{TrackRenderMode, TrackScene};
pub use train::ReplayState;
pub use world::WorldScene;

/// Plugin that wires up the camera, scene and update systems for the
/// experimental 3D viewer. Add it on top of [`DefaultPlugins`].
///
/// Requires [`TrackScene`], [`ReplayState`], [`HudTitle`], [`WorldScene`],
/// [`TerrainScene`], [`TrainConsistScene`] and [`RouteAssets`] resources (insert before adding this plugin).
pub struct ViewerPlugin;

impl Plugin for ViewerPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<terrain_material::TerrainMaterial>::default())
            .insert_resource(ClearColor(sky::sky_clear_color()))
            .init_resource::<camera::CameraMode>()
            .init_resource::<camera::CameraFollowMode>()
            .init_resource::<camera::CameraFollowTarget>()
            .init_resource::<camera::OrbitDistanceLimit>()
            .init_resource::<camera::LiveDriverCab>()
            .init_resource::<precipitation::PrecipitationState>()
            .init_resource::<teleport::TeleportDialog>()
            .init_resource::<cab_panel::CabPanelVisible>()
            .init_resource::<cab_view::CabInteriorState>()
            .init_resource::<hud::HudFps>()
            .init_resource::<hud::ProfileLog>()
            .init_resource::<gameplay::GameplayToast>()
            .init_resource::<overspeed_flash::OverspeedFlash>()
            .init_resource::<floating_origin::FloatingOrigin>()
            .add_systems(
                Startup,
                (
                    scene::spawn_ground_and_lights,
                    sky::spawn_sky_dome,
                    terrain::spawn_terrain_meshes,
                    track::spawn_track_meshes,
                    dyntrack::spawn_dyntrack_segments,
                    forest::spawn_forest_patches,
                    water::spawn_water_patches,
                    world::init_world_spawn_progress,
                )
                    .chain(),
            )
            .add_systems(Update, world::progressive_world_spawn_system)
            .add_systems(
                Startup,
                (
                    signals::spawn_signal_markers.run_if(live::live_mode_inactive),
                    precipitation::spawn_precipitation,
                    camera::spawn_camera,
                    hud::spawn_hud,
                    cab_panel::spawn_cab_panel,
                    teleport::spawn_teleport_ui,
                    track::frame_orbit_camera_on_track,
                    train::spawn_train_markers.run_if(live::live_mode_inactive),
                    live::spawn_live_train.run_if(live::live_mode_active),
                    live::enable_live_defaults.run_if(live::live_mode_active),
                    gameplay::spawn_gameplay_ui,
                    gameplay::spawn_gameplay_markers.run_if(live::live_mode_active),
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
                    precipitation::toggle_precipitation.run_if(teleport::teleport_closed),
                    camera::toggle_mode_system.run_if(teleport::teleport_closed),
                    camera::cycle_follow_mode.run_if(teleport::teleport_closed),
                    camera::update_primary_window_cursor,
                    train::replay_controls
                        .run_if(teleport::teleport_closed)
                        .run_if(live::live_mode_inactive),
                    live::live_driver_input
                        .run_if(teleport::teleport_closed)
                        .run_if(live::live_mode_active),
                    train::advance_replay_time.run_if(live::live_mode_inactive),
                    live::advance_live_sim.run_if(live::live_mode_active),
                    live::live_audio_frame.run_if(live::live_mode_active),
                    signals::update_live_signal_markers.run_if(live::live_mode_active),
                    train::update_train_markers.run_if(live::live_mode_inactive),
                    live::update_live_train_marker.run_if(live::live_mode_active),
                    precipitation::update_precipitation,
                    water::update_water_patches,
                    hud::tick_hud_fps,
                    hud::update_hud.after(hud::tick_hud_fps),
                ),
            )
            .add_systems(
                Update,
                (
                    gameplay::update_gameplay_markers.run_if(live::live_mode_active),
                    gameplay::update_gameplay_toast.run_if(live::live_mode_active),
                    gameplay::update_arrival_overlay.run_if(live::live_mode_active),
                    gameplay::update_driver_vignette,
                    live::update_driver_train_visibility.run_if(live::live_mode_active),
                    train::update_replay_train_visibility.run_if(live::live_mode_inactive),
                    cab_view::sync_cab_interior,
                    camera::update_driver_camera_fov,
                    overspeed_flash::tick_overspeed_flash.run_if(live::live_mode_active),
                    overspeed_flash::apply_overspeed_flash.run_if(live::live_mode_active),
                    gameplay::update_stop_billboards.run_if(live::live_mode_active),
                ),
            )
            .add_systems(Update, hud::log_profile)
            .add_systems(
                Update,
                (
                    cab_panel::toggle_cab_panel
                        .run_if(teleport::teleport_closed)
                        .run_if(live::live_mode_active),
                    cab_panel::update_cab_panel,
                    (camera::follow_train_camera, camera::orbit_camera_system)
                        .chain()
                        .run_if(camera::in_orbit_mode)
                        .run_if(teleport::teleport_closed)
                        .after(train::update_train_markers)
                        .after(live::update_live_train_marker),
                    camera::fly_camera_system
                        .run_if(camera::in_fly_mode)
                        .run_if(teleport::teleport_closed),
                ),
            );
    }
}
