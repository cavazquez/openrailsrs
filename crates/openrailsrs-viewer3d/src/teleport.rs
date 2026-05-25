//! Coordinate teleport dialog (`G`): type x,y,z and jump the camera there.

use bevy::prelude::*;

use crate::camera::{CameraMode, OrbitState, camera_transform_from_orbit_state};

/// Teleport dialog state (toggle with `G`).
#[derive(Resource, Clone, Debug)]
pub struct TeleportDialog {
    pub open: bool,
    pub buffer: String,
    pub status: String,
    backspace_hold_s: f32,
    backspace_repeat_delay_s: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct BackspaceRepeatState {
    hold_s: f32,
    repeat_delay_s: f32,
}

impl BackspaceRepeatState {
    fn reset(&mut self) {
        self.hold_s = 0.0;
        self.repeat_delay_s = BACKSPACE_INITIAL_DELAY_S;
    }
}

impl Default for TeleportDialog {
    fn default() -> Self {
        Self {
            open: false,
            buffer: String::new(),
            status: String::new(),
            backspace_hold_s: 0.0,
            backspace_repeat_delay_s: BACKSPACE_INITIAL_DELAY_S,
        }
    }
}

impl TeleportDialog {
    fn backspace_repeat_state(&self) -> BackspaceRepeatState {
        BackspaceRepeatState {
            hold_s: self.backspace_hold_s,
            repeat_delay_s: self.backspace_repeat_delay_s,
        }
    }

    fn set_backspace_repeat_state(&mut self, state: BackspaceRepeatState) {
        self.backspace_hold_s = state.hold_s;
        self.backspace_repeat_delay_s = state.repeat_delay_s;
    }
}

const BACKSPACE_INITIAL_DELAY_S: f32 = 0.35;
const BACKSPACE_REPEAT_INTERVAL_S: f32 = 0.04;

#[derive(Component)]
pub(crate) struct TeleportRoot;

#[derive(Component)]
pub(crate) struct TeleportBufferText;

#[derive(Component)]
pub(crate) struct TeleportStatusText;

#[derive(Component)]
pub(crate) struct TeleportGoButton;

#[derive(Component)]
pub(crate) struct TeleportCancelButton;

pub(crate) fn teleport_closed(dialog: Res<TeleportDialog>) -> bool {
    !dialog.open
}

/// Parse `x,y,z` or `x y z` (commas or spaces).
pub fn parse_coords(input: &str) -> Option<Vec3> {
    let normalized = input.replace(',', " ");
    let parts: Vec<f32> = normalized
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();
    if parts.len() == 3 {
        Some(Vec3::new(parts[0], parts[1], parts[2]))
    } else {
        None
    }
}

pub fn default_buffer_for_mode(
    mode: CameraMode,
    camera_pos: Vec3,
    orbit_focus: Option<Vec3>,
) -> String {
    let pos = match mode {
        CameraMode::Orbit => orbit_focus.unwrap_or(camera_pos),
        CameraMode::Fly => camera_pos,
    };
    format!("{:.0},{:.0},{:.0}", pos.x, pos.y, pos.z)
}

pub fn apply_teleport(
    target: Vec3,
    mode: CameraMode,
    transform: &mut Transform,
    orbit: &mut OrbitState,
) {
    match mode {
        CameraMode::Orbit => {
            orbit.focus = target;
            *transform = camera_transform_from_orbit_state(
                orbit.focus,
                orbit.yaw,
                orbit.pitch,
                orbit.distance,
            );
        }
        CameraMode::Fly => {
            transform.translation = target;
            let fwd = transform.forward().as_vec3();
            orbit.focus = target + fwd * orbit.distance;
        }
    }
}

pub(crate) fn spawn_teleport_ui(mut commands: Commands) {
    commands
        .spawn((
            TeleportRoot,
            Visibility::Hidden,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            ZIndex(200),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(14.0)),
                        row_gap: Val::Px(8.0),
                        min_width: Val::Px(360.0),
                        border: UiRect::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.06, 0.10, 0.14)),
                    BorderColor::all(Color::srgb(1.0, 0.67, 0.2)),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        Text::new("Ir a coordenadas"),
                        TextFont {
                            font_size: 16.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.9, 0.9, 0.9)),
                    ));
                    panel.spawn((
                        Text::new("Formato: x,y,z  —  orbit mueve el foco, fly mueve la cámara"),
                        TextFont {
                            font_size: 11.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.55, 0.62, 0.7)),
                    ));
                    panel.spawn((
                        Text::new("demo dyntrack: 80,0,1"),
                        TextFont {
                            font_size: 11.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.45, 0.55, 0.65)),
                    ));
                    panel
                        .spawn((
                            Node {
                                width: Val::Percent(100.0),
                                padding: UiRect::all(Val::Px(8.0)),
                                border: UiRect::all(Val::Px(1.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.02, 0.04, 0.06)),
                            BorderColor::all(Color::srgb(0.35, 0.45, 0.55)),
                        ))
                        .with_children(|field| {
                            field.spawn((
                                TeleportBufferText,
                                Text::new(""),
                                TextFont {
                                    font_size: 15.0,
                                    ..default()
                                },
                                TextColor(Color::srgb(0.85, 0.92, 1.0)),
                            ));
                        });
                    panel.spawn((
                        TeleportStatusText,
                        Text::new(""),
                        TextFont {
                            font_size: 11.0,
                            ..default()
                        },
                        TextColor(Color::srgb(1.0, 0.45, 0.45)),
                    ));
                    panel
                        .spawn(Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: Val::Px(10.0),
                            ..default()
                        })
                        .with_children(|row| {
                            spawn_dialog_button(row, TeleportGoButton, "Ir (Enter)", true);
                            spawn_dialog_button(row, TeleportCancelButton, "Cancelar (Esc)", false);
                        });
                });
        });
}

