//! Quiet filtered ambience, separate from authored locomotive SMS/WAV voices.
pub const SAMPLE_RATE: u32 = 22_050;
pub fn samples(wind: bool) -> Vec<f32> {
    let mut seed = 0x41c64e6du32;
    let mut low = 0.;
    let mut band = 0.;
    let mut samples = Vec::with_capacity(SAMPLE_RATE as usize * 4);
    for i in 0..SAMPLE_RATE * 4 {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let noise = seed as f64 / u32::MAX as f64 * 2. - 1.;
        low = low * 0.985 + noise as f32 * 0.015;
        band = band * 0.62 + noise as f32 * 0.38;
        // Short fades avoid a click where the bounded recording loops.
        let edge = (i as f32 / 256.)
            .min((SAMPLE_RATE * 4 - i) as f32 / 256.)
            .clamp(0., 1.);
        samples.push(if wind {
            low * 2.4 * edge
        } else {
            (band - low) * 0.18 * edge
        });
    }
    samples
}
pub fn gains(rain: f32, wind: f32, cab: bool, volume: f32, paused: bool) -> (f32, f32) {
    if paused {
        return (0., 0.);
    }
    let volume = volume.clamp(0., 1.);
    (
        rain.clamp(0., 1.).sqrt() * volume * if cab { 0.35 } else { 0.75 },
        (wind / 18.).clamp(0., 1.) * volume * if cab { 0.10 } else { 0.35 },
    )
}
/// The effective surface belongs to the listener, not the global weather.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RainSurface {
    #[default]
    Outside,
    Glass,
    MetalRoof,
    Shelter,
}
impl RainSurface {
    pub fn gain(self) -> f32 {
        match self {
            Self::Outside => 1.0,
            Self::Glass => 0.7,
            Self::MetalRoof => 0.45,
            Self::Shelter => 0.12,
        }
    }
}

impl RainSurface {
    pub const ALL: [Self; 4] = [Self::Outside, Self::Glass, Self::MetalRoof, Self::Shelter];
    pub fn index(self) -> usize {
        match self {
            Self::Outside => 0,
            Self::Glass => 1,
            Self::MetalRoof => 2,
            Self::Shelter => 3,
        }
    }
    pub fn samples(self) -> Vec<f32> {
        let mut filtered = 0.0;
        let mut ring = 0.0;
        samples(false)
            .into_iter()
            .enumerate()
            .map(|(i, s)| {
                let low = match self {
                    Self::Glass => 0.45,
                    Self::MetalRoof => 0.94,
                    Self::Shelter => 0.985,
                    Self::Outside => 0.62,
                };
                filtered = filtered * low + s * (1. - low);
                let pulse = if i % 313 == 0 { s * 0.4 } else { 0. };
                ring = ring * 0.998 + pulse;
                match self {
                    Self::Glass => filtered * 0.9 + ring * (i as f32 * 0.71).sin(),
                    Self::MetalRoof => filtered * 2.6 + ring * (i as f32 * 0.083).sin(),
                    Self::Shelter => filtered * 1.2,
                    Self::Outside => s,
                }
            })
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seeded_ambience_is_bounded_and_respects_intensity_mute_and_pause() {
        let rain = samples(false);
        assert_eq!(rain, samples(false));
        assert_ne!(rain, samples(true));
        assert!(rain.iter().all(|s| s.is_finite() && s.abs() < 0.4));
        assert!(gains(0.2, 1., false, 1., false).0 < gains(1., 1., false, 1., false).0);
        assert_eq!(gains(1., 18., true, 0., false), (0., 0.));
        assert_eq!(gains(1., 18., true, 1., true), (0., 0.));
    }

    #[test]
    fn plan30_rain_surface_banks_are_distinct_bounded_and_reproducible() {
        let banks = RainSurface::ALL.map(RainSurface::samples);
        for (i, bank) in banks.iter().enumerate() {
            assert_eq!(bank.len(), SAMPLE_RATE as usize * 4);
            assert_eq!(*bank, RainSurface::ALL[i].samples());
            assert!(bank.iter().all(|x| x.is_finite() && x.abs() < 0.5));
            for other in banks.iter().skip(i + 1) {
                assert_ne!(bank, other);
            }
        }
        assert!(RainSurface::Shelter.gain() < RainSurface::Glass.gain());
    }
}
