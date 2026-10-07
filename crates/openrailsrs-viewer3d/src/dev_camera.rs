//! Reproducible camera-only streaming journey. Enabled only in dev-tools builds
//! and with OPENRAILSRS_CAMERA_JOURNEY=aba. Railway time/physics stay paused.
use crate::camera::{CameraFollowMode, CameraMode, OrbitState};
use bevy::prelude::*;
#[derive(Resource, Default)]
pub struct CameraJourney {
    pub enabled: bool,
    pub complete: bool,
    stage: u8,
    elapsed_s: f64,
    from: Option<Vec3>,
    to: Vec3,
    pub distance_m: f32,
}
impl CameraJourney {
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"enabled":self.enabled,"complete":self.complete,"stage":self.stage,"distance_m":self.distance_m,"leg_duration_s":20,"minimum_ready_hold_s":3,"physics":"paused; only camera moves; fixed wall-time trajectory"})
    }
}
pub fn install(app: &mut App) {
    app.init_resource::<CameraJourney>().add_systems(
        Update,
        update
            .after(crate::camera::follow_train_camera)
            .after(crate::camera::fly_camera_system)
            .before(crate::camera::constrain_exterior_camera_to_terrain)
            .before(crate::view_window::sync_view_window_from_train)
            .run_if(in_state(crate::ViewerAppState::Playing))
            .run_if(|| std::env::var("OPENRAILSRS_CAMERA_JOURNEY").is_ok_and(|v| v == "aba")),
    );
}
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update(
    mut state: ResMut<CameraJourney>,
    readiness: JourneyReadiness,
    world: Option<Res<crate::world::WorldSpawnProgress>>,
    terrain_pending: Option<Res<crate::terrain_spawn::TerrainSpawnProgress>>,
    tiles: Option<Res<crate::terrain_spawn::TerrainTileStream>>,
    terrain: Option<Res<crate::terrain::TerrainElevation>>,
    scene: Res<crate::track::TrackScene>,
    assets: Res<crate::shapes::RouteAssets>,
    cache: Res<crate::track_position::TrackPositionResolverCache>,
    focus: Res<crate::world::RouteFocus>,
    offset: Res<crate::world::RouteWorldOffset>,
    origin: Res<crate::floating_origin::FloatingOrigin>,
    mut live: ResMut<crate::live::LiveDrive>,
    mut follow: ResMut<CameraFollowMode>,
    mut mode: ResMut<CameraMode>,
    mut camera: Query<(&mut Transform, &mut OrbitState), With<Camera3d>>,
) {
    let Ok((mut tf, mut orbit)) = camera.single_mut() else {
        return;
    };
    let ready = readiness.pipeline.ready()
        && world.is_none()
        && terrain_pending.is_none()
        && tiles.as_ref().is_none_or(|t| t.pending_work() == 0);
    if state.from.is_none() {
        if !ready
            || readiness
                .capture
                .as_ref()
                .is_some_and(|c| !c.journey_can_start(live.session.state.odometer_m))
        {
            return;
        }
        let Some((edge, distance)) = live.visual_position_for_service(0, 8000., 0.) else {
            return;
        };
        let resolver = assets
            .track_db()
            .map(|tdb| cache.resolver(tdb, Some(assets.tsection())));
        let Some((to, _)) = crate::track_position::vehicle_pose_on_graph_edge(
            &scene.graph,
            &edge,
            distance,
            resolver.as_ref(),
            &scene,
            offset.delta,
            &focus,
            terrain.as_deref(),
        ) else {
            return;
        };
        let from = orbit.focus + origin.shift;
        state.enabled = true;
        state.from = Some(from);
        state.to = to;
        state.distance_m = (to - from).length();
    }
    live.paused = true;
    *follow = CameraFollowMode::Off;
    *mode = CameraMode::Fly;
    let from = state.from.unwrap();
    let dt = readiness.time.delta_secs_f64();
    match state.stage {
        0 | 2 | 4 => {
            if ready {
                state.elapsed_s += dt;
            } else {
                state.elapsed_s = 0.;
            }
            if state.elapsed_s >= 3. {
                state.elapsed_s = 0.;
                if state.stage == 4 {
                    state.complete = true;
                } else {
                    state.stage += 1;
                }
            }
        }
        1 | 3 => {
            state.elapsed_s += dt;
            if state.elapsed_s >= 20. {
                state.stage += 1;
                state.elapsed_s = 0.;
            }
        }
        _ => {}
    }
    let t = match state.stage {
        1 => state.elapsed_s as f32 / 20.,
        2 => 1.,
        3 => 1. - state.elapsed_s as f32 / 20.,
        _ => 0.,
    };
    orbit.focus = from.lerp(state.to, t) - origin.shift;
    *tf = crate::camera::camera_transform_from_orbit_state(
        orbit.focus,
        orbit.yaw,
        orbit.pitch,
        orbit.distance,
    );
}

#[derive(bevy::ecs::system::SystemParam)]
pub struct JourneyReadiness<'w> {
    pipeline: Res<'w, crate::performance::ScenePipelineStatus>,
    capture: Option<Res<'w, crate::capture::CaptureState>>,
    time: Res<'w, Time<Real>>,
}
