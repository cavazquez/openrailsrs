//! Synthetic `ETCSStatus` for the DMI (no TCS scripting yet).

use openrailsrs_sim::LiveDriveSession;

use super::mode::DmiMode;

/// OR `Monitor` — ceiling / target / release speed monitoring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EtcsMonitor {
    CeilingSpeed,
    TargetSpeed,
    ReleaseSpeed,
}

/// OR `SupervisionStatus` — needle / TTI colour driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EtcsSupervision {
    Normal,
    Indication,
    Overspeed,
    Warning,
    Intervention,
}

/// OR `Mode` subset shown on DMI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EtcsMode {
    Fs,
    Os,
    Sr,
    Sh,
    Sb,
}

impl EtcsMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Fs => "FS",
            Self::Os => "OS",
            Self::Sr => "SR",
            Self::Sh => "SH",
            Self::Sb => "SB",
        }
    }
}

/// OR `Level` subset.
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

/// Planning speed-change symbol (OR `PL_21`/`PL_22`/`PL_23`).
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

/// OR `PlanningTarget` / PASP sample.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpeedTarget {
    pub distance_m: f64,
    pub speed_kmh: f64,
}

/// OR `GradientProfileElement`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientSegment {
    pub distance_m: f64,
    /// ‰ grade (positive uphill).
    pub grade_permille: i32,
}

/// OR `PlanningTrackCondition` visual.
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

#[derive(Clone, Debug, PartialEq)]
pub struct EtcsStatus {
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
    /// Distance where CSM→TSM (yellow line).
    pub indication_marker_m: Option<f64>,
    pub planning_symbol: PlanningSymbol,
    pub speed_targets: Vec<SpeedTarget>,
    pub gradient: Vec<GradientSegment>,
    pub track_conditions: Vec<TrackCondition>,
    pub messages: Vec<TextMessage>,
    pub soft_keys: Vec<String>,
    pub planning_max_m: f64,
    pub message_page: usize,
    pub pressed_hit: Option<super::input::DmiHit>,
    pub dmi_mode: DmiMode,
    /// Blinker phase 0..1 for ack frame (OR Blinker4Hz).
    pub blink_on: bool,
    pub needs_ack: bool,
}

impl Default for EtcsStatus {
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
            gradient: vec![
                GradientSegment {
                    distance_m: 0.0,
                    grade_permille: 0,
                },
                GradientSegment {
                    distance_m: 4000.0,
                    grade_permille: 0,
                },
            ],
            track_conditions: vec![],
            messages: vec![TextMessage {
                text: "FS / L1".into(),
                acknowledgeable: false,
                acknowledged: true,
            }],
            soft_keys: default_soft_keys(),
            planning_max_m: 4000.0,
            message_page: 0,
            pressed_hit: None,
            dmi_mode: DmiMode::FullSize,
            blink_on: false,
            needs_ack: false,
        }
    }
}

fn default_soft_keys() -> Vec<String> {
    vec![
        "Main".into(),
        "Over.".into(),
        "Data".into(),
        "Spec".into(),
        "Sett.".into(),
        "".into(),
    ]
}

