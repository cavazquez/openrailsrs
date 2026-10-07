//! Optional ground cover. Native textures/geometry remain authoritative.
//! Tiles own deterministic batches, never one ECS entity per blade. Unknown
//! textures/objects are excluded rather than placing grass on infrastructure.
use crate::{
    terrain::{TerrainElevation, TerrainScene},
    world::{RouteFocus, WorldScene},
    world_instancing::{WorldInstanceAppearance, WorldInstanceBuffer, WorldInstanceData},
};
use bevy::{
    asset::RenderAssetUsages,
    camera::primitives::Aabb,
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};
const CHUNK: f32 = 32.;
const TRACK_MASK_RADIUS: f32 = 1536.;
const TRACK_MASK_REBUILD_DISTANCE: f32 = 512.;
type NativePath = openrailsrs_bevy_scenery::spawn::tdb_track::TrackVectorPath;
struct RoadMaskContext {
    db: Arc<openrailsrs_formats::TrackDbFile>,
    catalog: openrailsrs_formats::TSectionCatalog,
}
enum MaskPath {
    Rail(Arc<NativePath>),
    Road {
        context: Arc<RoadMaskContext>,
        node_index: usize,
    },
}
struct NativePathMask {
    path: MaskPath,
    min: Vec2,
    max: Vec2,
}
impl NativePathMask {
    fn new(path: Arc<NativePath>) -> Self {
        let steps = (path.length_m() / 128.).ceil().clamp(1., 100_000.) as usize;
        let mut min = Vec2::splat(f32::INFINITY);
        let mut max = Vec2::splat(f32::NEG_INFINITY);
        for i in 0..=steps {
            let p = path
                .pose_in_frame(
                    path.length_m() * i as f64 / steps as f64,
                    bevy::math::DVec3::ZERO,
                )
                .position
                .xz();
            min = min.min(p);
            max = max.max(p);
        }
        // Any point between samples lies within the arc length travelled,
        // including curves whose endpoints fall outside the requested region.
        let padding = (path.length_m() / steps as f64).max(128.) as f32;
        Self {
            path: MaskPath::Rail(path),
            min: min - Vec2::splat(padding),
            max: max + Vec2::splat(padding),
        }
    }
    fn intersects(&self, center: Vec2) -> bool {
        center.distance(center.clamp(self.min, self.max)) <= TRACK_MASK_RADIUS + 8.
    }
    fn append_region_segments(&self, center: Vec2, segments: &mut Vec<[Vec2; 2]>) {
        if !self.intersects(center) {
            return;
        }
        let path = match &self.path {
            MaskPath::Rail(path) => path.clone(),
            MaskPath::Road {
                context,
                node_index,
            } => {
                let Some(path) =
                    NativePath::new(&context.db.nodes[*node_index], Some(&context.catalog))
                else {
                    return;
                };
                Arc::new(path)
            }
        };
        // Sample on the authored path's fixed grid, never a camera-relative
        // grid: rebuilding the index must preserve identical clearance masks.
        let steps = (path.length_m() / 8.).ceil().clamp(1., 100_000.) as usize;
        let mut previous = path
            .pose_in_frame(0., bevy::math::DVec3::ZERO)
            .position
            .xz();
        for i in 1..=steps {
            let p = path
                .pose_in_frame(
                    path.length_m() * i as f64 / steps as f64,
                    bevy::math::DVec3::ZERO,
                )
                .position
                .xz();
            if crate::track::point_segment_distance_xz(
                center.x, center.y, previous.x, previous.y, p.x, p.y,
            ) <= TRACK_MASK_RADIUS + 8.
            {
                segments.push([previous, p]);
            }
            previous = p;
        }
    }
}
fn append_road_paths(
    paths: &mut Vec<NativePathMask>,
    db: Arc<openrailsrs_formats::TrackDbFile>,
    mut catalog: openrailsrs_formats::TSectionCatalog,
) {
    // The shared rail sampler deliberately filters RoadShape. For exclusion
    // masks, use a private catalogue copy with that filter disabled: geometry
    // and coordinates stay authored, and the route/rail catalogue is untouched.
    for shape in catalog.shapes.values_mut() {
        shape.road_shape = false;
    }
    let context = Arc::new(RoadMaskContext { db, catalog });
    for (node_index, node) in context.db.nodes.iter().enumerate() {
        if let Some(path) = NativePath::new(node, Some(&context.catalog)) {
            let mut mask = NativePathMask::new(Arc::new(path));
            // Keep bounds and the shared authored node. Compiled road spans
            // are reconstructed only for paths intersecting the local region.
            mask.path = MaskPath::Road {
                context: context.clone(),
                node_index,
            };
            paths.push(mask);
        }
    }
}

