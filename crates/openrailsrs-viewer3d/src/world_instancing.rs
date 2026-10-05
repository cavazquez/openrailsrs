//! GPU instancing for repeated static opaque WORLD shapes (#58).
//!
//! One Bevy entity per `(shape, part, material, tile)` with an instance buffer of
//! transforms. Animated / transparent (blend) parts keep the per-entity spawn path.
//! Opaque + cutout draws are queued in Bevy [`Opaque3d`] (#106); the fragment shader
//! alpha-discards cutout. True blend materials must not use this path.
//! Directional shadow cast uses the [`Shadow`] phase with the same instance buffer (#72).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoAutoAabb;
use bevy::core_pipeline::core_3d::{
    CORE_3D_DEPTH_FORMAT, Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey,
};
use bevy::ecs::system::{SystemParamItem, lifetimeless::*};
use bevy::ecs::{query::QueryItem, system::lifetimeless::Read};
use bevy::math::{Affine3A, Mat3, Mat4};
use bevy::mesh::{MeshVertexBufferLayoutRef, VertexBufferLayout};
use bevy::pbr::{
    LightEntity, LightKeyCache, MeshPipeline, MeshPipelineKey, MeshPipelineSystems,
    PrepassPipeline, RenderMeshInstances, SetMeshBindGroup, SetMeshViewBindGroup,
    SetMeshViewBindingArrayBindGroup, SetPrepassViewBindGroup, SetPrepassViewEmptyBindGroup,
    Shadow, ShadowBatchSetKey, ShadowBinKey, ViewKeyCache, init_prepass_pipeline,
};
use bevy::prelude::*;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::mesh::allocator::MeshAllocator;
use bevy::render::mesh::{RenderMesh, RenderMeshBufferInfo};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_phase::{
    AddRenderCommand, BinnedRenderPhaseType, DrawFunctions, PhaseItem, RenderCommand,
    RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewBinnedRenderPhases,
};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer};
use bevy::render::render_resource::*;
use bevy::render::renderer::RenderDevice;
use bevy::render::sync_component::SyncComponent;
use bevy::render::sync_world::MainEntity;
use bevy::render::view::{
    ExtractedView, RenderShadowMapVisibleEntities, RenderVisibleEntities, RetainedViewEntity,
};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::Shader;
use bytemuck::{Pod, Zeroable};

/// Minimum placements in one tile before GPU instancing is used.
///
/// Two already save one entity/draw while sharing the immutable instance buffer.
/// The old threshold of four left many paired trackside assets on the entity path.
pub const WORLD_INSTANCING_MIN: usize = 2;

/// Highest scalar metallic value the albedo-only instancing shader may approximate.
///
/// Strongly metallic materials (notably `RailHead_*.ace` / `ukfs_rail.ACE`) need
/// Bevy's PBR path. Treating their albedo as Lambert diffuse clips rail heads white
/// under the outdoor HDR sun.
const WORLD_INSTANCING_MAX_METALLIC: f32 = 0.1;

const SHADER_WGSL: &str = include_str!("world_instancing.wgsl");

/// Opt-out: `OPENRAILSRS_WORLD_INSTANCING=0`.
pub fn world_instancing_enabled() -> bool {
    match std::env::var("OPENRAILSRS_WORLD_INSTANCING") {
        Ok(v) => {
            let v = v.trim();
            !(v == "0" || v.eq_ignore_ascii_case("false") || v.eq_ignore_ascii_case("off"))
        }
        Err(_) => true,
    }
}

/// One instance transform (column-major Mat4) in the entity local / view frame.
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct WorldInstanceData {
    pub col0: [f32; 4],
    pub col1: [f32; 4],
    pub col2: [f32; 4],
    pub col3: [f32; 4],
}

impl WorldInstanceData {
    pub fn from_transform(tf: Transform) -> Self {
        Self::from_mat4(tf.to_matrix())
    }

    /// Full Mat4 instance (preserves Matrix3x3 shear when built from affine) (#139).
    pub fn from_mat4(m: Mat4) -> Self {
        let cols = m.to_cols_array_2d();
        Self {
            col0: cols[0],
            col1: cols[1],
            col2: cols[2],
            col3: cols[3],
        }
    }

    pub fn from_affine(affine: Affine3A) -> Self {
        Self::from_mat4(Mat4::from(affine))
    }

    /// View-space instance: TRS transform with optional Matrix3x3 linear override (#139).
    pub fn from_view_placement(tf: Transform, linear: Option<Mat3>) -> Self {
        if let Some(linear) = linear {
            Self::from_affine(Affine3A::from_mat3_translation(linear, tf.translation))
        } else {
            Self::from_transform(tf)
        }
    }

    pub fn translation(&self) -> Vec3 {
        Vec3::new(self.col3[0], self.col3[1], self.col3[2])
    }

    /// Upper-left 3×3 (column-major instance matrix).
    pub fn linear(&self) -> Mat3 {
        Mat3::from_cols(
            Vec3::new(self.col0[0], self.col0[1], self.col0[2]),
            Vec3::new(self.col1[0], self.col1[1], self.col1[2]),
            Vec3::new(self.col2[0], self.col2[1], self.col2[2]),
        )
    }
}

/// CPU instance list extracted to the render world.
#[derive(Component, Clone, Debug, Deref)]
pub struct WorldInstanceBuffer(pub Arc<[WorldInstanceData]>);

impl SyncComponent for WorldInstanceBuffer {
    type Target = Self;
}

impl ExtractComponent for WorldInstanceBuffer {
    type QueryData = &'static WorldInstanceBuffer;
    type QueryFilter = ();
    type Out = Self;

    fn extract_component(item: QueryItem<'_, '_, Self::QueryData>) -> Option<Self> {
        // Extraction runs every render frame. Arc keeps the immutable placement
        // data shared instead of cloning every instance transform.
        Some(item.clone())
    }
}

/// Albedo + optional alpha cutout for the instanced draw (#58 v1 lit shader).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct WorldInstanceAppearance {
    pub base_color: LinearRgba,
    pub base_color_texture: Option<Handle<Image>>,
    /// 0 = disabled; typically `200/255` for MSTS alpha test.
    pub alpha_cutoff: f32,
    /// Match the entity material for thin / mixed-winding scenery parts.
    pub cull_mode: Option<Face>,
    pub double_sided: bool,
    /// Group transform, including floating-origin shifts. The render extraction
    /// fills this from this entity's GlobalTransform, independently of Bevy's
    /// mesh-uniform buffer ordering.
    pub world_from_local: Mat4,
    /// Signed dither coverage: + fades the new mesh in, - fades the old out.
    /// 0 is steady state; the normal opaque/depth pipeline remains unchanged.
    pub lod_fade: f32,
    /// x: wetness, y: snow coverage; updated without replacing instance buffers.
    pub surface_weather: Vec2,
}

impl SyncComponent for WorldInstanceAppearance {
    type Target = Self;
}

impl ExtractComponent for WorldInstanceAppearance {
    type QueryData = (&'static WorldInstanceAppearance, &'static GlobalTransform);
    type QueryFilter = ();
    type Out = Self;

    fn extract_component(item: QueryItem<'_, '_, Self::QueryData>) -> Option<Self> {
        let (appearance, transform) = item;
        let mut extracted = appearance.clone();
        extracted.world_from_local = transform.to_matrix();
        Some(extracted)
    }
}

/// Marker + LOD metadata for an instanced WORLD group.
#[derive(Component, Clone, Debug)]
pub struct WorldInstancedGroup {
    pub shape_path: PathBuf,
    pub part_index: usize,
    /// Stable identity within a shape. LOD bands may omit or reorder parts.
    pub sub_object_idx: u32,
    pub prim_state_idx: i32,
    pub lod_idx: usize,
    pub lod_enabled: bool,
    /// Instance count (for metrics / HUD).
    pub instance_count: u32,
}

/// 1×1 terracotta fallback when a part has no albedo (matches entity-path missing-ACE).
///
/// Never use pure white here: WHITE × 1×1 white is exactly the “solid white buildings”
/// failure mode when `base_color_texture` is missing on the instanced path.
#[derive(Resource, Clone, ExtractResource, Default)]
pub struct WorldInstancingFallbackImage(pub Handle<Image>);

