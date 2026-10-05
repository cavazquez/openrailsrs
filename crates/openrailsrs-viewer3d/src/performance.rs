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
    uploads: Arc<AtomicU64>,
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
        self.counts() == (0, 0) && self.pending_uploads() == 0
    }
    pub fn pending_uploads(&self) -> usize {
        self.uploads.load(Ordering::Relaxed) as usize
    }
}

#[derive(Resource, Default)]
pub struct RequiredRenderAssets {
    meshes: Vec<bevy::asset::AssetId<Mesh>>,
    images: Vec<bevy::asset::AssetId<Image>>,
    renderables: Vec<(
        bevy::render::sync_world::MainEntity,
        bevy::asset::AssetId<Mesh>,
    )>,
}

pub fn extract_required_render_assets(
    meshes: bevy::render::Extract<Res<Assets<Mesh>>>,
    images: bevy::render::Extract<Res<Assets<Image>>>,
    renderables: bevy::render::Extract<Query<(Entity, &Mesh3d)>>,
    mut required: ResMut<RequiredRenderAssets>,
) {
    use bevy::asset::RenderAssetUsages;
    required.meshes.clear();
    required.images.clear();
    required.renderables.clear();
    required.renderables.extend(
        renderables
            .iter()
            .map(|(entity, mesh)| (entity.into(), mesh.id())),
    );
    required
        .meshes
        .extend(meshes.iter().filter_map(|(id, mesh)| {
            mesh.asset_usage
                .contains(RenderAssetUsages::RENDER_WORLD)
                .then_some(id)
        }));
    required
        .images
        .extend(images.iter().filter_map(|(id, image)| {
            image
                .asset_usage
                .contains(RenderAssetUsages::RENDER_WORLD)
                .then_some(id)
        }));
}

/// Bevy 0.19 skips specialization when a visible mesh is still in the upload
/// queue, without adding that mesh to its pending-material queue. Retry once
/// when its GPU asset becomes available, including after streamed replacements.
pub fn retry_uploaded_mesh_specializations(
    required: Res<RequiredRenderAssets>,
    meshes: Res<bevy::render::render_asset::RenderAssets<bevy::render::mesh::RenderMesh>>,
    mut previous: Local<std::collections::HashSet<bevy::asset::AssetId<Mesh>>>,
    mut dirty: ResMut<bevy::render::camera::DirtySpecializations>,
) {
    let added = newly_ready_meshes(
        &required.meshes,
        |id| meshes.get(id).is_some(),
        &mut previous,
    );
    for &(entity, mesh) in &required.renderables {
        if added.contains(&mesh) {
            dirty.changed_renderables.insert(entity);
        }
    }
}

fn newly_ready_meshes(
    required: &[bevy::asset::AssetId<Mesh>],
    is_ready: impl Fn(bevy::asset::AssetId<Mesh>) -> bool,
    previous: &mut std::collections::HashSet<bevy::asset::AssetId<Mesh>>,
) -> std::collections::HashSet<bevy::asset::AssetId<Mesh>> {
    let ready: std::collections::HashSet<_> = required
        .iter()
        .copied()
        .filter(|id| is_ready(*id))
        .collect();
    let added = ready.difference(previous).copied().collect();
    *previous = ready;
    added
}

