use crate::LiveDriveSession;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AnnouncementState {
    pub package: openrailsrs_scenarios::announcements::AnnouncementPackage,
    pub played: BTreeSet<String>,
    pub recent: Vec<AnnouncementRecord>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnnouncementRecord {
    pub key: String,
    pub id: String,
    pub time_s: f64,
    pub text: String,
}
impl AnnouncementState {
    pub fn valid(&self) -> bool {
        self.package.valid()
            && self.played.len() <= 65536
            && self.recent.len() <= 32
            && self.played.iter().all(|s| s.len() <= 256)
            && self.recent.iter().all(|r| {
                self.played.contains(&r.key)
                    && r.id.len() <= 128
                    && r.text.len() <= 512
                    && r.time_s.is_finite()
                    && r.time_s >= 0.
            })
    }
}
impl LiveDriveSession {
    pub(crate) fn tick_announcements(&mut self) {
        use openrailsrs_scenarios::announcements::AnnouncementEvent;
        let now = self.time_s();
        let leg = self.operational_leg;
        let next = self.gameplay.next_stop_idx;
        let arrival = self.gameplay.current_arrival_s();
        let distance = self
            .gameplay
            .stop_targets
            .get(next)
            .map(|s| s.cum_dist_m - self.gameplay_chainage_m());
        let state = &mut self.state.announcements;
        for event in &state.package.events {
            if event
                .service
                .as_ref()
                .is_some_and(|s| s != &self.service_id && s != &self.scenario_name)
            {
                continue;
            }
            let key = format!("{leg}:{}", event.id);
            if state.played.contains(&key) {
                continue;
            }
            let trigger = match event.event {
                AnnouncementEvent::Approach => {
                    event.stop == next && distance.is_some_and(|d| (0. ..=200.).contains(&d))
                }
                AnnouncementEvent::Arrival => {
                    event.stop == next && arrival.is_some()
                        || self.gameplay.stop_results.len() > event.stop
                }
                AnnouncementEvent::Departure => self.gameplay.stop_results.len() > event.stop,
            };
            if trigger {
                state.played.insert(key.clone());
                state.recent.push(AnnouncementRecord {
                    key,
                    id: event.id.clone(),
                    time_s: now,
                    text: event.text.clone(),
                });
                if state.recent.len() > 32 {
                    state.recent.remove(0);
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn authorized_announcements_are_once_per_leg_and_survive_restore() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/traction_operation/announcements/scenario.toml");
        let scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
        let mut live =
            crate::LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap();
        live.step_realtime(1., |_| {});
        let first = live.state.announcements.played.clone();
        assert!(!first.is_empty());
        let saved = live.snapshot();
        let mut resumed =
            crate::LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap();
        resumed.restore_snapshot(saved).unwrap();
        resumed.step_realtime(1., |_| {});
        assert_eq!(resumed.state.announcements.played, first);
        assert!(resumed.state.announcements.package.root.is_empty());
        let mut invalid = scenario.clone();
        invalid.train.announcements.events[0].file = "../../private.wav".into();
        assert!(invalid.validate().is_err());
        invalid.train.announcements.license.clear();
        assert!(invalid.validate().is_err());
    }
}