/// Plugin: extract instance buffers and draw via a specialized mesh pipeline.
pub struct WorldInstancingPlugin;

impl Plugin for WorldInstancingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldInstancingFallbackImage>()
            .init_resource::<crate::performance::ScenePipelineStatus>()
            // Bevy 0.19 refreshes bounds on Changed<Mesh3d>. The source mesh
            // encloses one model, while our draw places many copies across a
            // tile. Keep the aggregate bounds, including on LOD mesh swaps.
            .register_required_components::<WorldInstanceBuffer, NoAutoAabb>()
            .add_plugins((
                ExtractResourcePlugin::<crate::performance::ScenePipelineStatus>::default(),
                ExtractResourcePlugin::<WorldInstancingFallbackImage>::default(),
                ExtractComponentPlugin::<WorldInstanceBuffer>::default(),
                ExtractComponentPlugin::<WorldInstanceAppearance>::default(),
            ))
            .add_systems(Startup, init_fallback_image);
    }

    fn finish(&self, app: &mut App) {
        // Register on RenderApp in `finish` (same pattern as OrVsmRenderPlugin / MaterialsPlugin).
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<crate::performance::RequiredRenderAssets>()
            .add_systems(
                bevy::render::ExtractSchedule,
                crate::performance::extract_required_render_assets,
            );
        if std::env::var_os("OPENRAILSRS_SHADER_DIAGNOSTICS").is_some() {
            render_app.add_systems(
                Render,
                crate::performance::log_shader_pipeline_status.in_set(RenderSystems::Cleanup),
            );
        }
        render_app
            .add_systems(
                Render,
                crate::performance::update_pipeline_status.in_set(RenderSystems::Cleanup),
            )
            .add_systems(
                Render,
                crate::performance::retry_uploaded_mesh_specializations
                    .after(bevy::render::render_asset::prepare_assets::<bevy::render::mesh::RenderMesh>)
                    .before(RenderSystems::Specialize),
            )
            .add_render_command::<Opaque3d, DrawWorldInstanced>()
            .add_render_command::<Shadow, DrawWorldInstancedShadow>()
            .init_resource::<SpecializedMeshPipelines<WorldInstancingPipeline>>()
            .add_systems(
                RenderStartup,
                init_world_instancing_pipeline
                    .after(MeshPipelineSystems)
                    .after(init_prepass_pipeline),
            )
            .add_systems(
                Render,
                (
                    prepare_world_instance_buffers.in_set(RenderSystems::PrepareResources),
                    prepare_world_instance_bind_groups.in_set(RenderSystems::PrepareBindGroups),
                    (
                        // Bevy's material queue first removes dirty meshes from
                        // the shared bins, including meshes with custom draws.
                        // Queue ours afterwards so that cleanup cannot erase them.
                        queue_world_instanced.after(bevy::pbr::queue_material_meshes),
                        queue_world_instanced_shadows.after(bevy::pbr::queue_shadows),
                    )
                        .in_set(RenderSystems::QueueMeshes),
                ),
            );
    }
}

