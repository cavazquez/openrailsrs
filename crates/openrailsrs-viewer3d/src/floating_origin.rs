//! Recentre the scene around the camera when far from the origin (Open Rails `PrepareFrame` style).

use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::system::ParamSet;
use bevy::prelude::*;

use crate::cab_view::{CabInteriorMarker, CabInteriorRoot};
use crate::camera::{CameraFollowMode, OrbitState, camera_transform_from_orbit_state};
use crate::launch::ViewerSceneryMode;
use crate::live::{LiveTrainBody, LiveTrainMarker};
use crate::track::TrackScene;
use crate::train::{ReplayState, TrainMarker, pose_at_time};
use crate::viewer_log;
use crate::world::{RouteFocus, RouteWorldOffset};

/// Cumulative horizontal shift (XZ only) for floating-origin rebasing.
/// Y stays in [`RouteFocus::height_origin`] / terrain MSL space.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct FloatingOrigin {
    pub shift: Vec3,
}

#[inline]
pub fn horizontal_shift(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

/// Map a render-space pose from `pose_at_time` / `position_on_graph` into view space.
#[inline]
pub fn view_position(render: Vec3, origin: &FloatingOrigin) -> Vec3 {
    let h = horizontal_shift(origin.shift);
    Vec3::new(render.x - h.x, render.y, render.z - h.z)
}

/// Same as [`view_position`] for static scenery spawned from `RouteFocus::scenery_to_render`.
#[inline]
pub fn view_translation(render: Vec3, origin: &FloatingOrigin) -> Vec3 {
    view_position(render, origin)
}

/// Apply floating-origin view shift to a focus-relative spawn transform.
#[inline]
pub fn view_transform(mut tf: Transform, origin: &FloatingOrigin) -> Transform {
    tf.translation = view_translation(tf.translation, origin);
    tf
}

/// Recentre when the camera drifts farther than this from the origin (metres).
pub const FLOATING_ORIGIN_THRESHOLD_M: f32 = 256.0;

/// One-shot: put the replay/live subject at the origin so grid + camera align in `--track-dev`.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn track_dev_recenter_at_subject(
    mode: Res<ViewerSceneryMode>,
    replay: Res<ReplayState>,
    scene: Res<TrackScene>,
    offset: Res<RouteWorldOffset>,
    focus: Res<RouteFocus>,
    mut origin: ResMut<FloatingOrigin>,
    mut queries: ParamSet<(
        Query<(&mut Transform, &mut OrbitState), With<Camera3d>>,
        Query<&Transform, Or<(With<TrainMarker>, With<LiveTrainMarker>)>>,
        Query<
            &mut Transform,
            (
                Without<Camera3d>,
                Without<Window>,
                Without<Node>,
                Without<ChildOf>,
                Without<LiveTrainMarker>,
                Without<TrainMarker>,
            ),
        >,
    )>,
    mut billboards: Query<&mut crate::gameplay::StopBillboard>,
) {
    if !mode.is_track_focused() {
        return;
    }

    let subject = queries
        .p1()
        .iter()
        .next()
        .map(|t| t.translation)
        .or_else(|| {
            replay.tracks.first().and_then(|track| {
                pose_at_time(
                    &scene.graph,
                    &track.rows,
                    0.0,
                    None,
                    &scene,
                    offset.delta,
                    &focus,
                )
                .map(|(pos, _, _)| pos)
            })
        });

    let Some(delta) = subject else {
        return;
    };
    if delta.length_squared() < 1.0 {
        return;
    }

    origin.shift += delta;
    for mut tf in queries.p2().iter_mut() {
        tf.translation -= delta;
    }

    let mut camera_q = queries.p0();
    let Ok((mut cam, mut orbit)) = camera_q.single_mut() else {
        return;
    };
    cam.translation -= delta;
    orbit.focus -= delta;
    *cam = camera_transform_from_orbit_state(orbit.focus, orbit.yaw, orbit.pitch, orbit.distance);

    for mut billboard in &mut billboards {
        billboard.world -= delta;
    }

    viewer_log!(
        "openrailsrs-viewer3d: track-focused — recentered scene at subject ({:.0}, {:.0}, {:.0})",
        delta.x,
        delta.y,
        delta.z
    );
}

