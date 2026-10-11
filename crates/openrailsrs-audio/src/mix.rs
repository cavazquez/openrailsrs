//! Player mixing preferences. Authored SMS curves remain the source of gain.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundGroup {
    Motor,
    Rolling,
    Cab,
    Ambient,
    Announcements,
}
impl SoundGroup {
    pub const ALL: [Self; 5] = [
        Self::Motor,
        Self::Rolling,
        Self::Cab,
        Self::Ambient,
        Self::Announcements,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Motor => "Motor",
            Self::Rolling => "Rodadura",
            Self::Cab => "Cabina y controles",
            Self::Ambient => "Ambiente",
            Self::Announcements => "Anuncios",
        }
    }
    pub fn index(self) -> usize {
        self as usize
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundMix {
    pub gains: [f32; 5],
}
impl Default for SoundMix {
    fn default() -> Self {
        Self { gains: [1.0; 5] }
    }
}
impl SoundMix {
    pub fn gain(self, group: SoundGroup) -> f32 {
        self.gains[group.index()].clamp(0.0, 1.0)
    }
    pub fn valid(self) -> bool {
        self.gains
            .iter()
            .all(|x| x.is_finite() && (0.0..=1.0).contains(x))
    }
}

/// Stream names are not standardized. Classify from authored sample names,
/// then fall back to the controls/event IDs present in the SMS stream.
pub fn classify(stream: &crate::sms::Stream, interior: bool) -> SoundGroup {
    use crate::sms::{Command, Control, TriggerKind};
    if stream
        .triggers
        .iter()
        .any(|t| matches!(t.kind, TriggerKind::Discrete(6 | 7)))
    {
        return SoundGroup::Cab;
    }
    let names = stream
        .triggers
        .iter()
        .flat_map(|t| &t.commands)
        .filter_map(|c| {
            if let Command::Play { files, .. } = c {
                Some(files)
            } else {
                None
            }
        })
        .flatten()
        .map(|s| s.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    if ["announce", "announcement", "anuncio", "stationvoice"]
        .iter()
        .any(|s| names.contains(s))
    {
        return SoundGroup::Announcements;
    }
    if [
        "rail", "wheel", "roll", "squeal", "curve", "track", "joint", "coupl",
    ]
    .iter()
    .any(|s| names.contains(s))
    {
        return SoundGroup::Rolling;
    }
    if [
        "engine", "motor", "diesel", "turbo", "rpm", "idle", "exhaust", "chuff",
    ]
    .iter()
    .any(|s| names.contains(s))
    {
        return SoundGroup::Motor;
    }
    if stream.volumes.iter().any(|c| {
        matches!(
            c.control,
            Control::Variable1 | Control::Variable2 | Control::Variable3
        )
    }) || stream.frequency.as_ref().is_some_and(|c| {
        matches!(
            c.control,
            Control::Variable1 | Control::Variable2 | Control::Variable3
        )
    }) {
        return SoundGroup::Motor;
    }
    if stream.volumes.iter().any(|c| c.control == Control::Speed) {
        return SoundGroup::Rolling;
    }
    if interior
        || stream
            .triggers
            .iter()
            .any(|t| matches!(t.kind, TriggerKind::Discrete(4..=17 | 30..=37)))
    {
        SoundGroup::Cab
    } else {
        SoundGroup::Ambient
    }
}

pub fn approach(current: f32, target: f32, dt: f32, seconds: f32) -> f32 {
    if dt <= 0.0 || !dt.is_finite() {
        return current;
    }
    current + (target - current) * (1.0 - (-dt / seconds.max(0.001)).exp())
}

/// No filesystem prefixes or personally identifying directory names in reports.
pub fn resource_name(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .unwrap_or("recurso")
        .chars()
        .filter(|c| !c.is_control())
        .take(160)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crossfade_is_frame_independent_and_preferences_validate() {
        let one = approach(0.0, 1.0, 1.0, 0.18);
        let mut many = 0.0;
        for _ in 0..120 {
            many = approach(many, 1.0, 1.0 / 120.0, 0.18);
        }
        assert!((one - many).abs() < 1e-6);
        assert_eq!(approach(many, 0.0, 0.0, 0.18), many);
        let mut mix = SoundMix::default();
        mix.gains[SoundGroup::Cab.index()] = 0.0;
        assert_eq!(mix.gain(SoundGroup::Motor), 1.0);
        assert!(mix.valid());
        mix.gains[0] = f32::NAN;
        assert!(!mix.valid());
        assert_eq!(
            resource_name("C:\\Users\\personal\\Sound\\engine.wav"),
            "engine.wav"
        );
    }
}
