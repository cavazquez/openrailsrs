//! Native 3D cab hit testing and lever dragging. Camera gestures consume only the background.
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use openrailsrs_formats::{CabControl, ControlType};

use crate::cab_cvf::{CabCvfPart, CabCvfState, MatrixDriver, cvf_control_at_order};
use crate::cab_view::CabInteriorMarker;
use crate::camera::CameraFollowMode;
use crate::live::LiveDrive;

#[derive(Resource, Default)]
pub struct CabMouseState {
    pub pointer_captured: bool,
    pub label: String,
    drag: Option<(ControlType, Vec2, f64)>,
    last_pick_s: f64,
    hover: Option<ControlType>,
}
#[derive(Component)]
pub(crate) struct CabMouseLabel;

pub fn spawn_cab_mouse_label(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(35.0),
                bottom: Val::Px(56.0),
                padding: UiRect::all(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.04, 0.06, 0.86)),
            Visibility::Hidden,
            CabMouseLabel,
        ))
        .with_children(|p| {
            p.spawn((
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(Color::srgb(0.45, 0.9, 1.0)),
            ));
        });
}
pub(crate) fn update_cab_mouse_label(
    state: Res<CabMouseState>,
    mut roots: Query<(&mut Visibility, &Children), With<CabMouseLabel>>,
    mut texts: Query<&mut Text>,
) {
    for (mut vis, children) in &mut roots {
        *vis = if state.label.is_empty() {
            Visibility::Hidden
        } else {
            Visibility::Visible
        };
        for child in children {
            if let Ok(mut text) = texts.get_mut(*child) {
                text.0 = state.label.clone();
            }
        }
    }
}
fn actionable(control: &ControlType) -> bool {
    matches!(
        control,
        ControlType::Throttle
            | ControlType::ThrottleDisplay
            | ControlType::TrainBrake
            | ControlType::DirectionDisplay
    ) || matches!(control,ControlType::Generic(name)if matches!(name.to_ascii_uppercase().as_str(),"HORN"|"WIPERS"|"WIPER"|"EXTERNALWIPERS"|"DOORS"|"PANTOGRAPH"|"PANTOGRAPHS"))
}
fn control_name(control: &ControlType) -> &str {
    match control {
        ControlType::Throttle | ControlType::ThrottleDisplay => "Regulador",
        ControlType::TrainBrake => "Freno de tren",
        ControlType::DirectionDisplay => "Inversor",
        ControlType::Generic(name) => name,
        _ => "Control",
    }
}
fn is_lever(control: &ControlType) -> bool {
    matches!(
        control,
        ControlType::Throttle
            | ControlType::ThrottleDisplay
            | ControlType::TrainBrake
            | ControlType::DirectionDisplay
    )
}
pub fn apply_cab_control(live: &mut LiveDrive, control: &ControlType, value: f64) {
    match control {
        ControlType::Throttle | ControlType::ThrottleDisplay => {
            live.session.driver_throttle = value.clamp(0.0, 1.0);
        }
        ControlType::TrainBrake => {
            live.session.driver_brake = value.clamp(0.0, 1.0);
        }
        ControlType::DirectionDisplay => {
            let value = if value < 0.25 {
                0.0
            } else if value > 0.75 {
                1.0
            } else {
                0.5
            };
            let _ = live.session.set_direction(value);
        }
        ControlType::Generic(name) => match name.to_ascii_uppercase().as_str() {
            "HORN" => live.session.trigger_horn(0.15),
            "WIPERS" | "WIPER" | "EXTERNALWIPERS" => live.session.toggle_wiper(),
            "DOORS" => live.session.toggle_doors(),
            "PANTOGRAPH" | "PANTOGRAPHS" => {
                live.session.exterior.pantograph_command_up =
                    !live.session.exterior.pantograph_command_up
            }
            _ => {}
        },
        _ => {}
    }
}

