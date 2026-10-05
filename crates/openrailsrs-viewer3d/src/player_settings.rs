//! Persisted player preferences and a single, validated key map.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FogQuality {
    #[default]
    Distance,
    Volumetric32,
    Volumetric64,
}
impl FogQuality {
    pub fn next(self) -> Self {
        match self {
            Self::Distance => Self::Volumetric32,
            Self::Volumetric32 => Self::Volumetric64,
            Self::Volumetric64 => Self::Distance,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Distance => "Atmosférica (liviana)",
            Self::Volumetric32 => "Volumétrica · 32 pasos",
            Self::Volumetric64 => "Volumétrica · 64 pasos",
        }
    }
    pub fn steps(self) -> Option<u32> {
        match self {
            Self::Distance => None,
            Self::Volumetric32 => Some(32),
            Self::Volumetric64 => Some(64),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerAction {
    ThrottleUp,
    ThrottleDown,
    BrakeUp,
    BrakeDown,
    Forward,
    Reverse,
    Neutral,
    Horn,
    Wiper,
    Headlights,
    CabLight,
    Doors,
    Emergency,
    Pause,
    Reset,
    Faster,
    Slower,
    TrackMonitor,
    DrivingHud,
    Help,
    Notebook,
    AdvancedHud,
    Formation,
    Map,
    Settings,
    OrbitCamera,
    FlyCamera,
    DriverCamera,
    ExteriorCamera,
    OrbitView,
    CabPanel,
    DebugHud,
}

impl PlayerAction {
    pub const ALL: [Self; 32] = [
        Self::ThrottleUp,
        Self::ThrottleDown,
        Self::BrakeUp,
        Self::BrakeDown,
        Self::Forward,
        Self::Reverse,
        Self::Neutral,
        Self::Horn,
        Self::Wiper,
        Self::Headlights,
        Self::CabLight,
        Self::Doors,
        Self::Emergency,
        Self::Pause,
        Self::Reset,
        Self::Faster,
        Self::Slower,
        Self::TrackMonitor,
        Self::DrivingHud,
        Self::Help,
        Self::Notebook,
        Self::AdvancedHud,
        Self::Formation,
        Self::Map,
        Self::Settings,
        Self::OrbitCamera,
        Self::FlyCamera,
        Self::DriverCamera,
        Self::ExteriorCamera,
        Self::OrbitView,
        Self::CabPanel,
        Self::DebugHud,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::ThrottleUp => "Subir regulador",
            Self::ThrottleDown => "Bajar regulador",
            Self::BrakeUp => "Aplicar freno",
            Self::BrakeDown => "Soltar freno",
            Self::Forward => "Inversor adelante",
            Self::Reverse => "Inversor atrás",
            Self::Neutral => "Inversor neutro",
            Self::Horn => "Bocina",
            Self::Wiper => "Limpiaparabrisas",
            Self::Headlights => "Faros: apagados / bajos / altos",
            Self::CabLight => "Luz de cabina",
            Self::Doors => "Puertas",
            Self::Emergency => "Emergencia",
            Self::Pause => "Pausar / continuar",
            Self::Reset => "Reiniciar servicio",
            Self::Faster => "Acelerar tiempo",
            Self::Slower => "Reducir tiempo",
            Self::TrackMonitor => "Monitor de vía",
            Self::DrivingHud => "HUD de conducción",
            Self::Help => "Ayuda",
            Self::Notebook => "Libreta del servicio",
            Self::AdvancedHud => "HUD avanzado",
            Self::Formation => "Operaciones de formación",
            Self::Map => "Mapa / despachador",
            Self::Settings => "Ajustes",
            Self::OrbitCamera => "Cámara orbital",
            Self::FlyCamera => "Cámara libre",
            Self::DriverCamera => "Cabina (Alt cambia 2D/3D)",
            Self::ExteriorCamera => "Vista exterior",
            Self::OrbitView => "Seguir en órbita",
            Self::CabPanel => "Panel digital de cabina",
            Self::DebugHud => "Diagnóstico",
        }
    }
    fn default_key(self) -> KeyCode {
        match self {
            Self::ThrottleUp => KeyCode::KeyD,
            Self::ThrottleDown => KeyCode::KeyA,
            Self::BrakeUp => KeyCode::Quote,
            Self::BrakeDown => KeyCode::Semicolon,
            Self::Forward => KeyCode::KeyW,
            Self::Reverse => KeyCode::KeyS,
            Self::Neutral => KeyCode::Backslash,
            Self::Horn => KeyCode::Space,
            Self::Wiper => KeyCode::KeyV,
            Self::Headlights => KeyCode::KeyH,
            Self::CabLight => KeyCode::KeyI,
            Self::Doors => KeyCode::KeyQ,
            Self::Emergency => KeyCode::Backspace,
            Self::Pause => KeyCode::KeyP,
            Self::Reset => KeyCode::KeyR,
            Self::Faster => KeyCode::Equal,
            Self::Slower => KeyCode::Minus,
            Self::TrackMonitor => KeyCode::F4,
            Self::DrivingHud => KeyCode::F5,
            Self::Help => KeyCode::F6,
            Self::Notebook => KeyCode::F7,
            Self::AdvancedHud => KeyCode::F8,
            Self::Formation => KeyCode::F9,
            Self::Map => KeyCode::KeyM,
            Self::Settings => KeyCode::F10,
            Self::OrbitCamera => KeyCode::F1,
            Self::FlyCamera => KeyCode::F2,
            Self::DriverCamera => KeyCode::Digit1,
            Self::ExteriorCamera => KeyCode::Digit2,
            Self::OrbitView => KeyCode::Digit3,
            Self::CabPanel => KeyCode::KeyC,
            Self::DebugHud => KeyCode::F3,
        }
    }
}

#[derive(Resource, Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerSettings {
    pub view_distance_m: f32,
    pub cab_fov_deg: f32,
    pub shadows: bool,
    pub fog: bool,
    pub fog_quality: FogQuality,
    pub weather_execution: crate::weather_execution::WeatherExecution,
    pub renderer: crate::weather_execution::RendererSelection,
    pub environment: crate::environment::EnvironmentSelection,
    pub lightning: bool,
    pub ui_scale: f32,
    pub mph: bool,
    pub audio_enabled: bool,
    pub audio_volume: f32,
    /// Short boarding and no booked-departure wait; distinct from normal service.
    pub quick_station_practice: bool,
    pub cab_profiles: BTreeMap<String, crate::cab_profile::CabProfile>,
    pub keys: BTreeMap<PlayerAction, String>,
}

impl Default for PlayerSettings {
    fn default() -> Self {
        Self {
            view_distance_m: 2000.0,
            cab_fov_deg: 60.0,
            shadows: true,
            fog: true,
            fog_quality: FogQuality::Distance,
            weather_execution: crate::weather_execution::WeatherExecution::Auto,
            renderer: crate::weather_execution::RendererSelection::Auto,
            environment: default(),
            lightning: true,
            ui_scale: 1.0,
            mph: false,
            audio_enabled: true,
            audio_volume: 0.4,
            quick_station_practice: false,
            cab_profiles: BTreeMap::new(),
            keys: PlayerAction::ALL
                .into_iter()
                .map(|a| (a, format!("{:?}", a.default_key())))
                .collect(),
        }
    }
}

impl PlayerSettings {
    pub fn key(&self, action: PlayerAction) -> KeyCode {
        self.keys
            .get(&action)
            .and_then(|s| parse_key(s))
            .unwrap_or_else(|| action.default_key())
    }
    pub fn just_pressed(&self, keys: &ButtonInput<KeyCode>, action: PlayerAction) -> bool {
        keys.just_pressed(self.key(action))
    }
    pub fn key_label(&self, action: PlayerAction) -> String {
        key_label(self.key(action))
    }
    pub fn rebind(&mut self, action: PlayerAction, key: KeyCode) -> Result<(), String> {
        if parse_key(&format!("{key:?}")).is_none() || reserved_key(key) {
            return Err("Tecla reservada para navegación, cámara o edición".into());
        }
        if let Some(other) = PlayerAction::ALL
            .into_iter()
            .find(|a| *a != action && self.key(*a) == key)
        {
            return Err(format!("{} ya usa {}", other.label(), key_label(key)));
        }
        self.keys.insert(action, format!("{key:?}"));
        Ok(())
    }
    pub fn validate(&self) -> Result<(), String> {
        if !self.view_distance_m.is_finite()
            || !(500.0..=4000.0).contains(&self.view_distance_m)
            || !self.cab_fov_deg.is_finite()
            || !(35.0..=90.0).contains(&self.cab_fov_deg)
            || !self.ui_scale.is_finite()
            || !(0.8..=1.5).contains(&self.ui_scale)
            || !self.audio_volume.is_finite()
            || !(0.0..=1.0).contains(&self.audio_volume)
        {
            return Err("Ajustes gráficos fuera del rango permitido".into());
        }
        if self.cab_profiles.values().any(|p| {
            !p.seat_height_m.is_finite()
                || !(-0.4..=0.4).contains(&p.seat_height_m)
                || !p.seat_back_m.is_finite()
                || !(-0.4..=0.4).contains(&p.seat_back_m)
                || !p.wipe_scale.is_finite()
                || !(0.6..=1.4).contains(&p.wipe_scale)
        }) {
            return Err("Ajustes de cabina fuera del rango permitido".into());
        }
        let mut used = BTreeMap::new();
        for action in PlayerAction::ALL {
            let name = self
                .keys
                .get(&action)
                .ok_or("Falta una asignación de controles")?;
            let key = parse_key(name).ok_or("Tecla desconocida")?;
            if reserved_key(key) || used.insert(name, action).is_some() {
                return Err("Controles repetidos o reservados".into());
            }
        }
        Ok(())
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = std::fs::read(path).map_err(|e| e.to_string())?;
        let mut s: Self = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
        // Upgrade old preference files without losing the user's key assignments.
        for action in PlayerAction::ALL {
            if s.keys.contains_key(&action) {
                continue;
            }
            let preferred = format!("{:?}", action.default_key());
            let key = std::iter::once(preferred)
                .chain(('A'..='Z').map(|c| format!("Key{c}")))
                .find(|name| {
                    !s.keys.values().any(|used| used == name)
                        && parse_key(name).is_some_and(|key| !reserved_key(key))
                })
                .ok_or("No queda una tecla libre para el control nuevo")?;
            s.keys.insert(action, key);
        }
        s.validate()?;
        Ok(s)
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        atomic_write(
            path,
            &serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
        )
    }
}

/// Fixed navigation remains exclusive even after rebinding driving commands.
fn reserved_key(key: KeyCode) -> bool {
    matches!(
        key,
        KeyCode::Escape
            | KeyCode::Enter
            | KeyCode::Tab
            | KeyCode::Delete
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::ArrowUp
            | KeyCode::ArrowDown
            | KeyCode::ArrowLeft
            | KeyCode::ArrowRight
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::ShiftLeft
            | KeyCode::ShiftRight
            | KeyCode::AltLeft
            | KeyCode::AltRight
            | KeyCode::ControlLeft
            | KeyCode::ControlRight
            | KeyCode::BracketLeft
            | KeyCode::BracketRight
            | KeyCode::Comma
            | KeyCode::Period
            | KeyCode::KeyF
            | KeyCode::KeyG
            | KeyCode::KeyT
            | KeyCode::Pause
            | KeyCode::Digit5
            | KeyCode::Digit8
            | KeyCode::F11
            | KeyCode::F12
    )
}

pub fn parse_key(s: &str) -> Option<KeyCode> {
    // String representation deliberately avoids enabling serde for the entire engine.
    Some(match s {
        "KeyA" => KeyCode::KeyA,
        "KeyB" => KeyCode::KeyB,
        "KeyC" => KeyCode::KeyC,
        "KeyD" => KeyCode::KeyD,
        "KeyE" => KeyCode::KeyE,
        "KeyF" => KeyCode::KeyF,
        "KeyG" => KeyCode::KeyG,
        "KeyH" => KeyCode::KeyH,
        "KeyI" => KeyCode::KeyI,
        "KeyJ" => KeyCode::KeyJ,
        "KeyK" => KeyCode::KeyK,
        "KeyL" => KeyCode::KeyL,
        "KeyM" => KeyCode::KeyM,
        "KeyN" => KeyCode::KeyN,
        "KeyO" => KeyCode::KeyO,
        "KeyP" => KeyCode::KeyP,
        "KeyQ" => KeyCode::KeyQ,
        "KeyR" => KeyCode::KeyR,
        "KeyS" => KeyCode::KeyS,
        "KeyT" => KeyCode::KeyT,
        "KeyU" => KeyCode::KeyU,
        "KeyV" => KeyCode::KeyV,
        "KeyW" => KeyCode::KeyW,
        "KeyX" => KeyCode::KeyX,
        "KeyY" => KeyCode::KeyY,
        "KeyZ" => KeyCode::KeyZ,
        "Digit0" => KeyCode::Digit0,
        "Digit1" => KeyCode::Digit1,
        "Digit2" => KeyCode::Digit2,
        "Digit3" => KeyCode::Digit3,
        "Digit4" => KeyCode::Digit4,
        "Digit6" => KeyCode::Digit6,
        "Digit7" => KeyCode::Digit7,
        "Digit9" => KeyCode::Digit9,
        "F1" => KeyCode::F1,
        "F2" => KeyCode::F2,
        "F3" => KeyCode::F3,
        "F4" => KeyCode::F4,
        "F5" => KeyCode::F5,
        "F6" => KeyCode::F6,
        "F7" => KeyCode::F7,
        "F8" => KeyCode::F8,
        "F9" => KeyCode::F9,
        "F10" => KeyCode::F10,
        "Space" => KeyCode::Space,
        "Pause" => KeyCode::Pause,
        "Quote" => KeyCode::Quote,
        "Semicolon" => KeyCode::Semicolon,
        "Backslash" => KeyCode::Backslash,
        "Backspace" => KeyCode::Backspace,
        "Equal" => KeyCode::Equal,
        "Minus" => KeyCode::Minus,
        _ => return None,
    })
}

pub fn key_label(key: KeyCode) -> String {
    match key {
        KeyCode::Space => "Espacio".into(),
        KeyCode::Pause => "Pause".into(),
        KeyCode::Backspace => "Retroceso".into(),
        KeyCode::Quote => "'".into(),
        KeyCode::Semicolon => ";".into(),
        KeyCode::Backslash => "\\".into(),
        KeyCode::Equal => "+".into(),
        KeyCode::Minus => "−".into(),
        _ => format!("{key:?}")
            .trim_start_matches("Key")
            .trim_start_matches("Digit")
            .to_string(),
    }
}

pub fn player_data_dir() -> PathBuf {
    std::env::var_os("OPENRAILSRS_PLAYER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("player-data"))
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let parent = path.parent().ok_or("Archivo sin directorio")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .ok_or("Archivo sin nombre")?
            .to_string_lossy(),
        std::process::id()
    ));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| e.to_string())?;
        std::fs::rename(&temporary, path).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controls_remain_exclusive_and_persist() {
        let mut s = PlayerSettings::default();
        s.validate().unwrap();
        assert!(s.rebind(PlayerAction::Doors, KeyCode::KeyW).is_err());
        assert!(s.rebind(PlayerAction::Doors, KeyCode::ArrowUp).is_err());
        s.rebind(PlayerAction::Doors, KeyCode::KeyZ).unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.json");
        s.save(&path).unwrap();
        assert_eq!(
            PlayerSettings::load(&path)
                .unwrap()
                .key(PlayerAction::Doors),
            KeyCode::KeyZ
        );
    }
    #[test]
    fn adding_lights_preserves_legacy_custom_controls_without_collisions() {
        let mut old = PlayerSettings::default();
        old.keys.remove(&PlayerAction::Headlights);
        old.keys.remove(&PlayerAction::CabLight);
        old.keys.insert(PlayerAction::Doors, "KeyH".into());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.json");
        std::fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
        let upgraded = PlayerSettings::load(&path).unwrap();
        assert_eq!(upgraded.key(PlayerAction::Doors), KeyCode::KeyH);
        assert_ne!(upgraded.key(PlayerAction::Headlights), KeyCode::KeyH);
        upgraded.validate().unwrap();
    }
}