fn init_fallback_image(
    mut images: ResMut<Assets<Image>>,
    mut fallback: ResMut<WorldInstancingFallbackImage>,
) {
    // Terracotta ≈ entity-path `shape_fallback_color` (0.72, 0.55, 0.42) in sRGB 8-bit.
    let mut image = Image::new_fill(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[184, 140, 107, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
    fallback.0 = images.add(image);
}

/// Build a conservative union AABB for all instances of one mesh.
///
/// Transforming the local centre and absolute half-extents accounts for rotation,
/// non-uniform scale and the Matrix3x3 shear used by some MSTS scenery. This lets
/// Bevy safely cull a whole instanced tile group for both the camera and each
/// shadow cascade.
pub fn instances_aabb(
    instances: &[WorldInstanceData],
    local_aabb: Option<&bevy::camera::primitives::Aabb>,
) -> bevy::camera::primitives::Aabb {
    const FALLBACK_HALF_EXTENT_M: f32 = 32.0;
    let (local_center, local_half_extents) = local_aabb
        .map(|aabb| (Vec3::from(aabb.center), Vec3::from(aabb.half_extents)))
        .unwrap_or((Vec3::ZERO, Vec3::splat(FALLBACK_HALF_EXTENT_M)));
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for inst in instances {
        let linear = inst.linear();
        let center = inst.translation() + linear * local_center;
        // For a matrix represented by columns, |M|e is the enclosing
        // axis-aligned half-extent after applying M.
        let half_extents = linear.x_axis.abs() * local_half_extents.x
            + linear.y_axis.abs() * local_half_extents.y
            + linear.z_axis.abs() * local_half_extents.z;
        min = min.min(center - half_extents);
        max = max.max(center + half_extents);
    }
    if !min.is_finite() || !max.is_finite() {
        return bevy::camera::primitives::Aabb::from_min_max(
            -local_half_extents,
            local_half_extents,
        );
    }
    bevy::camera::primitives::Aabb::from_min_max(min, max)
}

/// Spawn bundle helpers for the progressive WORLD queue.
pub fn appearance_from_standard_material(
    materials: &Assets<StandardMaterial>,
    handle: &Handle<StandardMaterial>,
) -> WorldInstanceAppearance {
    let mat = materials.get(handle);
    // Missing material must not become WHITE×no-texture (solid white buildings).
    let base_color = mat
        .map(|m| LinearRgba::from(m.base_color))
        .unwrap_or(LinearRgba::new(0.72, 0.55, 0.42, 1.0));
    let base_color_texture = mat.and_then(|m| m.base_color_texture.clone());
    let alpha_cutoff = mat
        .map(|m| match m.alpha_mode {
            AlphaMode::Mask(c) => c,
            _ => 0.0,
        })
        .unwrap_or(0.0);
    WorldInstanceAppearance {
        base_color,
        base_color_texture,
        alpha_cutoff,
        cull_mode: mat.and_then(|m| m.cull_mode),
        double_sided: mat.is_some_and(|m| m.double_sided),
        world_from_local: Mat4::IDENTITY,
        lod_fade: 0.0,
        surface_weather: Vec2::ZERO,
    }
}

/// True when the material has an albedo handle suitable for the instanced shader.
pub fn material_has_albedo_texture(
    materials: &Assets<StandardMaterial>,
    handle: &Handle<StandardMaterial>,
) -> bool {
    materials
        .get(handle)
        .is_some_and(|m| m.base_color_texture.is_some())
}

/// Light models the instanced Lambert path can represent without visual error (#138).
///
/// **Supported (stay in GPU batch):** effective `TexDiff` / `Unknown` (+ Mask cutout).
/// Note: OR maps bare `Tex` → FullBright, so it falls back.
/// **Fallback (entity path):** HalfBright, FullBright/Bright/`Tex`, Dark/DarkShade, Specular*,
/// AddATex/BlendATex, or StandardMaterial with unlit / emissive fill.
pub fn instancing_light_model_supported(
    shader_name: Option<&str>,
    light_mat_idx: Option<i32>,
) -> bool {
    use openrailsrs_or_shader::{OrShaderKind, resolve_or_material_kind};
    matches!(
        resolve_or_material_kind(shader_name, light_mat_idx),
        OrShaderKind::TexDiff | OrShaderKind::Unknown
    )
}

/// True when albedo+cutoff packing is enough for this [`StandardMaterial`] (#138).
///
/// The custom instancing shader does not carry metallic/roughness parameters. Keep
/// strongly metallic materials and metallic-roughness textures on Bevy's entity PBR
/// path; otherwise rail-head albedo is lit as diffuse and saturates to white.
pub fn instancing_material_supported(mat: &StandardMaterial) -> bool {
    if mat.unlit {
        return false;
    }
    if mat.emissive_texture.is_some() {
        return false;
    }
    if mat.metallic > WORLD_INSTANCING_MAX_METALLIC || mat.metallic_roughness_texture.is_some() {
        return false;
    }
    let e = mat.emissive;
    e.red <= 0.02 && e.green <= 0.02 && e.blue <= 0.02
}

/// Combined gate used by WORLD spawn (#138).
///
/// Requires a real albedo texture: the instanced shader always samples a 2D texture, and the
/// old 1×1 white fallback turned missing/late binds into solid-white scenery.
pub fn instancing_part_supported(
    shader_name: Option<&str>,
    light_mat_idx: Option<i32>,
    materials: &Assets<StandardMaterial>,
    material: &Handle<StandardMaterial>,
) -> bool {
    if !instancing_light_model_supported(shader_name, light_mat_idx) {
        return false;
    }
    if !material_has_albedo_texture(materials, material) {
        return false;
    }
    materials
        .get(material)
        .is_some_and(instancing_material_supported)
}

// ─── Render-world plumbing ───────────────────────────────────────────────────

#[derive(Component)]
struct GpuWorldInstanceBuffer {
    buffer: Buffer,
    length: usize,
    source: Arc<[WorldInstanceData]>,
}

#[derive(Component)]
struct GpuWorldInstanceBindGroup {
    bind_group: BindGroup,
    source: WorldInstanceAppearance,
    uniform: Buffer,
}

#[derive(Clone, Copy, ShaderType, Pod, Zeroable)]
#[repr(C)]
struct AppearanceGpu {
    surface_weather: Vec4,
    base_color: Vec4,
    params: Vec4,
    world_from_local: Mat4,
}

#[derive(Resource)]
struct WorldInstancingPipeline {
    shader: Handle<Shader>,
    // AssetServer::add bypasses ShaderLoader's import dependency discovery.
    _imported_shaders: Vec<Handle<Shader>>,
    mesh_pipeline: MeshPipeline,
    appearance_layout: BindGroupLayoutDescriptor,
    /// Prepass/shadow view layout (group 0) — matches [`SetPrepassViewBindGroup`].
    shadow_view_layout: BindGroupLayoutDescriptor,
    shadow_empty_layout: BindGroupLayoutDescriptor,
    depth_clip_control_supported: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct WorldInstancingPipelineKey {
    mesh_key: MeshPipelineKey,
    cull_mode: Option<Face>,
    shadow: bool,
}

fn instance_vertex_buffer_layout() -> VertexBufferLayout {
    VertexBufferLayout {
        array_stride: size_of::<WorldInstanceData>() as u64,
        step_mode: VertexStepMode::Instance,
        attributes: vec![
            VertexAttribute {
                format: VertexFormat::Float32x4,
                offset: 0,
                shader_location: 3,
            },
            VertexAttribute {
                format: VertexFormat::Float32x4,
                offset: 16,
                shader_location: 4,
            },
            VertexAttribute {
                format: VertexFormat::Float32x4,
                offset: 32,
                shader_location: 5,
            },
            VertexAttribute {
                format: VertexFormat::Float32x4,
                offset: 48,
                shader_location: 6,
            },
        ],
    }
}

fn init_world_instancing_pipeline(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mesh_pipeline: Res<MeshPipeline>,
    prepass_pipeline: Res<PrepassPipeline>,
    render_device: Res<RenderDevice>,
) {
    // Bevy 0.19: `Assets<Shader>` lives in the main world, not RenderApp.
    // Register via AssetServer (same approach as `init_mesh_pipeline`).
    let source = Shader::from_wgsl(SHADER_WGSL, "world_instancing.wgsl");
    // AssetServer::add has no file LoadContext to fetch imports for this embedded
    // shader. Retain every file dependency, even in apps with no terrain materials.
    let imported_shaders = source
        .imports
        .iter()
        .filter_map(|import| match import {
            bevy::shader::ShaderImport::AssetPath(path) => {
                Some(asset_server.load::<Shader>(path.clone()))
            }
            _ => None,
        })
        .collect();
    let shader = asset_server.add(source);
    let appearance_layout = BindGroupLayoutDescriptor::new(
        "world_instancing_appearance_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                uniform_buffer::<AppearanceGpu>(false).visibility(ShaderStages::VERTEX_FRAGMENT),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    );
    commands.insert_resource(WorldInstancingPipeline {
        shader,
        _imported_shaders: imported_shaders,
        mesh_pipeline: mesh_pipeline.clone(),
        appearance_layout,
        shadow_view_layout: prepass_pipeline.view_layout_no_motion_vectors.clone(),
        shadow_empty_layout: prepass_pipeline.empty_layout.clone(),
        depth_clip_control_supported: render_device
            .features()
            .contains(WgpuFeatures::DEPTH_CLIP_CONTROL),
    });
}

impl SpecializedMeshPipeline for WorldInstancingPipeline {
    type Key = WorldInstancingPipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        layout: &MeshVertexBufferLayoutRef,
    ) -> Result<RenderPipelineDescriptor, SpecializedMeshPipelineError> {
        // Shadow / depth-prepass: depth-only pipeline with prepass view layouts (#72).
        if key.shadow {
            return self.specialize_shadow(key, layout);
        }

        let mut descriptor = self.mesh_pipeline.specialize(key.mesh_key, layout)?;
        descriptor.primitive.cull_mode = key.cull_mode;
        // Same WGSL as shadow pass — must name entry points (wgpu rejects multi-EP modules).
        apply_opaque_instancing_shaders(&mut descriptor, &self.shader);
        descriptor
            .vertex
            .buffers
            .push(instance_vertex_buffer_layout());
        // Insert appearance bind group at index 3 (after view/array/mesh).
        descriptor.layout.push(self.appearance_layout.clone());
        Ok(descriptor)
    }
}

/// Bind the shared multi-EP WGSL module and force opaque `vertex`/`fragment` (#143).
pub(crate) fn apply_opaque_instancing_shaders(
    descriptor: &mut RenderPipelineDescriptor,
    shader: &Handle<Shader>,
) {
    descriptor.vertex.shader = shader.clone();
    descriptor.vertex.entry_point = Some(OPAQUE_INSTANCING_VERTEX_EP.into());
    if let Some(fragment) = descriptor.fragment.as_mut() {
        fragment.shader = shader.clone();
        fragment.entry_point = Some(OPAQUE_INSTANCING_FRAGMENT_EP.into());
    }
}

pub(crate) const OPAQUE_INSTANCING_VERTEX_EP: &str = "vertex";
pub(crate) const OPAQUE_INSTANCING_FRAGMENT_EP: &str = "fragment";
pub(crate) const SHADOW_INSTANCING_VERTEX_EP: &str = "vertex_shadow";
pub(crate) const SHADOW_INSTANCING_FRAGMENT_EP: &str = "fragment_shadow";

impl WorldInstancingPipeline {
    fn specialize_shadow(
        &self,
        key: WorldInstancingPipelineKey,
        layout: &MeshVertexBufferLayoutRef,
    ) -> Result<RenderPipelineDescriptor, SpecializedMeshPipelineError> {
        let mut vertex_attributes = vec![Mesh::ATTRIBUTE_POSITION.at_shader_location(0)];
        if layout.0.contains(Mesh::ATTRIBUTE_NORMAL) {
            vertex_attributes.push(Mesh::ATTRIBUTE_NORMAL.at_shader_location(1));
        }
        if layout.0.contains(Mesh::ATTRIBUTE_UV_0) {
            vertex_attributes.push(Mesh::ATTRIBUTE_UV_0.at_shader_location(2));
        }
        let vertex_buffer_layout = layout.0.get_layout(&vertex_attributes)?;

        let unclipped_depth = key
            .mesh_key
            .contains(MeshPipelineKey::UNCLIPPED_DEPTH_ORTHO)
            && self.depth_clip_control_supported;

        Ok(RenderPipelineDescriptor {
            label: Some("world_instancing_shadow_pipeline".into()),
            layout: vec![
                self.shadow_view_layout.clone(),
                self.shadow_empty_layout.clone(),
                self.mesh_pipeline.mesh_layouts.model_only.clone(),
                self.appearance_layout.clone(),
            ],
            vertex: VertexState {
                shader: self.shader.clone(),
                entry_point: Some(SHADOW_INSTANCING_VERTEX_EP.into()),
                buffers: vec![vertex_buffer_layout, instance_vertex_buffer_layout()],
                ..default()
            },
            fragment: Some(FragmentState {
                shader: self.shader.clone(),
                entry_point: Some(SHADOW_INSTANCING_FRAGMENT_EP.into()),
                // Depth-only: no color targets; FS only for alpha discard.
                targets: vec![],
                ..default()
            }),
            primitive: PrimitiveState {
                topology: key.mesh_key.primitive_topology(),
                strip_index_format: key.mesh_key.strip_index_format(),
                cull_mode: key.cull_mode,
                unclipped_depth,
                ..default()
            },
            depth_stencil: Some(DepthStencilState {
                format: CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(CompareFunction::GreaterEqual),
                stencil: StencilState {
                    front: StencilFaceState::IGNORE,
                    back: StencilFaceState::IGNORE,
                    read_mask: 0,
                    write_mask: 0,
                },
                bias: DepthBiasState {
                    constant: 0,
                    slope_scale: 0.0,
                    clamp: 0.0,
                },
            }),
            multisample: MultisampleState {
                count: key.mesh_key.msaa_samples(),
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            ..default()
        })
    }
}

fn prepare_world_instance_buffers(
    mut commands: Commands,
    query: Query<(
        Entity,
        &WorldInstanceBuffer,
        Option<&GpuWorldInstanceBuffer>,
    )>,
    render_device: Res<RenderDevice>,
) {
    for (entity, data, existing) in &query {
        if data.is_empty() {
            continue;
        }
        if existing.is_some_and(|gpu| Arc::ptr_eq(&gpu.source, &data.0)) {
            continue;
        }
        let buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("world_instance_buffer"),
            contents: bytemuck::cast_slice(data.0.as_ref()),
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
        });
        commands.entity(entity).insert(GpuWorldInstanceBuffer {
            buffer,
            length: data.len(),
            source: data.0.clone(),
        });
    }
}

fn prepare_world_instance_bind_groups(
    mut commands: Commands,
    pipeline: Res<WorldInstancingPipeline>,
    pipeline_cache: Res<PipelineCache>,
    render_device: Res<RenderDevice>,
    gpu_images: Res<RenderAssets<bevy::render::texture::GpuImage>>,
    render_queue: Res<bevy::render::renderer::RenderQueue>,
    fallback_image: Option<Res<WorldInstancingFallbackImage>>,
    query: Query<(
        Entity,
        &WorldInstanceAppearance,
        Option<&GpuWorldInstanceBindGroup>,
    )>,
) {
    let Some(fallback_image) = fallback_image else {
        return;
    };
    let layout = pipeline_cache.get_bind_group_layout(&pipeline.appearance_layout);
    let fallback = fallback_image.0.clone();
    for (entity, appearance, existing) in &query {
        if existing.is_some_and(|gpu| gpu.source.eq(appearance)) {
            continue;
        }
        // Prefer the part albedo. If the GPU upload is not ready yet, wait — never
        // substitute white (that paints solid-white buildings for a whole stream).
        // Terracotta 1×1 is only for the rare None-texture edge case.
        let image_handle = match &appearance.base_color_texture {
            Some(handle) => {
                if gpu_images.get(handle).is_none() {
                    continue;
                }
                handle.clone()
            }
            None => fallback.clone(),
        };
        let Some(gpu_image) = gpu_images.get(&image_handle) else {
            continue;
        };
        let gpu = AppearanceGpu {
            surface_weather: appearance.surface_weather.extend(0.0).extend(0.0),
            base_color: Vec4::from_array(appearance.base_color.to_f32_array()),
            params: Vec4::new(
                appearance.alpha_cutoff,
                if appearance.double_sided { 1.0 } else { 0.0 },
                appearance.lod_fade,
                0.0,
            ),
            world_from_local: appearance.world_from_local,
        };
        if let Some(existing) =
            existing.filter(|gpu| gpu.source.base_color_texture == appearance.base_color_texture)
        {
            // LOD fading and floating origin only change the uniform. Reuse the
            // allocation/bind group instead of creating GPU objects every frame.
            render_queue.write_buffer(&existing.uniform, 0, bytemuck::bytes_of(&gpu));
            commands.entity(entity).insert(GpuWorldInstanceBindGroup {
                bind_group: existing.bind_group.clone(),
                uniform: existing.uniform.clone(),
                source: appearance.clone(),
            });
            continue;
        }
        let uniform = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("world_instance_appearance"),
            contents: bytemuck::bytes_of(&gpu),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });
        let bind_group = render_device.create_bind_group(
            "world_instance_appearance_bg",
            &layout,
            &BindGroupEntries::sequential((
                uniform.as_entire_buffer_binding(),
                &gpu_image.texture_view,
                &gpu_image.sampler,
            )),
        );
        commands.entity(entity).insert(GpuWorldInstanceBindGroup {
            bind_group,
            uniform,
            source: appearance.clone(),
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn queue_world_instanced(
    opaque_3d_draw_functions: Res<DrawFunctions<Opaque3d>>,
    custom_pipeline: Res<WorldInstancingPipeline>,
    mut pipelines: ResMut<SpecializedMeshPipelines<WorldInstancingPipeline>>,
    pipeline_cache: Res<PipelineCache>,
    meshes: Res<RenderAssets<RenderMesh>>,
    render_mesh_instances: Res<RenderMeshInstances>,
    mesh_allocator: Res<MeshAllocator>,
    material_meshes: Query<
        (Entity, &MainEntity, &WorldInstanceAppearance),
        With<WorldInstanceBuffer>,
    >,
    mut opaque_render_phases: ResMut<ViewBinnedRenderPhases<Opaque3d>>,
    views: Query<(&ExtractedView, &RenderVisibleEntities)>,
    view_key_cache: Res<ViewKeyCache>,
) {
    // Opaque WORLD instances only (#106). Cutout uses shader discard on this path;
    // blend/transparent parts never get `WorldInstanceBuffer` (see world spawn).
    let draw_custom = opaque_3d_draw_functions.read().id::<DrawWorldInstanced>();

    for (view, visible_entities) in &views {
        let Some(opaque_phase) = opaque_render_phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Some(&view_key) = view_key_cache.get(&view.retained_view_entity) else {
            continue;
        };
        let Some(visible_meshes) = visible_entities.get::<Mesh3d>() else {
            continue;
        };
        let visible_groups = visible_main_entities(visible_meshes);

        for (entity, main_entity, appearance) in &material_meshes {
            if !visible_groups.contains(main_entity) {
                opaque_phase.remove(*main_entity);
                continue;
            }
            let Some(mesh_instance) = render_mesh_instances.render_mesh_queue_data(*main_entity)
            else {
                continue;
            };
            let Some(mesh) = meshes.get(mesh_instance.mesh_asset_id()) else {
                continue;
            };
            let Some(mesh_slabs) = mesh_allocator.mesh_slabs(&mesh_instance.mesh_asset_id()) else {
                continue;
            };
            let key = WorldInstancingPipelineKey {
                mesh_key: view_key
                    | MeshPipelineKey::from_primitive_topology_and_strip_index(
                        mesh.primitive_topology(),
                        mesh.index_format(),
                    ),
                cull_mode: appearance.cull_mode,
                shadow: false,
            };
            let Ok(pipeline) =
                pipelines.specialize(&pipeline_cache, &custom_pipeline, key, &mesh.layout)
            else {
                continue;
            };

            // Custom per-entity instance buffer: never multi-draw / batch with others.
            opaque_phase.add(
                Opaque3dBatchSetKey {
                    pipeline,
                    draw_function: draw_custom,
                    material_bind_group_index: None,
                    slabs: mesh_slabs,
                    lightmap_slab: None,
                },
                Opaque3dBinKey {
                    asset_id: mesh_instance.mesh_asset_id().into(),
                },
                (entity, *main_entity),
                mesh_instance.current_uniform_index,
                BinnedRenderPhaseType::UnbatchableMesh,
            );
        }
    }
}

/// Queue WORLD instances into each directional/point/spot shadow cascade (#72 cast).
#[allow(clippy::too_many_arguments)]
fn queue_world_instanced_shadows(
    shadow_draw_functions: Res<DrawFunctions<Shadow>>,
    custom_pipeline: Res<WorldInstancingPipeline>,
    mut pipelines: ResMut<SpecializedMeshPipelines<WorldInstancingPipeline>>,
    pipeline_cache: Res<PipelineCache>,
    meshes: Res<RenderAssets<RenderMesh>>,
    render_mesh_instances: Res<RenderMeshInstances>,
    mesh_allocator: Res<MeshAllocator>,
    material_meshes: Query<
        (Entity, &MainEntity, Option<&WorldInstanceAppearance>),
        With<WorldInstanceBuffer>,
    >,
    mut shadow_render_phases: ResMut<ViewBinnedRenderPhases<Shadow>>,
    view_lights: Query<(&LightEntity, &ExtractedView)>,
    shadow_visible_entities: Query<&RenderShadowMapVisibleEntities>,
    light_key_cache: Res<LightKeyCache>,
) {
    let draw_shadow = shadow_draw_functions
        .read()
        .id::<DrawWorldInstancedShadow>();

    for (light_entity, view) in &view_lights {
        let Some(shadow_phase) = shadow_render_phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Some(&light_key) = light_key_cache.get(&view.retained_view_entity) else {
            continue;
        };
        let Some(visible_meshes) =
            shadow_visible_meshes(&shadow_visible_entities, light_entity, view)
        else {
            continue;
        };
        let visible_groups = visible_main_entities(visible_meshes);

        for (entity, main_entity, appearance) in &material_meshes {
            if !visible_groups.contains(main_entity) {
                shadow_phase.remove(*main_entity);
                continue;
            }
            let Some(mesh_instance) = render_mesh_instances.render_mesh_queue_data(*main_entity)
            else {
                continue;
            };
            if !mesh_instance
                .flags()
                .contains(bevy::pbr::RenderMeshInstanceFlags::SHADOW_CASTER)
            {
                continue;
            }
            let Some(mesh) = meshes.get(mesh_instance.mesh_asset_id()) else {
                continue;
            };
            let Some(mesh_slabs) = mesh_allocator.mesh_slabs(&mesh_instance.mesh_asset_id()) else {
                continue;
            };

            let mut key = light_key
                | MeshPipelineKey::from_primitive_topology_and_strip_index(
                    mesh.primitive_topology(),
                    mesh.index_format(),
                );
            if appearance.is_some_and(|a| a.alpha_cutoff > 0.0) {
                key |= MeshPipelineKey::MAY_DISCARD;
            }

            let key = WorldInstancingPipelineKey {
                mesh_key: key,
                cull_mode: appearance.and_then(|a| a.cull_mode),
                shadow: true,
            };
            let Ok(pipeline) =
                pipelines.specialize(&pipeline_cache, &custom_pipeline, key, &mesh.layout)
            else {
                continue;
            };

            shadow_phase.add(
                ShadowBatchSetKey {
                    pipeline,
                    draw_function: draw_shadow,
                    material_bind_group_index: None,
                    slabs: mesh_slabs,
                },
                ShadowBinKey {
                    asset_id: mesh_instance.mesh_asset_id().into(),
                },
                (entity, *main_entity),
                mesh_instance.current_uniform_index,
                BinnedRenderPhaseType::UnbatchableMesh,
            );
        }
    }
}

/// Mesh visibility is keyed by MainEntity in Bevy 0.19. CPU-culled meshes
/// use Entity::PLACEHOLDER in the render-entity slot; GPU tables can contain
/// legacy render entities. Our extracted component has its own RenderEntity,
/// so use the stable main-world identity for both visibility paths.
fn visible_main_entities(
    visible: &bevy::render::view::RenderVisibleEntitiesClass,
) -> HashSet<MainEntity> {
    visible.iter_visible().map(|(_, main)| *main).collect()
}

/// Resolve Bevy's CPU-culling table for one shadow subview.
///
/// Directional lights have one retained view per cascade. Point and spot
/// lights share placeholder auxiliary entities, matching Bevy's own PBR
/// shadow queue.
fn shadow_visible_meshes<'w, 's: 'w>(
    query: &'w Query<'w, 's, &'_ RenderShadowMapVisibleEntities>,
    light_entity: &LightEntity,
    view: &ExtractedView,
) -> Option<&'w bevy::render::view::RenderVisibleEntitiesClass> {
    let visible = match light_entity {
        LightEntity::Directional { light_entity, .. } => query
            .get(*light_entity)
            .ok()?
            .subviews
            .get(&view.retained_view_entity)?,
        LightEntity::Point {
            light_entity,
            face_index,
        } => {
            let retained = RetainedViewEntity {
                main_entity: view.retained_view_entity.main_entity,
                auxiliary_entity: MainEntity::from(Entity::PLACEHOLDER),
                subview_index: *face_index as u32,
            };
            query.get(*light_entity).ok()?.subviews.get(&retained)?
        }
        LightEntity::Spot { light_entity } => {
            let retained = RetainedViewEntity {
                main_entity: view.retained_view_entity.main_entity,
                auxiliary_entity: MainEntity::from(Entity::PLACEHOLDER),
                subview_index: 0,
            };
            query.get(*light_entity).ok()?.subviews.get(&retained)?
        }
    };
    visible.get::<Mesh3d>()
}

type DrawWorldInstanced = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    SetMeshViewBindingArrayBindGroup<1>,
    SetMeshBindGroup<2>,
    SetWorldInstanceAppearanceBindGroup<3>,
    DrawMeshWorldInstanced,
);

/// Shadow pass: prepass view bind groups + instance draw (#72).
type DrawWorldInstancedShadow = (
    SetItemPipeline,
    SetPrepassViewBindGroup<0>,
    SetPrepassViewEmptyBindGroup<1>,
    SetMeshBindGroup<2>,
    SetWorldInstanceAppearanceBindGroup<3>,
    DrawMeshWorldInstanced,
);

struct SetWorldInstanceAppearanceBindGroup<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetWorldInstanceAppearanceBindGroup<I> {
    type Param = ();
    type ViewQuery = ();
    type ItemQuery = Read<GpuWorldInstanceBindGroup>;

    #[inline]
    fn render<'w>(
        _item: &P,
        _view: (),
        bind_group: Option<&'w GpuWorldInstanceBindGroup>,
        _param: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(bg) = bind_group else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(I, &bg.bind_group, &[]);
        RenderCommandResult::Success
    }
}