pub fn cab_mouse_controls(
    follow: Res<CameraFollowMode>,
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    mut state: ResMut<CabMouseState>,
    cvf: Res<CabCvfState>,
    mut live: ResMut<LiveDrive>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    parts: Query<
        (
            &Mesh3d,
            &GlobalTransform,
            Option<&CabCvfPart>,
            Option<&crate::cab_screen::CabNativeScreen>,
        ),
        With<CabInteriorMarker>,
    >,
    meshes: Res<Assets<Mesh>>,
    ui: Res<crate::player_ui::PlayerUiState>,
    pointer: Res<crate::player_ui::UiPointerCapture>,
) {
    state.pointer_captured = false;
    if *follow != CameraFollowMode::DriverCam
        || ui.panel != crate::player_ui::PlayerPanel::None
        || pointer.0
    {
        state.drag = None;
        state.hover = None;
        state.label.clear();
        return;
    }
    let Some(cursor) = windows.single().ok().and_then(Window::cursor_position) else {
        state.drag = None;
        return;
    };
    if !mouse.pressed(MouseButton::Left) {
        state.drag = None;
    }
    if let Some((control, start, value)) = state.drag.clone() {
        state.pointer_captured = true;
        let new_value = (value + f64::from(start.y - cursor.y) / 180.0).clamp(0.0, 1.0);
        apply_cab_control(&mut live, &control, new_value);
        state.label = format!("{} · {:.0}%", control_name(&control), new_value * 100.0);
        return;
    }
    if time.elapsed_secs_f64() - state.last_pick_s >= 0.12 || mouse.just_pressed(MouseButton::Left)
    {
        state.last_pick_s = time.elapsed_secs_f64();
        state.hover = None;
        let Ok((camera, transform)) = cameras.single() else {
            return;
        };
        let Ok(ray) = camera.viewport_to_world(transform, cursor) else {
            return;
        };
        let Some(runtime) = cvf.runtime.as_ref() else {
            return;
        };
        let mut best: Option<(f32, Option<ControlType>, bool)> = None;
        for (mesh, transform, part, screen) in &parts {
            let Some(mesh) = meshes.get(&mesh.0) else {
                continue;
            };
            let Some(distance) =
                raycast_mesh(mesh, transform.to_matrix(), ray.origin, *ray.direction)
            else {
                continue;
            };
            if best.as_ref().is_some_and(|(old, _, _)| distance >= *old) {
                continue;
            }
            let control = part
                .and_then(|part| runtime.matrix_drivers.get(&part.matrix_idx))
                .and_then(|driver| {
                    let (control, order) = match driver {
                        MatrixDriver::Lever { control, order, .. }
                        | MatrixDriver::MultiState { control, order, .. } => (control, *order),
                        _ => return None,
                    };
                    if !actionable(control) {
                        return None;
                    }
                    match cvf_control_at_order(&runtime.cvf, control, order) {
                        // Gauges and digits with the same type are readouts, not handles.
                        Some(CabControl::Gauge { .. } | CabControl::Digital { .. }) => None,
                        _ => Some(control.clone()),
                    }
                });
            best = Some((distance, control, screen.is_some()));
        }
        if let Some((_, control, screen)) = best {
            state.hover = control;
            state.pointer_captured = screen && mouse.pressed(MouseButton::Left);
        }
    }
    if let Some(control) = state.hover.clone() {
        let value = crate::cab_cvf::control_value(&control, &live.session.cab_telemetry());
        state.label = if is_lever(&control) {
            format!(
                "{} · {:.0}% · arrastrar ↑ / ↓",
                control_name(&control),
                value * 100.0
            )
        } else {
            format!("{} · clic", control_name(&control))
        };
        if mouse.just_pressed(MouseButton::Left) {
            state.pointer_captured = true;
            if is_lever(&control) {
                state.drag = Some((control, cursor, value));
            } else {
                apply_cab_control(&mut live, &control, 1.0);
            }
        } else if mouse.pressed(MouseButton::Left)
            && matches!(&control,ControlType::Generic(n)if n.eq_ignore_ascii_case("HORN"))
        {
            state.pointer_captured = true;
            live.session.trigger_horn(0.15);
        }
    } else {
        state.label.clear();
    }
}
/// Triangle intersection works without UVs and rejects occluded controls using nearest hit.
pub fn raycast_mesh(mesh: &Mesh, world: Mat4, origin: Vec3, direction: Vec3) -> Option<f32> {
    let points = mesh.attribute(Mesh::ATTRIBUTE_POSITION)?.as_float3()?;
    let local = world.inverse();
    let origin = local.transform_point3(origin);
    let dir = local.transform_vector3(direction);
    // Cheap slab rejection before visiting the triangle list.
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for p in points {
        let p = Vec3::from(*p);
        min = min.min(p);
        max = max.max(p);
    }
    let mut near = 0.0_f32;
    let mut far = f32::INFINITY;
    for axis in 0..3 {
        if dir[axis].abs() < 1e-7 {
            if origin[axis] < min[axis] || origin[axis] > max[axis] {
                return None;
            }
        } else {
            let a = (min[axis] - origin[axis]) / dir[axis];
            let b = (max[axis] - origin[axis]) / dir[axis];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if near > far {
                return None;
            }
        }
    }
    let indices: Vec<usize> = mesh
        .indices()
        .map(|i| i.iter().collect())
        .unwrap_or_else(|| (0..points.len()).collect());
    let mut closest = f32::INFINITY;
    for tri in indices.as_chunks::<3>().0 {
        let (Some(a), Some(b), Some(c)) =
            (points.get(tri[0]), points.get(tri[1]), points.get(tri[2]))
        else {
            continue;
        };
        let a = Vec3::from(*a);
        let e1 = Vec3::from(*b) - a;
        let e2 = Vec3::from(*c) - a;
        let p = dir.cross(e2);
        let det = e1.dot(p);
        if det.abs() < 1e-7 {
            continue;
        }
        let inverse = 1.0 / det;
        let t = origin - a;
        let u = t.dot(p) * inverse;
        if !(0.0..=1.0).contains(&u) {
            continue;
        }
        let q = t.cross(e1);
        let v = dir.dot(q) * inverse;
        if v < 0.0 || u + v > 1.0 {
            continue;
        }
        let distance = e2.dot(q) * inverse;
        if distance > 0.0 && distance < closest {
            closest = distance;
        }
    }
    closest.is_finite().then_some(closest)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_mesh_hit_uses_world_distance_under_scale() {
        let mesh = Mesh::from(Cuboid::new(2.0, 2.0, 2.0));
        let world = Mat4::from_scale_rotation_translation(
            Vec3::splat(2.0),
            Quat::IDENTITY,
            Vec3::new(0.0, 0.0, 10.0),
        );
        let hit = raycast_mesh(&mesh, world, Vec3::ZERO, Vec3::Z).unwrap();
        assert!((hit - 8.0).abs() < 0.01);
        assert!(raycast_mesh(&mesh, world, Vec3::new(5.0, 0.0, 0.0), Vec3::Z).is_none());
    }
}