impl EtcsStatus {
    pub fn from_telemetry(
        speed_kmh: f64,
        allowed_kmh: f64,
        overspeed: bool,
        target_distance_m: Option<f64>,
    ) -> Self {
        let speed = speed_kmh.max(0.0);
        let limit = allowed_kmh.max(0.0);
        let dist = target_distance_m;

        // Soft target + synthetic braking curve (allowed decays toward target in TSM).
        let (target_kmh, release_kmh, approaching_stop) = match dist {
            Some(d) if d < 3000.0 && limit > 10.0 => {
                let stop = d < 400.0;
                let tgt = if stop {
                    0.0
                } else {
                    (limit * 0.8).max(0.0)
                };
                let release = if stop { Some(40.0) } else { None };
                (Some(tgt), release, stop)
            }
            _ => (None, None, false),
        };

        let mut allowed = limit;
        if let (Some(t), Some(d)) = (target_kmh, dist) {
            if t + 0.5 < limit && d < 2000.0 {
                // Simple braking envelope: lerp allowed → target over last 2 km.
                let u = (1.0 - d / 2000.0).clamp(0.0, 1.0);
                allowed = limit + (t - limit) * u;
            }
        }

        let intervention = if overspeed {
            speed.max(allowed) + 5.0
        } else {
            allowed + (allowed * 0.05).max(5.0)
        };

        let has_lower_target = target_kmh.is_some_and(|t| t + 0.5 < limit);
        let monitor = if approaching_stop {
            EtcsMonitor::ReleaseSpeed
        } else if has_lower_target {
            EtcsMonitor::TargetSpeed
        } else {
            EtcsMonitor::CeilingSpeed
        };

        let supervision =
            derive_supervision(speed, allowed, intervention, overspeed, monitor, target_kmh);
        let (tti_indication_s, tti_permitted_s) =
            derive_tti(speed, dist, monitor, supervision);

        let indication_marker_m = dist.and_then(|d| {
            if has_lower_target && d > 200.0 {
                Some((d * 0.55).min(d - 50.0).max(0.0))
            } else {
                None
            }
        });

        let planning_symbol = if !has_lower_target {
            PlanningSymbol::None
        } else if matches!(
            supervision,
            EtcsSupervision::Indication | EtcsSupervision::Normal
        ) {
            PlanningSymbol::YellowSpeedReduction
        } else {
            PlanningSymbol::SpeedReduction
        };

        let speed_targets = build_speed_targets(limit, target_kmh, dist);
        let gradient = build_gradient(dist);
        let track_conditions = build_track_conditions(dist);
        let messages = derive_messages(
            supervision,
            monitor,
            overspeed,
            target_kmh,
            dist,
            approaching_stop,
        );
        let needs_ack = messages.iter().any(|m| m.acknowledgeable && !m.acknowledged);

        Self {
            active: true,
            speed_kmh: speed,
            allowed_kmh: allowed,
            target_kmh,
            target_distance_m: dist,
            release_kmh,
            intervention_kmh: intervention,
            overspeed,
            dial_max_kmh: pick_dial_scale(limit.max(speed)),
            monitor,
            supervision,
            mode: EtcsMode::Fs,
            level: EtcsLevel::L1,
            tti_indication_s,
            tti_permitted_s,
            indication_marker_m,
            planning_symbol,
            speed_targets,
            gradient,
            track_conditions,
            messages,
            soft_keys: default_soft_keys(),
            planning_max_m: 4000.0,
            message_page: 0,
            pressed_hit: None,
            dmi_mode: DmiMode::FullSize,
            blink_on: false,
            needs_ack,
        }
    }

    pub fn message_lines(&self) -> Vec<&str> {
        self.messages.iter().map(|m| m.text.as_str()).collect()
    }
}

pub fn etcs_status_from_live(session: &LiveDriveSession) -> EtcsStatus {
    let tel = session.cab_telemetry();
    EtcsStatus::from_telemetry(
        tel.speed_kmh,
        tel.limit_kmh,
        tel.overspeed,
        session.distance_to_next_stop_m(),
    )
}

fn build_speed_targets(
    limit: f64,
    target: Option<f64>,
    dist: Option<f64>,
) -> Vec<SpeedTarget> {
    let mut v = vec![SpeedTarget {
        distance_m: 0.0,
        speed_kmh: limit,
    }];
    if let (Some(t), Some(d)) = (target, dist) {
        v.push(SpeedTarget {
            distance_m: d,
            speed_kmh: t,
        });
        if t > 0.5 {
            v.push(SpeedTarget {
                distance_m: (d + 800.0).min(4000.0),
                speed_kmh: t,
            });
        }
    }
    v
}

fn build_gradient(dist: Option<f64>) -> Vec<GradientSegment> {
    let mid = dist.unwrap_or(1500.0).clamp(200.0, 3000.0);
    vec![
        GradientSegment {
            distance_m: 0.0,
            grade_permille: 5,
        },
        GradientSegment {
            distance_m: mid,
            grade_permille: -8,
        },
        GradientSegment {
            distance_m: 4000.0,
            grade_permille: 0,
        },
    ]
}

fn build_track_conditions(dist: Option<f64>) -> Vec<TrackCondition> {
    let d = dist.unwrap_or(1200.0);
    let mut v = vec![TrackCondition {
        distance_m: (d * 0.35).clamp(100.0, 3500.0),
        kind: TrackConditionKind::Bridge,
    }];
    if d > 600.0 {
        v.push(TrackCondition {
            distance_m: (d * 0.7).clamp(200.0, 3800.0),
            kind: TrackConditionKind::Station,
        });
    }
    v
}

fn derive_supervision(
    speed: f64,
    allowed: f64,
    intervention: f64,
    overspeed: bool,
    monitor: EtcsMonitor,
    target_kmh: Option<f64>,
) -> EtcsSupervision {
    if speed > intervention + 0.5 {
        return EtcsSupervision::Intervention;
    }
    if overspeed || speed > allowed + 0.5 {
        if speed > allowed + (intervention - allowed) * 0.6 {
            return EtcsSupervision::Warning;
        }
        return EtcsSupervision::Overspeed;
    }
    if matches!(monitor, EtcsMonitor::TargetSpeed | EtcsMonitor::ReleaseSpeed) {
        if let Some(t) = target_kmh {
            if speed + 0.5 >= t {
                return EtcsSupervision::Indication;
            }
        }
    }
    EtcsSupervision::Normal
}