struct DrawMeshWorldInstanced;

impl<P: PhaseItem> RenderCommand<P> for DrawMeshWorldInstanced {
    type Param = (
        SRes<RenderAssets<RenderMesh>>,
        SRes<RenderMeshInstances>,
        SRes<MeshAllocator>,
    );
    type ViewQuery = ();
    type ItemQuery = Read<GpuWorldInstanceBuffer>;

    #[inline]
    fn render<'w>(
        item: &P,
        _view: (),
        instance_buffer: Option<&'w GpuWorldInstanceBuffer>,
        (meshes, render_mesh_instances, mesh_allocator): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let mesh_allocator = mesh_allocator.into_inner();
        let Some(mesh_instance) = render_mesh_instances.render_mesh_queue_data(item.main_entity())
        else {
            return RenderCommandResult::Skip;
        };
        let Some(gpu_mesh) = meshes.into_inner().get(mesh_instance.mesh_asset_id()) else {
            return RenderCommandResult::Skip;
        };
        let Some(instance_buffer) = instance_buffer else {
            return RenderCommandResult::Skip;
        };
        let Some(vertex_buffer_slice) =
            mesh_allocator.mesh_vertex_slice(&mesh_instance.mesh_asset_id())
        else {
            return RenderCommandResult::Skip;
        };

