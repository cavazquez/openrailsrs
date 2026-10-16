//! Per-view compaction for large opaque WORLD batches. Shadows use the full
//! original buffer. Unsupported devices and unready pipelines draw normally.
use super::*;
use bevy::render::{
    render_resource::binding_types::{storage_buffer_read_only_sized, storage_buffer_sized},
    renderer::RenderQueue,
};
use std::collections::HashMap;
const COMPACTED_BYTES_BUDGET: u64 = 64 * 1024 * 1024;
fn heavy_enough(instances: usize, vertices: u64, force: bool) -> bool {
    instances >= 16 && (force || (instances as u64).saturating_mul(vertices) >= 1_000_000)
}

pub(super) struct CulledBuffers {
    pub output: Buffer,
    pub indirect: Buffer,
    uniform: Buffer,
    bind_group: BindGroup,
}
#[derive(Resource)]
pub(super) struct CullPipeline {
    layout: BindGroupLayoutDescriptor,
    pipeline: Option<CachedComputePipelineId>,
}
#[derive(Clone, Copy, Pod, Zeroable, ShaderType)]
#[repr(C)]
struct CullParams {
    clip: Mat4,
    center: Vec4,
    half: Vec4,
    counts: UVec4,
}

pub(super) fn init(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "WORLD visibility compaction",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                storage_buffer_read_only_sized(false, None),
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
                uniform_buffer::<CullParams>(false),
            ),
        ),
    );
    let pipeline = if device.limits().max_compute_workgroups_per_dimension > 0
        && device.limits().max_storage_buffers_per_shader_stage >= 3
    {
        let shader = asset_server.add(Shader::from_wgsl(
            include_str!("world_compute_cull.wgsl"),
            "world_compute_cull.wgsl",
        ));
        Some(cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some("WORLD per-instance compaction".into()),
            layout: vec![layout.clone()],
            shader,
            ..default()
        }))
    } else {
        None
    };
    commands.insert_resource(CullPipeline { layout, pipeline });
}