fn road_catalog(
    db: &openrailsrs_formats::TrackDbFile,
    source: &openrailsrs_formats::TSectionCatalog,
) -> openrailsrs_formats::TSectionCatalog {
    let mut catalog = openrailsrs_formats::TSectionCatalog::default();
    for node in &db.nodes {
        if let openrailsrs_formats::TrackNodeKind::Vector { sections, .. } = &node.kind {
            for record in sections {
                if let Some(definition) = source.sections.get(&record.section_index) {
                    catalog
                        .sections
                        .entry(record.section_index)
                        .or_insert(*definition);
                }
                if let Some(definition) = source.shapes.get(&record.shape_index) {
                    catalog
                        .shapes
                        .entry(record.shape_index)
                        .or_insert_with(|| definition.clone());
                }
            }
        }
    }
    // Procedural fallbacks may refer to sections through a shape path rather
    // than directly through the RDB record. Preserve that dependency closure.
    for shape in catalog.shapes.values() {
        for index in shape.paths.iter().flat_map(|path| &path.section_indices) {
            if let Some(definition) = source.sections.get(index) {
                catalog.sections.entry(*index).or_insert(*definition);
            }
        }
    }
    catalog
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SceneryProfile {
    #[default]
    Authentic,
    Enhanced,
}
impl SceneryProfile {
    pub fn next(self) -> Self {
        if self == Self::Authentic {
            Self::Enhanced
        } else {
            Self::Authentic
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Authentic => "Auténtica",
            Self::Enhanced => "Mejorada",
        }
    }
    fn from_env() -> Option<Self> {
        match std::env::var("OPENRAILSRS_SCENERY_PROFILE").ok()?.as_str() {
            "authentic" => Some(Self::Authentic),
            "enhanced" => Some(Self::Enhanced),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SceneryQuality {
    Low,
    #[default]
    Medium,
    High,
}
impl SceneryQuality {
    fn from_env() -> Option<Self> {
        match std::env::var("OPENRAILSRS_SCENERY_QUALITY").ok()?.as_str() {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Low => Self::Medium,
            Self::Medium => Self::High,
            Self::High => Self::Low,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Baja · 2.048 matas",
            Self::Medium => "Media · 8.192 matas",
            Self::High => "Alta · 16.384 matas",
        }
    }
    fn limits(self) -> (f32, usize, f32) {
        match self {
            Self::Low => (64., 2048, 4.),
            Self::Medium => (96., 8192, 2.5),
            Self::High => (128., 16384, 2.),
        }
    }
    fn chunk_budget(self) -> usize {
        let (radius, budget, _) = self.limits();
        // Includes the unload hysteresis. This square encloses every retained
        // chunk, even when the camera sits on a grid boundary. No arrival-order
        // truncation is needed, and all retained buffers fit the global limit.
        let ring = ((radius + CHUNK * 2.) / CHUNK).ceil() as usize + 1;
        budget / (ring * 2 + 1).pow(2)
    }
}
#[derive(Component)]
pub struct GrassChunk {
    pub key: (i32, i32),
    pub tile: (i32, i32),
    born: f64,
}
#[derive(Clone)]
struct MaskImage {
    size: usize,
    green: Vec<bool>,
}
fn is_grass(rgb: [u8; 3]) -> bool {
    let [r, g, b] = rgb.map(f32::from);
    g > r * 1.035 && g > b * 0.98 && g > 20. && r < 220.
}
impl MaskImage {
    fn from_image(image: &Image) -> Option<Self> {
        use bevy::render::render_resource::TextureFormat;
        if !matches!(
            image.texture_descriptor.format,
            TextureFormat::Rgba8UnormSrgb | TextureFormat::Rgba8Unorm
        ) {
            return None;
        }
        let data = image.data.as_ref()?;
        let w = image.width() as usize;
        let h = image.height() as usize;
        let size = 64.min(w).min(h);
        if size == 0 {
            return None;
        }
        let mut green = vec![false; size * size];
        for y in 0..size {
            for x in 0..size {
                let offset = ((y * h / size) * w + x * w / size) * 4;
                if let Some(p) = data.get(offset..offset + 4) {
                    green[y * size + x] = p[3] > 240 && is_grass([p[0], p[1], p[2]]);
                }
            }
        }
        Some(Self { size, green })
    }
    fn allows(&self, u: f32, v: f32) -> bool {
        let x = (u.rem_euclid(1.) * self.size as f32) as i32;
        let y = (v.rem_euclid(1.) * self.size as f32) as i32;
        // Dilate non-grass texels: small paved paths in TERRTEX also stay clear.
        (-1..=1).all(|dy| {
            (-1..=1).all(|dx| {
                self.green[((y + dy).rem_euclid(self.size as i32) as usize) * self.size
                    + (x + dx).rem_euclid(self.size as i32) as usize]
            })
        })
    }
}
#[derive(Clone)]
struct Exclusion {
    center: Vec2,
    half: Vec2,
    inverse: Mat2,
}
#[derive(Clone, Copy)]
struct ShapeFootprint {
    min: Vec3,
    max: Vec3,
}
fn shape_footprint(shape: &openrailsrs_formats::ShapeFile) -> Option<ShapeFootprint> {
    use openrailsrs_bevy_scenery::shapes::mesh::{
        primitive_matrix_chain_bake, resolve_shape_vertex, transform_shape_point,
    };
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for control in &shape.lod_controls {
        for level in &control.distance_levels {
            for sub in &level.sub_objects {
                for primitive in &sub.primitives {
                    let start = shape
                        .prim_states
                        .get(primitive.prim_state_idx.max(0) as usize)
                        .and_then(|p| shape.vtx_states.get(p.vertex_state_idx.max(0) as usize))
                        .map_or(0, |v| v.matrix_idx);
                    let matrices = primitive_matrix_chain_bake(shape, level, start, false);
                    for index in &primitive.vertex_indices {
                        let Some((point, ..)) = resolve_shape_vertex(shape, sub, *index) else {
                            continue;
                        };
                        let Some(point) = shape.points.get(point) else {
                            continue;
                        };
                        let p = transform_shape_point(
                            openrailsrs_bevy_scenery::shapes::shape_point_to_bevy(*point),
                            &matrices,
                        );
                        if p.is_finite() {
                            min = min.min(p);
                            max = max.max(p);
                        }
                    }
                }
            }
        }
    }
    (min.is_finite() && max.is_finite()).then_some(ShapeFootprint { min, max })
}
fn placed_footprint(
    bounds: ShapeFootprint,
    position: Vec3,
    rotation: Quat,
    scale: Vec3,
) -> Exclusion {
    let mut min = Vec2::splat(f32::INFINITY);
    let mut max = Vec2::splat(f32::NEG_INFINITY);
    for x in [bounds.min.x, bounds.max.x] {
        for y in [bounds.min.y, bounds.max.y] {
            for z in [bounds.min.z, bounds.max.z] {
                let p = (position + rotation * (Vec3::new(x, y, z) * scale)).xz();
                min = min.min(p);
                max = max.max(p);
            }
        }
    }
    Exclusion {
        center: (min + max) * 0.5,
        half: (max - min) * 0.5 + Vec2::splat(4.),
        inverse: Mat2::IDENTITY,
    }
}
impl Exclusion {
    fn contains(&self, p: Vec2) -> bool {
        (self.inverse * (p - self.center))
            .abs()
            .cmple(self.half)
            .all()
    }
}
struct Builder {
    key: (i32, i32),
    tile: (i32, i32),
    cursor: usize,
    side: usize,
    spacing: f32,
    instances: Vec<WorldInstanceData>,
    seed: u32,
    exclusions: Vec<Exclusion>,
}
#[derive(Resource, Default)]
pub struct EnhancedScenery {
    pub profile: SceneryProfile,
    quality: SceneryQuality,
    groups: BTreeMap<(i32, i32), Entity>,
    empty: HashSet<(i32, i32)>,
    history: BTreeMap<(i32, i32), u64>,
    builder: Option<Builder>,
    mesh: Option<Handle<Mesh>>,
    white: Option<Handle<Image>>,
    tracks: Option<Arc<crate::track::TrackSegmentIndex>>,
    track_center: Vec2,
    track_task: Option<Task<(Vec2, crate::track::TrackSegmentIndex)>>,
    track_sources: Option<Arc<Vec<NativePathMask>>>,
    track_source_task: Option<Task<Vec<NativePathMask>>>,
    masks: HashMap<String, Option<MaskImage>>,
    mask_task: Option<Task<Vec<(String, Option<MaskImage>)>>>,
    bounds: HashMap<String, Option<ShapeFootprint>>,
    bounds_task: Option<Task<Vec<(String, Option<ShapeFootprint>)>>>,
    instances: usize,
    pub generated: u64,
    pub returned: u64,
    pub hash_mismatches: u64,
    pub rejected: u64,
    rejections: [u64; 4],
    pub max_build_ms: f64,
}
impl EnhancedScenery {
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"profile":self.profile,"quality":self.quality,"groups":self.groups.len(),"instances":self.instances,"maximum_instances":self.quality.limits().1,"generated":self.generated,"returned":self.returned,"hash_mismatches":self.hash_mismatches,"rejected_candidates":self.rejected,"rejections_infrastructure_rail_texture_slope":self.rejections,"max_build_ms":self.max_build_ms,"texture_masks":self.masks.len(),"track_mask_segments":self.tracks.as_ref().map_or(0,|i|i.segment_count()),"track_mask_radius_m":TRACK_MASK_RADIUS,"shape_masks":self.bounds.len(),"stable_hashes":self.history.iter().map(|(key,hash)|serde_json::json!({"chunk":key,"hash":hash})).collect::<Vec<_>>(),"lod":"dense near, sparse/lower mid, dither to texture-only far","scope":"original TERRTEX green mask; unknown/unclassified content is excluded"})
    }
}
fn seed(key: (i32, i32), texture: &str) -> u32 {
    let mut h = 0x811c9dc5_u32;
    for b in key
        .0
        .to_le_bytes()
        .into_iter()
        .chain(key.1.to_le_bytes())
        .chain(texture.bytes())
    {
        h = (h ^ u32::from(b)).wrapping_mul(16777619);
    }
    h
}
fn buffer_hash(data: &[WorldInstanceData]) -> u64 {
    bytemuck::cast_slice::<_, u8>(data)
        .iter()
        .fold(0xcbf29ce484222325, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
        })
}
fn tuft_mesh() -> Mesh {
    let mut p = vec![];
    let mut n = vec![];
    let mut uv = vec![];
    let mut indices = vec![];
    for i in 0..3 {
        let a = i as f32 * std::f32::consts::PI / 3.;
        let side = Vec3::new(a.cos(), 0., a.sin());
        let start = p.len() as u32;
        p.extend(
            [
                side * (-0.055),
                side * 0.055,
                side * (-0.006) + Vec3::Y * 0.55 + Vec3::X * 0.035,
                side * 0.006 + Vec3::Y * 0.55 + Vec3::X * 0.035,
            ]
            .map(|v| v.to_array()),
        );
        n.extend([Vec3::new(-a.sin(), 0.55, a.cos()).normalize().to_array(); 4]);
        uv.extend([[0., 0.], [1., 0.], [0., 1.], [1., 1.]]);
        indices.extend([start, start + 1, start + 2, start + 1, start + 3, start + 2]);
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, p)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, n)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
    .with_inserted_indices(Indices::U32(indices))
}
pub fn install(app: &mut App) {
    app.init_resource::<EnhancedScenery>()
        .add_systems(OnEnter(crate::ViewerAppState::Playing), reset)
        .add_systems(
            Update,
            update
                .after(crate::view_window::sync_view_window_from_train)
                .after(crate::floating_origin::apply_floating_origin)
                .run_if(in_state(crate::ViewerAppState::Playing)),
        );
}
fn reset(mut commands: Commands, mut state: ResMut<EnhancedScenery>) {
    for entity in state.groups.values() {
        commands.entity(*entity).try_despawn();
    }
    *state = EnhancedScenery::default();
}
#[derive(bevy::ecs::system::SystemParam)]
pub struct SceneryResources<'w> {
    terrain: Option<Res<'w, TerrainScene>>,
    elevation: Option<Res<'w, TerrainElevation>>,
    world: Option<Res<'w, WorldScene>>,
    cache: Res<'w, crate::world::WorldShapeLodCache>,
    assets: Option<Res<'w, crate::shapes::RouteAssets>>,
    pending: Option<Res<'w, crate::world::WorldSpawnProgress>>,
    terrain_pending: Option<Res<'w, crate::terrain_spawn::TerrainSpawnProgress>>,
    live: Option<Res<'w, crate::live::LiveDrive>>,
    weather: Res<'w, crate::weather_state::WeatherState>,
    meshes: ResMut<'w, Assets<Mesh>>,
    images: ResMut<'w, Assets<Image>>,
    materials: ResMut<'w, Assets<crate::terrain_material::TerrainMaterial>>,
    forest: ResMut<'w, Assets<openrailsrs_bevy_scenery::OrForestMaterial>>,
}
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update(
    mut commands: Commands,
    time: Res<Time<Real>>,
    preferences: Res<crate::player_settings::PlayerSettings>,
    focus: Res<RouteFocus>,
    origin: Res<crate::floating_origin::FloatingOrigin>,
    window: Res<crate::view_window::ViewWindow>,
    mut state: ResMut<EnhancedScenery>,
    resources: SceneryResources,
    cameras: Query<&Transform, (With<Camera3d>, Without<GrassChunk>)>,
    mut chunks: Query<(
        Entity,
        &GrassChunk,
        &mut Transform,
        &mut WorldInstanceAppearance,
        &WorldInstanceBuffer,
    )>,
) {
    let SceneryResources {
        terrain,
        elevation,
        world,
        cache,
        assets,
        pending,
        terrain_pending,
        live,
        weather,
        mut meshes,
        mut images,
        mut materials,
        mut forest,
    } = resources;
    let camera_xz = cameras
        .single()
        .map_or(Vec2::ZERO, |camera| camera.translation.xz());
    let profile = SceneryProfile::from_env().unwrap_or(preferences.scenery_profile);
    let quality = SceneryQuality::from_env().unwrap_or(preferences.scenery_quality);
    if profile == SceneryProfile::Authentic && state.profile == profile {
        return;
    }
    if profile != state.profile || quality != state.quality {
        for entity in state.groups.values() {
            commands.entity(*entity).try_despawn();
        }
        state.groups.clear();
        state.empty.clear();
        state.builder = None;
        state.instances = 0;
        state.history.clear();
        state.profile = profile;
        state.quality = quality;
    }
    let enabled = profile == SceneryProfile::Enhanced;
    let clock = live.as_ref().map_or(0., |l| l.session.time_s()) as f32;
    let terrain_value = if enabled {
        Vec4::new(1., 0., origin.shift.x, origin.shift.z)
    } else {
        Vec4::ZERO
    };
    let changed: Vec<_> = materials
        .iter()
        .filter(|(_, m)| m.enhancement != terrain_value)
        .map(|(id, _)| id)
        .collect();
    for id in changed {
        materials.get_mut(id).unwrap().enhancement = terrain_value;
    }
    let forest_value = if enabled {
        Vec4::new(
            preferences.view_distance_m,
            (clock * 10.).floor() / 10.,
            weather.atmosphere.wind_mps.x,
            weather.atmosphere.wind_mps.z,
        )
    } else {
        Vec4::ZERO
    };
    let changed: Vec<_> = forest
        .iter()
        .filter(|(_, m)| m.params.enhancement != forest_value)
        .map(|(id, _)| id)
        .collect();
    for id in changed {
        forest.get_mut(id).unwrap().params.enhancement = forest_value;
    }
    if !enabled {
        state.track_task = None;
        state.mask_task = None;
        state.tracks = None;
        state.track_sources = None;
        state.track_source_task = None;
        state.masks.clear();
        state.bounds.clear();
        state.bounds_task = None;
        return;
    }
    let (Some(terrain), Some(elevation), Some(world), Some(assets)) =
        (terrain, elevation, world, assets)
    else {
        return;
    };
    if let Some(task) = state.bounds_task.as_mut()
        && let Some(bounds) =
            bevy::tasks::block_on(bevy::tasks::futures_lite::future::poll_once(task))
    {
        state.bounds.extend(bounds);
        state.bounds_task = None;
    }
    if state.bounds_task.is_none() {
        let names: HashSet<_> = world
            .items
            .iter()
            .filter(|obj| obj.position.xz().distance(window.center_world.xz()) < 800.)
            .filter_map(|obj| {
                obj.shape_file
                    .as_ref()
                    .map(|name| name.to_ascii_lowercase())
            })
            .collect();
        state.bounds.retain(|name, _| names.contains(name));
        let mut needed: Vec<_> = cache
            .shapes
            .iter()
            .filter_map(|(path, shape)| {
                let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
                (names.contains(&name) && !state.bounds.contains_key(&name))
                    .then(|| (name, shape.clone()))
            })
            .collect();
        needed.sort_by(|a, b| a.0.cmp(&b.0));
        needed.truncate(16.min(512_usize.saturating_sub(state.bounds.len())));
        if !needed.is_empty() {
            state.bounds_task = Some(AsyncComputeTaskPool::get().spawn(async move {
                needed
                    .into_iter()
                    .map(|(name, shape)| (name, shape_footprint(&shape)))
                    .collect()
            }));
        }
    }
    if state.track_sources.is_none() && state.track_source_task.is_none() {
        let paths: Vec<_> = assets.banked_paths.values().cloned().collect();
        let roads = assets.shared_road_db();
        let catalog = roads.as_ref().map(|db| road_catalog(db, assets.tsection()));
        state.track_source_task = Some(AsyncComputeTaskPool::get().spawn(async move {
            let mut masks: Vec<_> = paths.into_iter().map(NativePathMask::new).collect();
            if let Some(db) = roads {
                append_road_paths(&mut masks, db, catalog.unwrap());
            }
            masks
        }));
    }
    if let Some(task) = state.track_source_task.as_mut()
        && let Some(paths) =
            bevy::tasks::block_on(bevy::tasks::futures_lite::future::poll_once(task))
    {
        state.track_sources = Some(Arc::new(paths));
        state.track_source_task = None;
    }
    if state.track_task.is_none()
        && (state.tracks.is_none()
            || state.track_center.distance(window.center_world.xz()) > TRACK_MASK_REBUILD_DISTANCE)
        && let Some(paths) = state.track_sources.clone()
    {
        let center = window.center_world.xz();
        state.track_task = Some(AsyncComputeTaskPool::get().spawn(async move {
            let mut segments = vec![];
            for source in paths.iter() {
                source.append_region_segments(center, &mut segments);
            }
            (
                center,
                crate::track::TrackSegmentIndex::from_segments(segments),
            )
        }));
    }
    if let Some(task) = state.track_task.as_mut()
        && let Some((center, index)) =
            bevy::tasks::block_on(bevy::tasks::futures_lite::future::poll_once(task))
    {
        state.tracks = Some(Arc::new(index));
        state.track_center = center;
        state.track_task = None;
    }
    if let Some(task) = state.mask_task.as_mut()
        && let Some(masks) =
            bevy::tasks::block_on(bevy::tasks::futures_lite::future::poll_once(task))
    {
        state.masks.extend(masks);
        state.mask_task = None;
    }
    let (radius, budget, spacing) = quality.limits();
    let center = window.center_world.xz();
    if state.mask_task.is_none() {
        // Preloaded terrain can contain hundreds of materials unrelated to
        // the small grass window. Load only patches intersecting that window,
        // so alphabetical far-away textures cannot exhaust the mask budget.
        let mut active = HashSet::new();
        for tile in &terrain.tiles {
            let (ox, oz) = openrailsrs_formats::msts_tile_world_origin(tile.tile_x, tile.tile_z);
            let tile_center = Vec2::new(ox + 1024., oz + 1024.);
            if (center - tile_center).abs().max_element() > 1024. + radius + CHUNK * 3. {
                continue;
            }
            let Some(set) = tile.file.primary_patch_set() else {
                continue;
            };
            if set.npatches == 0 {
                continue;
            }
            let size = 2048. / set.npatches as f32;
            for z in 0..set.npatches {
                for x in 0..set.npatches {
                    let patch_center =
                        Vec2::new(ox + (x as f32 + 0.5) * size, oz + (z as f32 + 0.5) * size);
                    if (center - patch_center).abs().max_element()
                        > size * 0.5 + radius + CHUNK * 3.
                    {
                        continue;
                    }
                    if let Some(texture) = set
                        .patch_at(x, z)
                        .and_then(|patch| tile.file.shaders.get(patch.shader_index as usize))
                        .and_then(|shader| shader.texslots.first())
                    {
                        active.insert(texture.filename.clone());
                    }
                }
            }
        }
        state.masks.retain(|name, _| active.contains(name));
        let mut needed: Vec<_> = active
            .into_iter()
            .filter(|name| !state.masks.contains_key(name))
            .collect();
        needed.sort();
        needed.truncate(4.min(64_usize.saturating_sub(state.masks.len())));
        if !needed.is_empty() && state.masks.len() < 64 {
            let root = assets.route_dir.clone();
            state.mask_task = Some(AsyncComputeTaskPool::get().spawn(async move {
                needed
                    .into_iter()
                    .map(|name| {
                        let mask = crate::terrain_assets::resolve_terrtex_for_environment(
                            &root,
                            &name,
                            openrailsrs_bevy_scenery::textures::TextureEnvironment::summer_day(),
                        )
                        .and_then(|path| {
                            openrailsrs_bevy_scenery::texture_cache::load(
                                &path,
                                bevy::image::CompressedImageFormats::NONE,
                                None,
                                None,
                            )
                            .ok()
                        })
                        .and_then(|texture| MaskImage::from_image(&texture.image));
                        (name, mask)
                    })
                    .collect()
            }));
        }
    }
    let key = (
        (center.x / CHUNK).floor() as i32,
        (center.y / CHUNK).floor() as i32,
    );
    let loaded: HashSet<_> = terrain.tiles.iter().map(|t| (t.tile_x, t.tile_z)).collect();
    let mut remove = vec![];
    let mut instances = 0;
    for (entity, chunk, mut tf, mut appearance, buffer) in &mut chunks {
        let chunk_center = Vec2::new(
            (chunk.key.0 as f32 + 0.5) * CHUNK,
            (chunk.key.1 as f32 + 0.5) * CHUNK,
        );
        let distance = chunk_center.distance(center);
        if !loaded.contains(&chunk.tile) || distance > radius + CHUNK * 2. {
            commands.entity(entity).try_despawn();
            remove.push(chunk.key);
            continue;
        }
        instances += buffer.len();
        tf.translation = Vec3::new(
            chunk.key.0 as f32 * CHUNK - focus.center.x - origin.shift.x,
            0.,
            chunk.key.1 as f32 * CHUNK - focus.center.z - origin.shift.z,
        );
        appearance.vegetation_view = camera_xz;
        appearance.vegetation = Vec4::new(
            radius,
            clock,
            weather.atmosphere.wind_mps.x,
            weather.atmosphere.wind_mps.z,
        );
        let age = time.elapsed_secs_f64() - chunk.born;
        appearance.lod_fade = if age < 0.5 {
            (age as f32 / 0.5).max(0.001)
        } else {
            0.
        };
        // Shadow casters are restricted to the closest ring.
        if distance < 28. {
            commands.entity(entity).remove::<NotShadowCaster>();
        } else {
            commands.entity(entity).insert(NotShadowCaster);
        }
    }
    for key in remove {
        state.groups.remove(&key);
    }
    state.instances = instances;
    state
        .empty
        .retain(|(x, z)| (x - key.0).abs() <= 8 && (z - key.1).abs() <= 8);
    if pending.is_some()
        || terrain_pending.is_some()
        || state.tracks.is_none()
        || state.track_center.distance(center) + radius + CHUNK * 2. > TRACK_MASK_RADIUS - 32.
        || state.bounds_task.is_some()
    {
        return;
    }
    if state.mesh.is_none() {
        state.mesh = Some(meshes.add(tuft_mesh()));
        state.white = Some(images.add(Image::new_fill(
            bevy::render::render_resource::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            bevy::render::render_resource::TextureDimension::D2,
            &[255, 255, 255, 255],
            bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        )));
    }
    if state.builder.is_none() && instances < budget {
        let ring = (radius / CHUNK).ceil() as i32 + 1;
        let mut wanted = vec![];
        for x in key.0 - ring..=key.0 + ring {
            for z in key.1 - ring..=key.1 + ring {
                let p = Vec2::new((x as f32 + 0.5) * CHUNK, (z as f32 + 0.5) * CHUNK);
                if p.distance(center) < radius + CHUNK
                    && !state.groups.contains_key(&(x, z))
                    && !state.empty.contains(&(x, z))
                {
                    wanted.push(((x, z), p.distance_squared(center)));
                }
            }
        }
        wanted.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        if let Some(((x, z), _)) = wanted.first().copied() {
            let p = Vec2::new((x as f32 + 0.5) * CHUNK, (z as f32 + 0.5) * CHUNK);
            let tile_key = (
                openrailsrs_formats::msts_tile_x_index_for_coord(p.x),
                openrailsrs_formats::msts_tile_z_index_for_coord(p.y),
            );
            if loaded.contains(&tile_key) && world.loaded_tiles.contains(&tile_key) {
                let exclusions = world
                    .items
                    .iter()
                    .filter(|obj| obj.position.xz().distance(p) < 512.)
                    .filter_map(|obj| {
                        if obj.kind == "TrackObj" && !assets.banked_paths.is_empty() {
                            return None;
                        }
                        if let Some(bounds) = obj
                            .shape_file
                            .as_ref()
                            .and_then(|name| state.bounds.get(&name.to_ascii_lowercase()))
                            .and_then(|bounds| *bounds)
                        {
                            return Some(placed_footprint(
                                bounds,
                                obj.position,
                                obj.rotation,
                                obj.scale,
                            ));
                        }
                        let half = if let Some(f) = &obj.forest {
                            Vec2::new(
                                if f.patch_half_x > 0. {
                                    f.patch_half_x
                                } else {
                                    128.
                                },
                                if f.patch_half_z > 0. {
                                    f.patch_half_z
                                } else {
                                    128.
                                },
                            ) + Vec2::splat(f.tree_width * 0.5 + 4.)
                        } else if let Some(w) = &obj.water {
                            Vec2::new(w.half_x, w.half_z) + Vec2::splat(4.)
                        } else if let Some(t) = &obj.transfer {
                            Vec2::new(t.width, t.height) * 0.5 + Vec2::splat(4.)
                        } else {
                            let radius = obj
                                .shape_file
                                .as_ref()
                                .and_then(|name| {
                                    cache
                                        .shapes
                                        .iter()
                                        .find(|(p, _)| {
                                            p.file_name().is_some_and(|f| {
                                                f.to_string_lossy().eq_ignore_ascii_case(name)
                                            })
                                        })
                                        .map(|(_, s)| s.view_sphere_radius_or_default())
                                })
                                .unwrap_or(100.);
                            Vec2::splat(radius * obj.scale.abs().max_element() + 5.)
                        };
                        let yaw = obj.rotation.to_euler(EulerRot::YXZ).0;
                        Some(Exclusion {
                            center: obj.position.xz(),
                            half,
                            inverse: Mat2::from_angle(-yaw),
                        })
                    })
                    .collect();
                state.builder = Some(Builder {
                    key: (x, z),
                    tile: tile_key,
                    cursor: 0,
                    side: (CHUNK / spacing).floor() as usize,
                    spacing,
                    instances: vec![],
                    seed: seed((x, z), "terrain"),
                    exclusions,
                });
            }
        }
    }
    let started = Instant::now();
    let Some(mut builder) = state.builder.take() else {
        return;
    };
    let builder_center = Vec2::new(
        (builder.key.0 as f32 + 0.5) * CHUNK,
        (builder.key.1 as f32 + 0.5) * CHUNK,
    );
    if !loaded.contains(&builder.tile) || builder_center.distance(center) > radius + CHUNK * 2. {
        return;
    }
    while builder.cursor < builder.side * builder.side
        && started.elapsed() < Duration::from_micros(750)
    {
        let i = builder.cursor;
        builder.cursor += 1;
        let rng =
            |salt| crate::precipitation::rain_rng01(builder.seed.wrapping_add(salt), i as u32);
        let local = Vec2::new(
            (i % builder.side) as f32 * builder.spacing + rng(1) * builder.spacing,
            (i / builder.side) as f32 * builder.spacing + rng(2) * builder.spacing,
        );
        let p = Vec2::new(builder.key.0 as f32 * CHUNK, builder.key.1 as f32 * CHUNK) + local;
        if builder.exclusions.iter().any(|m| m.contains(p)) {
            state.rejected += 1;
            state.rejections[0] += 1;
            continue;
        }
        let rail_distance = state
            .tracks
            .as_ref()
            .unwrap()
            .min_distance_xz(p.x, p.y, 16.);
        if rail_distance < 6. {
            state.rejected += 1;
            state.rejections[1] += 1;
            continue;
        }
        let Some(tile) = terrain
            .tiles
            .iter()
            .find(|t| (t.tile_x, t.tile_z) == builder.tile)
        else {
            continue;
        };
        let Some(set) = tile.file.primary_patch_set() else {
            continue;
        };
        let (ox, oz) = openrailsrs_formats::msts_tile_world_origin(builder.tile.0, builder.tile.1);
        let patch_size = 2048. / set.npatches as f32;
        let patch_x = (p.x - ox) / patch_size;
        let patch_z = (p.y - oz) / patch_size;
        let Some(patch) = set
            .patch_at(patch_x.floor() as u32, patch_z.floor() as u32)
            .filter(|p| p.drawing_enabled() && !p.water_enabled())
        else {
            state.rejected += 1;
            continue;
        };
        let Some(shader) = tile.file.shaders.get(patch.shader_index as usize) else {
            continue;
        };
        let Some(texture) = shader.texslots.first() else {
            continue;
        };
        let Some(mask) = state.masks.get(&texture.filename) else {
            if state.masks.len() < 64 {
                builder.cursor -= 1;
                break;
            }
            // A route with more than 64 simultaneous terrain materials still
            // progresses safely: unclassified textures never receive grass.
            state.rejected += 1;
            continue;
        };
        let Some(mask) = mask else {
            state.rejected += 1;
            continue;
        };
        // Native affine coefficients take cell coordinates (0..16), not a
        // normalized patch fraction. Use the same mapping as the terrain mesh.
        let [u, v] = openrailsrs_formats::patch_affine_uv(
            patch,
            patch_x.fract() * 16.,
            patch_z.fract() * 16.,
        );
        if !mask.allows(u, v) {
            state.rejected += 1;
            state.rejections[2] += 1;
            continue;
        }
        let Some(y) = elevation.sample_world_y(p.x, p.y) else {
            continue;
        };
        let Some(yx) = elevation.sample_world_y(p.x + 8., p.y) else {
            continue;
        };
        let Some(yz) = elevation.sample_world_y(p.x, p.y + 8.) else {
            continue;
        };
        let slope = ((y - yx).abs() + (y - yz).abs()) / 8.;
        if slope > 0.7 || rng(3) > ((rail_distance - 6.) / 7.).clamp(0., 1.) * (1. - slope).max(0.2)
        {
            state.rejected += 1;
            state.rejections[3] += 1;
            continue;
        }
        let material_seed = seed(builder.tile, &shader.name);
        let scale = 0.45 + crate::precipitation::rain_rng01(material_seed, i as u32) * 0.8;
        builder.instances.push(WorldInstanceData::from_transform(
            Transform::from_xyz(local.x, y - focus.height_origin + 0.015, local.y)
                .with_rotation(Quat::from_rotation_y(rng(4) * std::f32::consts::TAU))
                .with_scale(Vec3::new(
                    scale,
                    scale * if rail_distance < 10. { 0.4 } else { 1. },
                    scale,
                )),
        ));
    }
    state.max_build_ms = state
        .max_build_ms
        .max(started.elapsed().as_secs_f64() * 1000.);
    if builder.cursor == builder.side * builder.side {
        if builder.instances.is_empty() {
            state.empty.insert(builder.key);
            return;
        }
        // A per-chunk cap is independent of camera arrival/order: returning
        // never changes which candidates survive the global budget.
        builder.instances.sort_by_key(|instance| {
            let p = instance.translation();
            p.x.to_bits().wrapping_mul(0x9e3779b9)
                ^ p.z.to_bits().wrapping_mul(0x85ebca6b)
                ^ builder.seed
        });
        builder.instances.truncate(quality.chunk_budget());
        let hash = buffer_hash(&builder.instances);
        if let Some(old) = state.history.get(&builder.key).copied() {
            state.returned += 1;
            if old != hash {
                state.hash_mismatches += 1;
            }
        }
        if state.history.len() < 512 {
            state.history.insert(builder.key, hash);
        }
        state.generated += 1;
        state.instances += builder.instances.len();
        let count = builder.instances.len();
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        for i in &builder.instances {
            min = min.min(i.translation() - Vec3::splat(0.5));
            max = max.max(i.translation() + Vec3::new(0.5, 1., 0.5));
        }
        let tint = match live.as_ref().map(|l| l.season.as_str()) {
            Some("autumn") => LinearRgba::new(0.18, 0.20, 0.06, 1.),
            Some("winter") => LinearRgba::new(0.14, 0.17, 0.10, 1.),
            _ => LinearRgba::new(0.12, 0.20, 0.045, 1.),
        };
        let entity = commands
            .spawn((
                GrassChunk {
                    key: builder.key,
                    tile: builder.tile,
                    born: time.elapsed_secs_f64(),
                },
                Mesh3d(state.mesh.as_ref().unwrap().clone()),
                WorldInstanceBuffer(Arc::from(builder.instances)),
                WorldInstanceAppearance {
                    base_color: tint,
                    base_color_texture: state.white.clone(),
                    alpha_cutoff: 0.,
                    cull_mode: None,
                    double_sided: true,
                    world_from_local: Mat4::IDENTITY,
                    lod_fade: 0.001,
                    surface_weather: Vec2::new(
                        weather.atmosphere.rain,
                        weather.atmosphere.snow_cover,
                    ),
                    vegetation_view: camera_xz,
                    vegetation: Vec4::new(
                        radius,
                        clock,
                        weather.atmosphere.wind_mps.x,
                        weather.atmosphere.wind_mps.z,
                    ),
                },
                Transform::from_xyz(
                    builder.key.0 as f32 * CHUNK - focus.center.x - origin.shift.x,
                    0.,
                    builder.key.1 as f32 * CHUNK - focus.center.z - origin.shift.z,
                ),
                Aabb::from_min_max(min, max),
                crate::world::WorldTileBound {
                    tile_x: builder.tile.0,
                    tile_z: builder.tile.1,
                },
                NotShadowCaster,
                Name::new(format!("grass:{}:{}:{count}", builder.key.0, builder.key.1)),
            ))
            .id();
        state.groups.insert(builder.key, entity);
    } else {
        state.builder = Some(builder);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_road_shapes_are_excluded_without_changing_the_rail_catalogue() {
        use openrailsrs_formats::{
            TSectionCatalog, TrVectorSectionRecord, TrackDbFile, TrackDbNode, TrackNodeKind,
            TrackVectorPoint,
            typed::{TrackSectionDef, TrackShapeDef, TrackShapePath},
        };
        let start = TrackVectorPoint {
            tile_x: 0,
            tile_z: 0,
            x: 0.,
            y: 0.,
            z: 0.,
        };
        let node = TrackDbNode {
            id: 1,
            position: Some(start),
            pin_refs: vec![],
            kind: TrackNodeKind::Vector {
                length_m: 100.,
                speed_limit_mps: 0.,
                pins: (0, 0),
                item_ids: vec![],
                sections: vec![TrVectorSectionRecord {
                    section_index: 1,
                    shape_index: 1,
                    header_tile_x: 0,
                    header_tile_z: 0,
                    start,
                    ax: 0.,
                    ay: 0.,
                    az: 0.,
                }],
                geometry: None,
            },
        };
        let mut catalog = TSectionCatalog::default();
        catalog.sections.insert(
            1,
            TrackSectionDef {
                gauge_m: 5.,
                length_m: 100.,
                curve_radius_m: None,
                curve_angle_deg: None,
                skew_deg: None,
            },
        );
        catalog.shapes.insert(
            1,
            TrackShapeDef {
                file_name: "road.s".into(),
                main_route: None,
                clearance_dist_m: None,
                road_shape: true,
                paths: vec![TrackShapePath {
                    num_sections: 1,
                    offset: [0.; 3],
                    angle_deg: 0.,
                    section_indices: vec![2],
                }],
            },
        );
        catalog.sections.insert(2, catalog.sections[&1]);
        catalog.shapes.insert(9999, catalog.shapes[&1].clone());
        catalog.sections.insert(9999, catalog.sections[&1]);
        assert!(NativePath::new(&node, Some(&catalog)).is_none());
        let mut db = TrackDbFile::default();
        db.nodes.push(node);
        let mut paths = vec![];
        let db = Arc::new(db);
        let compact = road_catalog(&db, &catalog);
        assert_eq!(compact.sections.len(), 2);
        assert_eq!(compact.shapes.len(), 1);
        assert_eq!(compact.procedural_dims(1), catalog.procedural_dims(1));
        append_road_paths(&mut paths, db.clone(), compact);
        assert_eq!(paths.len(), 1);
        assert!(catalog.shapes[&1].road_shape);
        let source = paths.pop().unwrap();
        let MaskPath::Road { context, .. } = &source.path else {
            panic!("road spans must not be retained");
        };
        assert!(Arc::ptr_eq(&context.db, &db));
        let mut segments = vec![];
        source.append_region_segments(Vec2::ZERO, &mut segments);
        let index = crate::track::TrackSegmentIndex::from_segments(segments);
        assert_eq!(index.min_distance_xz(0., -50., 10.), 0.);
        assert!(index.min_distance_xz(20., -50., 10.) >= 10.);
    }
    #[test]
    fn regional_masks_preserve_curved_clearance_across_camera_rebuilds() {
        use openrailsrs_formats::{
            TSectionCatalog, TrVectorSectionRecord, TrackDbNode, TrackNodeKind, TrackVectorPoint,
            typed::TrackSectionDef,
        };
        let start = TrackVectorPoint {
            tile_x: 0,
            tile_z: 0,
            x: 0.,
            y: 0.,
            z: 0.,
        };
        let mut catalog = TSectionCatalog::default();
        catalog.sections.insert(
            1,
            TrackSectionDef {
                gauge_m: 1.435,
                length_m: 0.,
                curve_radius_m: Some(2000.),
                curve_angle_deg: Some(180.),
                skew_deg: None,
            },
        );
        let path = NativePath::new(
            &TrackDbNode {
                id: 1,
                position: Some(start),
                pin_refs: vec![],
                kind: TrackNodeKind::Vector {
                    length_m: 2000. * std::f64::consts::PI,
                    speed_limit_mps: 0.,
                    pins: (0, 0),
                    item_ids: vec![],
                    sections: vec![TrVectorSectionRecord {
                        section_index: 1,
                        shape_index: 0,
                        header_tile_x: 0,
                        header_tile_z: 0,
                        start,
                        ax: 0.,
                        ay: 0.,
                        az: 0.,
                    }],
                    geometry: None,
                },
            },
            Some(&catalog),
        )
        .unwrap();
        let center = path
            .pose_in_frame(path.length_m() / 2., bevy::math::DVec3::ZERO)
            .position
            .xz();
        // A curve can enter the region even when both endpoints are outside it.
        for at in [0., path.length_m()] {
            assert!(
                path.pose_in_frame(at, bevy::math::DVec3::ZERO)
                    .position
                    .xz()
                    .distance(center)
                    > TRACK_MASK_RADIUS
            );
        }
        let path = Arc::new(path);
        let source = NativePathMask::new(path.clone());
        let index = |at| {
            let mut segments = vec![];
            source.append_region_segments(at, &mut segments);
            crate::track::TrackSegmentIndex::from_segments(segments)
        };
        let a = index(center);
        let b = index(center + Vec2::X * TRACK_MASK_REBUILD_DISTANCE);
        assert!(a.segment_count() > 0);
        assert!(a.segment_count() < (path.length_m() / 8.).ceil() as usize);
        assert_eq!(index(center + Vec2::splat(100_000.)).segment_count(), 0);
        for distance in [-100., -50., 0., 50., 100.] {
            let p = path
                .pose_in_frame(path.length_m() / 2. + distance, bevy::math::DVec3::ZERO)
                .position
                .xz();
            for offset in [0., 1., 4., 10.] {
                let p = p + Vec2::X * offset;
                assert_eq!(
                    a.min_distance_xz(p.x, p.y, 20.),
                    b.min_distance_xz(p.x, p.y, 20.)
                );
            }
        }
    }
    #[test]
    fn unknown_pavement_water_and_white_textures_are_excluded() {
        assert!(is_grass([60, 90, 40]));
        for rgb in [[90, 90, 90], [30, 50, 90], [230, 230, 230], [0, 0, 0]] {
            assert!(!is_grass(rgb));
        }
    }
    #[test]
    fn seeds_are_stable_but_tiles_and_materials_differ() {
        assert_eq!(seed((-22, 33), "grass"), seed((-22, 33), "grass"));
        assert_ne!(seed((-22, 33), "grass"), seed((-21, 33), "grass"));
        assert_ne!(seed((-22, 33), "grass"), seed((-22, 33), "water"));
    }
    #[test]
    fn safety_masks_include_rotated_footprints_and_quality_is_bounded() {
        let mask = Exclusion {
            center: Vec2::new(10., 20.),
            half: Vec2::new(4., 1.),
            inverse: Mat2::from_angle(-std::f32::consts::FRAC_PI_2),
        };
        assert!(mask.contains(Vec2::new(10., 23.)));
        assert!(!mask.contains(Vec2::new(13., 20.)));
        assert_eq!(SceneryProfile::default(), SceneryProfile::Authentic);
        for q in [
            SceneryQuality::Low,
            SceneryQuality::Medium,
            SceneryQuality::High,
        ] {
            assert!(q.limits().1 <= 16384);
        }
    }
    #[test]
    fn moving_the_camera_cannot_overfill_retained_chunks() {
        for q in [
            SceneryQuality::Low,
            SceneryQuality::Medium,
            SceneryQuality::High,
        ] {
            let (radius, budget, _) = q.limits();
            // Exercise fractional camera positions including grid boundaries,
            // and the larger unload ring, not just the currently visible ring.
            for dx in [0., 0.1, 15.9, 16., 31.9] {
                for dz in [0., 16., 31.9] {
                    let center = Vec2::new(dx, dz);
                    let retained = (-10..=10)
                        .flat_map(|x| (-10..=10).map(move |z| (x, z)))
                        .filter(|(x, z)| {
                            Vec2::new((*x as f32 + 0.5) * CHUNK, (*z as f32 + 0.5) * CHUNK)
                                .distance(center)
                                <= radius + CHUNK * 2.
                        })
                        .count();
                    assert!(retained * q.chunk_budget() <= budget);
                }
            }
        }
    }
    #[test]
    fn narrow_paths_are_excluded_across_wrapped_texture_edges() {
        let mut mask = MaskImage {
            size: 8,
            green: vec![true; 64],
        };
        mask.green[3 * 8] = false;
        assert!(!mask.allows(0.99, 3. / 8.));
        assert!(!mask.allows(0., 3. / 8.));
        assert!(mask.allows(0.5, 3. / 8.));
    }
    #[test]
    fn geometry_offsets_and_negative_scales_define_the_exclusion_not_view_radius() {
        use openrailsrs_formats::{
            DistanceLevel, LodControl, Primitive, ShapeFile, SubObject, Vertex,
        };
        let shape = ShapeFile {
            points: vec![
                openrailsrs_formats::Vec3 {
                    x: 200.,
                    y: 0.,
                    z: 0.,
                },
                openrailsrs_formats::Vec3 {
                    x: 204.,
                    y: 0.,
                    z: 0.,
                },
                openrailsrs_formats::Vec3 {
                    x: 200.,
                    y: 4.,
                    z: 2.,
                },
            ],
            lod_controls: vec![LodControl {
                distance_levels: vec![DistanceLevel {
                    sub_objects: vec![SubObject {
                        vertices: (0..3)
                            .map(|point_idx| Vertex {
                                point_idx,
                                ..default()
                            })
                            .collect(),
                        primitives: vec![Primitive {
                            vertex_indices: vec![0, 1, 2],
                            ..default()
                        }],
                        ..default()
                    }],
                    ..default()
                }],
            }],
            ..default()
        };
        let bounds = shape_footprint(&shape).unwrap();
        let mask = placed_footprint(
            bounds,
            Vec3::new(10., 0., 20.),
            Quat::IDENTITY,
            Vec3::new(-2., 1., 1.),
        );
        assert!(mask.contains(Vec2::new(-394., 19.)));
        assert!(!mask.contains(Vec2::new(10., 20.)));
        assert!(shape_footprint(&ShapeFile::default()).is_none());
    }
}
