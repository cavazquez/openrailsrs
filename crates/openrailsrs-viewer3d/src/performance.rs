//! Bounded journey telemetry, independent of simulation time compression.
//! A fixed millisecond histogram avoids retaining an ever-growing frame log.
use bevy::prelude::*;
use serde::Serialize;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

const RECENT_SPAN_LIMIT: usize = 512;
const FRAME_CONTEXT_LIMIT: usize = 16;
const CONTEXT_SPAN_LIMIT: usize = 48;
static SPAN_CLOCK: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
static RECENT_SPANS: std::sync::OnceLock<std::sync::Mutex<std::collections::VecDeque<CpuSpan>>> =
    std::sync::OnceLock::new();

fn frame_spans_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("OPENRAILSRS_FRAME_SPANS").is_ok_and(|v| v == "1"))
}
fn span_seconds(instant: std::time::Instant) -> f64 {
    instant
        .saturating_duration_since(*SPAN_CLOCK.get_or_init(|| instant))
        .as_secs_f64()
}

#[derive(Clone, Serialize)]
pub struct CpuSpan {
    stage: &'static str,
    object: String,
    thread: String,
    started_s: f64,
    ended_s: f64,
    elapsed_ms: f64,
}
fn record_span(
    stage: &'static str,
    object: &str,
    started: std::time::Instant,
    ended: std::time::Instant,
) {
    if !frame_spans_enabled() || ended.duration_since(started).as_micros() < 1000 {
        return;
    }
    let span = CpuSpan {
        stage,
        object: object.to_owned(),
        thread: format!("{:?}", std::thread::current().id()),
        started_s: span_seconds(started),
        ended_s: span_seconds(ended),
        elapsed_ms: ended.duration_since(started).as_secs_f64() * 1000.,
    };
    let mut spans = RECENT_SPANS.get_or_init(Default::default).lock().unwrap();
    if spans.len() == RECENT_SPAN_LIMIT {
        spans.pop_front();
    }
    spans.push_back(span);
}

#[derive(Clone, Serialize)]
pub struct JourneyFrameContext {
    frame: u64,
    delta_ms: f64,
    startup: bool,
    streaming: bool,
    scene_phase: String,
    render_interval_start_s: Option<f64>,
    render_interval_end_s: Option<f64>,
    main_observed_s: f64,
    overlapping_spans: Vec<CpuSpan>,
    omitted_spans: usize,
}

#[derive(Clone, Default, Serialize)]
pub struct JourneyFrameContexts {
    maximum_gameplay: Option<JourneyFrameContext>,
    maximum_startup: Option<JourneyFrameContext>,
    recent_slow_frames: std::collections::VecDeque<JourneyFrameContext>,
    scope: &'static str,
}
fn overlapping_spans(
    spans: impl Iterator<Item = CpuSpan>,
    start: f64,
    end: f64,
) -> (Vec<CpuSpan>, usize) {
    let mut overlapping = spans
        .filter(|span| span.started_s < end && span.ended_s > start)
        .collect::<Vec<_>>();
    // Preserve the longest spans when concurrent workers exceed the diagnostic
    // cap. The omitted count makes truncation explicit; never sum nested spans.
    overlapping.sort_by(|a, b| b.elapsed_ms.total_cmp(&a.elapsed_ms));
    let omitted = overlapping.len().saturating_sub(CONTEXT_SPAN_LIMIT);
    overlapping.truncate(CONTEXT_SPAN_LIMIT);
    (overlapping, omitted)
}

/// CPU lifecycle attribution is bounded independently of the number of shapes.
/// Paths are reduced to basenames so exported reports do not expose home paths.
#[derive(Default, Serialize)]
struct ShapeStage {
    calls: u64,
    total_us: u64,
    maximum_us: u64,
    slowest_shape: String,
}
static SHAPE_STAGES: std::sync::OnceLock<
    std::sync::Mutex<std::collections::BTreeMap<&'static str, ShapeStage>>,
