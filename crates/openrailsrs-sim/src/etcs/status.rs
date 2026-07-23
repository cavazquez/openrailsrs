//! Core ETCS status produced by the Rust TCS (OR `ETCSStatus` subset).

use super::menu::{MenuWindowDef, SoftKeyDef, default_soft_keys, main_menu_def, settings_menu_def};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EtcsMonitor {
    CeilingSpeed,
    TargetSpeed,
    ReleaseSpeed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EtcsSupervision {
    Normal,
    Indication,
    Overspeed,
    Warning,
    Intervention,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EtcsMode {
    Fs,
    Os,
    Sr,
    Sh,
    Sb,
    /// National system / STM stub.
    Sn,
}

impl EtcsMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Fs => "FS",
            Self::Os => "OS",
            Self::Sr => "SR",
            Self::Sh => "SH",
            Self::Sb => "SB",
            Self::Sn => "SN",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EtcsLevel {
    L0,
    L1,
    L2,
}

impl EtcsLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::L0 => "L0",
            Self::L1 => "L1",
            Self::L2 => "L2",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanningSymbol {
    None,
    SpeedIncrease,
    SpeedReduction,
    YellowSpeedReduction,
}

impl PlanningSymbol {
    pub fn texture(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::SpeedIncrease => Some("PL_21.png"),
            Self::SpeedReduction => Some("PL_22.png"),
            Self::YellowSpeedReduction => Some("PL_23.png"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpeedTarget {
    pub distance_m: f64,
    pub speed_kmh: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientSegment {
    pub distance_m: f64,
    pub grade_permille: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackConditionKind {
    Tunnel,
    Bridge,
    Station,
    EndOfTrack,
}

impl TrackConditionKind {
    pub fn texture(self) -> &'static str {
        match self {
            Self::Tunnel => "PL_tunnel.png",
            Self::Bridge => "PL_bridge.png",
            Self::Station => "PL_station.png",
            Self::EndOfTrack => "PL_endoftrack.png",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackCondition {
    pub distance_m: f64,
    pub kind: TrackConditionKind,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextMessage {
    pub text: String,
    pub acknowledgeable: bool,
    pub acknowledged: bool,
}

/// Physics-derived ETCS snapshot for the DMI (no UI-only fields).
#[derive(Clone, Debug, PartialEq)]
pub struct EtcsTcsStatus {
    pub active: bool,
    pub speed_kmh: f64,
    pub allowed_kmh: f64,
    pub target_kmh: Option<f64>,
    pub target_distance_m: Option<f64>,
    pub release_kmh: Option<f64>,
    pub intervention_kmh: f64,
    pub overspeed: bool,
    pub dial_max_kmh: u32,
    pub monitor: EtcsMonitor,
    pub supervision: EtcsSupervision,
    pub mode: EtcsMode,
    pub level: EtcsLevel,
    pub tti_indication_s: Option<f64>,
    pub tti_permitted_s: Option<f64>,
    pub indication_marker_m: Option<f64>,
    pub planning_symbol: PlanningSymbol,
    pub speed_targets: Vec<SpeedTarget>,
    pub gradient: Vec<GradientSegment>,
    pub track_conditions: Vec<TrackCondition>,
    pub messages: Vec<TextMessage>,
    pub soft_keys: Vec<SoftKeyDef>,
    pub main_menu: MenuWindowDef,
    pub settings_menu: MenuWindowDef,
    pub needs_ack: bool,
    pub stop_label: Option<String>,
}

impl Default for EtcsTcsStatus {
    fn default() -> Self {
        Self {
            active: true,
            speed_kmh: 0.0,
            allowed_kmh: 80.0,
            target_kmh: None,
            target_distance_m: None,
            release_kmh: None,
            intervention_kmh: 85.0,
            overspeed: false,
            dial_max_kmh: 140,
            monitor: EtcsMonitor::CeilingSpeed,
            supervision: EtcsSupervision::Normal,
            mode: EtcsMode::Fs,
            level: EtcsLevel::L1,
            tti_indication_s: None,
            tti_permitted_s: None,
            indication_marker_m: None,
            planning_symbol: PlanningSymbol::None,
            speed_targets: vec![SpeedTarget {
                distance_m: 0.0,
                speed_kmh: 80.0,
            }],
            gradient: vec![],
            track_conditions: vec![],
            messages: vec![TextMessage {
                text: "FS / L1".into(),
                acknowledgeable: false,
                acknowledged: true,
            }],
            soft_keys: default_soft_keys(),
            main_menu: main_menu_def(),
            settings_menu: settings_menu_def(),
            needs_ack: false,
            stop_label: None,
        }
    }
}

pub fn pick_dial_scale(need: f64) -> u32 {
    const SCALES: [u32; 8] = [140, 150, 180, 240, 250, 260, 280, 400];
    for s in SCALES {
        if need <= f64::from(s) {
            return s;
        }
    }
    400
}
