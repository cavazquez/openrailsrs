//! Execution policy for precipitation, independent of the graphics adapter.
use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RendererSelection {
    #[default]
    Auto,
    Gpu,
    Cpu,
}
impl RendererSelection {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "gpu" => Some(Self::Gpu),
            "cpu" | "software" => Some(Self::Cpu),
            _ => None,
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Auto => Self::Gpu,
            Self::Gpu => Self::Cpu,
            Self::Cpu => Self::Auto,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Automático",
            Self::Gpu => "GPU",
            Self::Cpu => "CPU por software",
        }
    }
}

/// An explicit hardware choice must not silently benchmark a software adapter.
pub fn verify_renderer(
    selected: bevy::prelude::Res<RendererSelection>,
    status: bevy::prelude::Res<crate::performance::ScenePipelineStatus>,
    mut checked: bevy::prelude::Local<bool>,
    mut exit: bevy::prelude::MessageWriter<bevy::app::AppExit>,
) {
    if *checked {
        return;
    }
    let Some(device) = status.device() else {
        return;
    };
    *checked = true;
    if *selected == RendererSelection::Gpu && !device.hardware {
        eprintln!(
            "Se pidió renderizado GPU, pero el adaptador es {} ({}). Usar --renderer auto o --renderer cpu, o revisar el controlador.",
            device.name, device.device_type
        );
        exit.write(bevy::app::AppExit::error());
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeatherExecution {
    #[default]
    Auto,
    Gpu,
    Cpu,
    Hybrid,
}
impl WeatherExecution {
    pub fn next(self) -> Self {
        match self {
            Self::Auto => Self::Gpu,
            Self::Gpu => Self::Cpu,
            Self::Cpu => Self::Hybrid,
            Self::Hybrid => Self::Auto,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Automático",
            Self::Gpu => "GPU",
            Self::Cpu => "CPU",
            Self::Hybrid => "Mixto",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "gpu" => Some(Self::Gpu),
            "cpu" => Some(Self::Cpu),
            "hybrid" | "mixed" => Some(Self::Hybrid),
            _ => None,
        }
    }
    pub fn from_env() -> Option<Self> {
        std::env::var("OPENRAILSRS_WEATHER_EXECUTION")
            .ok()
            .and_then(|s| Self::parse(&s))
    }
    pub fn resolved(self, hardware: bool, pressure: bool) -> Self {
        if !hardware {
            return Self::Cpu;
        }
        match self {
            Self::Auto if pressure => Self::Hybrid,
            Self::Auto => Self::Gpu,
            other => other,
        }
    }
}

/// Three bounded quality levels. CPU work cannot repair a fill-rate bottleneck;
/// under pressure, also lower the number of transparent fragments and vertices.
#[derive(Debug, Default)]
pub struct AdaptiveWeather {
    pub level: usize,
    slow_s: f32,
    fast_s: f32,
    filtered_ms: f32,
}
impl AdaptiveWeather {
    pub fn observe(&mut self, dt: f32, loading: bool, pressure: bool) {
        if loading || !dt.is_finite() || dt <= 0.0 || dt > 0.5 {
            return;
        }
        self.filtered_ms = if self.filtered_ms == 0.0 {
            dt * 1000.0
        } else {
            self.filtered_ms * 0.95 + dt * 1000.0 * 0.05
        };
        if self.filtered_ms > 38.0 || pressure {
            self.slow_s += dt;
            self.fast_s = 0.0;
            if self.slow_s >= 2.0 {
                self.level = (self.level + 1).min(2);
                self.slow_s = 0.0;
            }
        } else if self.filtered_ms < 25.0 {
            self.fast_s += dt;
            self.slow_s = 0.0;
            if self.fast_s >= 10.0 {
                self.level = self.level.saturating_sub(1);
                self.fast_s = 0.0;
            }
        } else {
            self.slow_s = 0.0;
            self.fast_s = 0.0;
        }
    }
    pub fn counts(&self, mode: WeatherExecution) -> (usize, usize) {
        let factor = [1, 2, 4][self.level.min(2)];
        match mode {
            WeatherExecution::Gpu | WeatherExecution::Auto => (8192 / factor, 0),
            WeatherExecution::Cpu => (0, 2048 / factor),
            WeatherExecution::Hybrid => (3072 / factor, 1024 / factor),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pressure_uses_hybrid_and_software_never_selects_gpu_work() {
        assert_eq!(
            WeatherExecution::Auto.resolved(true, true),
            WeatherExecution::Hybrid
        );
        assert_eq!(
            WeatherExecution::Gpu.resolved(false, false),
            WeatherExecution::Cpu
        );
        let mut a = AdaptiveWeather::default();
        for _ in 0..200 {
            a.observe(0.05, false, false);
        }
        assert_eq!(a.level, 2);
        let old = a.counts(WeatherExecution::Hybrid);
        for _ in 0..300 {
            a.observe(0.02, true, false);
        }
        assert_eq!(a.counts(WeatherExecution::Hybrid), old);
        for _ in 0..1200 {
            a.observe(0.01, false, false);
        }
        assert!(a.level < 2);
    }
}
