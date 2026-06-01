//! Recentre the scene around the camera when far from the origin (Open Rails `PrepareFrame` style).

use bevy::prelude::*;

use crate::camera::{OrbitState, camera_transform_from_orbit_state};
use crate::launch::ViewerSceneryMode;
use crate::live::LiveTrainMarker;
use crate::track::TrackScene;
use crate::train::{ReplayState, TrainMarker, pose_at_time};
use crate::viewer_log;
use crate::world::{RouteFocus, RouteWorldOffset};

/// Cumulative shift applied when floating origin is active.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct FloatingOrigin {
    pub shift: Vec3,
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
    mut cameras: Query<(&mut Transform, &mut OrbitState), With<Camera3d>>,
    mut transforms: Query<
        &mut Transform,
        (
            Without<Camera3d>,
            Without<Window>,
            Without<Node>,
            Without<TrainMarker>,
            Without<LiveTrainMarker>,
        ),
    >,
    mut train_tf: Query<&mut Transform, Or<(With<TrainMarker>, With<LiveTrainMarker>)>>,
    mut billboards: Query<&mut crate::gameplay::StopBillboard>,
) {
    if !mode.is_track_dev() {
        return;
    }

    let subject = train_tf
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
    for mut tf in &mut train_tf {
        tf.translation -= delta;
    }
    for mut tf in &mut transforms {
        tf.translation -= delta;
    }
    for mut billboard in &mut billboards {
        billboard.world -= delta;
    }

    let Ok((mut cam, mut orbit)) = cameras.single_mut() else {
        return;
    };
    cam.translation -= delta;
    orbit.focus -= delta;
    *cam = camera_transform_from_orbit_state(orbit.focus, orbit.yaw, orbit.pitch, orbit.distance);

    viewer_log!(
        "openrailsrs-viewer3d: track_dev — recentered scene at subject ({:.0}, {:.0}, {:.0})",
        delta.x,
        delta.y,
        delta.z
    );
}

/// Move the camera and non-UI scene transforms toward the origin for f32 stability.
#[allow(clippy::type_complexity)]
pub(crate) fn apply_floating_origin(
    mode: Res<ViewerSceneryMode>,
    mut origin: ResMut<FloatingOrigin>,
    mut cameras: Query<(&mut Transform, &mut OrbitState), With<Camera3d>>,
    mut transforms: Query<&mut Transform, (Without<Camera3d>, Without<Window>, Without<Node>)>,
    mut billboards: Query<&mut crate::gameplay::StopBillboard>,
) {
    if mode.is_track_dev() {
        return;
    }
    let Ok((mut cam, mut orbit)) = cameras.single_mut() else {
        return;
    };
    if cam.translation.length() < FLOATING_ORIGIN_THRESHOLD_M {
        return;
    }
    let delta = cam.translation;
    origin.shift += delta;
    cam.translation -= delta;
    orbit.focus -= delta;
    for mut tf in transforms.iter_mut() {
        tf.translation -= delta;
    }
    for mut billboard in &mut billboards {
        billboard.world -= delta;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_is_sub_kilometre() {
        assert_eq!(FLOATING_ORIGIN_THRESHOLD_M, 256.0);
    }
}
