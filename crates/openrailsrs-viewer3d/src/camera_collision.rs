//! Conservative swept eye volume with wall sliding. Cab eyes remain authored.
use crate::effect_obstacles::{EffectObstacles, Solid};
use bevy::prelude::*;
const CLEARANCE: f32 = 0.25;
#[derive(Resource, Default)]
pub struct CameraCollision {
    previous: Option<Vec3>,
    pub corrections: u64,
}
fn sweep(from: Vec3, to: Vec3, solid: &Solid) -> Option<(f32, Vec3)> {
    let delta = to - from;
    let min = solid.min - Vec3::splat(CLEARANCE);
    let max = solid.max + Vec3::splat(CLEARANCE);
    if from.cmpgt(min).all() && from.cmplt(max).all() {
        return None;
    }
    let mut enter = 0f32;
    let mut leave = 1f32;
    let mut normal = Vec3::ZERO;
    for axis in 0..3 {
        if delta[axis].abs() < 1e-7 {
            if from[axis] < min[axis] || from[axis] > max[axis] {
                return None;
            }
            continue;
        }
        let a = (min[axis] - from[axis]) / delta[axis];
        let b = (max[axis] - from[axis]) / delta[axis];
        if a.min(b) >= enter {
            enter = a.min(b);
            normal = Vec3::ZERO;
            normal[axis] = -delta[axis].signum();
        }
        leave = leave.min(a.max(b));
        if enter > leave {
            return None;
        }
    }
    (enter <= 1. && leave >= 0. && normal != Vec3::ZERO).then_some((enter, normal))
}
pub fn clip_motion(from: Vec3, to: Vec3, solids: &[Solid]) -> Vec3 {
    if !from.is_finite() || !to.is_finite() {
        return from;
    }
    let mut position = from;
    let mut goal = to;
    for _ in 0..3 {
        let hit = solids
            .iter()
            .filter_map(|s| sweep(position, goal, s))
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let Some((time, normal)) = hit else {
            position = goal;
            break;
        };
        let delta = goal - position;
        position += delta * time.max(0.) + normal * 0.001;
        let remaining = delta * (1. - time);
        goal = position + remaining - normal * remaining.dot(normal).min(0.);
    }
    // If a streamed object initially encloses the eye, exit by its nearest side
    // or roof, never by its bottom into the terrain.
    for solid in solids.iter().take(4096) {
        let min = solid.min - Vec3::splat(CLEARANCE);
        let max = solid.max + Vec3::splat(CLEARANCE);
        if position.cmpgt(min).all() && position.cmplt(max).all() {
            let exits = [
                (
                    position.x - min.x,
                    Vec3::new(min.x - 0.001, position.y, position.z),
                ),
                (
                    max.x - position.x,
                    Vec3::new(max.x + 0.001, position.y, position.z),
                ),
                (
                    position.z - min.z,
                    Vec3::new(position.x, position.y, min.z - 0.001),
                ),
                (
                    max.z - position.z,
                    Vec3::new(position.x, position.y, max.z + 0.001),
                ),
                (
                    max.y - position.y,
                    Vec3::new(position.x, max.y + 0.001, position.z),
                ),
            ];
            position = exits
                .into_iter()
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .unwrap()
                .1;
        }
    }
    position
}
pub fn update(
    mut state: ResMut<CameraCollision>,
    obstacles: Option<Res<EffectObstacles>>,
    follow: Res<crate::camera::CameraFollowMode>,
    mode: Res<crate::camera::CameraMode>,
    origin: Res<crate::floating_origin::FloatingOrigin>,
    mut camera: Query<
        (&mut Transform, &mut crate::camera::OrbitState),
        (With<Camera3d>, Without<crate::mirrors::MirrorCamera>),
    >,
) {
    let _profile = crate::performance::profile_system("camera_collision");
    let Ok((mut camera, mut orbit)) = camera.single_mut() else {
        return;
    };
    let world = camera.translation + origin.shift;
    if matches!(
        *follow,
        crate::camera::CameraFollowMode::DriverCam
            | crate::camera::CameraFollowMode::Cab2d
            | crate::camera::CameraFollowMode::PassengerCam
    ) {
        state.previous = None;
        return;
    }
    let Some(obstacles) = obstacles else {
        state.previous = Some(world);
        return;
    };
    let from = state.previous.unwrap_or(world) - obstacles.origin;
    let to = world - obstacles.origin;
    let resolved = clip_motion(from, to, &obstacles.solids) + obstacles.origin - origin.shift;
    if resolved.distance_squared(camera.translation) > 0.000001 {
        state.corrections += 1;
        camera.translation = resolved;
        if *mode == crate::camera::CameraMode::Orbit {
            let delta = resolved - orbit.focus;
            orbit.distance = delta.length().max(0.5);
            orbit.pitch = (delta.y / orbit.distance).clamp(-1., 1.).asin();
            orbit.yaw = delta.x.atan2(delta.z);
            camera.look_at(orbit.focus, Vec3::Y);
        }
    }
    state.previous = Some(camera.translation + origin.shift);
}
#[cfg(test)]
mod tests {
    use super::*;
    fn wall() -> Solid {
        Solid {
            min: Vec3::new(0., -5., -5.),
            max: Vec3::new(1., 5., 5.),
            owner: None,
        }
    }
    #[test]
    fn eye_cannot_tunnel_through_a_wall_but_slides_along_it() {
        let blocked = clip_motion(Vec3::new(-2., 0., 0.), Vec3::new(3., 0., 2.), &[wall()]);
        assert!(blocked.x < -CLEARANCE);
        assert!((blocked.z - 2.).abs() < 0.01);
        assert_eq!(
            clip_motion(Vec3::new(-2., 0., 0.), Vec3::new(-2., 0., 2.), &[wall()]),
            Vec3::new(-2., 0., 2.)
        );
    }
    #[test]
    fn consecutive_zoom_and_corner_motion_stay_clear_without_repeated_jumps() {
        let walls = [
            wall(),
            Solid {
                min: Vec3::new(-5., -5., 3.),
                max: Vec3::new(1., 5., 4.),
                owner: None,
            },
        ];
        let mut point = Vec3::new(-2., 0., 0.);
        for _ in 0..120 {
            let next = clip_motion(point, point + Vec3::new(0.1, 0., 0.1), &walls);
            assert!(next.x < -CLEARANCE && next.z < 3. - CLEARANCE);
            assert!(point.distance(next) < 0.15);
            point = next;
        }
        let paused = clip_motion(point, point, &walls);
        assert!(point.distance(paused) < 1e-6);
        // Obstacles and previous eye use the same absolute frame across rebase.
        let shift = Vec3::new(10000., 0., -20000.);
        let translated = walls.map(|s| Solid {
            min: s.min + shift,
            max: s.max + shift,
            ..s
        });
        let a = clip_motion(Vec3::new(-2., 0., 0.), Vec3::new(2., 0., 2.), &walls);
        let b = clip_motion(
            Vec3::new(-2., 0., 0.) + shift,
            Vec3::new(2., 0., 2.) + shift,
            &translated,
        ) - shift;
        assert!(a.distance(b) < 0.01);
    }
    #[test]
    fn initial_stream_overlap_exits_the_volume_and_never_exits_below_the_floor() {
        let p = clip_motion(Vec3::new(0.5, 0., 0.), Vec3::new(0.5, 0., 0.), &[wall()]);
        assert!(p.x < -CLEARANCE || p.x > 1. + CLEARANCE);
        assert_eq!(p.y, 0.);
    }
}
