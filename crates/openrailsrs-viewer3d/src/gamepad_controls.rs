//! Analog controls acquire an axis only after movement; keyboard remains usable.
use bevy::{
    input::gamepad::{GamepadAxis, GamepadButton},
    prelude::*,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct AxisBinding {
    pub axis: u8,
    pub deadzone: f32,
    pub inverted: bool,
    pub steps: u8,
    pub full_range: bool,
}
impl AxisBinding {
    pub fn valid(&self) -> bool {
        self.axis < 8
            && self.deadzone.is_finite()
            && (0. ..=0.45).contains(&self.deadzone)
            && (self.steps == 0 || (2..=41).contains(&self.steps))
    }
    pub fn normalized(self, raw: f32, direction: bool) -> f64 {
        let raw = if raw.is_finite() {
            raw.clamp(-1., 1.)
        } else {
            0.
        };
        let raw = raw * if self.inverted { -1. } else { 1. };
        let r = if raw.abs() <= self.deadzone {
            0.
        } else {
            raw.signum() * (raw.abs() - self.deadzone) / (1. - self.deadzone)
        };
        let v = if direction || self.full_range {
            (r + 1.) * 0.5
        } else {
            r.max(0.)
        };
        if self.steps >= 2 {
            (v * (self.steps - 1) as f32).round() as f64 / (self.steps - 1) as f64
        } else {
            f64::from(v)
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GamepadProfile {
    pub axes: [AxisBinding; 3],
}
impl Default for GamepadProfile {
    fn default() -> Self {
        Self {
            axes: [
                AxisBinding {
                    axis: 1,
                    deadzone: 0.1,
                    inverted: true,
                    steps: 0,
                    full_range: false,
                },
                AxisBinding {
                    axis: 3,
                    deadzone: 0.1,
                    inverted: false,
                    steps: 0,
                    full_range: false,
                },
                AxisBinding {
                    axis: 0,
                    deadzone: 0.25,
                    inverted: false,
                    steps: 3,
                    full_range: true,
                },
            ],
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GamepadPreferences {
    pub enabled: bool,
    pub device: Option<String>,
    pub selected: String,
    pub profiles: BTreeMap<String, GamepadProfile>,
}
impl Default for GamepadPreferences {
    fn default() -> Self {
        let normal = GamepadProfile::default();
        let mut notched = normal.clone();
        notched.axes[0].steps = 11;
        notched.axes[1].steps = 21;
        Self {
            enabled: false,
            device: None,
            selected: "Mando general".into(),
            profiles: BTreeMap::from([
                ("Mando general".into(), normal),
                ("Palancas con pasos".into(), notched),
            ]),
        }
    }
}
impl GamepadPreferences {
    pub fn valid(&self) -> bool {
        self.profiles.len() <= 16
            && !self.profiles.is_empty()
            && self.profiles.contains_key(&self.selected)
            && self
                .device
                .as_ref()
                .is_none_or(|d| d.len() <= 128 && !d.chars().any(char::is_control))
            && self.profiles.iter().all(|(name, p)| {
                !name.is_empty()
                    && name.len() <= 128
                    && p.axes.iter().all(|a| a.valid())
                    && p.axes
                        .iter()
                        .map(|a| a.axis)
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        == 3
            })
    }
    pub fn edit(&mut self, index: usize, field: u8, delta: f32) -> Result<(), String> {
        let mut candidate = self.clone();
        let axis = candidate
            .profiles
            .get_mut(&self.selected)
            .and_then(|p| p.axes.get_mut(index))
            .ok_or("Eje inexistente")?;
        match field {
            0 => axis.axis = (i32::from(axis.axis) + delta as i32).rem_euclid(8) as u8,
            1 => axis.deadzone = (axis.deadzone + delta).clamp(0., 0.45),
            2 => axis.inverted = !axis.inverted,
            3 => {
                axis.steps = match axis.steps {
                    0 => 11,
                    11 => 21,
                    21 => 3,
                    _ => 0,
                }
            }
            4 => axis.full_range = !axis.full_range,
            _ => return Err("Campo inexistente".into()),
        }
        if !candidate.valid() {
            return Err("Cada eje debe cumplir una sola función; elegí otro eje".into());
        }
        *self = candidate;
        Ok(())
    }
}
#[derive(Resource, Default)]
pub struct AnalogControls {
    device: Option<Entity>,
    profile: String,
    last: [Option<f32>; 3],
    owns_throttle: bool,
    owns_horn: bool,
    pub status: String,
}
const AXES: [GamepadAxis; 6] = [
    GamepadAxis::LeftStickX,
    GamepadAxis::LeftStickY,
    GamepadAxis::RightStickX,
    GamepadAxis::RightStickY,
    GamepadAxis::LeftZ,
    GamepadAxis::RightZ,
];
pub fn update(
    pads: Query<(Entity, &Name, &Gamepad)>,
    mut controls: ResMut<AnalogControls>,
    settings: Res<crate::player_settings::PlayerSettings>,
    mut live: Option<ResMut<crate::live::LiveDrive>>,
    mut ui: ResMut<crate::player_ui::PlayerUiState>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    let mut names = pads
        .iter()
        .map(|(_, n, _)| {
            n.as_str()
                .chars()
                .filter(|c| !c.is_control())
                .take(128)
                .collect::<String>()
        })
        .take(32)
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    if ui.gamepad_devices != names {
        ui.gamepad_devices = names;
        ui.rebuild = true;
    }
    let Some(live) = live.as_mut() else {
        return;
    };
    let pad = if settings.gamepad.enabled {
        pads.iter()
            .filter(|(_, n, _)| {
                settings
                    .gamepad
                    .device
                    .as_ref()
                    .is_none_or(|d| d == n.as_str())
            })
            .min_by_key(|(e, _, _)| e.to_bits())
    } else {
        None
    };
    use crate::player_settings::PlayerAction as A;
    if settings.just_pressed(&keys, A::ThrottleUp) || settings.just_pressed(&keys, A::ThrottleDown)
    {
        controls.owns_throttle = false;
    }
    if settings.just_pressed(&keys, A::Horn) {
        controls.owns_horn = false;
    }
    let active = pad.map(|(e, _, _)| e);
    let profile = serde_json::to_string(&settings.gamepad).unwrap_or_default();
    if active != controls.device || profile != controls.profile {
        if controls.owns_throttle {
            live.session.driver_throttle = 0.;
        }
        if controls.owns_horn {
            live.session.stop_horn();
        }
        controls.owns_throttle = false;
        controls.owns_horn = false;
        controls.last = [None; 3];
        controls.device = active;
        controls.profile = profile;
    }
    let Some((_, name, pad)) = pad else {
        controls.status = "Sin mando activo; teclado disponible".into();
        return;
    };
    controls.status = format!(
        "{} · {} · mover un eje toma su control",
        name, settings.gamepad.selected
    );
    if live.paused || ui.panel != crate::player_ui::PlayerPanel::None {
        if controls.owns_horn {
            live.session.stop_horn();
            controls.owns_horn = false;
        }
        controls.last = [None; 3];
        return;
    }
    let Some(profile) = settings.gamepad.profiles.get(&settings.gamepad.selected) else {
        return;
    };
    for (index, binding) in profile.axes.iter().enumerate() {
        let raw = if binding.axis < 6 {
            pad.get(AXES[binding.axis as usize]).unwrap_or(0.)
        } else {
            let button = if binding.axis == 6 {
                GamepadButton::LeftTrigger2
            } else {
                GamepadButton::RightTrigger2
            };
            pad.get(button).unwrap_or(0.).clamp(0., 1.)
        };
        let moved = controls.last[index].is_some_and(|old| (raw - old).abs() > 0.005);
        controls.last[index] = Some(raw);
        if !moved {
            continue;
        }
        let value = binding.normalized(raw, index == 2);
        let keyboard = match index {
            0 => {
                settings.just_pressed(&keys, A::ThrottleUp)
                    || settings.just_pressed(&keys, A::ThrottleDown)
            }
            1 => {
                settings.just_pressed(&keys, A::BrakeUp)
                    || settings.just_pressed(&keys, A::BrakeDown)
            }
            _ => {
                settings.just_pressed(&keys, A::Forward) || settings.just_pressed(&keys, A::Reverse)
            }
        };
        if keyboard {
            continue;
        }
        match index {
            0 => {
                live.session.driver_throttle = value;
                controls.owns_throttle = true;
            }
            1 => live.session.driver_brake = value,
            _ => {
                let direction = if value < 0.25 {
                    0.
                } else if value > 0.75 {
                    1.
                } else {
                    0.5
                };
                let _ = live.session.set_direction(direction);
            }
        }
    }
    if pad.pressed(GamepadButton::South) {
        live.session.trigger_horn(0.08);
        controls.owns_horn = true;
    } else if controls.owns_horn {
        live.session.stop_horn();
        controls.owns_horn = false;
    }
    if pad.just_pressed(GamepadButton::East) {
        live.session.state.brake_system.emergency = true;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profiles_bound_axes_and_center_neutral() {
        let mut prefs = GamepadPreferences::default();
        assert!(prefs.valid());
        let profile = &prefs.profiles[&prefs.selected];
        assert_eq!(profile.axes[0].normalized(0., false), 0.);
        assert_eq!(profile.axes[0].normalized(-1., false), 1.);
        assert_eq!(profile.axes[2].normalized(0.05, true), 0.5);
        assert_eq!(profile.axes[2].normalized(-1., true), 0.);
        assert!(prefs.edit(0, 0, -1.).is_err());
        assert!(prefs.edit(0, 1, 0.1).is_ok());
    }
}

/// Keep Bevy's axis filter neutral; the selected per-control profile owns calibration.
pub fn configure(
    settings: Res<crate::player_settings::PlayerSettings>,
    mut pads: Query<&mut bevy::input::gamepad::GamepadSettings>,
) {
    if !settings.gamepad.enabled {
        return;
    }
    let neutral = bevy::input::gamepad::AxisSettings::new(-1., 0., 0., 1., 0.001).unwrap();
    for mut pad in &mut pads {
        if pad.default_axis_settings != neutral {
            pad.default_axis_settings = neutral.clone();
            pad.axis_settings.clear();
        }
    }
}
#[cfg(test)]
mod integration_tests {
    use super::*;
    #[test]
    fn disconnect_and_pause_release_owned_commands_without_overwriting_keyboard() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/traction_operation/scenario.toml");
        let mut app = App::new();
        let mut settings = crate::player_settings::PlayerSettings::default();
        settings.gamepad.enabled = true;
        app.insert_resource(crate::live::LiveDrive::from_scenario_path(&path).unwrap())
            .insert_resource(settings)
            .init_resource::<crate::player_ui::PlayerUiState>()
            .init_resource::<AnalogControls>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, update);
        let entity = app
            .world_mut()
            .spawn((
                Name::new("Virtual acceptance controller"),
                Gamepad::default(),
            ))
            .id();
        app.update();
        assert_eq!(
            app.world()
                .resource::<crate::live::LiveDrive>()
                .session
                .driver_throttle,
            0.
        );
        app.world_mut()
            .get_mut::<Gamepad>(entity)
            .unwrap()
            .analog_mut()
            .set(GamepadAxis::LeftStickY, -0.7);
        app.update();
        assert!(
            app.world()
                .resource::<crate::live::LiveDrive>()
                .session
                .driver_throttle
                > 0.5
        );
        // No axis motion: keyboard commands retain control.
        app.world_mut()
            .resource_mut::<crate::live::LiveDrive>()
            .session
            .driver_throttle = 0.2;
        app.update();
        assert_eq!(
            app.world()
                .resource::<crate::live::LiveDrive>()
                .session
                .driver_throttle,
            0.2
        );
        app.world_mut().despawn(entity);
        app.update();
        assert_eq!(
            app.world()
                .resource::<crate::live::LiveDrive>()
                .session
                .driver_throttle,
            0.
        );
        assert!(
            app.world()
                .resource::<AnalogControls>()
                .status
                .contains("Sin mando")
        );
    }
}
#[cfg(test)]
mod keyboard_acceptance {
    use super::*;
    #[test]
    fn keyboard_reclaims_owned_throttle_before_device_disconnect() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/traction_operation/scenario.toml");
        let mut app = App::new();
        let mut settings = crate::player_settings::PlayerSettings::default();
        settings.gamepad.enabled = true;
        app.insert_resource(crate::live::LiveDrive::from_scenario_path(&path).unwrap())
            .insert_resource(settings)
            .init_resource::<crate::player_ui::PlayerUiState>()
            .init_resource::<AnalogControls>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, update);
        let device = app
            .world_mut()
            .spawn((Name::new("Virtual acceptance"), Gamepad::default()))
            .id();
        app.update();
        app.world_mut()
            .get_mut::<Gamepad>(device)
            .unwrap()
            .analog_mut()
            .set(GamepadAxis::LeftStickY, -0.6);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyA);
        app.world_mut()
            .resource_mut::<crate::live::LiveDrive>()
            .session
            .driver_throttle = 0.3;
        app.update();
        app.world_mut().despawn(device);
        app.update();
        assert_eq!(
            app.world()
                .resource::<crate::live::LiveDrive>()
                .session
                .driver_throttle,
            0.3
        );
    }
}