pub fn update_pipeline_status(
    cache: Res<bevy::render::render_resource::PipelineCache>,
    status: Res<ScenePipelineStatus>,
    adapter: Res<bevy::render::renderer::RenderAdapterInfo>,
    required: Res<RequiredRenderAssets>,
    meshes: Res<bevy::render::render_asset::RenderAssets<bevy::render::mesh::RenderMesh>>,
    images: Res<bevy::render::render_asset::RenderAssets<bevy::render::texture::GpuImage>>,
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
    let pending_uploads = required
        .meshes
        .iter()
        .filter(|id| meshes.get(**id).is_none())
        .count()
        + required
            .images
            .iter()
            .filter(|id| images.get(**id).is_none())
            .count();
    status
        .uploads
        .store(pending_uploads as u64, Ordering::Relaxed);
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
    gameplay_histogram: [u64; 4097],
    gameplay_frames: u64,
    frames: u64,
    loading_frames: u64,
    elapsed_s: f64,
    longest_ms: f64,
    hitches: u64,
    rss_clock_s: f64,
    rss_mib: Option<f64>,
    peak_rss_mib: Option<f64>,
    gameplay_longest_ms: f64,
    gameplay_hitches: u64,
    startup_longest_ms: f64,
}
impl Default for JourneyPerformance {
    fn default() -> Self {
        Self {
            histogram: [0; 4097],
            gameplay_histogram: [0; 4097],
            gameplay_frames: 0,
            frames: 0,
            loading_frames: 0,
            elapsed_s: 0.0,
            longest_ms: 0.0,
            hitches: 0,
            rss_clock_s: 0.0,
            rss_mib: None,
            peak_rss_mib: None,
            gameplay_longest_ms: 0.0,
            gameplay_hitches: 0,
            startup_longest_ms: 0.0,
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
    pub gameplay_frames: u64,
    pub gameplay_frame_p50_ms: u32,
    pub gameplay_frame_p95_ms: u32,
    pub gameplay_frame_p99_ms: u32,
    pub longest_frame_ms: f64,
    pub hitches_over_100_ms: u64,
    pub rss_mib: Option<f64>,
    pub peak_rss_mib: Option<f64>,
    pub gameplay_longest_frame_ms: f64,
    pub gameplay_hitches_over_100_ms: u64,
    pub startup_longest_frame_ms: f64,
}
impl JourneyPerformance {
    fn record(&mut self, seconds: f64, loading: bool, startup: bool) {
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
        if startup {
            self.startup_longest_ms = self.startup_longest_ms.max(ms);
        } else {
            self.gameplay_histogram[(ms.round() as usize).min(4096)] += 1;
            self.gameplay_frames += 1;
            self.gameplay_longest_ms = self.gameplay_longest_ms.max(ms);
            self.gameplay_hitches += u64::from(ms > 100.0);
        }
    }
    fn percentile(histogram: &[u64; 4097], frames: u64, fraction: f64) -> u32 {
        let target = (frames as f64 * fraction).ceil() as u64;
        if target == 0 {
            return 0;
        }
        let mut count = 0;
        for (ms, frames) in histogram.iter().enumerate() {
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
            frame_p50_ms: Self::percentile(&self.histogram, self.frames, 0.5),
            frame_p95_ms: Self::percentile(&self.histogram, self.frames, 0.95),
            frame_p99_ms: Self::percentile(&self.histogram, self.frames, 0.99),
            gameplay_frames: self.gameplay_frames,
            gameplay_frame_p50_ms: Self::percentile(
                &self.gameplay_histogram,
                self.gameplay_frames,
                0.5,
            ),
            gameplay_frame_p95_ms: Self::percentile(
                &self.gameplay_histogram,
                self.gameplay_frames,
                0.95,
            ),
            gameplay_frame_p99_ms: Self::percentile(
                &self.gameplay_histogram,
                self.gameplay_frames,
                0.99,
            ),
            longest_frame_ms: self.longest_ms,
            hitches_over_100_ms: self.hitches,
            rss_mib: self.rss_mib,
            peak_rss_mib: self.peak_rss_mib,
            gameplay_longest_frame_ms: self.gameplay_longest_ms,
            gameplay_hitches_over_100_ms: self.gameplay_hitches,
            startup_longest_frame_ms: self.startup_longest_ms,
        }
    }
    pub fn hud_text(&self) -> String {
        let p = self.report();
        let ram =
            |value: Option<f64>| value.map_or_else(|| "—".to_string(), |v| format!("{v:.0} MiB"));
        format!(
            "Viaje: P50 {} ms · P95 {} ms · P99 {} ms\nMáximo {:.0} ms · {} cuadros >100 ms · {} cuadros de carga\nRAM {} · pico {}",
            p.gameplay_frame_p50_ms,
            p.gameplay_frame_p95_ms,
            p.gameplay_frame_p99_ms,
            p.gameplay_longest_frame_ms,
            p.gameplay_hitches_over_100_ms,
            p.loading_frames,
            ram(p.rss_mib),
            ram(p.peak_rss_mib)
        )
    }
}

pub fn measure_journey(
    time: Res<Time<Real>>,
    progress: Option<Res<crate::world::WorldSpawnProgress>>,
    terrain: Option<Res<crate::terrain_spawn::TerrainSpawnProgress>>,
    stream: Option<Res<crate::terrain_spawn::TerrainTileStream>>,
    startup: Option<Res<crate::route_bootstrap::ViewerLoadingScreen>>,
    mut metrics: ResMut<JourneyPerformance>,
    mut graphics: ResMut<crate::gpu_memory::GraphicsMemory>,
) {
    let initial_load = startup.is_some() || metrics.frames == 0;
    metrics.record(
        time.delta_secs_f64(),
        progress.is_some()
            || terrain.is_some()
            || stream.is_some_and(|stream| stream.pending_work() > 0)
            || initial_load,
        initial_load,
    );
    metrics.rss_clock_s += time.delta_secs_f64();
    if metrics.rss_clock_s < 1.0 {
        return;
    }
    metrics.rss_clock_s = 0.0;
    graphics.sample();
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
    fn delayed_mesh_upload_and_replacement_retry_once_when_ready() {
        let mut meshes = Assets::<Mesh>::default();
        let id = meshes.add(Cuboid::default()).id();
        let required = [id];
        let mut previous = std::collections::HashSet::new();
        assert!(newly_ready_meshes(&required, |_| false, &mut previous).is_empty());
        assert!(newly_ready_meshes(&required, |_| true, &mut previous).contains(&id));
        assert!(newly_ready_meshes(&required, |_| true, &mut previous).is_empty());
        assert!(newly_ready_meshes(&required, |_| false, &mut previous).is_empty());
        assert!(newly_ready_meshes(&required, |_| true, &mut previous).contains(&id));
        assert!(newly_ready_meshes(&[], |_| true, &mut previous).is_empty());
        assert!(previous.is_empty());
    }
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
        status.uploads.store(2, Ordering::Relaxed);
        assert_eq!(extracted.pending_uploads(), 2);
        assert!(!extracted.ready());
        status.uploads.store(0, Ordering::Relaxed);
        assert!(extracted.ready());
    }

    #[test]
    fn full_journey_histogram_counts_hitches_without_unbounded_storage() {
        let mut m = JourneyPerformance::default();
        for _ in 0..95 {
            m.record(0.016, false, false);
        }
        for _ in 0..4 {
            m.record(0.060, true, true);
        }
        m.record(0.8, true, true);
        let report = m.report();
        assert_eq!(report.frame_p50_ms, 16);
        assert_eq!(report.frame_p95_ms, 16);
        assert_eq!(report.frame_p99_ms, 60);
        assert_eq!(report.longest_frame_ms, 800.0);
        assert_eq!(report.loading_frames, 5);
        assert_eq!(report.hitches_over_100_ms, 1);
        assert_eq!(report.gameplay_frames, 95);
        assert_eq!(report.gameplay_frame_p99_ms, 16);
        assert_eq!(report.gameplay_hitches_over_100_ms, 0);
        // Streaming after startup remains part of gameplay performance.
        m.record(0.12, true, false);
        assert_eq!(m.report().gameplay_hitches_over_100_ms, 1);
    }
}
