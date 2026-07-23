//! OR `DriverMachineInterface.DMIMode` + screen sizes (#162).

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DmiMode {
    #[default]
    FullSize,
    SpeedArea,
    PlanningArea,
    GaugeOnly,
}

impl DmiMode {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "speedarea" | "speed" => Self::SpeedArea,
            "planningarea" | "planning" => Self::PlanningArea,
            "gaugeonly" | "gauge" => Self::GaugeOnly,
            _ => Self::FullSize,
        }
    }

    pub fn size(self) -> (u32, u32) {
        match self {
            Self::FullSize => (640, 480),
            Self::SpeedArea | Self::PlanningArea => (334, 480),
            Self::GaugeOnly => (280, 300),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_modes() {
        assert_eq!(DmiMode::parse("full"), DmiMode::FullSize);
        assert_eq!(DmiMode::parse("SpeedArea"), DmiMode::SpeedArea);
        assert_eq!(DmiMode::parse("gaugeonly"), DmiMode::GaugeOnly);
        assert_eq!(DmiMode::GaugeOnly.size(), (280, 300));
    }
}
