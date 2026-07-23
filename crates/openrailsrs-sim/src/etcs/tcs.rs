//! Rust TCS runtime — OR `MSTSTrainControlSystem` / script ETCS subset without C# (#163).

use super::braking::{
    EMERGENCY_DECEL_MPS2, SERVICE_DECEL_MPS2, allowed_on_curve, indication_distance_m,
    time_to_distance_s,
};
use super::menu::{default_soft_keys, main_menu_def, settings_menu_def};
use super::status::{
    EtcsLevel, EtcsMode, EtcsMonitor, EtcsSupervision, EtcsTcsStatus, GradientSegment,
    PlanningSymbol, SpeedTarget, TextMessage, TrackCondition, TrackConditionKind, pick_dial_scale,
};
use crate::live_drive::LiveDriveSession;

/// Trait for ETCS status providers (swap-in for future script hosts).
pub trait EtcsTcs {
    fn compute(&self, session: &LiveDriveSession) -> EtcsTcsStatus;
}

/// Built-in TCS: derives supervision from speed limit + distance to next stop.
#[derive(Clone, Debug, Default)]
pub struct BasicEtcsTcs {
    /// Optional national STM stub (sets Mode::Sn).
    pub stm: bool,
}

impl EtcsTcs for BasicEtcsTcs {
    fn compute(&self, session: &LiveDriveSession) -> EtcsTcsStatus {
        let tel = session.cab_telemetry();
        self.compute_from_inputs(
            tel.speed_kmh,
            tel.limit_kmh,
            tel.overspeed,
            session.distance_to_next_stop_m(),
            session.next_stop_label().map(|s| s.to_string()),
        )
    }
}

impl BasicEtcsTcs {
    pub fn compute_from_inputs(
        &self,
        speed_kmh: f64,
        limit_kmh: f64,
        overspeed_flag: bool,
        target_distance_m: Option<f64>,
        stop_label: Option<String>,
    ) -> EtcsTcsStatus {
        let speed = speed_kmh.max(0.0);
        let limit = limit_kmh.max(0.0);
        let speed_mps = speed / 3.6;
        let limit_mps = limit / 3.6;
        let dist = target_distance_m;

        // Target: stop → 0 km/h; otherwise soft reduction at far approach.
        let (target_kmh, release_kmh, approaching_stop) = match dist {
            Some(d) if d < 5000.0 && limit > 5.0 => {
                let stop = d < 450.0;
                let tgt = if stop { 0.0 } else { (limit * 0.85).max(0.0) };
                let release = if stop { Some(40.0) } else { None };
                (Some(tgt), release, stop)
            }
            _ => (None, None, false),
        };
        let target_mps = target_kmh.map(|t| t / 3.6).unwrap_or(limit_mps);

        let brake_dist = if target_kmh.is_some_and(|t| t + 0.5 < limit) {
            indication_distance_m(limit_mps.max(speed_mps), target_mps)
                .max(200.0)
                .min(4000.0)
        } else {
            0.0
        };

        let indication_marker_m = dist.and_then(|d| {
            if brake_dist > 0.0 && d > 50.0 {
                Some(brake_dist.min(d))
            } else {
                None
            }
        });

        // Allowed follows braking envelope when inside brake_dist of target.
        let mut allowed_mps = limit_mps;
        if let Some(d) = dist {
            if brake_dist > 0.0 && target_kmh.is_some_and(|t| t + 0.5 < limit) {
                allowed_mps = allowed_on_curve(d, brake_dist, limit_mps, target_mps);
            }
        }
        let allowed = allowed_mps * 3.6;

        // Intervention ≈ permitted + margin (OR SetIntervention / built-in).
        let mut intervention_mps =
            allowed_mps + (allowed_mps * 0.05).max(5.0 / 3.6);
        if overspeed_flag || speed_mps > allowed_mps {
            intervention_mps = intervention_mps.max(speed_mps + 1.0 / 3.6);
        }
        // Emergency envelope: if remaining distance < emergency braking distance → intervention speed.
        if let Some(d) = dist {
            let e_dist = (speed_mps * speed_mps) / (2.0 * EMERGENCY_DECEL_MPS2);
            if d < e_dist && speed_mps > target_mps + 1.0 {
                intervention_mps = intervention_mps.min(speed_mps);
            }
        }
        let intervention = intervention_mps * 3.6;

        let has_lower = target_kmh.is_some_and(|t| t + 0.5 < limit);
        let in_tsm = has_lower
            && dist.is_some_and(|d| d <= brake_dist.max(indication_marker_m.unwrap_or(0.0)));

        let monitor = if approaching_stop && in_tsm {
            EtcsMonitor::ReleaseSpeed
        } else if in_tsm {
            EtcsMonitor::TargetSpeed
        } else {
            EtcsMonitor::CeilingSpeed
        };

        let supervision = derive_supervision(
            speed_mps,
            allowed_mps,
            intervention_mps,
            overspeed_flag,
            monitor,
            target_mps,
        );

        // TTI: CSM → time to indication point; TSM → time to permitted (service curve).
        let (tti_indication_s, tti_permitted_s) = derive_tti(
            speed_mps,
            dist,
            indication_marker_m,
            monitor,
            SERVICE_DECEL_MPS2,
        );

        let planning_symbol = if !has_lower {
            PlanningSymbol::None
        } else if matches!(
            supervision,
            EtcsSupervision::Indication | EtcsSupervision::Normal
        ) {
            PlanningSymbol::YellowSpeedReduction
        } else {
            PlanningSymbol::SpeedReduction
        };

        let mode = if self.stm {
            EtcsMode::Sn
        } else if approaching_stop && speed < 5.0 {
            EtcsMode::Os
        } else {
            EtcsMode::Fs
        };

        let messages = derive_messages(
            supervision,
            monitor,
            overspeed_flag,
            target_kmh,
            dist,
            approaching_stop,
            stop_label.as_deref(),
            mode,
        );
        let needs_ack = messages
            .iter()
            .any(|m| m.acknowledgeable && !m.acknowledged);

        EtcsTcsStatus {
            active: true,
            speed_kmh: speed,
            allowed_kmh: allowed,
            target_kmh,
            target_distance_m: dist,
            release_kmh,
            intervention_kmh: intervention,
            overspeed: overspeed_flag || speed > allowed + 0.5,
            dial_max_kmh: pick_dial_scale(limit.max(speed)),
            monitor,
            supervision,
            mode,
            level: EtcsLevel::L1,
            tti_indication_s,
            tti_permitted_s,
            indication_marker_m,
            planning_symbol,
            speed_targets: build_speed_targets(limit, target_kmh, dist),
            gradient: build_gradient(dist),
            track_conditions: build_track_conditions(dist),
            messages,
            soft_keys: default_soft_keys(),
            main_menu: main_menu_def(),
            settings_menu: settings_menu_def(),
            needs_ack,
            stop_label,
        }
    }
}

