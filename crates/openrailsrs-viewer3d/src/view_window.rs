//! Mobile view window centred on the train (or route anchor when static).

use bevy::prelude::*;

use crate::launch::{run_corridor_half_width_m, view_radius_m};
use crate::live::LiveTrainMarker;
use crate::world::RouteFocus;

/// Horizontal cull/stream radius around [`Self::center_world`] (default 120 m).
#[derive(Resource, Clone, Copy, Debug)]
pub struct ViewWindow {
    pub center_world: Vec3,
    pub radius_m: f32,
    pub half_width_m: f32,
}

impl Default for ViewWindow {
    fn default() -> Self {
        Self {
            center_world: Vec3::ZERO,
            radius_m: view_radius_m(),
            half_width_m: run_corridor_half_width_m(),
        }
    }
}

impl ViewWindow {
    pub fn from_route_focus(focus: &RouteFocus) -> Self {
        Self {
            center_world: focus.center,
            radius_m: view_radius_m(),
            half_width_m: run_corridor_half_width_m(),
        }
    }

    /// [`RouteFocus`] for distance/collect helpers at the mobile centre (keeps MSL origin).
    pub fn route_focus_at_center(&self, height_origin: f32) -> RouteFocus {
        RouteFocus {
            center: self.center_world,
            height_origin,
        }
    }

    pub fn horizontal_distance_world(&self, world: Vec3) -> f32 {
        let dx = world.x - self.center_world.x;
        let dz = world.z - self.center_world.z;
        (dx * dx + dz * dz).sqrt()
    }

    pub fn contains_world_xz(&self, world: Vec3) -> bool {
        self.horizontal_distance_world(world) <= self.radius_m
    }
}

/// Keep [`ViewWindow`] aligned with the live train head (MSTS world XZ + render Y from marker).
pub fn sync_view_window_from_train(
    opts: Res<crate::launch::ViewerLaunchOpts>,
    mut window: ResMut<ViewWindow>,
    focus: Res<RouteFocus>,
    train: Query<&Transform, With<LiveTrainMarker>>,
    origin: Res<crate::floating_origin::FloatingOrigin>,
) {
    if !opts.live {
        return;
    }
    let Ok(tf) = train.single() else {
        return;
    };
    let msts_x = tf.translation.x + focus.center.x + origin.shift.x;
    let msts_z = tf.translation.z + focus.center.z + origin.shift.z;
    window.center_world = Vec3::new(msts_x, focus.center.y, msts_z);
}

pub fn view_window_stream_center(window: &ViewWindow, focus: &RouteFocus, live: bool) -> Vec3 {
    if live {
        window.center_world
    } else {
        focus.center
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_radius_is_120() {
        let w = ViewWindow::default();
        assert!((w.radius_m - 120.0).abs() < 0.1);
    }

    #[test]
    fn contains_world_within_radius() {
        let w = ViewWindow {
            center_world: Vec3::new(100.0, 0.0, 200.0),
            radius_m: 120.0,
            half_width_m: 120.0,
        };
        assert!(w.contains_world_xz(Vec3::new(150.0, 0.0, 200.0)));
        assert!(!w.contains_world_xz(Vec3::new(300.0, 0.0, 200.0)));
    }
}