> = std::sync::OnceLock::new();
pub struct ShapeTimer {
    stage: &'static str,
    name: String,
    started: std::time::Instant,
}
impl ShapeTimer {
    pub fn start(stage: &'static str, path: &std::path::Path) -> Self {
        let started = std::time::Instant::now();
        if frame_spans_enabled() {
            SPAN_CLOCK.get_or_init(|| started);
        }
        Self {
            stage,
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .chars()
                .take(160)
                .collect(),
            started,
        }
    }
}
impl Drop for ShapeTimer {
    fn drop(&mut self) {
        let ended = std::time::Instant::now();
        let us = ended
            .duration_since(self.started)
            .as_micros()
            .min(u64::MAX as u128) as u64;
        record_span(self.stage, &self.name, self.started, ended);
        let mut stages = SHAPE_STAGES.get_or_init(Default::default).lock().unwrap();
        let slot = stages.entry(self.stage).or_default();
        slot.calls += 1;
        slot.total_us = slot.total_us.saturating_add(us);
        if us > slot.maximum_us {
            slot.maximum_us = us;
            slot.slowest_shape.clone_from(&self.name);
        }
    }
}
pub fn shape_stages_report() -> serde_json::Value {
    serde_json::to_value(&*SHAPE_STAGES.get_or_init(Default::default).lock().unwrap())
        .unwrap_or_default()
}

#[derive(Default)]
struct SystemStage {
    calls: u64,
    total_us: u64,
    maximum_us: u64,
    bins: Vec<u64>,
}
static SYSTEM_STAGES: std::sync::OnceLock<
    std::sync::Mutex<std::collections::BTreeMap<&'static str, SystemStage>>,
> = std::sync::OnceLock::new();

/// Opt-in CPU attribution. Fixed histogram storage; no per-frame file writes.
pub struct SystemTimer {
    name: &'static str,
    started: std::time::Instant,
}
pub fn profile_system(name: &'static str) -> Option<SystemTimer> {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    ENABLED
        .get_or_init(|| std::env::var("OPENRAILSRS_PROFILE_SYSTEMS").is_ok_and(|v| v == "1"))
        .then(|| {
            let started = std::time::Instant::now();
            if frame_spans_enabled() {
                SPAN_CLOCK.get_or_init(|| started);
            }
            SystemTimer { name, started }
        })
}
impl Drop for SystemTimer {
    fn drop(&mut self) {
        let ended = std::time::Instant::now();
        let us = ended
            .duration_since(self.started)
            .as_micros()
            .min(u64::MAX as u128) as u64;
        record_span(self.name, "system (inclusive)", self.started, ended);
        let mut stages = SYSTEM_STAGES.get_or_init(Default::default).lock().unwrap();
        let slot = stages.entry(self.name).or_default();
        slot.calls += 1;
        slot.total_us = slot.total_us.saturating_add(us);
        slot.maximum_us = slot.maximum_us.max(us);
        if slot.bins.is_empty() {
            slot.bins = vec![0; 2048];
        }
        slot.bins[(us as usize / 100).min(2047)] += 1;
    }
}
pub fn system_stages_report() -> serde_json::Value {
    let stages = SYSTEM_STAGES.get_or_init(Default::default).lock().unwrap();
    let report: std::collections::BTreeMap<_, _> = stages
        .iter()
        .map(|(name, stage)| {
            let percentile = |percent: u64| {
                let target = (stage.calls * percent).div_ceil(100);
                let mut seen = 0;
                stage
                    .bins
                    .iter()
                    .position(|count| {
                        seen += count;
                        seen >= target
                    })
                    .map(|bin| (bin + 1) as f64 / 10.)
            };
            (
                *name,
                serde_json::json!({"calls":stage.calls,"total_us":stage.total_us,
                "maximum_us":stage.maximum_us,"p50_ms":percentile(50),"p99_ms":percentile(99),
                "histogram_width_ms":0.1,"histogram_ceiling_ms":204.8}),
            )
        })
        .collect();
    serde_json::to_value(report).unwrap_or_default()
}

