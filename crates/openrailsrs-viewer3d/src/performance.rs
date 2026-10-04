//! Bounded journey telemetry, independent of simulation time compression.
//! A fixed millisecond histogram avoids retaining an ever-growing frame log.
use bevy::prelude::*;
use serde::Serialize;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[derive(Resource, Clone, Default, bevy::render::extract_resource::ExtractResource)]
pub struct ScenePipelineStatus {
    counts: Arc<AtomicU64>,
    device: Arc<std::sync::Mutex<Option<RendererDevice>>>,
}
#[derive(Clone, Debug, Serialize)]
pub struct RendererDevice {
    pub name: String,
    pub backend: String,
    pub device_type: String,
    pub hardware: bool,
}
impl ScenePipelineStatus {
    pub fn device(&self) -> Option<RendererDevice> {
        self.device.lock().unwrap().clone()
    }
    pub fn counts(&self) -> (usize, usize) {
        let counts = self.counts.load(Ordering::Relaxed);
        ((counts >> 32) as usize, (counts as u32) as usize)
    }
    pub fn ready(&self) -> bool {
        self.counts() == (0, 0)
    }
}

pub fn update_pipeline_status(
    cache: Res<bevy::render::render_resource::PipelineCache>,
    status: Res<ScenePipelineStatus>,
    adapter: Res<bevy::render::renderer::RenderAdapterInfo>,
) {
    use bevy::render::render_resource::CachedPipelineState;
    if status.device.lock().unwrap().is_none() {
        *status.device.lock().unwrap() = Some(RendererDevice {
            name: adapter.name.clone(),
            backend: format!("{:?}", adapter.backend),
            device_type: format!("{:?}", adapter.device_type),
            hardware: matches!(
                format!("{:?}", adapter.device_type).as_str(),
                "DiscreteGpu" | "IntegratedGpu"
            ),
        });
    }
    let (mut pending, mut failed) = (0, 0);
    for pipeline in cache.pipelines() {
        match pipeline.state {
            CachedPipelineState::Ok(_) => {}
            CachedPipelineState::Queued | CachedPipelineState::Creating(_) => pending += 1,
            CachedPipelineState::Err(_) => failed += 1,
        }
    }
    status
        .counts
        .store((pending << 32) | failed, Ordering::Relaxed);
}

/// Opt-in diagnostics include unresolved imports that Bevy retries silently.
pub fn log_shader_pipeline_status(
    pipelines: Res<bevy::render::render_resource::PipelineCache>,
    mut previous: Local<Option<std::time::Instant>>,
) {
    use bevy::render::render_resource::CachedPipelineState;
    let now = std::time::Instant::now();
    if previous.is_some_and(|last| now.duration_since(last).as_secs_f32() < 5.0) {
        return;
    }
    *previous = Some(now);
    let mut ready = 0;
    let mut pending = 0;
    for (index, cached) in pipelines.pipelines().enumerate() {
        match &cached.state {
            CachedPipelineState::Ok(_) => ready += 1,
            CachedPipelineState::Queued | CachedPipelineState::Creating(_) => pending += 1,
            CachedPipelineState::Err(error) => {
                crate::viewer_log!("shader pipeline {index}: {error:?}")
            }
        }
    }
    crate::viewer_log!("shader pipelines: {ready} ready, {pending} pending");
}

pub fn log_shader_dependencies(
    shaders: Res<Assets<bevy::shader::Shader>>,
    mut previous: Local<Option<std::time::Instant>>,
) {
    let now = std::time::Instant::now();
    if previous.is_some_and(|last| now.duration_since(last).as_secs_f32() < 5.0) {
        return;
    }
    *previous = Some(now);
    let loaded: std::collections::HashSet<_> =
        shaders.iter().map(|(_, s)| &s.import_path).collect();
    for (id, shader) in shaders
        .iter()
        .filter(|(_, s)| s.path.starts_with("shaders/"))
    {
        crate::viewer_log!(
            "shader source {id:?}: {} {:?} imports {:?}",
            shader.path,
            shader.import_path,
            shader.imports
        );
        for import in &shader.imports {
            if !loaded.contains(import) {
                crate::viewer_log!("shader dependency missing: {} -> {import:?}", shader.path);
            }
        }
    }
}

#[derive(Resource)]
pub struct JourneyPerformance {
    histogram: [u64; 4097],
    frames: u64,
    loading_frames: u64,
    elapsed_s: f64,
    longest_ms: f64,
    hitches: u64,
    rss_clock_s: f64,
    rss_mib: Option<f64>,
    peak_rss_mib: Option<f64>,
}
impl Default for JourneyPerformance {
    fn default() -> Self {
        Self {
            histogram: [0; 4097],
            frames: 0,
            loading_frames: 0,
            elapsed_s: 0.0,
            longest_ms: 0.0,
            hitches: 0,
            rss_clock_s: 0.0,
            rss_mib: None,
            peak_rss_mib: None,
        }
    }
}

