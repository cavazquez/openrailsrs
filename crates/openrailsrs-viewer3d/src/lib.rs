//! Experimental 3D viewer for `openrailsrs` (Bevy).
//!
//! Loads a route directory (`track.toml`) and renders the logical graph as
//! 3D geometry: edges as orange cylinders, nodes as coloured spheres (white
//! Plain, cyan Switch, yellow Station). Optionally replays train position from
//! simulation CSV when launched with a `scenario.toml`.
//!
//! See `docs/OPEN_RAILS_VIEWER_3D.md` for the full roadmap (issue #8).

pub mod cab_cvf;
pub mod cab_cvf_overlay;
pub mod cab_diag;
pub mod cab_panel;
pub mod cab_render;
pub mod cab_view;
pub mod camera;
pub mod capture;
pub use openrailsrs_or_shader::coordinates;
pub mod dyntrack;
pub mod floating_origin;
pub mod forest;
pub mod gameplay;
pub mod hud;
pub mod launch;
pub mod live;
pub mod log;
pub mod or_cab_material;
pub mod or_shader {
    pub use openrailsrs_or_shader::*;
}
pub mod overhead_wire;
pub mod overspeed_flash;
pub mod placement_audit;
pub mod precipitation;
pub mod road_cars;
pub mod rolling_stock;
pub mod rolling_stock_anim;
pub mod route_bootstrap;
pub mod scene;
pub mod scenery_audit;
pub mod shapes;
pub mod signal_lamps;
pub mod signals;
pub mod sky;
pub mod tdb_track;
pub mod teleport;
pub mod terrain;
pub mod terrain_assets;
pub(crate) mod terrain_io;
pub mod terrain_material;
pub(crate) mod terrain_sampler;
pub(crate) mod terrain_spawn;
pub mod tile_bundle;
pub mod tr_item_audit;
pub mod tr_item_index;
pub mod track;
pub mod track_audit;
pub mod track_position;
pub mod train;
pub mod train_diagnostics;
pub mod transfer;
pub mod view_window;
pub mod water;
pub mod world;
pub mod world_instancing;

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
#[cfg(test)]
mod visual_regression;

use bevy::prelude::*;

pub use hud::HudTitle;
pub use launch::{
    RunCorridorPath, VIEWING_DISTANCE_M, ViewerLaunchOpts, ViewerSceneryMode, view_radius_m,
};
pub use live::LiveDrive;
pub use log::init as init_viewer_log;
pub use log::log_step;
pub use rolling_stock::TrainConsistScene;
pub use route_bootstrap::{
    ViewerAppState, ViewerBootClock, log_time_to_first_presented_frame, viewer_playing,
};
pub use shapes::RouteAssets;
pub use terrain::{TerrainElevation, TerrainScene};
pub use track::{TrackRenderMode, TrackScene};
pub use train::ReplayState;
pub use world::WorldScene;

/// Plugin that wires up the camera, scene and update systems for the
/// experimental 3D viewer. Add it on top of [`DefaultPlugins`].
///
/// Scenery startup runs on [`OnEnter(ViewerAppState::Playing)`] (#55). Insert route
/// resources then enter `Playing`, or start directly in `Playing` (tests).
pub struct ViewerPlugin;

