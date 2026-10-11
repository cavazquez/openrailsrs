//! Wiper modes share one railway-time clock with blade animation and audio.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WiperMode {
    #[default]
    Off,
    Intermittent,
    Slow,
    Fast,
}
impl WiperMode {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Intermittent,
            Self::Intermittent => Self::Slow,
            Self::Slow => Self::Fast,
            Self::Fast => Self::Off,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Apagado",
            Self::Intermittent => "Intermitente",
            Self::Slow => "Lento",
            Self::Fast => "Rápido",
        }
    }
    pub fn sweep_s(self) -> f64 {
        if self == Self::Fast { 0.9 } else { 1.8 }
    }
    pub fn moving(self, clock: f64) -> bool {
        self != Self::Off && (self != Self::Intermittent || clock.rem_euclid(6.0) < 1.8)
    }
    pub fn phase(self, clock: f64) -> f64 {
        if !self.moving(clock) {
            return 0.0;
        }
        let clock = if self == Self::Intermittent {
            clock.rem_euclid(6.0)
        } else {
            clock
        };
        let t = clock.rem_euclid(self.sweep_s()) / self.sweep_s();
        if t <= 0.5 { t * 2.0 } else { (1.0 - t) * 2.0 }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modes_have_distinct_pause_safe_cadence_and_serialize() {
        assert_eq!(WiperMode::Slow.phase(0.9), 1.0);
        assert_eq!(WiperMode::Fast.phase(0.45), 1.0);
        assert_eq!(WiperMode::Intermittent.phase(3.0), 0.0);
        assert!(!WiperMode::Intermittent.moving(3.0));
        assert!(WiperMode::Intermittent.moving(6.9));
        for mode in [
            WiperMode::Off,
            WiperMode::Intermittent,
            WiperMode::Slow,
            WiperMode::Fast,
        ] {
            let restored: WiperMode =
                serde_json::from_str(&serde_json::to_string(&mode).unwrap()).unwrap();
            for t in [0.0, 0.45, 0.9, 3.0, 1234.5] {
                assert_eq!(restored.phase(t), mode.phase(t));
            }
        }
    }
}