fn spawn_dialog_button(
    parent: &mut ChildSpawnerCommands<'_>,
    marker: impl Bundle,
    label: &str,
    primary: bool,
) {
    let (bg, border) = if primary {
        (Color::srgb(0.18, 0.42, 0.62), Color::srgb(0.35, 0.65, 0.9))
    } else {
        (Color::srgb(0.12, 0.16, 0.2), Color::srgb(0.35, 0.4, 0.45))
    };
    parent
        .spawn((
            marker,
            Button,
            Node {
                padding: UiRect::axes(Val::Px(14.0), Val::Px(6.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(bg),
            BorderColor::all(border),
        ))
        .with_children(|btn| {
            btn.spawn((
                Text::new(label),
                TextFont {
                    font_size: 13.0,
                    ..default()
                },
                TextColor(Color::srgb(0.92, 0.92, 0.92)),
            ));
        });
}

pub(crate) fn toggle_teleport_dialog(
    keys: Res<ButtonInput<KeyCode>>,
    mode: Res<CameraMode>,
    mut dialog: ResMut<TeleportDialog>,
    camera: Query<(&Transform, Option<&OrbitState>), With<Camera3d>>,
) {
    if !keys.just_pressed(KeyCode::KeyG) {
        return;
    }

    dialog.open = !dialog.open;
    if dialog.open {
        let (camera_pos, orbit_focus) = camera
            .iter()
            .next()
            .map(|(transform, orbit)| (transform.translation, orbit.map(|o| o.focus)))
            .unwrap_or((Vec3::ZERO, None));
        dialog.buffer = default_buffer_for_mode(*mode, camera_pos, orbit_focus);
        dialog.status.clear();
    }
}

pub fn close_teleport_dialog(dialog: &mut TeleportDialog) {
    dialog.open = false;
    dialog.status.clear();
    reset_backspace_repeat(dialog);
}

fn append_char(buffer: &mut String, ch: char) {
    if buffer.len() < 64 {
        buffer.push(ch);
    }
}

fn backspace(buffer: &mut String) {
    buffer.pop();
}

/// How many backspace deletes to apply this frame while the key is held.
fn backspace_repeat_count(
    dt: f32,
    just_pressed: bool,
    held: bool,
    state: &mut BackspaceRepeatState,
) -> u32 {
    if just_pressed {
        state.hold_s = 0.0;
        state.repeat_delay_s = BACKSPACE_INITIAL_DELAY_S;
        return 1;
    }
    if !held {
        state.reset();
        return 0;
    }
    state.hold_s += dt;
    let mut count = 0u32;
    while state.hold_s >= state.repeat_delay_s {
        state.hold_s -= state.repeat_delay_s;
        state.repeat_delay_s = BACKSPACE_REPEAT_INTERVAL_S;
        count += 1;
    }
    count
}

fn reset_backspace_repeat(dialog: &mut TeleportDialog) {
    dialog.backspace_hold_s = 0.0;
    dialog.backspace_repeat_delay_s = BACKSPACE_INITIAL_DELAY_S;
}

fn key_char(code: KeyCode) -> Option<char> {
    match code {
        KeyCode::Digit0 | KeyCode::Numpad0 => Some('0'),
        KeyCode::Digit1 | KeyCode::Numpad1 => Some('1'),
        KeyCode::Digit2 | KeyCode::Numpad2 => Some('2'),
        KeyCode::Digit3 | KeyCode::Numpad3 => Some('3'),
        KeyCode::Digit4 | KeyCode::Numpad4 => Some('4'),
        KeyCode::Digit5 | KeyCode::Numpad5 => Some('5'),
        KeyCode::Digit6 | KeyCode::Numpad6 => Some('6'),
        KeyCode::Digit7 | KeyCode::Numpad7 => Some('7'),
        KeyCode::Digit8 | KeyCode::Numpad8 => Some('8'),
        KeyCode::Digit9 | KeyCode::Numpad9 => Some('9'),
        KeyCode::Minus | KeyCode::NumpadSubtract => Some('-'),
        KeyCode::Period | KeyCode::NumpadDecimal => Some('.'),
        KeyCode::Comma => Some(','),
        KeyCode::Space => Some(' '),
        _ => None,
    }
}

#[allow(clippy::type_complexity)]
pub fn try_submit_teleport(
    dialog: &mut TeleportDialog,
    mode: CameraMode,
    camera: &mut Query<
        (&mut Transform, &mut OrbitState),
        (With<Camera3d>, Without<crate::train::TrainMarker>),
    >,
) -> bool {
    let Some(target) = parse_coords(&dialog.buffer) else {
        dialog.status = "Formato inválido — usá x,y,z".into();
        return false;
    };

    let Ok((mut transform, mut orbit)) = camera.single_mut() else {
        dialog.status = "Cámara no disponible".into();
        return false;
    };

    apply_teleport(target, mode, &mut transform, &mut orbit);
    close_teleport_dialog(dialog);
    true
}

#[allow(clippy::type_complexity)]
pub(crate) fn teleport_input_system(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mode: Res<CameraMode>,
    mut dialog: ResMut<TeleportDialog>,
    mut camera: Query<
        (&mut Transform, &mut OrbitState),
        (With<Camera3d>, Without<crate::train::TrainMarker>),
    >,
) {
    if !dialog.open {
        return;
    }

    if keys.just_pressed(KeyCode::Escape) {
        close_teleport_dialog(&mut dialog);
        return;
    }

    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) {
        try_submit_teleport(&mut dialog, *mode, &mut camera);
        return;
    }

    let mut backspace_state = dialog.backspace_repeat_state();
    let backspace_count = backspace_repeat_count(
        time.delta_secs(),
        keys.just_pressed(KeyCode::Backspace),
        keys.pressed(KeyCode::Backspace),
        &mut backspace_state,
    );
    dialog.set_backspace_repeat_state(backspace_state);
    if backspace_count > 0 {
        for _ in 0..backspace_count {
            backspace(&mut dialog.buffer);
        }
        dialog.status.clear();
    }

    for code in keys.get_just_pressed() {
        if *code == KeyCode::Backspace {
            continue;
        }
        if let Some(ch) = key_char(*code) {
            append_char(&mut dialog.buffer, ch);
            dialog.status.clear();
        }
    }
}

#[allow(clippy::type_complexity)]
pub(crate) fn teleport_button_system(
    mode: Res<CameraMode>,
    mut dialog: ResMut<TeleportDialog>,
    mut camera: Query<
        (&mut Transform, &mut OrbitState),
        (With<Camera3d>, Without<crate::train::TrainMarker>),
    >,
    go: Query<&Interaction, (Changed<Interaction>, With<TeleportGoButton>)>,
    cancel: Query<&Interaction, (Changed<Interaction>, With<TeleportCancelButton>)>,
) {
    if !dialog.open {
        return;
    }

    for interaction in &go {
        if *interaction == Interaction::Pressed {
            try_submit_teleport(&mut dialog, *mode, &mut camera);
        }
    }

    for interaction in &cancel {
        if *interaction == Interaction::Pressed {
            close_teleport_dialog(&mut dialog);
        }
    }
}

pub(crate) fn sync_teleport_ui(
    dialog: Res<TeleportDialog>,
    mut root: Query<&mut Visibility, With<TeleportRoot>>,
    mut buffer_text: Query<&mut Text, (With<TeleportBufferText>, Without<TeleportStatusText>)>,
    mut status_text: Query<&mut Text, (With<TeleportStatusText>, Without<TeleportBufferText>)>,
) {
    for mut vis in &mut root {
        *vis = if dialog.open {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }

    if !dialog.is_changed() && !dialog.open {
        return;
    }

    for mut text in &mut buffer_text {
        let display = if dialog.buffer.is_empty() {
            "_".to_string()
        } else {
            format!("{}_", dialog.buffer)
        };
        *text = Text::new(display);
    }

    for mut text in &mut status_text {
        *text = Text::new(dialog.status.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_coords_accepts_commas_and_spaces() {
        assert_eq!(parse_coords("80,0,1"), Some(Vec3::new(80.0, 0.0, 1.0)));
        assert_eq!(parse_coords(" 80 0 1 "), Some(Vec3::new(80.0, 0.0, 1.0)));
        assert_eq!(parse_coords("80,0"), None);
    }

    #[test]
    fn default_buffer_uses_focus_in_orbit() {
        let buf = default_buffer_for_mode(
            CameraMode::Orbit,
            Vec3::new(1.0, 2.0, 3.0),
            Some(Vec3::new(10.0, 0.0, 5.0)),
        );
        assert_eq!(buf, "10,0,5");
    }

    #[test]
    fn backspace_repeat_after_hold_delay() {
        let mut state = BackspaceRepeatState {
            hold_s: 0.0,
            repeat_delay_s: BACKSPACE_INITIAL_DELAY_S,
        };
        assert_eq!(backspace_repeat_count(0.0, true, true, &mut state), 1);

        assert_eq!(backspace_repeat_count(0.2, false, true, &mut state), 0);
        let burst = backspace_repeat_count(0.2, false, true, &mut state);
        assert!(
            burst >= 1,
            "expected repeats after initial delay, got {burst}"
        );
    }
}
