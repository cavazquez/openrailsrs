//! Render-only quality controller. Physics clock, signals and weather never change.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct QualityLimits {
    pub enabled: bool,
    pub target_fps: u32,
    pub min_level: u8,
    pub max_level: u8,
}
impl Default for QualityLimits {
    fn default() -> Self {
        Self {
            enabled: false,
            target_fps: 60,
            min_level: 0,
            max_level: 3,
        }
    }
}
impl QualityLimits {
    pub fn valid(&self) -> bool {
        (20..=144).contains(&self.target_fps)
            && self.min_level <= self.max_level
            && self.max_level <= 3
    }
}
#[derive(Resource, Default)]
pub struct AdaptiveGraphics {
    pub level: u8,
    pub filtered_ms: f32,
    slow: f32,
    fast: f32,
    cooldown: f32,
    pub changes: u32,
}
impl AdaptiveGraphics {
    pub fn observe(&mut self, dt: f32, running: bool, limits: &QualityLimits) {
        if !limits.enabled {
            self.level = 0;
            self.slow = 0.;
            self.fast = 0.;
            return;
        }
        self.level = self.level.clamp(limits.min_level, limits.max_level);
        if !running || !dt.is_finite() || dt <= 0. {
            return;
        }
        let dt = dt.min(0.25);
        let ms = dt * 1000.;
        let target = 1000. / limits.target_fps as f32;
        self.filtered_ms = if self.filtered_ms == 0. {
            ms
        } else {
            self.filtered_ms + (ms - self.filtered_ms) * (1. - (-dt / 3.).exp())
        };
        self.cooldown = (self.cooldown - dt).max(0.);
        if self.filtered_ms > target * 1.25 {
            self.slow += dt;
            self.fast = 0.;
        } else if self.filtered_ms < target * 0.72 {
            self.fast += dt;
            self.slow = 0.;
        } else {
            self.slow = 0.;
            self.fast = 0.;
        }
        let next = if self.cooldown == 0. && self.slow >= 6. {
            (self.level + 1).min(limits.max_level)
        } else if self.cooldown == 0. && self.fast >= 20. {
            self.level.saturating_sub(1).max(limits.min_level)
        } else {
            self.level
        };
        if next != self.level {
            self.level = next;
            self.changes += 1;
            self.cooldown = 15.;
            self.slow = 0.;
            self.fast = 0.;
        }
    }
    pub fn factor(&self) -> usize {
        1usize << self.level.min(3)
    }
}
pub fn update(
    time: Res<Time<Real>>,
    settings: Res<crate::player_settings::PlayerSettings>,
    live: Option<Res<crate::live::LiveDrive>>,
    loading: Option<Res<crate::route_bootstrap::ViewerLoadingScreen>>,
    pending: Option<Res<crate::world::WorldSpawnProgress>>,
    mut quality: ResMut<AdaptiveGraphics>,
) {
    quality.observe(
        time.delta_secs(),
        live.is_some_and(|l| !l.paused && l.session.time_s() > 10.)
            && loading.is_none()
            && pending.is_none(),
        &settings.quality_limits,
    );
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sustained_pressure_and_recovery_are_slow_and_respect_limits() {
        let limits = QualityLimits {
            enabled: true,
            min_level: 0,
            max_level: 2,
            ..default()
        };
        let mut q = AdaptiveGraphics::default();
        for _ in 0..1800 {
            q.observe(0.05, true, &limits);
        }
        assert_eq!(q.level, 2);
        assert_eq!(q.changes, 2);
        let changes = q.changes;
        for i in 0..10000 {
            q.observe(if i % 2 == 0 { 0.025 } else { 0.018 }, true, &limits);
        }
        assert_eq!(q.changes, changes);
        for _ in 0..10000 {
            q.observe(0.008, true, &limits);
        }
        assert_eq!(q.level, 0);
        q.observe(0.2, false, &limits);
        assert_eq!(q.level, 0);
        assert!(limits.valid());
    }
}
