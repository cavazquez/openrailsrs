//! Movement is confined to declared interior space, in the current car frame.
use bevy::prelude::*;
pub fn constrained_move(head: Vec3, delta: Vec3, boxes: &[[[f32; 3]; 2]]) -> Vec3 {
    if boxes.is_empty() || !delta.is_finite() || !head.is_finite() {
        return head;
    }
    let delta = delta.clamp_length_max(20.);
    let steps = (delta.length() / 0.04).ceil().max(1.) as usize;
    let part = delta / steps as f32;
    let mut p = head;
    for _ in 0..steps {
        let next = p + part;
        if boxes.iter().any(|b| {
            next.cmpge(Vec3::from_array(b[0])).all() && next.cmple(Vec3::from_array(b[1])).all()
        }) {
            p = next;
        } else {
            break;
        }
    }
    p
}
pub fn update(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    follow: Res<crate::camera::CameraFollowMode>,
    presentation: Res<crate::train_presentation::TrainPresentation>,
    mut camera: ResMut<crate::camera::PassengerCamState>,
    ui: Option<Res<crate::player_ui::PlayerUiState>>,
) {
    if !follow.is_passenger()
        || ui
            .as_ref()
            .is_some_and(|u| u.panel != crate::player_ui::PlayerPanel::None)
        || !keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight])
    {
        return;
    }
    let Some(profile) = presentation
        .vehicles
        .iter()
        .find(|p| p.vehicle == camera.consist_car)
    else {
        return;
    };
    let x =
        f32::from(keys.pressed(KeyCode::ArrowRight)) - f32::from(keys.pressed(KeyCode::ArrowLeft));
    let z = f32::from(keys.pressed(KeyCode::ArrowUp)) - f32::from(keys.pressed(KeyCode::ArrowDown));
    let motion = Vec3::new(x, 0., z).normalize_or_zero() * time.delta_secs().min(0.1) * 1.3;
    camera.head_msts = constrained_move(camera.head_msts, motion, &profile.walk_boxes_m);
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn walking_never_crosses_gap_between_allowed_rooms_even_on_large_delta() {
        let boxes = vec![
            [[-1., 1., -5.], [1., 2., 0.]],
            [[-1., 1., 1.], [1., 2., 5.]],
        ];
        let p = constrained_move(Vec3::new(0., 1.7, -1.), Vec3::Z * 10., &boxes);
        assert!(p.z <= 0.);
        assert!((constrained_move(p, Vec3::X * 10., &boxes).x - 1.).abs() < 0.05);
        assert_eq!(constrained_move(p, Vec3::Z, &[]), p);
    }
}