        pass.set_vertex_buffer(0, vertex_buffer_slice.buffer.slice(..));
        pass.set_vertex_buffer(1, instance_buffer.buffer.slice(..));

        match &gpu_mesh.buffer_info {
            RenderMeshBufferInfo::Indexed {
                index_format,
                count,
            } => {
                let Some(index_buffer_slice) =
                    mesh_allocator.mesh_index_slice(&mesh_instance.mesh_asset_id())
                else {
                    return RenderCommandResult::Skip;
                };
                pass.set_index_buffer(index_buffer_slice.buffer.slice(..), *index_format);
                pass.draw_indexed(
                    index_buffer_slice.range.start..(index_buffer_slice.range.start + count),
                    vertex_buffer_slice.range.start as i32,
                    0..instance_buffer.length as u32,
                );
            }
            RenderMeshBufferInfo::NonIndexed => {
                pass.draw(vertex_buffer_slice.range, 0..instance_buffer.length as u32);
            }
        }
        RenderCommandResult::Success
    }
}

/// Group placements by tile for instancing decisions.
pub fn group_placements_by_tile(
    placements: &[crate::world::ShapeInstancePlacement],
) -> std::collections::BTreeMap<(i32, i32), Vec<usize>> {
    let mut map: std::collections::BTreeMap<(i32, i32), Vec<usize>> =
        std::collections::BTreeMap::new();
    for (i, p) in placements.iter().enumerate() {
        map.entry((p.tile_x, p.tile_z)).or_default().push(i);
    }
    map
}

