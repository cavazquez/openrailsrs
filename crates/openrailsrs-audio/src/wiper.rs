//! Mechanical fallback for content without a playable SMS wiper event.
//! One round trip takes 1.8 seconds, shared with the windscreen blade animation.
pub const PERIOD_S: f64 = 1.8;
pub const SAMPLE_RATE: u32 = 22050;

pub fn phase(time_s: f64) -> f64 {
    1.0 - ((time_s / PERIOD_S).rem_euclid(1.0) * 2.0 - 1.0).abs()
}

/// Motor hum, rubber scrape and a quiet gearbox click at each reversal.
/// The loop contains an integer number of motor periods and fades its seam.
pub fn samples(start_time_s: f64) -> Vec<f32> {
    let count = (PERIOD_S * f64::from(SAMPLE_RATE)).round() as usize;
    let offset = (start_time_s.rem_euclid(PERIOD_S) * f64::from(SAMPLE_RATE)) as usize;
    (0..count)
        .map(|i| {
            let index = (i + offset) % count;
            let t = index as f64 / f64::from(SAMPLE_RATE);
            let stroke = (t / (PERIOD_S * 0.5)).fract();
            let movement = (std::f64::consts::PI * stroke).sin();
            let mut hash = (index as u32).wrapping_add(0x9e3779b9);
            hash = (hash ^ (hash >> 16)).wrapping_mul(0x85ebca6b);
            let noise = (hash as f64 / f64::from(u32::MAX) * 2.0 - 1.0) as f32;
            let motor = (std::f64::consts::TAU * 170.0 * t).sin() as f32;
            let click =
                (std::f64::consts::TAU * 900.0 * t).sin() as f32 * (-stroke * 70.0).exp() as f32;
            0.06 * motor * movement as f32
                + 0.12 * noise * (movement * movement) as f32
                + 0.06 * click
        })
        .collect()
}