pub(super) fn prepare(
    options: Res<visibility::VisibilityOptions>,
    pipeline: Res<CullPipeline>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    views: Query<(&ExtractedView, &RenderVisibleEntities)>,
    mut groups: Query<(
        &MainEntity,
        &mut GpuWorldInstanceBuffer,
        &WorldInstanceAppearance,
        Option<&visibility::InstanceBounds>,
    )>,
    meshes: Res<RenderAssets<RenderMesh>>,
    instances: Res<RenderMeshInstances>,
    allocator: Res<MeshAllocator>,
    counters: Res<WorldDrawCounters>,
) {
    let ready = pipeline
        .pipeline
        .and_then(|id| cache.get_compute_pipeline(id));
    if !options.gpu || ready.is_none() {
        for (_, mut group, _, _) in &mut groups {
            group.culled.clear();
        }
        return;
    }
    let compute = ready.expect("ready pipeline");
    let live = views
        .iter()
        .take(4)
        .map(|(v, _)| v.retained_view_entity)
        .collect::<HashSet<_>>();
    let mut encoder: Option<CommandEncoder> = None;
    let mut retained = groups
        .iter()
        .map(|(_, group, _, _)| group.culled.values().map(|b| b.output.size()).sum::<u64>())
        .sum::<u64>();
    for (main, mut group, appearance, bounds) in &mut groups {
        group.culled.retain(|v, _| live.contains(v));
        let Some(bounds) = bounds else {
            group.culled.clear();
            continue;
        };
        // Small batches cost less as direct draws. Wind/snow displacement uses
        // the full path until its animated envelope is available.
        if group.length < 16
            || appearance.vegetation.x > 0.
            || appearance.surface_weather.y > 0.
            || group.buffer.size() > device.limits().max_storage_buffer_binding_size
            || !group.buffer.usage().contains(BufferUsages::STORAGE)
        {
            group.culled.clear();
            continue;
        }
        let Some(instance) = instances.render_mesh_queue_data(*main) else {
            continue;
        };
        let Some(mesh) = meshes.get(instance.mesh_asset_id()) else {
            continue;
        };
        let work = match &mesh.buffer_info {
            RenderMeshBufferInfo::Indexed { count, .. } => u64::from(*count),
            RenderMeshBufferInfo::NonIndexed => u64::from(mesh.vertex_count),
        };
        if !heavy_enough(group.length, work, options.force_gpu) {
            group.culled.clear();
            continue;
        }
        let Some(vertices) = allocator.mesh_vertex_slice(&instance.mesh_asset_id()) else {
            continue;
        };
        let args = match &mesh.buffer_info {
            RenderMeshBufferInfo::Indexed { count, .. } => {
                let Some(indices) = allocator.mesh_index_slice(&instance.mesh_asset_id()) else {
                    continue;
                };
                [*count, 0, indices.range.start, vertices.range.start, 0]
            }
            RenderMeshBufferInfo::NonIndexed => [
                vertices.range.end - vertices.range.start,
                0,
                vertices.range.start,
                0,
                0,
            ],
        };
        for (view, visible) in views.iter().take(4) {
            if !visible
                .get::<Mesh3d>()
                .is_some_and(|v| v.iter_visible().any(|(_, id)| id == main))
            {
                group.culled.remove(&view.retained_view_entity);
                continue;
            }
            let clip = view.clip_from_world.unwrap_or_else(|| {
                view.clip_from_view * view.world_from_view.to_matrix().inverse()
            });
            let params = CullParams {
                clip: clip * appearance.world_from_local,
                center: bounds.center,
                half: bounds.half,
                counts: UVec4::new(group.length as u32, view.viewport.z, view.viewport.w, 0),
            };
            if !params.clip.is_finite() {
                group.culled.remove(&view.retained_view_entity);
                continue;
            }
            if !group.culled.contains_key(&view.retained_view_entity) {
                if retained.saturating_add(group.buffer.size()) > COMPACTED_BYTES_BUDGET {
                    continue;
                }
                retained += group.buffer.size();
                let output = device.create_buffer(&BufferDescriptor {
                    label: Some("WORLD compacted placements"),
                    size: group.buffer.size(),
                    usage: BufferUsages::VERTEX | BufferUsages::STORAGE,
                    mapped_at_creation: false,
                });
                let indirect = device.create_buffer_with_data(&BufferInitDescriptor {
                    label: Some("WORLD compacted draw args"),
                    contents: bytemuck::cast_slice(&args),
                    usage: BufferUsages::INDIRECT | BufferUsages::STORAGE | BufferUsages::COPY_DST,
                });
                let uniform = device.create_buffer_with_data(&BufferInitDescriptor {
                    label: Some("WORLD visibility params"),
                    contents: bytemuck::bytes_of(&params),
                    usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                });
                let bind_group = device.create_bind_group(
                    "WORLD compaction",
                    &cache.get_bind_group_layout(&pipeline.layout),
                    &BindGroupEntries::sequential((
                        group.buffer.as_entire_buffer_binding(),
                        output.as_entire_buffer_binding(),
                        indirect.as_entire_buffer_binding(),
                        uniform.as_entire_buffer_binding(),
                    )),
                );
                group.culled.insert(
                    view.retained_view_entity,
                    CulledBuffers {
                        output,
                        indirect,
                        uniform,
                        bind_group,
                    },
                );
            }
            let buffers = &group.culled[&view.retained_view_entity];
            queue.write_buffer(&buffers.indirect, 0, bytemuck::cast_slice(&args));
            queue.write_buffer(&buffers.uniform, 0, bytemuck::bytes_of(&params));
            {
                let encoder = encoder.get_or_insert_with(|| {
                    device.create_command_encoder(&CommandEncoderDescriptor {
                        label: Some("WORLD instance visibility"),
                    })
                });
                let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                    label: Some("WORLD compact"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(compute);
                pass.set_bind_group(0, &buffers.bind_group, &[]);
                pass.dispatch_workgroups((group.length as u32).div_ceil(64), 1, 1);
            }
            counters
                .0
                .compute
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
    if let Some(encoder) = encoder {
        queue.submit([encoder.finish()]);
    }
}

pub(super) fn empty_views() -> HashMap<RetainedViewEntity, CulledBuffers> {
    HashMap::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_compaction_avoids_the_measured_small_batch_overhead() {
        assert!(!heavy_enough(128, 3456, false));
        assert!(heavy_enough(2048, 3456, false));
        assert!(heavy_enough(128, 3456, true));
        assert!(!heavy_enough(2, 1_000_000, true));
        assert!(heavy_enough(usize::MAX, u64::MAX, false));
    }
}
