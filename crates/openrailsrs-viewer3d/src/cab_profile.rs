//! Comfort adjustments are keyed by original CVF, preserving each authored eye.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct CabProfile {
    pub seat_height_m: f32,
    pub seat_back_m: f32,
    pub wipe_scale: f32,
    pub look_yaw: f32,
    pub look_pitch: f32,
    pub instrument_light: f32,
    pub console_light: f32,
    pub interior_light: f32,
}
impl Default for CabProfile {
    fn default() -> Self {
        Self {
            seat_height_m: 0.0,
            seat_back_m: 0.0,
            wipe_scale: 1.0,
            look_yaw: 0.0,
            look_pitch: 0.0,
            instrument_light: 1.0,
            console_light: 1.0,
            interior_light: 1.0,
        }
    }
}

/// UV rectangle of the native windshield, including CVF letterboxing.
pub fn window_uv(view: &openrailsrs_formats::CabView, screen: Vec2) -> Vec4 {
    let scale = (screen.x / 640.0).min(screen.y / 480.0);
    let origin = (screen - Vec2::new(640.0, 480.0) * scale) * 0.5;
    let r = &view.window;
    if r.width <= 0.0 || r.height <= 0.0 {
        return Vec4::new(0.0, 0.0, 1.0, 1.0);
    }
    Vec4::new(
        (origin.x + r.x as f32 * scale) / screen.x,
        (origin.y + r.y as f32 * scale) / screen.y,
        r.width as f32 * scale / screen.x,
        r.height as f32 * scale / screen.y,
    )
}

pub fn blade_count(runtime: &crate::cab_cvf::CabCvfRuntime, path: &std::path::Path) -> usize {
    let named = runtime
        .shape
        .matrices
        .iter()
        .filter(|m| m.name.to_ascii_uppercase().contains("WIPER"))
        .count();
    if named > 0 {
        return named.min(2);
    }
    let name = path.to_string_lossy().to_ascii_lowercase();
    // Classic single-pane steam/railcar cabs have one blade. CVF has no blade
    // geometry; named 3D bones take precedence and unknown cabs use two blades.
    let steam = runtime
        .cvf
        .controls
        .iter()
        .any(|control| match control.control_type() {
            Some(openrailsrs_formats::ControlType::Generic(name)) => {
                name.to_ascii_uppercase().contains("BOILER")
            }
            _ => false,
        });
    if steam
        || ["hall", "king", "121", "rstock", "r-stock", "r_stock"]
            .iter()
            .any(|s| name.contains(s))
    {
        1
    } else {
        2
    }
}

/// Portable authored identity below TRAINSET; CVFs shared by models share preferences.
pub fn model_key(path: &std::path::Path) -> String {
    let parts: Vec<_> = path
        .components()
        .filter_map(|c| {
            if let std::path::Component::Normal(p) = c {
                Some(p.to_string_lossy().to_ascii_lowercase())
            } else {
                None
            }
        })
        .collect();
    let start = parts
        .iter()
        .rposition(|p| p == "trainset")
        .map_or(parts.len().saturating_sub(3), |i| i + 1);
    format!("cab:{}", parts[start..].join("/"))
}

