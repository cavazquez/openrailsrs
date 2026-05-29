//! Recentre the scene around the camera when far from the origin (Open Rails `PrepareFrame` style).
//!
//! Disabled in [`crate::ViewerPlugin`] until reparenting preserves world transforms on Chiltern-scale routes.

use bevy::prelude::*;

/// Cumulative shift applied when floating origin is active.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct FloatingOrigin {
    pub shift: Vec3,
}

/// Recentre when the camera drifts farther than this from the origin (metres).
pub const FLOATING_ORIGIN_THRESHOLD_M: f32 = 256.0;

/// Move the camera and non-UI scene transforms toward the origin for f32 stability.
#[allow(dead_code, clippy::type_complexity)]
pub(crate) fn apply_floating_origin(
    mut origin: ResMut<FloatingOrigin>,
    mut transforms: ParamSet<(
        Query<&mut Transform, With<Camera3d>>,
        Query<&mut Transform, (Without<Camera3d>, Without<Window>, Without<Node>)>,
    )>,
    mut billboards: Query<&mut crate::gameplay::StopBillboard>,
) {
    let mut cameras = transforms.p0();
    let Ok(mut cam) = cameras.single_mut() else {
        return;
    };
    if cam.translation.length() < FLOATING_ORIGIN_THRESHOLD_M {
        return;
    }
    let delta = cam.translation;
    origin.shift += delta;
    cam.translation -= delta;
    for mut tf in transforms.p1().iter_mut() {
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
