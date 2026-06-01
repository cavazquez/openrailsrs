//! Recentre the scene around the camera when far from the origin (Open Rails `PrepareFrame` style).

use bevy::ecs::system::ParamSet;
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

/// Map a render-space pose from `pose_at_time` / `position_on_graph` into view space.
#[inline]
pub fn view_position(render: Vec3, origin: &FloatingOrigin) -> Vec3 {
    render - origin.shift
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
        Query<&mut Transform, (Without<Camera3d>, Without<Window>, Without<Node>)>,
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
    // Shift all other transforms by -delta to keep track, scenery and consists aligned
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

/// Move the camera and non-UI scene transforms toward the origin for f32 stability.
#[allow(clippy::type_complexity)]
pub(crate) fn apply_floating_origin(
    mode: Res<ViewerSceneryMode>,
    mut origin: ResMut<FloatingOrigin>,
    mut cameras: Query<(&mut Transform, &mut OrbitState), With<Camera3d>>,
    mut transforms: Query<&mut Transform, (Without<Camera3d>, Without<Window>, Without<Node>)>,
    mut billboards: Query<&mut crate::gameplay::StopBillboard>,
) {
    if mode.is_track_focused() {
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

    #[test]
    fn view_position_subtracts_shift() {
        let origin = FloatingOrigin {
            shift: Vec3::new(100.0, 0.0, -50.0),
        };
        let render = Vec3::new(110.0, 5.0, -40.0);
        assert_eq!(view_position(render, &origin), Vec3::new(10.0, 5.0, 10.0));
    }
}
