//! Bounded acoustic presentation; the simulator supplies openings and cover.
use rodio::Source;
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};
pub fn window_gain(closed: f32, opening: f32) -> f32 {
    closed.clamp(0., 1.) + (1. - closed.clamp(0., 1.)) * opening.clamp(0., 1.)
}
pub fn horn_gain(front_cosine: f32, blocked: bool) -> f32 {
    (0.35 + 0.65 * ((front_cosine.clamp(-1., 1.) + 1.) * 0.5).powi(2))
        * if blocked { 0.3 } else { 1. }
}
/// Two stereo delay taps, no feedback: finite tail, fixed memory, no overload.
/// Smooth wet changes on the audio clock. Signals retain the dry channel.
pub struct CoveredSource<S: Source<Item = f32>> {
    source: S,
    wet: Arc<AtomicU32>,
    current: f32,
    delays: Vec<f32>,
    index: usize,
    tap: usize,
    channels: usize,
    tail: usize,
}
impl<S: Source<Item = f32>> CoveredSource<S> {
    pub fn new(source: S, wet: Arc<AtomicU32>) -> Self {
        let channels = source.channels().get() as usize;
        let tap = ((source.sample_rate().get() as f32 * 0.061) as usize).max(1) * channels;
        Self {
            source,
            wet,
            current: 0.,
            delays: vec![0.; tap * 3],
            index: 0,
            tap,
            channels,
            tail: 0,
        }
    }
}
impl<S: Source<Item = f32>> Iterator for CoveredSource<S> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let dry = if let Some(v) = self.source.next() {
            v
        } else {
            if self.tail >= self.delays.len() {
                return None;
            }
            self.tail += 1;
            0.
        };
        let target = f32::from_bits(self.wet.load(Ordering::Relaxed)).clamp(0., 0.28);
        self.current += (target - self.current) * 0.0002;
        let len = self.delays.len();
        let a = self.delays[(self.index + len - self.tap) % len];
        let b = self.delays[(self.index + len - 2 * self.tap) % len];
        self.delays[self.index] = dry;
        self.index = (self.index + 1) % len;
        Some(dry + self.current * (a * 0.65 + b * 0.35))
    }
}
impl<S: Source<Item = f32>> Source for CoveredSource<S> {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        rodio::ChannelCount::new(self.channels as u16).unwrap()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        self.source.sample_rate()
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aperture_and_direction_use_actual_state() {
        assert_eq!(window_gain(0.25, 0.), 0.25);
        assert_eq!(window_gain(0.25, 1.), 1.);
        assert!(horn_gain(1., false) > horn_gain(-1., false));
        assert!(horn_gain(1., true) < horn_gain(-1., false));
    }
    #[test]
    fn covered_impulse_has_bounded_tail_and_preserves_stereo() {
        let mut data = vec![0.; 9600];
        data[4000] = 0.5;
        data[4001] = -0.5;
        let wet = Arc::new(AtomicU32::new(0.28f32.to_bits()));
        let s = rodio::buffer::SamplesBuffer::new(
            rodio::ChannelCount::new(2).unwrap(),
            rodio::SampleRate::new(48000).unwrap(),
            data,
        );
        let out: Vec<_> = CoveredSource::new(s, wet).collect();
        assert_eq!(out[4000], 0.5);
        assert!(out[9856] > 0.01);
        assert!(out[9857] < -0.01);
        assert!(out.iter().all(|s| s.abs() <= 0.5));
        assert!(out.len() < 30000);
    }
}
