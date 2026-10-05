//! Small deterministic procedural thunder source, generated in the audio worker.
//! A filtered broadband crack becomes a low-frequency rolling decay. No files or
//! extra output device are needed; native SMS and thunder share the same limiter.
#[derive(Clone, Copy, Debug)]
pub struct ThunderEvent {
    pub seed: u32,
    pub distance_m: f32,
}
pub const SAMPLE_RATE: u32 = 22050;
pub fn samples(seed: u32, distance_m: f32) -> Vec<f32> {
    let mut random = seed.wrapping_add(0x12345);
    let mut low = 0.0;
    let mut roll = 0.0;
    let distant = (distance_m / 2000.0).clamp(0., 1.);
    let mut samples = Vec::with_capacity(SAMPLE_RATE as usize * 6);
    for i in 0..SAMPLE_RATE * 6 {
        random ^= random << 13;
        random ^= random >> 17;
        random ^= random << 5;
        let noise = random as f32 / u32::MAX as f32 * 2. - 1.;
        low += (noise - low) * (0.12 - 0.08 * distant);
        roll += (noise - roll) * 0.016;
        let t = i as f32 / SAMPLE_RATE as f32;
        let attack = (t / 0.02).min(1.);
        let end = ((6. - t) / 0.7).clamp(0., 1.);
        let crack = low * (-t / (0.2 + 0.3 * distant)).exp();
        let rumble = roll * (-t / 1.9).exp() * (0.65 + 0.35 * (t * 8.0).sin());
        samples.push((crack * 0.9 + rumble * 3.0) * attack * end);
    }
    samples
}
pub fn gain(distance_m: f32, cab: bool) -> f32 {
    (1.0 / (1.0 + distance_m.max(0.) / 1200.0)) * if cab { 0.38 } else { 0.85 }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thunder_is_finite_bounded_deterministic_and_decays() {
        let a = samples(1, 1000.);
        assert_eq!(a, samples(1, 1000.));
        assert_eq!(a.len(), SAMPLE_RATE as usize * 6);
        assert!(a.iter().all(|v| v.is_finite() && v.abs() < 1.));
        let energy = |slice: &[f32]| slice.iter().map(|x| x * x).sum::<f32>() / slice.len() as f32;
        assert!(energy(&a[..SAMPLE_RATE as usize]) > energy(&a[5 * SAMPLE_RATE as usize..]) * 20.);
        assert!(gain(1000., true) < gain(1000., false));
        assert!(gain(1000., false) > gain(3000., false));
    }
}
