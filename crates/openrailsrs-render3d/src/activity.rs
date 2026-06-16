//! Actividad MSTS (`.act`): estación, hora de inicio y modo noche para el render.

use std::path::{Path, PathBuf};

use bevy::prelude::{Color, Quat, Resource};
use openrailsrs_formats::ActivityFile;

use crate::textures::{Season, TextureEnvironment};

/// Sesión visual cargada desde un `.act` (sin simular tren/AI).
#[derive(Clone, Debug, Resource)]
pub struct ActivitySession {
    pub name: String,
    pub path: PathBuf,
    pub start_time_s: f64,
    pub season: Season,
    pub night: bool,
    /// Path relativo al consist del jugador (`Player_Train_Init_Cons`).
    pub player_consist: String,
    #[allow(dead_code)]
    pub player_path: String,
}

impl ActivitySession {
    pub fn start_time_hms(&self) -> (u32, u32, u32) {
        let t = self.start_time_s.max(0.0) as u32;
        let h = (t / 3600) % 24;
        let m = (t / 60) % 60;
        let s = t % 60;
        (h, m, s)
    }
}

/// Resuelve y parsea un `.act` bajo la ruta MSTS.
pub fn load_activity_session(route_dir: &Path, activity: &Path) -> Option<ActivitySession> {
    let path = resolve_activity_path(route_dir, activity)?;
    let file = ActivityFile::from_path(&path).ok()?;
    let start_time_s = file.start_time_s;
    let season = file
        .season
        .as_deref()
        .map(Season::parse)
        .unwrap_or(Season::Summer);
    Some(ActivitySession {
        name: if file.name.is_empty() {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("activity")
                .to_string()
        } else {
            file.name
        },
        path,
        start_time_s,
        season,
        night: is_night_time(start_time_s),
        player_consist: file.player_consist,
        player_path: file.player_path,
    })
}

/// Combina `.act` + overrides CLI en el entorno de texturas.
pub fn build_texture_environment(
    session: Option<&ActivitySession>,
    season_override: Option<&str>,
    weather_override: Option<&str>,
    night_override: Option<bool>,
) -> TextureEnvironment {
    let season = season_override
        .map(Season::parse)
        .or_else(|| session.map(|s| s.season))
        .unwrap_or(Season::Summer);
    let snow_weather = weather_override.is_some_and(|w| w.eq_ignore_ascii_case("snow"));
    let night = night_override.unwrap_or_else(|| session.is_some_and(|s| s.night));
    TextureEnvironment {
        season,
        snow_weather,
        night,
    }
}

/// Día civil aproximado: noche fuera de 06:00–20:00 (paridad visual MSTS/OR).
pub fn is_night_time(start_time_s: f64) -> bool {
    const DAY_START: f64 = 6.0 * 3600.0;
    const DAY_END: f64 = 20.0 * 3600.0;
    !(DAY_START..DAY_END).contains(&start_time_s)
}

/// Rotación del sol direccional según hora MSTS (`StartTime`).
pub fn sun_transform(start_time_s: f64, night: bool) -> (Quat, f32, Color, Color) {
    use std::f32::consts::{FRAC_PI_4, PI};

    if night {
        return (
            Quat::from_rotation_x(-PI * 0.85),
            800.0,
            Color::srgb(0.75, 0.78, 0.95),
            Color::srgb(0.08, 0.10, 0.18),
        );
    }

    let hour = (start_time_s / 3600.0).rem_euclid(24.0) as f32;
    let daylight = ((hour - 6.0) / 14.0).clamp(0.0, 1.0);
    let pitch = -0.25 - daylight * 1.05;
    let yaw = FRAC_PI_4 + (hour - 12.0) * 0.04;
    let rotation = Quat::from_euler(bevy::math::EulerRot::YXZ, yaw, pitch, 0.0);
    let illuminance = 4_000.0 + daylight * 8_000.0;
    let sun_color = Color::srgb(1.0, 0.96 + daylight * 0.02, 0.88 + daylight * 0.08);
    let ambient = Color::srgb(0.45 + daylight * 0.15, 0.52 + daylight * 0.18, 0.65);
    (rotation, illuminance, sun_color, ambient)
}

pub fn resolve_activity_path(route_dir: &Path, activity: &Path) -> Option<PathBuf> {
    if activity.is_file() {
        return Some(activity.to_path_buf());
    }
    if activity.is_absolute() && !activity.exists() {
        return None;
    }
    let file_name = activity.file_name()?;
    for base in [route_dir.to_path_buf()] {
        let direct = base.join(activity);
        if direct.is_file() {
            return Some(direct);
        }
        for sub in ["ACTIVITIES", "activities"] {
            let candidate = base.join(sub).join(file_name);
            if candidate.is_file() {
                return Some(candidate);
            }
            let nested = base.join(sub).join(activity);
            if nested.is_file() {
                return Some(nested);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../openrailsrs-msts/tests/fixtures")
    }

    #[test]
    fn minimal_act_start_time_is_day() {
        let path = fixtures_dir().join("minimal.act");
        let session = load_activity_session(&fixtures_dir(), &path).expect("load");
        assert_eq!(session.start_time_s, 8.0 * 3600.0);
        assert!(!session.night);
        assert_eq!(session.season, Season::Summer);
    }

    #[test]
    fn traffic_act_has_season_summer() {
        let dir = fixtures_dir().join("with_traffic");
        let path = dir.join("traffic.act");
        let session = load_activity_session(&dir, &path).expect("load");
        assert_eq!(session.season, Season::Summer);
        assert_eq!(session.name, "Traffic Test");
    }

    #[test]
    fn night_time_detection() {
        assert!(!is_night_time(8.0 * 3600.0));
        assert!(is_night_time(22.0 * 3600.0));
        assert!(is_night_time(5.0 * 3600.0));
    }

    #[test]
    fn cli_overrides_act_season() {
        let dir = fixtures_dir().join("with_traffic");
        let session = load_activity_session(&dir, Path::new("traffic.act")).unwrap();
        let env = build_texture_environment(Some(&session), Some("winter"), None, None);
        assert_eq!(env.season, Season::Winter);
        assert!(!env.night);
    }

    #[test]
    fn act_drives_night_without_override() {
        let session = ActivitySession {
            name: "night".into(),
            path: PathBuf::from("x.act"),
            start_time_s: 23.0 * 3600.0,
            season: Season::Summer,
            night: true,
            player_consist: String::new(),
            player_path: String::new(),
        };
        let env = build_texture_environment(Some(&session), None, None, None);
        assert!(env.night);
    }
}