fn derive_tti(
    speed_kmh: f64,
    target_distance_m: Option<f64>,
    monitor: EtcsMonitor,
    _supervision: EtcsSupervision,
) -> (Option<f64>, Option<f64>) {
    let Some(dist) = target_distance_m else {
        return (None, None);
    };
    if speed_kmh < 5.0 || dist <= 0.0 {
        return (None, None);
    }
    let tti = dist / (speed_kmh / 3.6);
    if !(0.0..14.0).contains(&tti) {
        return (None, None);
    }
    match monitor {
        EtcsMonitor::CeilingSpeed => (Some(tti), None),
        EtcsMonitor::TargetSpeed | EtcsMonitor::ReleaseSpeed => (None, Some(tti)),
    }
}

fn derive_messages(
    supervision: EtcsSupervision,
    monitor: EtcsMonitor,
    overspeed: bool,
    target_kmh: Option<f64>,
    target_distance_m: Option<f64>,
    approaching_stop: bool,
) -> Vec<TextMessage> {
    let mut msgs = vec![TextMessage {
        text: "FS / L1".into(),
        acknowledgeable: false,
        acknowledged: true,
    }];
    let mon = match monitor {
        EtcsMonitor::CeilingSpeed => "CSM",
        EtcsMonitor::TargetSpeed => "TSM",
        EtcsMonitor::ReleaseSpeed => "RSM",
    };
    msgs.push(TextMessage {
        text: mon.into(),
        acknowledgeable: false,
        acknowledged: true,
    });
    match supervision {
        EtcsSupervision::Intervention => msgs.push(ack("Intervention")),
        EtcsSupervision::Warning => msgs.push(ack("Warning")),
        EtcsSupervision::Overspeed => msgs.push(ack("Overspeed")),
        EtcsSupervision::Indication => msgs.push(TextMessage {
            text: "Indication".into(),
            acknowledgeable: false,
            acknowledged: true,
        }),
        EtcsSupervision::Normal if overspeed => msgs.push(ack("Overspeed")),
        EtcsSupervision::Normal => {}
    }
    if approaching_stop {
        msgs.push(ack("Acknowledge mode"));
    }
    if let (Some(t), Some(d)) = (target_kmh, target_distance_m) {
        if d < 3000.0 {
            msgs.push(TextMessage {
                text: format!("Target {t:.0} in {d:.0}m"),
                acknowledgeable: false,
                acknowledged: true,
            });
        }
    }
    msgs
}

fn ack(text: &str) -> TextMessage {
    TextMessage {
        text: text.into(),
        acknowledgeable: true,
        acknowledged: false,
    }
}

fn pick_dial_scale(need: f64) -> u32 {
    const SCALES: [u32; 8] = [140, 150, 180, 240, 250, 260, 280, 400];
    for s in SCALES {
        if need <= f64::from(s) {
            return s;
        }
    }
    400
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dial_scale_picks_standard() {
        assert_eq!(pick_dial_scale(90.0), 140);
        assert_eq!(pick_dial_scale(200.0), 240);
    }

    #[test]
    fn overspeed_raises_intervention() {
        let s = EtcsStatus::from_telemetry(100.0, 80.0, true, Some(500.0));
        assert!(s.intervention_kmh >= 100.0);
        assert!(s.target_kmh.is_some());
    }

    #[test]
    fn approaching_target_enters_tsm() {
        let s = EtcsStatus::from_telemetry(60.0, 100.0, false, Some(800.0));
        assert_eq!(s.monitor, EtcsMonitor::TargetSpeed);
        assert!(s.messages.iter().any(|m| m.text == "TSM"));
        assert!(!s.speed_targets.is_empty());
        assert!(!s.gradient.is_empty());
    }

    #[test]
    fn close_stop_sets_release() {
        let s = EtcsStatus::from_telemetry(40.0, 100.0, false, Some(200.0));
        assert_eq!(s.monitor, EtcsMonitor::ReleaseSpeed);
        assert_eq!(s.release_kmh, Some(40.0));
        assert!(s.needs_ack);
    }

    #[test]
    fn close_approach_yields_tti() {
        let s = EtcsStatus::from_telemetry(72.0, 100.0, false, Some(100.0));
        assert!(s.tti_permitted_s.is_some() || s.tti_indication_s.is_some());
    }
}