#[derive(Resource, Clone, Default, bevy::render::extract_resource::ExtractResource)]
pub struct ScenePipelineStatus {
    counts: Arc<AtomicU64>,
    uploads: Arc<AtomicU64>,
    device: Arc<std::sync::Mutex<Option<RendererDevice>>>,
    render_resources: Arc<std::sync::Mutex<Option<RenderResourceReport>>>,
}
#[derive(Clone, Debug, Serialize)]
pub struct RenderResourceReport {
    pub views: usize,
    pub meshes: usize,
    pub images: usize,
    /// Opt-in image ownership snapshot. None means not measured, never no images.
    pub image_ownership: Option<GpuImageOwnershipReport>,
    pub mesh_slabs: usize,
    pub mesh_slab_bytes: u64,
    pub indexed_mesh_allocations: usize,
    /// Bindless allocators only; non-bindless materials report zero by Bevy API.
    pub material_slabs: usize,
    pub material_slab_bytes: u64,
    pub bindless_material_allocations: u64,
    /// Raw bindless slot count after the local alias fix; distinct from material owners.
    pub bindless_material_allocations_scope: &'static str,
    pub material_owners: MaterialOwnershipReport,
    pub oit_node_capacity: Option<usize>,
    pub oit_head_capacity: Option<usize>,
    pub pipelines: usize,
    /// Published OIT resolve variants, including queued/failed entries.
    pub oit_resolve_pipelines: usize,
    /// Backend-dependent, opt-in. None means unsupported or disabled, never zero usage.
    pub allocator: Option<GpuAllocatorReport>,
    pub bin_unpacking_lifetime: Option<BinGroupCleanupStats>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GpuImageOwner {
    pub asset_id: String,
    pub extent: [u32; 3],
    pub format: String,
    pub dimension: String,
    pub mip_levels: u32,
    pub sample_count: u32,
    pub label: Option<String>,
    pub present_in_main_assets: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct GpuImageOwnershipReport {
    pub available: bool,
    pub sample_age_s: f64,
    pub main_render_images: usize,
    pub render_images: usize,
    pub absent_from_main_assets: usize,
    pub omitted_entries: usize,
    pub entries: Vec<GpuImageOwner>,
    pub scope: &'static str,
}

fn bounded_image_ownership(
    owners: impl IntoIterator<Item = GpuImageOwner>,
    main_render_images: usize,
    limit: usize,
) -> GpuImageOwnershipReport {
    let mut entries: Vec<_> = owners.into_iter().collect();
    let render_images = entries.len();
    let absent_from_main_assets = entries
        .iter()
        .filter(|image| !image.present_in_main_assets)
        .count();
    // Keep orphan candidates first when a large original route exceeds the
    // diagnostic cap. IDs include their generation; never merge recycled slots.
    entries.sort_unstable_by(|a, b| {
        (a.present_in_main_assets, &a.asset_id).cmp(&(b.present_in_main_assets, &b.asset_id))
    });
    let omitted_entries = entries.len().saturating_sub(limit);
    entries.truncate(limit);
    GpuImageOwnershipReport {
        available: true,
        sample_age_s: 0.,
        main_render_images,
        render_images,
        absent_from_main_assets,
        omitted_entries,
        entries,
        scope: "Opt-in 30-second RenderAssets<GpuImage> inventory. Absence from Main Assets<Image> is an ownership diagnostic, not proof of a leak or permission to remove an active GPU image. Descriptor extents are not allocator byte measurements.",
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MaterialOwnerCounts {
    pub available: bool,
    pub total: Option<usize>,
    pub by_type: std::collections::BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MaterialOwnershipReport {
    pub bindings: MaterialOwnerCounts,
    pub prepared: MaterialOwnerCounts,
}

fn material_type_name(id: std::any::TypeId) -> String {
    use std::any::TypeId;
    macro_rules! known {
        ($($ty:ty => $name:literal),* $(,)?) => {
            $(if id == TypeId::of::<$ty>() { return $name.to_owned(); })*
        };
    }
    known!(
        StandardMaterial => "StandardMaterial",
        openrailsrs_bevy_scenery::materials::terrain::TerrainMaterial => "TerrainMaterial",
        openrailsrs_bevy_scenery::materials::or_terrain::OrTerrainMaterial => "OrTerrainMaterial",
        openrailsrs_bevy_scenery::materials::scenery::OrSceneryMaterial => "OrSceneryMaterial",
        openrailsrs_bevy_scenery::materials::cab::OrCabMaterial => "OrCabMaterial",
        openrailsrs_bevy_scenery::materials::forest::OrForestMaterial => "OrForestMaterial",
        crate::weather_particles::ParticleMaterial => "ParticleMaterial",
        crate::surface_weather::SnowMaterial => "SnowMaterial",
        crate::sky::RailwaySkyMaterial => "RailwaySkyMaterial",
        crate::moon::MoonMaterial => "MoonMaterial",
    );
    // Keep unexpected types separate too. TypeId is binary-specific and carries
    // no asset path; known types above have stable human-readable labels.
    format!("unknown:{id:?}")
}

fn material_owner_counts(
    ids: Option<impl Iterator<Item = bevy::asset::UntypedAssetId>>,
) -> MaterialOwnerCounts {
    let Some(ids) = ids else {
        return MaterialOwnerCounts::default();
    };
    let mut by_type = std::collections::HashMap::<std::any::TypeId, usize>::new();
    let mut total = 0;
    for id in ids {
        *by_type.entry(id.type_id()).or_default() += 1;
        total += 1;
    }
    MaterialOwnerCounts {
        available: true,
        total: Some(total),
        by_type: by_type
            .into_iter()
            .map(|(id, count)| (material_type_name(id), count))
            .collect(),
    }
}

/// Optional 30-second endurance statistics, never a HUD/frame allocator probe.
/// mallinfo2 accounts for libc allocations, not all process PSS or GPU mappings.
static RUST_GLOBAL_ALLOCATOR: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();

/// The executable owns the global allocator; library users may select another.
pub fn register_rust_global_allocator(name: &'static str) {
    let _ = RUST_GLOBAL_ALLOCATOR.set(name);
}

#[derive(Clone, Debug, Serialize)]
pub struct CpuHeapAllocatorReport {
    pub available: bool,
    pub allocator: &'static str,
    pub rust_global_allocator: Option<&'static str>,
    pub arena: Option<u64>,
    pub uordblks: Option<u64>,
    pub fordblks: Option<u64>,
    pub hblkhd: Option<u64>,
    pub scope: &'static str,
}

pub fn heap_allocator_report() -> CpuHeapAllocatorReport {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        // SAFETY: mallinfo2 takes no pointers or arguments and only reads libc's
        // allocator statistics. It does not release, move or mutate our objects.
        let info = unsafe { libc::mallinfo2() };
        CpuHeapAllocatorReport {
            available: true,
            allocator: "glibc mallinfo2",
            rust_global_allocator: RUST_GLOBAL_ALLOCATOR.get().copied(),
            arena: Some(info.arena as u64),
            uordblks: Some(info.uordblks as u64),
            fordblks: Some(info.fordblks as u64),
            hblkhd: Some(info.hblkhd as u64),
            scope: "Raw libc bytes: arena includes libc heap capacity; uordblks/fordblks describe arena use/free space, hblkhd separate mmap allocation space. A different Rust global allocator is excluded from these libc counters. They do not equal total PSS, Rust live-object bytes or all driver allocations. No malloc_trim is called.",
        }
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    {
        CpuHeapAllocatorReport {
            available: false,
            allocator: "unsupported platform",
            rust_global_allocator: RUST_GLOBAL_ALLOCATOR.get().copied(),
            arena: None,
            uordblks: None,
            fordblks: None,
            hblkhd: None,
            scope: "glibc mallinfo2 is available only for Linux GNU targets; unavailable readings are not zero-filled.",
        }
    }
}

/// Real before/after values from the dead-view bind-group cleanup, not estimates.
#[derive(Resource, Clone, Debug, Default, Serialize)]
pub struct BinGroupCleanupStats {
    pub cached_keys: usize,
    pub source_keys: usize,
    pub stale_keys_before: usize,
    pub stale_keys_after: usize,
    pub retired_keys: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct GpuAllocatorReport {
    pub allocated_bytes: u64,
    pub reserved_bytes: u64,
    pub allocations: usize,
    pub blocks: usize,
    pub largest_labels: Vec<GpuAllocationLabel>,
    /// Inspect this small shared image before truncating the largest labels.
    /// None means the label is absent, not a fabricated zero allocation count.
    pub ground_fog_density: Option<GpuAllocationLabel>,
    pub sparse_blocks: Vec<GpuAllocationBlock>,
}
#[derive(Clone, Debug, Serialize)]
pub struct GpuAllocationLabel {
    pub name: String,
    pub bytes: u64,
    pub allocations: usize,
}
#[derive(Clone, Debug, Serialize)]
pub struct GpuAllocationBlock {
    pub reserved_bytes: u64,
    pub allocated_bytes: u64,
    pub allocations: usize,
    pub largest_labels: Vec<GpuAllocationLabel>,
}

fn allocation_labels<'a>(
    allocations: impl Iterator<Item = (&'a str, u64)>,
    limit: usize,
) -> Vec<GpuAllocationLabel> {
    let mut labels = std::collections::HashMap::<String, (u64, usize)>::new();
    for (name, bytes) in allocations {
        // Reports keep names useful without exporting authored absolute paths.
        let name: String = name
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(name)
            .chars()
            .take(160)
            .collect();
        let entry = labels.entry(name).or_default();
        entry.0 = entry.0.saturating_add(bytes);
        entry.1 += 1;
    }
    let mut labels: Vec<_> = labels
        .into_iter()
        .map(|(name, (bytes, allocations))| GpuAllocationLabel {
            name,
            bytes,
            allocations,
        })
        .collect();
    labels.sort_unstable_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
    labels.truncate(limit);
    labels
}

fn ground_fog_allocations<'a>(
    allocations: impl Iterator<Item = (&'a str, u64)>,
) -> Option<GpuAllocationLabel> {
    let mut label = None::<GpuAllocationLabel>;
    for (name, bytes) in allocations {
        if name != "openrailsrs-ground-fog-density" {
            continue;
        }
        let total = label.get_or_insert_with(|| GpuAllocationLabel {
            name: name.to_owned(),
            bytes: 0,
            allocations: 0,
        });
        total.bytes = total.bytes.saturating_add(bytes);
        total.allocations += 1;
    }
    label
}
#[derive(Clone, Debug, Serialize)]
pub struct RendererDevice {
    pub name: String,
    pub backend: String,
    pub device_type: String,
    pub hardware: bool,
}
impl ScenePipelineStatus {
    pub fn render_resources(&self) -> Option<RenderResourceReport> {
        self.render_resources.lock().unwrap().clone()
    }
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
    allocator: Res<bevy::render::mesh::allocator::MeshAllocator>,
    material_allocators: Res<bevy::pbr::MaterialBindGroupAllocators>,
    material_owners: (
        Option<Res<bevy::pbr::RenderMaterialBindings>>,
        Option<
            Res<bevy::render::erased_render_asset::ErasedRenderAssets<bevy::pbr::PreparedMaterial>>,
        >,
    ),
    bin_group_cleanup: Option<Res<BinGroupCleanupStats>>,
    views: Query<&bevy::render::view::ExtractedView>,
    oit: Option<Res<bevy::core_pipeline::oit::OitBuffers>>,
    render_device: Res<bevy::render::renderer::RenderDevice>,
    mut last_resources: Local<Option<std::time::Instant>>,
    mut allocator_report: Local<(
        Option<std::time::Instant>,
        Option<GpuAllocatorReport>,
        Option<GpuImageOwnershipReport>,
    )>,
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
    let now = std::time::Instant::now();
    if last_resources.is_none_or(|last| now.duration_since(last).as_secs_f32() >= 1.) {
        *last_resources = Some(now);
        static ALLOCATIONS_ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let enabled = *ALLOCATIONS_ENABLED.get_or_init(|| {
            std::env::var_os("OPENRAILSRS_SOAK_SECONDS").is_some()
                || std::env::var("OPENRAILSRS_GPU_ALLOCATIONS").is_ok_and(|v| v == "1")
        });
        if enabled
            && allocator_report
                .0
                .is_none_or(|last| now.duration_since(last).as_secs_f32() >= 30.)
        {
            allocator_report.0 = Some(now);
            let main_ids: std::collections::HashSet<_> = required.images.iter().copied().collect();
            allocator_report.2 = Some(bounded_image_ownership(
                images.iter().map(|(id, image)| {
                    let descriptor = &image.texture_descriptor;
                    GpuImageOwner {
                        asset_id: format!("{id:?}"),
                        extent: [
                            descriptor.size.width,
                            descriptor.size.height,
                            descriptor.size.depth_or_array_layers,
                        ],
                        format: format!("{:?}", descriptor.format),
                        dimension: format!("{:?}", descriptor.dimension),
                        mip_levels: descriptor.mip_level_count,
                        sample_count: descriptor.sample_count,
                        label: descriptor.label.map(str::to_owned),
                        present_in_main_assets: main_ids.contains(&id),
                    }
                }),
                main_ids.len(),
                4096,
            ));
            allocator_report.1 =
                render_device
                    .wgpu_device()
                    .generate_allocator_report()
                    .map(|report| {
                        let mut sparse_blocks: Vec<_> = report
                            .blocks
                            .iter()
                            .map(|block| {
                                let live = &report.allocations[block.allocations.clone()];
                                GpuAllocationBlock {
                                    reserved_bytes: block.size,
                                    allocated_bytes: live.iter().map(|a| a.size).sum(),
                                    allocations: live.len(),
                                    largest_labels: allocation_labels(
                                        live.iter().map(|a| (a.name.as_str(), a.size)),
                                        5,
                                    ),
                                }
                            })
                            .collect();
                        sparse_blocks.sort_unstable_by_key(|b| {
                            std::cmp::Reverse(b.reserved_bytes.saturating_sub(b.allocated_bytes))
                        });
                        sparse_blocks.truncate(16);
                        GpuAllocatorReport {
                            allocated_bytes: report.total_allocated_bytes,
                            reserved_bytes: report.total_reserved_bytes,
                            allocations: report.allocations.len(),
                            blocks: report.blocks.len(),
                            largest_labels: allocation_labels(
                                report.allocations.iter().map(|a| (a.name.as_str(), a.size)),
                                16,
                            ),
                            ground_fog_density: ground_fog_allocations(
                                report.allocations.iter().map(|a| (a.name.as_str(), a.size)),
                            ),
                            sparse_blocks,
                        }
                    });
        }
        *status.render_resources.lock().unwrap() = Some(RenderResourceReport {
            views: views.iter().count(),
            meshes: meshes.iter().count(),
            images: images.iter().count(),
            image_ownership: allocator_report.2.as_ref().map(|inventory| {
                let mut inventory = inventory.clone();
                inventory.sample_age_s = allocator_report
                    .0
                    .map(|sample| now.duration_since(sample).as_secs_f64())
                    .unwrap_or_default();
                inventory
            }),
            mesh_slabs: allocator.slab_count(),
            mesh_slab_bytes: allocator.slabs_size(),
            indexed_mesh_allocations: allocator.index_allocation_count(),
            material_slabs: material_allocators.values().map(|a| a.slab_count()).sum(),
            material_slab_bytes: material_allocators
                .values()
                .map(|a| a.slabs_size() as u64)
                .sum(),
            bindless_material_allocations: material_allocators
                .values()
                .map(|a| a.allocations())
                .sum(),
            bindless_material_allocations_scope: "Raw PBR allocated_resource_count with the local 0.19.1 aliased-binding fix; occupied slots are distinct from material owner maps and GPU bytes.",
            material_owners: MaterialOwnershipReport {
                bindings: material_owner_counts(
                    material_owners.0.as_ref().map(|m| m.keys().copied()),
                ),
                prepared: material_owner_counts(
                    material_owners
                        .1
                        .as_ref()
                        .map(|m| m.iter().map(|(id, _)| id)),
                ),
            },
            oit_node_capacity: oit.as_ref().map(|o| o.nodes.capacity()),
            oit_head_capacity: oit.as_ref().map(|o| o.heads.capacity()),
            pipelines: cache.pipelines().count(),
            oit_resolve_pipelines: cache.pipelines().filter(|pipeline| {
                matches!(&pipeline.descriptor,
                    bevy::render::render_resource::PipelineDescriptor::RenderPipelineDescriptor(descriptor)
                    if descriptor.label.as_deref() == Some("oit_resolve_pipeline"))
            }).count(),
            allocator: allocator_report.1.clone(),
            bin_unpacking_lifetime: bin_group_cleanup.as_deref().cloned(),
        });
    }
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
    frame_contexts: Option<JourneyFrameContexts>,
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
            frame_contexts: None,
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
    /// Opt-in correlation only. Does not replace the Journey histogram or its
    /// scope; unavailable when OPENRAILSRS_FRAME_SPANS is not enabled.
    pub frame_contexts: Option<JourneyFrameContexts>,
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
            frame_contexts: self.frame_contexts.clone(),
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
    let streaming = progress.is_some()
        || terrain.is_some()
        || stream
            .as_ref()
            .is_some_and(|stream| stream.pending_work() > 0)
        || initial_load;
    let delta_ms = time.delta_secs_f64() * 1000.;
    let new_maximum = if initial_load {
        delta_ms > metrics.startup_longest_ms
    } else {
        delta_ms > metrics.gameplay_longest_ms
    };
    metrics.record(time.delta_secs_f64(), streaming, initial_load);
    if frame_spans_enabled()
        && delta_ms.is_finite()
        && delta_ms > 0.
        && (new_maximum || delta_ms >= 50.)
    {
        let now = std::time::Instant::now();
        let interval = time.last_update().and_then(|end| {
            end.checked_sub(time.delta())
                .map(|start| (span_seconds(start), span_seconds(end)))
        });
        let (overlapping, omitted) = interval.map_or_else(
            || (Vec::new(), 0),
            |(start, end)| {
                overlapping_spans(
                    RECENT_SPANS
                        .get_or_init(Default::default)
                        .lock()
                        .unwrap()
                        .iter()
                        .cloned(),
                    start,
                    end,
                )
            },
        );
        let context = JourneyFrameContext {
            frame: metrics.frames,
            delta_ms,
            startup: initial_load,
            streaming,
            scene_phase: progress.as_ref().map_or_else(
                || {
                    if terrain.is_some() {
                        "terrain".into()
                    } else {
                        "no foreground WORLD task".into()
                    }
                },
                |progress| progress.diagnostic_phase(),
            ),
            render_interval_start_s: interval.map(|pair| pair.0),
            render_interval_end_s: interval.map(|pair| pair.1),
            main_observed_s: span_seconds(now),
            overlapping_spans: overlapping,
            omitted_spans: omitted,
        };
        let diagnostics = metrics.frame_contexts.get_or_insert_with(|| JourneyFrameContexts {
            scope: "Opt-in monotonic clock correlation from first Playing frame. Time<Real> render-world interval, observed Update phase and completed CPU spans across threads. Spans may be inclusive/nested; never sum them or assert physical presentation/Last-to-Last equivalence. Main observed phase may follow the pipelined render interval.",
            ..default()
        });
        if new_maximum {
            if initial_load {
                diagnostics.maximum_startup = Some(context.clone());
            } else {
                diagnostics.maximum_gameplay = Some(context.clone());
            }
        }
        if delta_ms >= 50. {
            if diagnostics.recent_slow_frames.len() == FRAME_CONTEXT_LIMIT {
                diagnostics.recent_slow_frames.pop_front();
            }
            diagnostics.recent_slow_frames.push_back(context);
        }
    }
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
    fn frame_context_correlates_true_overlaps_preserves_longest_and_reports_truncation() {
        let span = |index, start, end| CpuSpan {
            stage: "test",
            object: format!("{index}"),
            thread: "worker".into(),
            started_s: start,
            ended_s: end,
            elapsed_ms: (end - start) * 1000.,
        };
        let values = std::iter::once(span(0, 1., 2.))
            .chain(std::iter::once(span(1, 3., 4.)))
            .chain((0..60).map(|i| span(i + 2, 2. + i as f64 / 1000., 3.)))
            .chain(std::iter::once(span(99, 2.1, 2.9)));
        let (matched, omitted) = overlapping_spans(values, 2., 3.);
        assert_eq!(matched.len(), CONTEXT_SPAN_LIMIT);
        assert_eq!(omitted, 13);
        assert!(
            matched
                .iter()
                .all(|span| span.started_s < 3. && span.ended_s > 2.)
        );
        assert_eq!(matched[0].object, "2");
        assert!(
            !matched
                .iter()
                .any(|span| span.object == "0" || span.object == "1")
        );
        // This extra diagnostic does not change the established histogram.
        let mut metrics = JourneyPerformance::default();
        for _ in 0..100 {
            metrics.record(0.025, false, false);
        }
        metrics.record(0.105, true, false);
        assert_eq!(metrics.report().gameplay_frame_p99_ms, 25);
        assert_eq!(metrics.report().gameplay_longest_frame_ms, 105.);
        assert!(metrics.report().frame_contexts.is_none());
    }

    #[test]
    fn image_inventory_preserves_orphan_candidates_and_reports_truncation() {
        let owners = [
            ("index:7/generation:2", true),
            ("index:7/generation:1", false),
            ("index:9/generation:1", false),
        ]
        .map(|(id, present)| GpuImageOwner {
            asset_id: id.into(),
            extent: [256, 256, 1],
            format: "Rgba8UnormSrgb".into(),
            dimension: "D2".into(),
            mip_levels: 1,
            sample_count: 1,
            label: None,
            present_in_main_assets: present,
        });
        let report = bounded_image_ownership(owners.clone(), 1, 2);
        assert!(report.available);
        assert_eq!(report.render_images, 3);
        assert_eq!(report.main_render_images, 1);
        assert_eq!(report.absent_from_main_assets, 2);
        assert_eq!(report.omitted_entries, 1);
        assert_eq!(report.entries.len(), 2);
        assert!(report.entries.iter().all(|e| !e.present_in_main_assets));
        assert_eq!(report.entries[0].asset_id, "index:7/generation:1");
        let omitted = bounded_image_ownership(owners, 1, 0);
        assert!(omitted.entries.is_empty());
        assert_eq!(omitted.render_images, 3);
        assert_eq!(omitted.absent_from_main_assets, 2);
        assert_eq!(omitted.omitted_entries, 3);
    }
    #[test]
    fn shared_fog_allocation_is_counted_even_below_the_largest_label_cutoff() {
        let mut source = vec![("larger texture", 1_048_576); 20];
        source.extend([
            ("openrailsrs-ground-fog-density", 32_768),
            ("openrailsrs-ground-fog-density", 32_768),
        ]);
        let largest = allocation_labels(source.iter().copied(), 1);
        assert_eq!(largest[0].name, "larger texture");
        let observed = ground_fog_allocations(source.into_iter()).unwrap();
        assert_eq!(observed.allocations, 2);
        assert_eq!(observed.bytes, 65_536);
        assert_eq!(observed.name, "openrailsrs-ground-fog-density");
    }

    #[test]
    fn missing_fog_allocation_label_stays_unavailable_instead_of_zero_filled() {
        assert!(ground_fog_allocations([("another texture", 32_768)].into_iter()).is_none());
        assert!(ground_fog_allocations(std::iter::empty()).is_none());
    }

    #[test]
    fn material_owner_maps_distinguish_absence_empty_and_retired_types() {
        let unavailable =
            material_owner_counts(None::<std::iter::Empty<bevy::asset::UntypedAssetId>>);
        assert!(!unavailable.available);
        assert_eq!(unavailable.total, None);
        let mut materials = Assets::<StandardMaterial>::default();
        let first = materials.add(StandardMaterial::default()).id();
        let second = materials.add(StandardMaterial::default()).id();
        let cab = Handle::<crate::or_cab_material::OrCabMaterial>::default().id();
        let mut prepared = bevy::render::erased_render_asset::ErasedRenderAssets::<u8>::default();
        let empty = material_owner_counts(Some(prepared.iter().map(|(id, _)| id)));
        assert!(empty.available);
        assert_eq!(empty.total, Some(0));
        prepared.insert(first, 1);
        prepared.insert(second, 2);
        prepared.insert(cab, 3);
        let current = material_owner_counts(Some(prepared.iter().map(|(id, _)| id)));
        assert_eq!(current.total, Some(3));
        assert_eq!(current.by_type.get("StandardMaterial"), Some(&2));
        assert_eq!(current.by_type.get("OrCabMaterial"), Some(&1));
        prepared.remove(first);
        prepared.remove(cab);
        let retired = material_owner_counts(Some(prepared.iter().map(|(id, _)| id)));
        assert_eq!(retired.total, Some(1));
        assert_eq!(retired.by_type.get("StandardMaterial"), Some(&1));
        assert!(!retired.by_type.contains_key("OrCabMaterial"));
    }

    #[test]
    fn material_owner_maps_keep_unexpected_types_separate() {
        let images = Handle::<Image>::default().id().untyped();
        let meshes = Handle::<Mesh>::default().id().untyped();
        let report = material_owner_counts(Some([images, meshes].into_iter()));
        assert_eq!(report.total, Some(2));
        assert_eq!(report.by_type.len(), 2);
        assert!(
            report
                .by_type
                .keys()
                .all(|name| name.starts_with("unknown:"))
        );
    }

    #[test]
    fn heap_allocator_availability_does_not_fabricate_zero_readings() {
        let report = heap_allocator_report();
        let fields = [
            report.arena,
            report.uordblks,
            report.fordblks,
            report.hblkhd,
        ];
        if cfg!(all(target_os = "linux", target_env = "gnu")) {
            assert!(report.available);
            assert!(fields.iter().all(Option::is_some));
        } else {
            assert!(!report.available);
            assert!(fields.iter().all(Option::is_none));
        }
    }

    #[test]
    fn gpu_allocation_labels_are_aggregated_bounded_and_do_not_export_paths() {
        let labels = allocation_labels(
            [
                ("/private/route/shelter", 16),
                ("C:\\private\\route\\shelter", 32),
                ("mesh vertex buffer", 512),
                ("small", 1),
            ]
            .into_iter(),
            2,
        );
        assert_eq!(labels.len(), 2);
        assert_eq!(
            (
                labels[0].name.as_str(),
                labels[0].bytes,
                labels[0].allocations
            ),
            ("mesh vertex buffer", 512, 1)
        );
        assert_eq!(
            (
                labels[1].name.as_str(),
                labels[1].bytes,
                labels[1].allocations
            ),
            ("shelter", 48, 2)
        );
    }

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
