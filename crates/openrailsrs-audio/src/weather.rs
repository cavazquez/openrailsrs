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
}