impl Plugin for ViewerPlugin {
    fn build(&self, app: &mut App) {
        use bevy::state::condition::in_state;
        use openrailsrs_bevy_scenery::ScenerySpawnSet;

        app.add_plugins(openrailsrs_bevy_scenery::OrSceneryPlugins)
            .add_plugins(world_instancing::WorldInstancingPlugin)
            .init_state::<ViewerAppState>()
            .insert_resource(ClearColor(sky::sky_clear_color()))
            .init_resource::<camera::CameraMode>()
            .init_resource::<camera::CameraFollowMode>()
            .init_resource::<camera::CameraFollowTarget>()
            .init_resource::<camera::OrbitDistanceLimit>()
            .init_resource::<camera::LiveDriverCab>()
            .init_resource::<camera::DriverLookOffset>()
            .init_resource::<precipitation::PrecipitationState>()
            .init_resource::<sky::FogState>()
            .init_resource::<overhead_wire::RouteWireConfig>()
            .init_resource::<teleport::TeleportDialog>()
            .init_resource::<cab_panel::CabPanelVisible>()
            .init_resource::<cab_view::CabInteriorState>()
            .init_resource::<cab_cvf::CabCvfState>()
            .init_resource::<cab_cvf_overlay::CabCvfOverlayState>()
            .init_resource::<cab_render::CabRenderDiagnostic>()
            .init_resource::<cab_render::CabRenderDiagLatch>()
            .init_resource::<live::DriverCamState>()
            .init_resource::<hud::HudFps>()
            .init_resource::<hud::ProfileLog>()
            .init_resource::<gameplay::GameplayToast>()
            .init_resource::<overspeed_flash::OverspeedFlash>()
            .init_resource::<floating_origin::FloatingOrigin>()
            .init_resource::<world::WorldSceneryStreamState>()
            .init_resource::<world::WorldShapeLodCache>()
            .init_resource::<world::WorldLodCameraState>()
            .init_resource::<tile_bundle::TileBundleHandles>()
            .init_resource::<launch::ViewerSceneryMode>()
            .init_resource::<launch::RunCorridorPath>()
            .init_resource::<view_window::ViewWindow>()
            .init_resource::<tdb_track::TdbTrackStream>()
            .add_systems(
                OnEnter(ViewerAppState::Playing),
                (
                    scene::spawn_ground_and_lights,
                    sky::spawn_sky_dome.run_if(launch::sky_dome_active),
                    terrain::init_terrain_spawn_progress.run_if(launch::full_scenery_active),
                    track::spawn_track_meshes,
                    tdb_track::spawn_tdb_graph_track.run_if(tdb_track::tdb_startup_spawn_active),
                    dyntrack::spawn_dyntrack_segments.run_if(launch::full_scenery_active),
                    forest::spawn_forest_patches.run_if(launch::full_scenery_active),
                    water::spawn_water_patches.run_if(launch::full_scenery_active),
                    transfer::spawn_transfer_patches.run_if(launch::full_scenery_active),
                    road_cars::spawn_road_cars.run_if(launch::full_scenery_active),
                    world::init_world_spawn_progress.in_set(ScenerySpawnSet::Catalog),
                    world::init_scenery_stream_state.in_set(ScenerySpawnSet::Ready),
                )
                    .chain(),
            )
            .add_systems(
                OnEnter(ViewerAppState::Playing),
                signal_lamps::spawn_signal_lamps
                    .run_if(launch::full_scenery_active)
                    .after(world::init_scenery_stream_state),
            )
            .add_systems(
                Update,
                terrain::progressive_terrain_spawn_system
                    .run_if(in_state(ViewerAppState::Playing))
                    .run_if(launch::full_scenery_active)
                    .in_set(ScenerySpawnSet::Terrain),
            )
            .add_systems(
                Update,
                track::tile_lab_frame_camera_once.run_if(in_state(ViewerAppState::Playing)),
            )
            .add_systems(
                Update,
                (
                    openrailsrs_bevy_scenery::shapes::update_world_shape_anim,
                    rolling_stock_anim::update_rolling_stock_part_anim
                        .after(live::update_live_train_marker)
                        .after(train::update_train_markers),
                )
                    .run_if(in_state(ViewerAppState::Playing)),
            )
            .add_systems(
                Update,
                (
                    view_window::sync_view_window_from_train,
                    tdb_track::tdb_track_stream_system.run_if(tdb_track::tdb_stream_active),
                )
                    .chain()
                    .after(live::update_live_train_marker)
                    .run_if(in_state(ViewerAppState::Playing)),
            )
            .add_systems(
                Update,
                world::progressive_world_spawn_system
                    .after(view_window::sync_view_window_from_train)
                    .run_if(in_state(ViewerAppState::Playing))
                    .in_set(ScenerySpawnSet::Objects),
            )
            .add_systems(
                Update,
                (
                    world::update_world_scenery_lod,
                    world_instancing::update_world_instanced_lod,
                )
                    .after(world::progressive_world_spawn_system)
                    .run_if(in_state(ViewerAppState::Playing))
                    .in_set(ScenerySpawnSet::Ready),
            )
            .add_systems(
                Update,
                (
                    world::world_tile_stream_system,
                    world::world_tile_bundle_materialize_system,
                    world::world_tile_unload_system.run_if(live::live_mode_active),
                    tr_item_index::sync_tr_item_world_index,
                    world::world_stream_scenery_system,
                )
                    .chain()
                    .after(view_window::sync_view_window_from_train)
                    .run_if(in_state(ViewerAppState::Playing))
                    .in_set(ScenerySpawnSet::Ready),
            )
            .add_systems(
                Update,
                (
                    terrain::terrain_tile_stream_system,
                    terrain::terrain_tile_bundle_materialize_system,
                    terrain::terrain_tile_spawn_stream_system,
                    terrain::terrain_tile_unload_system,
                )
                    .chain()
                    .after(view_window::sync_view_window_from_train)
                    .run_if(live::live_mode_active)
                    .run_if(launch::full_scenery_active)
                    .run_if(in_state(ViewerAppState::Playing))
                    .in_set(ScenerySpawnSet::Terrain),
            )
            .add_systems(
                OnEnter(ViewerAppState::Playing),
                (
                    signals::spawn_signal_markers.run_if(live::live_mode_inactive),
                    precipitation::spawn_precipitation.run_if(launch::full_scenery_active),
                    camera::spawn_camera,
                    hud::spawn_hud,
                    cab_panel::spawn_cab_panel,
                    teleport::spawn_teleport_ui,
                    track::frame_orbit_camera_on_track.run_if(live::live_mode_inactive),
                    train::spawn_train_markers.run_if(live::live_mode_inactive),
                    live::spawn_live_train.run_if(live::live_mode_active),
                    floating_origin::track_dev_recenter_at_subject.run_if(launch::track_dev_active),
                    live::enable_live_defaults.run_if(live::live_mode_active),
                    gameplay::spawn_gameplay_ui,
                    gameplay::spawn_gameplay_markers.run_if(live::live_mode_active),
                )
                    .chain()
                    .after(world::init_scenery_stream_state),
            )
            .add_systems(
                Update,
                floating_origin::apply_floating_origin
                    .before(live::update_live_train_marker)
                    .before(train::update_train_markers)
                    .run_if(in_state(ViewerAppState::Playing)),
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
                )
                    .run_if(in_state(ViewerAppState::Playing)),
            )
            .add_systems(
                Update,
                road_cars::update_road_cars.run_if(in_state(ViewerAppState::Playing)),
            )
            .add_systems(
                Update,
                signal_lamps::update_signal_lamps.run_if(in_state(ViewerAppState::Playing)),
            )
            .add_systems(
                Update,
                sky::toggle_distance_fog
                    .run_if(teleport::teleport_closed)
                    .run_if(in_state(ViewerAppState::Playing)),
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
                    cab_render::tag_train_exterior_render_layers.run_if(live::live_mode_active),
                    cab_render::sync_camera_render_layers,
                    cab_view::sync_cab_interior,
                    cab_cvf_overlay::sync_cab_cvf_overlay.after(cab_view::sync_cab_interior),
                    cab_render::tag_cab_interior_render_layers.after(cab_view::sync_cab_interior),
                    cab_cvf::update_cab_cvf_controls.after(cab_view::sync_cab_interior),
                    cab_cvf_overlay::update_cab_cvf_overlay
                        .after(cab_cvf_overlay::sync_cab_cvf_overlay),
                    cab_render::update_cab_render_diagnostic
                        .after(cab_render::tag_cab_interior_render_layers)
                        .after(camera::follow_train_camera)
                        .run_if(live::live_mode_active),
                    camera::update_driver_camera_fov,
                    overspeed_flash::tick_overspeed_flash.run_if(live::live_mode_active),
                    overspeed_flash::apply_overspeed_flash.run_if(live::live_mode_active),
                    gameplay::update_stop_billboards.run_if(live::live_mode_active),
                )
                    .run_if(in_state(ViewerAppState::Playing)),
            )
            .add_systems(Startup, capture::init_capture)
            .add_systems(
                Update,
                capture::capture_system.run_if(in_state(ViewerAppState::Playing)),
            )
            .add_systems(
                Update,
                live::live_autodrive
                    .run_if(live::live_mode_active)
                    .run_if(live::autodrive_enabled)
                    .run_if(in_state(ViewerAppState::Playing))
                    .before(live::advance_live_sim),
            )
            .add_systems(
                Update,
                hud::log_profile.run_if(in_state(ViewerAppState::Playing)),
            )
            .add_systems(
                Update,
                (
                    cab_panel::toggle_cab_panel
                        .run_if(teleport::teleport_closed)
                        .run_if(live::live_mode_active),
                    cab_panel::update_cab_panel,
                    (camera::orbit_camera_system)
                        .run_if(camera::in_orbit_mode)
                        .run_if(teleport::teleport_closed),
                    camera::follow_train_camera
                        .run_if(camera::follow_train_camera_active)
                        .run_if(teleport::teleport_closed)
                        .after(train::update_train_markers)
                        .after(live::update_live_train_marker),
                    camera::fly_camera_system
                        .run_if(camera::in_fly_mode)
                        .run_if(camera::fly_camera_allowed)
                        .run_if(teleport::teleport_closed),
                )
                    .run_if(in_state(ViewerAppState::Playing)),
            );
    }
}
