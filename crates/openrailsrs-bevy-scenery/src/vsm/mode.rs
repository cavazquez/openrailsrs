//! Configuracion de sombras estilo Open Rails (VSM vs PCF Bevy).

/// Modo de sombras para materiales OR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OrVsmMode {
    /// PCF Bevy + modulacion OR (_PSGetShadowEffect con NormalLighting).
    #[default]
    PcfOr,
    /// Chebyshev OR con varianza estimada desde PCF multi-tap (sin textura de momentos).
    Approx,
    /// Reservado: momentos Rg32 + blur como OR (pass dedicado).
    Exact,
}

impl OrVsmMode {
    pub fn from_env() -> Self {
        match std::env::var("OPENRAILSRS_OR_VSM")
            .ok()
            .as_deref()
            .map(str::trim)
        {
            Some("approx") | Some("APPROX") => Self::Approx,
            Some("exact") | Some("EXACT") => Self::Exact,
            Some(s) if s.eq_ignore_ascii_case("approx") => Self::Approx,
            Some(s) if s.eq_ignore_ascii_case("exact") => Self::Exact,
            _ => Self::PcfOr,
        }
    }

    /// Valor empaquetado en `OrSceneryGpuParams.vsm_mode`.
    pub fn as_gpu(self) -> f32 {
        match self {
            Self::PcfOr => 0.0,
            Self::Approx => 1.0,
            Self::Exact => 2.0,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::PcfOr => Self::Approx,
            Self::Approx => Self::Exact,
            Self::Exact => Self::PcfOr,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::PcfOr => "pcf+or",
            Self::Approx => "approx",
            Self::Exact => "exact",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_pcf_or() {
        assert_eq!(OrVsmMode::from_env(), OrVsmMode::PcfOr);
    }

    #[test]
    fn approx_mode_value() {
        assert_eq!(OrVsmMode::Approx.as_gpu(), 1.0);
    }

    #[test]
    fn mode_cycles() {
        assert_eq!(OrVsmMode::PcfOr.next(), OrVsmMode::Approx);
        assert_eq!(OrVsmMode::Exact.next(), OrVsmMode::PcfOr);
    }
}