fn derive_supervision(
    speed_mps: f64,
    allowed_mps: f64,
    intervention_mps: f64,
    overspeed_flag: bool,
    monitor: EtcsMonitor,
    target_mps: f64,
) -> EtcsSupervision {
    if speed_mps > intervention_mps + 0.2 {
        return EtcsSupervision::Intervention;
    }
    if overspeed_flag || speed_mps > allowed_mps + 0.15 {
        let band = (intervention_mps - allowed_mps).max(0.5);
        if speed_mps > allowed_mps + band * 0.55 {
            return EtcsSupervision::Warning;
        }
        return EtcsSupervision::Overspeed;
    }
    if matches!(monitor, EtcsMonitor::TargetSpeed | EtcsMonitor::ReleaseSpeed)
        && speed_mps + 0.15 >= target_mps
        && target_mps + 0.15 < allowed_mps
    {
        return EtcsSupervision::Indication;
    }
    EtcsSupervision::Normal
}

fn derive_tti(
    speed_mps: f64,
    dist: Option<f64>,
    indication_m: Option<f64>,
    monitor: EtcsMonitor,
    _decel: f64,
) -> (Option<f64>, Option<f64>) {
    let Some(d) = dist else {
        return (None, None);
    };
    if speed_mps < 1.0 {
        return (None, None);
    }
    match monitor {
        EtcsMonitor::CeilingSpeed => {
            // Time until train reaches indication marker (start of TSM).
            let Some(im) = indication_m else {
                return (None, None);
            };
            let remain = (d - im).max(0.0);
            let t = time_to_distance_s(remain, speed_mps);
            if (0.0..14.0).contains(&t) {
                (Some(t), None)
            } else {
                (None, None)
            }
        }
        EtcsMonitor::TargetSpeed | EtcsMonitor::ReleaseSpeed => {
            let t = time_to_distance_s(d, speed_mps);
            if (0.0..14.0).contains(&t) {
                (None, Some(t))
            } else {
                (None, None)
            }
        }
    }
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
                distance_m: (d + 600.0).min(4000.0),
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

fn derive_messages(
    supervision: EtcsSupervision,
    monitor: EtcsMonitor,
    overspeed: bool,
    target_kmh: Option<f64>,
    target_distance_m: Option<f64>,
    approaching_stop: bool,
    stop_label: Option<&str>,
    mode: EtcsMode,
) -> Vec<TextMessage> {
    let mut msgs = vec![TextMessage {
        text: format!("{} / L1", mode.label()),
        acknowledgeable: false,
        acknowledged: true,
    }];
    let mon = match monitor {
        EtcsMonitor::CeilingSpeed => "CSM",
        EtcsMonitor::TargetSpeed => "TSM",
        EtcsMonitor::ReleaseSpeed => "RSM",
    };
    msgs.push(plain(mon));
    match supervision {
        EtcsSupervision::Intervention => msgs.push(ack("Intervention")),
        EtcsSupervision::Warning => msgs.push(ack("Warning")),
        EtcsSupervision::Overspeed => msgs.push(ack("Overspeed")),
        EtcsSupervision::Indication => msgs.push(plain("Indication")),
        EtcsSupervision::Normal if overspeed => msgs.push(ack("Overspeed")),
        EtcsSupervision::Normal => {}
    }
    if approaching_stop {
        let text = match stop_label {
            Some(n) => format!("Ack stop {n}"),
            None => "Acknowledge mode".into(),
        };
        msgs.push(ack(&text));
    }
    if let (Some(t), Some(d)) = (target_kmh, target_distance_m) {
        if d < 5000.0 {
            msgs.push(plain(&format!("Target {t:.0} in {d:.0}m")));
        }
    }
    msgs
}

fn plain(text: &str) -> TextMessage {
    TextMessage {
        text: text.into(),
        acknowledgeable: false,
        acknowledged: true,
    }
}

fn ack(text: &str) -> TextMessage {
    TextMessage {
        text: text.into(),
        acknowledgeable: true,
        acknowledged: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn far_away_is_csm() {
        let tcs = BasicEtcsTcs::default();
        let s = tcs.compute_from_inputs(80.0, 100.0, false, Some(8000.0), None);
        assert_eq!(s.monitor, EtcsMonitor::CeilingSpeed);
    }

    #[test]
    fn near_reduction_enters_tsm() {
        let tcs = BasicEtcsTcs::default();
        // Inside service braking distance to the soft target → TSM/RSM.
        let s = tcs.compute_from_inputs(90.0, 100.0, false, Some(120.0), None);
        assert!(matches!(
            s.monitor,
            EtcsMonitor::TargetSpeed | EtcsMonitor::ReleaseSpeed
        ));
        assert!(s.allowed_kmh <= 100.0);
    }

    #[test]
    fn stop_sets_release_and_ack() {
        let tcs = BasicEtcsTcs::default();
        let s = tcs.compute_from_inputs(30.0, 80.0, false, Some(200.0), Some("Birmingham".into()));
        assert_eq!(s.release_kmh, Some(40.0));
        assert!(s.needs_ack);
        assert!(s.messages.iter().any(|m| m.text.contains("Birmingham")));
    }

    #[test]
    fn overspeed_warning_band() {
        let tcs = BasicEtcsTcs::default();
        let s = tcs.compute_from_inputs(110.0, 80.0, true, None, None);
        assert!(matches!(
            s.supervision,
            EtcsSupervision::Overspeed | EtcsSupervision::Warning | EtcsSupervision::Intervention
        ));
    }

    #[test]
    fn stm_sets_sn_mode() {
        let tcs = BasicEtcsTcs { stm: true };
        let s = tcs.compute_from_inputs(50.0, 80.0, false, None, None);
        assert_eq!(s.mode, EtcsMode::Sn);
    }

    #[test]
    fn soft_keys_include_main() {
        let tcs = BasicEtcsTcs::default();
        let s = tcs.compute_from_inputs(0.0, 80.0, false, None, None);
        assert_eq!(s.soft_keys[0].label, "Main");
        assert_eq!(s.main_menu.title, "Main");
    }
}