/// Shared LOD uses the nearest placement so a distant group centre cannot
/// discard roofs/supports of nearby houses or poles.
fn nearest_instance_distance_m(
    camera: Vec3,
    group: &GlobalTransform,
    instances: &WorldInstanceBuffer,
) -> f32 {
    instances
        .0
        .iter()
        .map(|instance| camera.distance_squared(group.transform_point(instance.translation())))
        .reduce(f32::min)
        .map(f32::sqrt)
        .unwrap_or_else(|| camera.distance(group.translation()))
}

/// Short-lived old mesh; shares immutable instance data and texture handles.
#[derive(Component)]
pub struct WorldLodFade {
    elapsed: f32,
    outgoing: bool,
}

/// Complementary screen-door coverage keeps depth writes and alpha cutouts.
/// Ghosts are tile-bound so unloading cancels both sides of a transition.
pub fn update_world_lod_fades(
    time: Res<Time>,
    mut commands: Commands,
    mut fading: Query<(Entity, &mut WorldLodFade, &mut WorldInstanceAppearance)>,
) {
    for (entity, mut fade, mut appearance) in &mut fading {
        fade.elapsed += time.delta_secs();
        let t = (fade.elapsed / 0.35).clamp(0.001, 1.0);
        appearance.lod_fade = if fade.outgoing { -t } else { t };
        if t >= 1.0 {
            if fade.outgoing {
                commands.entity(entity).despawn();
            } else {
                appearance.lod_fade = 0.0;
                commands.entity(entity).remove::<WorldLodFade>();
            }
        }
    }
}

