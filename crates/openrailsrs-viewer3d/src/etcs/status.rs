//! Synthetic `ETCSStatus` for the DMI (no TCS scripting yet).

use openrailsrs_sim::LiveDriveSession;

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

#[derive(Clone, Debug, PartialEq)]
pub struct EtcsStatus {
    pub active: bool,
    pub speed_kmh: f64,
    pub allowed_kmh: f64,
    pub target_kmh: Option<f64>,
    pub target_distance_m: Option<f64>,
    pub intervention_kmh: f64,
    pub overspeed: bool,
    /// Dial full-scale (standard OR scale ≥ allowed).
    pub dial_max_kmh: u32,
    pub monitor: EtcsMonitor,
    pub supervision: EtcsSupervision,
    /// CSM TTI (white square); seconds, OR `TimeToIndicationS`.
    pub tti_indication_s: Option<f64>,
    /// TSM/RSM TTI (yellow/orange/red); OR `TimeToPermittedS`.
    pub tti_permitted_s: Option<f64>,
    pub planning_symbol: PlanningSymbol,
    /// Message-area lines (newest last; painter pages with `message_page`).
    pub messages: Vec<String>,
    /// Soft-key labels for the right menu bar (up to 6).
    pub soft_keys: Vec<String>,
    /// Planning distance scale (m), OR `MaxViewingDistanceM`.
    pub planning_max_m: f64,
    /// Message list page (0 = newest chunk).
    pub message_page: usize,
    /// Soft key currently drawn as pressed (#161).
    pub pressed_hit: Option<super::input::DmiHit>,
}

impl Default for EtcsStatus {
    fn default() -> Self {
        Self {
            active: true,
            speed_kmh: 0.0,
            allowed_kmh: 80.0,
            target_kmh: None,
            target_distance_m: None,
            intervention_kmh: 85.0,
            overspeed: false,
            dial_max_kmh: 140,
            monitor: EtcsMonitor::CeilingSpeed,
            supervision: EtcsSupervision::Normal,
            tti_indication_s: None,
            tti_permitted_s: None,
            planning_symbol: PlanningSymbol::None,
            messages: vec!["FS / L1".into()],
            soft_keys: default_soft_keys(),
            planning_max_m: 4000.0,
            message_page: 0,
            pressed_hit: None,
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
        let allowed = allowed_kmh.max(0.0);
        // Soft target: next reduction ≈ 80% of allowed when approaching a stop/limit.
        let target_kmh = target_distance_m.and_then(|d| {
            if d < 3000.0 && allowed > 10.0 {
                Some((allowed * 0.8).max(0.0))
            } else {
                None
            }
        });
        let intervention = if overspeed {
            speed.max(allowed) + 5.0
        } else {
            allowed + (allowed * 0.05).max(5.0)
        };

        let approaching_zero = target_kmh.is_some_and(|t| t < 5.0);
        let has_lower_target =
            target_kmh.is_some_and(|t| t + 0.5 < allowed);

        let monitor = if approaching_zero {
            EtcsMonitor::ReleaseSpeed
        } else if has_lower_target {
            EtcsMonitor::TargetSpeed
        } else {
            EtcsMonitor::CeilingSpeed
        };

        let supervision = derive_supervision(speed, allowed, intervention, overspeed, monitor, target_kmh);

        let (tti_indication_s, tti_permitted_s) =
            derive_tti(speed, target_distance_m, monitor, supervision);

        let planning_symbol = if !has_lower_target {
            PlanningSymbol::None
        } else if matches!(
            supervision,
            EtcsSupervision::Indication | EtcsSupervision::Normal
        ) && matches!(monitor, EtcsMonitor::TargetSpeed | EtcsMonitor::ReleaseSpeed)
        {
            PlanningSymbol::YellowSpeedReduction
        } else {
            PlanningSymbol::SpeedReduction
        };

        let messages = derive_messages(supervision, monitor, overspeed, target_kmh, target_distance_m);

        Self {
            active: true,
            speed_kmh: speed,
            allowed_kmh: allowed,
            target_kmh,
            target_distance_m,
            intervention_kmh: intervention,
            overspeed,
            dial_max_kmh: pick_dial_scale(allowed.max(speed)),
            monitor,
            supervision,
            tti_indication_s,
            tti_permitted_s,
            planning_symbol,
            messages,
            soft_keys: default_soft_keys(),
            planning_max_m: 4000.0,
            message_page: 0,
            pressed_hit: None,
        }
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
        // Warning band: between allowed and intervention.
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

/// Approximate TTI from distance/speed (OR displays 0–14 s bands).
fn derive_tti(
    speed_kmh: f64,
    target_distance_m: Option<f64>,
    monitor: EtcsMonitor,
    supervision: EtcsSupervision,
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
        EtcsMonitor::TargetSpeed | EtcsMonitor::ReleaseSpeed => {
            // Prefer permitted TTI while in TSM/RSM colours.
            if matches!(
                supervision,
                EtcsSupervision::Normal | EtcsSupervision::Indication
            ) {
                (None, Some(tti))
            } else {
                (None, Some(tti))
            }
        }
    }
}

fn derive_messages(
    supervision: EtcsSupervision,
    monitor: EtcsMonitor,
    overspeed: bool,
    target_kmh: Option<f64>,
    target_distance_m: Option<f64>,
) -> Vec<String> {
    let mut msgs = vec!["FS / L1".into()];
    match monitor {
        EtcsMonitor::CeilingSpeed => msgs.push("CSM".into()),
        EtcsMonitor::TargetSpeed => msgs.push("TSM".into()),
        EtcsMonitor::ReleaseSpeed => msgs.push("RSM".into()),
    }
    match supervision {
        EtcsSupervision::Intervention => msgs.push("Intervention".into()),
        EtcsSupervision::Warning => msgs.push("Warning".into()),
        EtcsSupervision::Overspeed => msgs.push("Overspeed".into()),
        EtcsSupervision::Indication => msgs.push("Indication".into()),
        EtcsSupervision::Normal if overspeed => msgs.push("Overspeed".into()),
        EtcsSupervision::Normal => {}
    }
    if let (Some(t), Some(d)) = (target_kmh, target_distance_m) {
        if d < 3000.0 {
            msgs.push(format!("Target {t:.0} in {d:.0}m"));
        }
    }
    msgs
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
        assert!(matches!(
            s.supervision,
            EtcsSupervision::Overspeed | EtcsSupervision::Warning | EtcsSupervision::Intervention
        ));
    }

    #[test]
    fn approaching_target_enters_tsm() {
        let s = EtcsStatus::from_telemetry(60.0, 100.0, false, Some(800.0));
        assert_eq!(s.monitor, EtcsMonitor::TargetSpeed);
        assert!(s.messages.iter().any(|m| m == "TSM"));
        assert_ne!(s.planning_symbol, PlanningSymbol::None);
    }

    #[test]
    fn close_approach_yields_tti() {
        // 100 m at 72 km/h ≈ 5 s → within 14 s display window.
        let s = EtcsStatus::from_telemetry(72.0, 100.0, false, Some(100.0));
        assert!(s.tti_permitted_s.is_some() || s.tti_indication_s.is_some());
    }
}