/// Move root-level scenery toward the origin for f32 stability.
///
/// Train hierarchies are excluded: [`live::update_live_train_marker`] / replay markers
/// already use [`view_position`], and shifting child transforms would corrupt locals.
#[allow(clippy::type_complexity)]
pub(crate) fn apply_floating_origin(
    mode: Res<ViewerSceneryMode>,
    opts: Res<crate::launch::ViewerLaunchOpts>,
    follow: Res<CameraFollowMode>,
    mut origin: ResMut<FloatingOrigin>,
    mut queries: ParamSet<(
        Query<&Transform, Or<(With<LiveTrainMarker>, With<TrainMarker>)>>,
        Query<(&mut Transform, &mut OrbitState), With<Camera3d>>,
        Query<
            &mut Transform,
            (
                Without<Camera3d>,
                Without<Window>,
                Without<Node>,
                Without<ChildOf>,
                Without<LiveTrainMarker>,
                Without<TrainMarker>,
                Without<LiveTrainBody>,
                Without<CabInteriorMarker>,
                Without<CabInteriorRoot>,
            ),
        >,
    )>,
    mut billboards: Query<&mut crate::gameplay::StopBillboard>,
) {
    if mode.is_tile_lab() {
        return;
    }
    // `--track-dev` and static `--run-corridor` rely on the one-shot startup recenter.
    if mode.is_track_dev() || (mode.is_run_corridor() && !opts.live) {
        return;
    }

    let subject = queries.p0().iter().next().map(|t| t.translation);

    let reference = {
        let camera_q = queries.p1();
        let Ok((cam, _)) = camera_q.single() else {
            return;
        };
        horizontal_shift(subject.unwrap_or(cam.translation))
    };

    if reference.length() < FLOATING_ORIGIN_THRESHOLD_M {
        return;
    }

    let delta = reference;
    origin.shift += delta;
    let driver_cam = *follow == CameraFollowMode::DriverCam;

    for mut tf in queries.p2().iter_mut() {
        tf.translation.x -= delta.x;
        tf.translation.z -= delta.z;
    }
    for mut billboard in &mut billboards {
        billboard.world.x -= delta.x;
        billboard.world.z -= delta.z;
    }

    // Driver view: camera is set every frame from the lead vehicle; do not zero it here.
    if !driver_cam {
        let mut camera_q = queries.p1();
        let Ok((mut cam, mut orbit)) = camera_q.single_mut() else {
            return;
        };
        cam.translation.x -= delta.x;
        cam.translation.z -= delta.z;
        orbit.focus.x -= delta.x;
        orbit.focus.z -= delta.z;
    }

    viewer_log!(
        "openrailsrs-viewer3d: floating origin — XZ shift ({:.0}, {:.0}) m (total {:.0}, {:.0})",
        delta.x,
        delta.z,
        origin.shift.x,
        origin.shift.z,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_is_sub_kilometre() {
        assert_eq!(FLOATING_ORIGIN_THRESHOLD_M, 256.0);
    }

    #[test]
    fn view_position_subtracts_shift() {
        let origin = FloatingOrigin {
            shift: Vec3::new(100.0, 0.0, -50.0),
        };
        let render = Vec3::new(110.0, 5.0, -40.0);
        assert_eq!(view_position(render, &origin), Vec3::new(10.0, 5.0, 10.0));
    }

    #[test]
    fn view_position_preserves_render_y() {
        let origin = FloatingOrigin {
            shift: Vec3::new(100.0, 9.0, -50.0),
        };
        let render = Vec3::new(110.0, 0.3, -40.0);
        assert_eq!(view_position(render, &origin).y, 0.3);
    }
}