#[derive(Serialize)]
pub struct JourneyPerformanceReport {
    pub frames: u64,
    pub loading_frames: u64,
    pub elapsed_s: f64,
    pub frame_p50_ms: u32,
    pub frame_p95_ms: u32,
    pub frame_p99_ms: u32,
    pub longest_frame_ms: f64,
    pub hitches_over_100_ms: u64,
    pub rss_mib: Option<f64>,
    pub peak_rss_mib: Option<f64>,
}
impl JourneyPerformance {
    fn record(&mut self, seconds: f64, loading: bool) {
        if !seconds.is_finite() || seconds <= 0.0 {
            return;
        }
        let ms = seconds * 1000.0;
        self.histogram[(ms.round() as usize).min(4096)] += 1;
        self.frames += 1;
        self.loading_frames += u64::from(loading);
        self.elapsed_s += seconds;
        self.longest_ms = self.longest_ms.max(ms);
        self.hitches += u64::from(ms > 100.0);
    }
    fn percentile(&self, fraction: f64) -> u32 {
        let target = (self.frames as f64 * fraction).ceil() as u64;
        if target == 0 {
            return 0;
        }
        let mut count = 0;
        for (ms, frames) in self.histogram.iter().enumerate() {
            count += frames;
            if count >= target {
                return ms as u32;
            }
        }
        4096
    }
    pub fn report(&self) -> JourneyPerformanceReport {
        JourneyPerformanceReport {
            frames: self.frames,
            loading_frames: self.loading_frames,
            elapsed_s: self.elapsed_s,
            frame_p50_ms: self.percentile(0.5),
            frame_p95_ms: self.percentile(0.95),
            frame_p99_ms: self.percentile(0.99),
            longest_frame_ms: self.longest_ms,
            hitches_over_100_ms: self.hitches,
            rss_mib: self.rss_mib,
            peak_rss_mib: self.peak_rss_mib,
        }
    }
    pub fn hud_text(&self) -> String {
        let p = self.report();
        let ram =
            |value: Option<f64>| value.map_or_else(|| "—".to_string(), |v| format!("{v:.0} MiB"));
        format!(
            "Viaje: P50 {} ms · P95 {} ms · P99 {} ms\nMáximo {:.0} ms · {} cuadros >100 ms · {} cuadros de carga\nRAM {} · pico {}",
            p.frame_p50_ms,
            p.frame_p95_ms,
            p.frame_p99_ms,
            p.longest_frame_ms,
            p.hitches_over_100_ms,
            p.loading_frames,
            ram(p.rss_mib),
            ram(p.peak_rss_mib)
        )
    }
}

pub fn measure_journey(
    time: Res<Time<Real>>,
    progress: Option<Res<crate::world::WorldSpawnProgress>>,
    mut metrics: ResMut<JourneyPerformance>,
) {
    metrics.record(time.delta_secs_f64(), progress.is_some());
    metrics.rss_clock_s += time.delta_secs_f64();
    if metrics.rss_clock_s < 1.0 {
        return;
    }
    metrics.rss_clock_s = 0.0;
    // Linux native QA. Other platforms keep this field explicitly unavailable.
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        let rss = status
            .lines()
            .find_map(|line| {
                line.strip_prefix("VmRSS:")?
                    .split_whitespace()
                    .next()?
                    .parse::<f64>()
                    .ok()
            })
            .map(|kib| kib / 1024.0);
        metrics.rss_mib = rss;
        if let Some(rss) = rss {
            metrics.peak_rss_mib = Some(metrics.peak_rss_mib.unwrap_or(0.0).max(rss));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_readiness_rejects_uncompiled_and_failed_shaders() {
        let status = ScenePipelineStatus::default();
        let extracted = status.clone();
        status.counts.store(4 << 32, Ordering::Relaxed);
        assert_eq!(extracted.counts(), (4, 0));
        assert!(!extracted.ready());
        status.counts.store(1, Ordering::Relaxed);
        assert_eq!(extracted.counts(), (0, 1));
        assert!(!extracted.ready());
        status.counts.store(0, Ordering::Relaxed);
        assert!(extracted.ready());
    }

    #[test]
    fn full_journey_histogram_counts_hitches_without_unbounded_storage() {
        let mut m = JourneyPerformance::default();
        for _ in 0..95 {
            m.record(0.016, false);
        }
        for _ in 0..4 {
            m.record(0.060, true);
        }
        m.record(0.8, true);
        let report = m.report();
        assert_eq!(report.frame_p50_ms, 16);
        assert_eq!(report.frame_p95_ms, 16);
        assert_eq!(report.frame_p99_ms, 60);
        assert_eq!(report.longest_frame_ms, 800.0);
        assert_eq!(report.loading_frames, 5);
        assert_eq!(report.hitches_over_100_ms, 1);
    }
}
