//! Viewer ETCS status: TCS core ([`openrailsrs_sim::etcs`]) + UI-only fields (#163).

use openrailsrs_sim::LiveDriveSession;
use openrailsrs_sim::etcs::{
    self as sim_etcs, BasicEtcsTcs, EtcsTcsStatus, MenuWindowDef, SoftKeyDef,
};

pub use openrailsrs_sim::etcs::{
    EtcsLevel, EtcsMode, EtcsMonitor, EtcsSupervision, GradientSegment, PlanningSymbol,
    SoftKeyAction, SpeedTarget, TextMessage, TrackCondition,
};

use super::mode::DmiMode;

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
    pub indication_marker_m: Option<f64>,
    pub planning_symbol: PlanningSymbol,
    pub speed_targets: Vec<SpeedTarget>,
    pub gradient: Vec<GradientSegment>,
    pub track_conditions: Vec<TrackCondition>,
    pub messages: Vec<TextMessage>,
    pub soft_keys: Vec<SoftKeyDef>,
    pub main_menu: MenuWindowDef,
    pub settings_menu: MenuWindowDef,
    pub planning_max_m: f64,
    pub message_page: usize,
    pub pressed_hit: Option<super::input::DmiHit>,
    pub dmi_mode: DmiMode,
    pub blink_on: bool,
    pub needs_ack: bool,
}

impl Default for EtcsStatus {
    fn default() -> Self {
        Self::from_tcs(EtcsTcsStatus::default())
    }
}

impl EtcsStatus {
    pub fn from_tcs(t: EtcsTcsStatus) -> Self {
        Self {
            active: t.active,
            speed_kmh: t.speed_kmh,
            allowed_kmh: t.allowed_kmh,
            target_kmh: t.target_kmh,
            target_distance_m: t.target_distance_m,
            release_kmh: t.release_kmh,
            intervention_kmh: t.intervention_kmh,
            overspeed: t.overspeed,
            dial_max_kmh: t.dial_max_kmh,
            monitor: t.monitor,
            supervision: t.supervision,
            mode: t.mode,
            level: t.level,
            tti_indication_s: t.tti_indication_s,
            tti_permitted_s: t.tti_permitted_s,
            indication_marker_m: t.indication_marker_m,
            planning_symbol: t.planning_symbol,
            speed_targets: t.speed_targets,
            gradient: t.gradient,
            track_conditions: t.track_conditions,
            messages: t.messages,
            soft_keys: t.soft_keys,
            main_menu: t.main_menu,
            settings_menu: t.settings_menu,
            planning_max_m: 4000.0,
            message_page: 0,
            pressed_hit: None,
            dmi_mode: DmiMode::FullSize,
            blink_on: false,
            needs_ack: t.needs_ack,
        }
    }

    pub fn from_telemetry(
        speed_kmh: f64,
        allowed_kmh: f64,
        overspeed: bool,
        target_distance_m: Option<f64>,
    ) -> Self {
        let t = BasicEtcsTcs::default().compute_from_inputs(
            speed_kmh,
            allowed_kmh,
            overspeed,
            target_distance_m,
            None,
        );
        Self::from_tcs(t)
    }

    pub fn message_lines(&self) -> Vec<&str> {
        self.messages.iter().map(|m| m.text.as_str()).collect()
    }

    pub fn soft_key_labels(&self) -> Vec<String> {
        self.soft_keys.iter().map(|k| k.label.clone()).collect()
    }
}

pub fn etcs_status_from_live(session: &LiveDriveSession) -> EtcsStatus {
    EtcsStatus::from_tcs(session.etcs_status())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_telemetry_uses_tcs() {
        let s = EtcsStatus::from_telemetry(90.0, 100.0, false, Some(500.0));
        assert!(!s.soft_keys.is_empty());
        assert_eq!(s.main_menu.title, "Main");
        assert!(matches!(
            s.monitor,
            EtcsMonitor::TargetSpeed | EtcsMonitor::ReleaseSpeed | EtcsMonitor::CeilingSpeed
        ));
    }

    #[test]
    fn dial_scale_reexport() {
        assert_eq!(sim_etcs::pick_dial_scale(90.0), 140);
    }
}