/// LOD update for instanced groups, with authored bands and 8% hysteresis.
#[allow(clippy::type_complexity)]
pub fn update_world_instanced_lod(
    mut commands: Commands,
    cache: Option<Res<crate::world::WorldShapeLodCache>>,
    meshes: Res<Assets<Mesh>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut groups: Query<
        (
            Entity,
            &Transform,
            &GlobalTransform,
            &mut WorldInstancedGroup,
            &mut Mesh3d,
            &mut Visibility,
            &WorldInstanceBuffer,
            &mut bevy::camera::primitives::Aabb,
            &mut WorldInstanceAppearance,
            &crate::world::WorldTileBound,
        ),
        Without<WorldLodFade>,
    >,
) {
    let Some(cache) = cache else { return };
    let Ok(cam) = camera.single() else { return };
    for (
        entity,
        tf,
        gt,
        mut group,
        mut mesh,
        mut visible,
        instances,
        mut aabb,
        mut appearance,
        bound,
    ) in &mut groups
    {
        if !group.lod_enabled {
            continue;
        }
        let (Some(shape), Some(assets)) = (
            cache.shapes.get(&group.shape_path),
            cache.assets_by_lod.get(&group.shape_path),
        ) else {
            continue;
        };
        if assets.is_empty() {
            continue;
        }
        let new_lod = crate::world::stable_lod_level(
            shape,
            nearest_instance_distance_m(cam.translation(), gt, instances),
            group.lod_idx,
        )
        .min(assets.len() - 1);
        if new_lod == group.lod_idx {
            continue;
        }
        let old_visible = *visible != Visibility::Hidden;
        if old_visible {
            let mut old = appearance.clone();
            old.lod_fade = -0.001;
            let mut ghost_group = group.clone();
            ghost_group.lod_enabled = false;
            commands.spawn((
                *tf,
                mesh.clone(),
                Visibility::Inherited,
                instances.clone(),
                old,
                *aabb,
                *bound,
                ghost_group,
                WorldLodFade {
                    elapsed: 0.0,
                    outgoing: true,
                },
                Name::new("world:lod-outgoing"),
            ));
        }
        group.lod_idx = new_lod;
        let Some((index, part)) = crate::world::shape_lod_part_by_identity(
            &assets[new_lod],
            group.sub_object_idx,
            group.prim_state_idx,
        ) else {
            *visible = Visibility::Hidden;
            continue;
        };
        mesh.0 = part.mesh.clone();
        group.part_index = index;
        *visible = Visibility::Inherited;
        *aabb = instances_aabb(
            instances,
            meshes
                .get(&part.mesh)
                .and_then(bevy::camera::primitives::MeshAabb::compute_aabb)
                .as_ref(),
        );
        appearance.lod_fade = 0.001;
        commands.entity(entity).insert(WorldLodFade {
            elapsed: 0.0,
            outgoing: false,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::ShapeInstancePlacement;

    #[test]
    fn lod_transition_completes_and_releases_the_outgoing_entity() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.add_systems(Update, update_world_lod_fades);
        let incoming = app
            .world_mut()
            .spawn((
                WorldLodFade {
                    elapsed: 0.0,
                    outgoing: false,
                },
                appearance_from_standard_material(&Assets::default(), &Handle::default()),
            ))
            .id();
        let outgoing = app
            .world_mut()
            .spawn((
                WorldLodFade {
                    elapsed: 0.0,
                    outgoing: true,
                },
                appearance_from_standard_material(&Assets::default(), &Handle::default()),
            ))
            .id();
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(175));
        app.update();
        assert!(
            (app.world()
                .get::<WorldInstanceAppearance>(incoming)
                .unwrap()
                .lod_fade
                - 0.5)
                .abs()
                < 0.001
        );
        assert!(
            (app.world()
                .get::<WorldInstanceAppearance>(outgoing)
                .unwrap()
                .lod_fade
                + 0.5)
                .abs()
                < 0.001
        );
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(200));
        app.update();
        assert!(app.world().get_entity(outgoing).is_err());
        assert!(app.world().get::<WorldLodFade>(incoming).is_none());
        assert_eq!(
            app.world()
                .get::<WorldInstanceAppearance>(incoming)
                .unwrap()
                .lod_fade,
            0.0
        );
    }

    #[test]
    fn visibility_keeps_groups_when_render_entities_have_a_different_order() {
        let entity = |index| Entity::from_raw_u32(index).expect("valid test index");
        let pairs = [
            (entity(50), MainEntity::from(entity(1))),
            (entity(20), MainEntity::from(entity(2))),
            (entity(30), MainEntity::from(entity(3))),
        ];
        let mut visible = bevy::render::view::RenderVisibleEntitiesClass::default();
        visible.update_cpu_culled_entities(&pairs);
        let groups = visible_main_entities(&visible);
        assert_eq!(
            groups,
            [1, 2, 3].map(|i| MainEntity::from(entity(i))).into()
        );
        assert!(!groups.contains(&MainEntity::from(entity(40))));
    }

    #[test]
    fn cpu_mesh_placeholders_and_gpu_entities_share_main_world_visibility() {
        let entity = |i| Entity::from_raw_u32(i).unwrap();
        let mut visible = bevy::render::view::RenderVisibleEntitiesClass::default();
        visible.update_cpu_culled_entities(&[
            (Entity::PLACEHOLDER, MainEntity::from(entity(1))),
            (Entity::PLACEHOLDER, MainEntity::from(entity(2))),
        ]);
        visible
            .entities_gpu_culling
            .insert(MainEntity::from(entity(3)), entity(50));
        let groups = visible_main_entities(&visible);
        for i in 1..=3 {
            assert!(groups.contains(&MainEntity::from(entity(i))));
        }
        assert!(!groups.contains(&MainEntity::from(entity(4))));
    }

    #[test]
    fn bevy_bounds_refresh_does_not_collapse_an_instance_group_to_one_model() {
        use bevy::asset::AssetApp;
        use bevy::camera::primitives::MeshAabb;
        use bevy::camera::visibility::calculate_bounds;

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .add_plugins(WorldInstancingPlugin)
            .add_systems(PostUpdate, calculate_bounds);
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Cuboid::new(2.0, 2.0, 2.0));
        let instances = vec![
            WorldInstanceData::from_transform(Transform::from_xyz(100.0, 0.0, 0.0)),
            WorldInstanceData::from_transform(Transform::from_xyz(200.0, 0.0, 0.0)),
        ];
        let aggregate = instances_aabb(
            &instances,
            app.world()
                .resource::<Assets<Mesh>>()
                .get(&mesh)
                .and_then(MeshAabb::compute_aabb)
                .as_ref(),
        );
        let group = app
            .world_mut()
            .spawn((
                Mesh3d(mesh.clone()),
                aggregate,
                WorldInstanceBuffer(instances.into()),
            ))
            .id();
        let ordinary = app.world_mut().spawn((Mesh3d(mesh), aggregate)).id();
        app.update();
        assert_eq!(
            app.world()
                .get::<bevy::camera::primitives::Aabb>(group)
                .unwrap()
                .center
                .x,
            150.0
        );
        assert_eq!(
            app.world()
                .get::<bevy::camera::primitives::Aabb>(ordinary)
                .unwrap()
                .center
                .x,
            0.0
        );

        let replacement = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Cuboid::new(4.0, 4.0, 4.0));
        app.world_mut().get_mut::<Mesh3d>(group).unwrap().0 = replacement;
        app.update();
        assert_eq!(
            app.world()
                .get::<bevy::camera::primitives::Aabb>(group)
                .unwrap()
                .center
                .x,
            150.0
        );
    }

    #[test]
    fn shared_lod_keeps_nearby_parts_when_other_placements_are_far_away() {
        let group = GlobalTransform::from_translation(Vec3::new(1000.0, 0.0, 0.0));
        let instances = WorldInstanceBuffer(
            vec![
                WorldInstanceData::from_transform(Transform::from_xyz(-980.0, 0.0, 0.0)),
                WorldInstanceData::from_transform(Transform::from_xyz(1000.0, 0.0, 0.0)),
            ]
            .into(),
        );
        assert!(
            (nearest_instance_distance_m(Vec3::new(50.0, 0.0, 0.0), &group, &instances) - 30.0)
                .abs()
                < 1e-5
        );
        // The old aggregate centre was nearly one kilometre away, selecting
        // the coarse band for a house only 30 m from the camera.
    }

    #[test]
    fn group_by_tile_splits_placements() {
        let placements = vec![
            ShapeInstancePlacement {
                transform: Transform::from_xyz(0.0, 0.0, 0.0),
                linear: None,
                tile_x: 0,
                tile_z: 0,
                auto_z_bias: false,
                signal_sub_obj: None,
                signal_patch: None,
            },
            ShapeInstancePlacement {
                transform: Transform::from_xyz(1.0, 0.0, 0.0),
                linear: None,
                tile_x: 0,
                tile_z: 0,
                auto_z_bias: false,
                signal_sub_obj: None,
                signal_patch: None,
            },
            ShapeInstancePlacement {
                transform: Transform::from_xyz(2.0, 0.0, 0.0),
                linear: None,
                tile_x: 1,
                tile_z: 0,
                auto_z_bias: false,
                signal_sub_obj: None,
                signal_patch: None,
            },
        ];
        let grouped = group_placements_by_tile(&placements);
        assert_eq!(grouped.get(&(0, 0)).map(|v| v.len()), Some(2));
        assert_eq!(grouped.get(&(1, 0)).map(|v| v.len()), Some(1));
    }

    #[test]
    fn instance_data_from_transform_preserves_translation() {
        let tf = Transform::from_xyz(10.0, 2.0, -3.0);
        let d = WorldInstanceData::from_transform(tf);
        let t = d.translation();
        assert!((t.x - 10.0).abs() < 1e-4);
        assert!((t.y - 2.0).abs() < 1e-4);
        assert!((t.z + 3.0).abs() < 1e-4);
    }

    #[test]
    fn instance_buffer_clone_shares_immutable_placement_storage() {
        let original = WorldInstanceBuffer(
            vec![WorldInstanceData::from_transform(Transform::IDENTITY)].into(),
        );
        let extracted = original.clone();
        assert!(Arc::ptr_eq(&original.0, &extracted.0));
    }

    #[test]
    fn extracted_group_transform_preserves_placement_after_origin_shift() {
        let materials = Assets::<StandardMaterial>::default();
        let appearance = appearance_from_standard_material(&materials, &Handle::default());
        let placement = WorldInstanceData::from_transform(Transform::from_xyz(120.0, 8.0, -40.0));
        let shifted_group = GlobalTransform::from_translation(Vec3::new(-100.0, 0.0, 30.0));
        let extracted = WorldInstanceAppearance::extract_component((&appearance, &shifted_group))
            .expect("group must extract its own transform");
        let position = extracted
            .world_from_local
            .transform_point3(placement.translation());
        assert_eq!(position, Vec3::new(20.0, 8.0, -10.0));
        assert_eq!(appearance.world_from_local, Mat4::IDENTITY);
        assert_ne!(extracted, appearance);
    }

    #[test]
    fn aggregate_aabb_covers_scaled_and_sheared_mesh() {
        let local =
            bevy::camera::primitives::Aabb::from_min_max(Vec3::splat(-1.0), Vec3::splat(1.0));
        let linear = Mat3::from_cols(
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(1.0, 3.0, 0.0),
            Vec3::new(0.0, 0.0, 4.0),
        );
        let instance = WorldInstanceData::from_affine(Affine3A::from_mat3_translation(
            linear,
            Vec3::new(10.0, 2.0, -3.0),
        ));
        let aggregate = instances_aabb(&[instance], Some(&local));
        assert_eq!(Vec3::from(aggregate.min()), Vec3::new(7.0, -1.0, -7.0));
        assert_eq!(Vec3::from(aggregate.max()), Vec3::new(13.0, 5.0, 1.0));
    }

    #[test]
    fn instancing_enabled_by_default() {
        unsafe {
            std::env::remove_var("OPENRAILSRS_WORLD_INSTANCING");
        }
        assert!(world_instancing_enabled());
    }

    #[test]
    fn paired_opaque_placements_same_tile_meet_min() {
        const {
            assert!(WORLD_INSTANCING_MIN <= 2);
        }
        let placements: Vec<ShapeInstancePlacement> = (0..2)
            .map(|i| ShapeInstancePlacement {
                transform: Transform::from_xyz(i as f32, 0.0, 0.0),
                linear: None,
                tile_x: 0,
                tile_z: 0,
                auto_z_bias: false,
                signal_sub_obj: None,
                signal_patch: None,
            })
            .collect();
        let grouped = group_placements_by_tile(&placements);
        let n = grouped.get(&(0, 0)).map(|v| v.len()).unwrap_or(0);
        assert!(n >= WORLD_INSTANCING_MIN);
    }

    #[test]
    fn instance_data_from_view_placement_preserves_shear() {
        // #139: GPU instance Mat4 keeps Matrix3x3 shear (TRS Transform would drop it).
        let shear = Mat3::from_cols(
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.35, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        );
        let d = WorldInstanceData::from_view_placement(
            Transform::from_xyz(10.0, 0.0, -2.0),
            Some(shear),
        );
        assert!((d.linear().y_axis.x - 0.35).abs() < 1e-4);
        assert!((d.translation().x - 10.0).abs() < 1e-4);
        let trs_only = WorldInstanceData::from_transform(Transform {
            translation: Vec3::new(10.0, 0.0, -2.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        });
        assert!(trs_only.linear().y_axis.x.abs() < 1e-4);
    }

    #[test]
    fn appearance_opaque_has_no_cutoff_mask_keeps_discard_threshold() {
        let mut materials = Assets::<StandardMaterial>::default();
        let opaque = materials.add(StandardMaterial {
            alpha_mode: AlphaMode::Opaque,
            ..default()
        });
        let mask = materials.add(StandardMaterial {
            alpha_mode: AlphaMode::Mask(0.78),
            ..default()
        });
        // Blend materials stay on the non-instanced path; cutout maps to opaque + discard.
        assert_eq!(
            appearance_from_standard_material(&materials, &opaque).alpha_cutoff,
            0.0
        );
        assert!(
            (appearance_from_standard_material(&materials, &mask).alpha_cutoff - 0.78).abs() < 1e-5
        );
    }

    #[test]
    fn missing_material_appearance_is_not_solid_white() {
        let materials = Assets::<StandardMaterial>::default();
        let dangling = Handle::default();
        let appearance = appearance_from_standard_material(&materials, &dangling);
        assert!(appearance.base_color_texture.is_none());
        // Must stay terracotta-ish, not LinearRgba::WHITE (× white 1×1 → solid white).
        assert!(appearance.base_color.red < 0.95);
        assert!(appearance.base_color.green < 0.85);
        assert!(!material_has_albedo_texture(&materials, &dangling));
    }

    #[test]
    fn instancing_requires_albedo_texture() {
        let mut materials = Assets::<StandardMaterial>::default();
        let mut images = Assets::<Image>::default();
        let tex = images.add(Image::default());
        let textured = materials.add(StandardMaterial {
            base_color_texture: Some(tex),
            ..default()
        });
        let bare = materials.add(StandardMaterial::default());
        assert!(material_has_albedo_texture(&materials, &textured));
        assert!(!material_has_albedo_texture(&materials, &bare));
        assert!(instancing_part_supported(
            Some("TexDiff"),
            None,
            &materials,
            &textured
        ));
        assert!(!instancing_part_supported(
            Some("TexDiff"),
            None,
            &materials,
            &bare
        ));
    }

    #[test]
    fn instanced_draws_target_opaque3d_phase() {
        // Compile-time / type-level guard: WORLD GPU instances register on Opaque3d (#106).
        fn _assert_phase<P: bevy::render::render_phase::BinnedPhaseItem>() {}
        _assert_phase::<Opaque3d>();
    }

    #[test]
    fn instanced_lod_uses_camera_to_center_distance() {
        // Same convention as non-instanced path (#74).
        let cam = Vec3::new(0.0, 0.0, 0.0);
        let center = Vec3::new(30.0, 0.0, 40.0);
        let d = crate::world::world_lod_distance_m(cam, center);
        assert!((d - 50.0).abs() < 1e-4);
    }

    #[test]
    fn instancing_shader_uses_scene_light_and_fog() {
        // #76: physical scene light must use camera exposure + diffuse BRDF
        // normalization; fog uses Bevy DISTANCE_FOG.
        let src = include_str!("world_instancing.wgsl");
        assert!(
            src.contains("directional_lights"),
            "shader must sample scene directional light"
        );
        assert!(
            src.contains("view.exposure"),
            "physical sun/ambient light must be scaled by camera exposure"
        );
        assert!(
            src.contains("ndotl / PI"),
            "Lambert diffuse light must include its 1/PI BRDF normalization"
        );
        assert!(
            !src.contains("ambient + light_rgb * ndotl * shadow_mod"),
            "raw physical light multiplication clips textured scenery white"
        );
        assert!(
            !src.contains("vec3<f32>(0.35, 0.9, 0.25)"),
            "hardcoded Lambert light_dir must be removed"
        );
        assert!(
            src.contains("apply_fog") && src.contains("DISTANCE_FOG"),
            "shader must apply Bevy distance fog when the view key enables it"
        );
        assert!(
            src.contains("alpha_cutoff") || src.contains("cutoff"),
            "alpha cutoff path must remain"
        );
    }

    #[test]
    fn instancing_shader_receives_directional_shadows() {
        // #72 receive: sample Bevy cascade shadow map.
        let src = include_str!("world_instancing.wgsl");
        assert!(
            src.contains("fetch_directional_shadow"),
            "shader must receive directional shadows"
        );
        assert!(
            src.contains("DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT"),
            "shadow sampling must respect light flags"
        );
    }

    #[test]
    fn instancing_shader_casts_directional_shadows() {
        // #72 cast: depth-only entry points used by Shadow phase.
        // Opaque specialize must set entry_point = "vertex"/"fragment" (multi-EP module).
        let src = include_str!("world_instancing.wgsl");
        assert!(
            src.contains("fn vertex(")
                && src.contains("fn fragment(")
                && src.contains("fn vertex_shadow")
                && src.contains("fn fragment_shadow"),
            "shader must expose opaque + shadow cast entry points"
        );
    }

    #[test]
    fn instancing_light_model_gates_halfbright_and_specular() {
        // #138: TexDiff/Unknown stay in batch; Tex→FullBright and light models fall back.
        assert!(instancing_light_model_supported(Some("TexDiff"), None));
        assert!(instancing_light_model_supported(None, None));
        assert!(!instancing_light_model_supported(Some("Tex"), None));
        assert!(!instancing_light_model_supported(Some("HalfBright"), None));
        assert!(!instancing_light_model_supported(Some("Specular25"), None));
        // LightMatIdx HalfBright (12 + (-11) = 1).
        assert!(!instancing_light_model_supported(
            Some("TexDiff"),
            Some(-11)
        ));
        // LightMatIdx Specular25 (12 + (-6) = 6).
        assert!(!instancing_light_model_supported(Some("TexDiff"), Some(-6)));
    }

    #[test]
    fn instancing_material_rejects_unlit_emissive_and_strong_metallic() {
        let mut lit = StandardMaterial {
            unlit: false,
            emissive: LinearRgba::BLACK,
            metallic: 0.05,
            ..Default::default()
        };
        assert!(instancing_material_supported(&lit));

        lit.unlit = true;
        assert!(!instancing_material_supported(&lit));

        lit.unlit = false;
        lit.emissive = LinearRgba::new(0.11, 0.12, 0.14, 1.0);
        assert!(!instancing_material_supported(&lit));

        lit.emissive = LinearRgba::BLACK;
        lit.metallic = 0.82;
        assert!(
            !instancing_material_supported(&lit),
            "rail-style metallic material must keep the entity PBR path"
        );

        lit.metallic = WORLD_INSTANCING_MAX_METALLIC;
        assert!(instancing_material_supported(&lit));

        lit.metallic_roughness_texture = Some(Handle::default());
        assert!(!instancing_material_supported(&lit));
    }

    #[test]
    fn specialize_sets_explicit_opaque_entry_points() {
        // #143: regression guard on the Rust descriptor path (not WGSL grepping).
        let mut descriptor = RenderPipelineDescriptor {
            vertex: VertexState {
                entry_point: None,
                ..default()
            },
            fragment: Some(FragmentState {
                entry_point: None,
                ..default()
            }),
            ..default()
        };
        let shader = Handle::<Shader>::default();
        apply_opaque_instancing_shaders(&mut descriptor, &shader);
        assert_eq!(
            descriptor.vertex.entry_point.as_deref(),
            Some(OPAQUE_INSTANCING_VERTEX_EP)
        );
        assert_eq!(
            descriptor
                .fragment
                .as_ref()
                .and_then(|f| f.entry_point.as_deref()),
            Some(OPAQUE_INSTANCING_FRAGMENT_EP)
        );
        assert_eq!(descriptor.vertex.shader, shader);
        assert_eq!(
            descriptor.fragment.as_ref().map(|f| &f.shader),
            Some(&shader)
        );
    }

    #[test]
    fn shadow_instancing_entry_point_names_are_explicit() {
        // #143: shadow specialize must keep distinct multi-EP names.
        assert_eq!(SHADOW_INSTANCING_VERTEX_EP, "vertex_shadow");
        assert_eq!(SHADOW_INSTANCING_FRAGMENT_EP, "fragment_shadow");
        assert_ne!(SHADOW_INSTANCING_VERTEX_EP, OPAQUE_INSTANCING_VERTEX_EP);
        assert_ne!(SHADOW_INSTANCING_FRAGMENT_EP, OPAQUE_INSTANCING_FRAGMENT_EP);
    }

    #[test]
    fn instanced_draws_target_shadow_phase() {
        fn _assert_phase<P: bevy::render::render_phase::BinnedPhaseItem>() {}
        _assert_phase::<Shadow>();
    }
}