/// Persist view direction after a short idle period. Loading a saved game takes
/// precedence over a model preference. No filesystem read belongs to this loop.
pub fn remember_view(
    cvf: Res<crate::cab_cvf::CabCvfState>,
    follow: Res<crate::camera::CameraFollowMode>,
    mut look: ResMut<crate::camera::DriverLookOffset>,
    mut settings: ResMut<crate::player_settings::PlayerSettings>,
    time: Res<Time>,
    saved: Option<Res<crate::saved_game::PendingSavedCamera>>,
    mut memo: Local<Option<(String, [f32; 2], f64, bool)>>,
) {
    if !matches!(
        *follow,
        crate::camera::CameraFollowMode::DriverCam | crate::camera::CameraFollowMode::Cab2d
    ) {
        return;
    }
    let Some(path) = &cvf.cvf_path else {
        return;
    };
    let key = model_key(path);
    let now = time.elapsed_secs_f64();
    if memo.as_ref().is_none_or(|m| m.0 != key) {
        let profile = settings.cab_profile(path);
        if saved.is_none() {
            look.yaw = profile.look_yaw;
            look.pitch = profile.look_pitch;
        }
        *memo = Some((key, [look.yaw, look.pitch], now, false));
        return;
    }
    let (_, angles, last, dirty) = memo.as_mut().unwrap();
    let current = [look.yaw, look.pitch];
    if *angles != current {
        *angles = current;
        *last = now;
        *dirty = true;
        let profile = settings.cab_profile_mut(path);
        profile.look_yaw = look.yaw;
        profile.look_pitch = look.pitch;
    } else if *dirty && now - *last >= 1.0 {
        let target = crate::player_settings::player_data_dir().join("settings.json");
        let mut persisted =
            crate::player_settings::PlayerSettings::load(&target).unwrap_or_default();
        persisted.cab_profiles.clone_from(&settings.cab_profiles);
        if let Err(e) = persisted.save(&target) {
            bevy::log::warn!("Could not save cab view: {e}");
        }
        *dirty = false;
    }
}
impl crate::player_settings::PlayerSettings {
    pub(crate) fn cab_profile(&self, path: &std::path::Path) -> CabProfile {
        self.cab_profiles
            .get(&model_key(path))
            .or_else(|| self.cab_profiles.get(path.to_string_lossy().as_ref()))
            .copied()
            .unwrap_or_default()
    }
    pub(crate) fn cab_profile_mut(&mut self, path: &std::path::Path) -> &mut CabProfile {
        let key = model_key(path);
        if !self.cab_profiles.contains_key(&key) {
            let old = self
                .cab_profiles
                .remove(path.to_string_lossy().as_ref())
                .unwrap_or_default();
            self.cab_profiles.insert(key.clone(), old);
        }
        self.cab_profiles.get_mut(&key).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windshield_rectangle_tracks_native_panel_letterboxing() {
        let v = openrailsrs_formats::CabView {
            texture_ace: String::new(),
            window: openrailsrs_formats::ScreenRect {
                x: 64.0,
                y: 48.0,
                width: 512.0,
                height: 288.0,
            },
            position_m: [0.0; 3],
            direction_deg: [0.0; 3],
        };
        let a = window_uv(&v, Vec2::new(640.0, 480.0));
        assert!(a.abs_diff_eq(Vec4::new(0.1, 0.1, 0.8, 0.6), 1e-6));
        let b = window_uv(&v, Vec2::new(1280.0, 720.0));
        assert!(b.abs_diff_eq(Vec4::new(0.2, 0.1, 0.6, 0.6), 1e-6));
    }

    #[test]
    fn plan30_model_preferences_survive_relocation_and_migrate_legacy_keys() {
        use std::path::Path;
        let a = Path::new("/old/TRAINS/TRAINSET/Model/CABVIEW/driver.cvf");
        let b = Path::new("/new/TRAINS/TRAINSET/model/cabview/driver.cvf");
        let mut settings = crate::player_settings::PlayerSettings::default();
        settings.cab_profiles.insert(
            a.to_string_lossy().into(),
            CabProfile {
                seat_height_m: 0.1,
                look_pitch: 0.2,
                ..default()
            },
        );
        assert_eq!(settings.cab_profile_mut(a).seat_height_m, 0.1);
        assert_eq!(settings.cab_profile(b).look_pitch, 0.2);
        assert!(
            !settings
                .cab_profiles
                .contains_key(a.to_string_lossy().as_ref())
        );
        settings.cab_profile_mut(b).look_pitch = 2.;
        assert!(settings.validate().is_err());
        *settings.cab_profile_mut(b) = default();
        assert!(settings.validate().is_ok());
    }
}
