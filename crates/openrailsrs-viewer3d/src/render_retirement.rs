//! Retire Bevy 0.19 bin-unpacking bindings when their view/phase buffers retire.
//!
//! Bevy prunes `BinUnpackingBuffers::view_phase_buffers`, but its
//! `BinUnpackingBindGroups` cache only inserts/replaces keys. A retired bind
//! group still owns its metadata, instance, work-item, and bin-offset buffers.
//! Use the public source map as the authority; do not reset instance history or
//! discard active shadow/mirror bindings.
use bevy::{
    pbr::{BinUnpackingBindGroups, prepare_preprocess_bind_groups},
    platform::collections::HashMap,
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        batching::gpu_preprocessing::{BinUnpackingBuffers, BinUnpackingBuffersKey},
    },
};

use crate::performance::BinGroupCleanupStats;

pub fn install(app: &mut App) {
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app
        .init_resource::<BinGroupCleanupStats>()
        .add_systems(
            Render,
            retire_bin_unpacking_bind_groups
                // Bevy writes/prunes source buffers in PrepareResourcesWritePhaseBuffers,
                // then builds bindings in PrepareBindGroups. Retire only after that
                // build, before Render dispatches; every current source key survives.
                .in_set(RenderSystems::PrepareBindGroups)
                .after(prepare_preprocess_bind_groups),
        );
}

fn retain_source_keys<V>(
    cached: &mut HashMap<BinUnpackingBuffersKey, V>,
    sources: &BinUnpackingBuffers,
) -> usize {
    let before = cached.len();
    cached.retain(|key, _| sources.view_phase_buffers.contains_key(key));
    before - cached.len()
}

fn retire_bin_unpacking_bind_groups(
    sources: Option<Res<BinUnpackingBuffers>>,
    cached: Option<ResMut<BinUnpackingBindGroups>>,
    mut stats: ResMut<BinGroupCleanupStats>,
) {
    // GPU preprocessing may be unavailable on a CPU/fallback renderer. Do not
    // initialize GPU resources here or interpret their absence as a dead scene.
    let (Some(sources), Some(mut cached)) = (sources, cached) else {
        stats.cached_keys = 0;
        stats.source_keys = 0;
        stats.stale_keys_before = 0;
        stats.stale_keys_after = 0;
        return;
    };
    let retired = retain_source_keys(&mut cached.0, &sources);
    stats.cached_keys = cached.len();
    stats.source_keys = sources.view_phase_buffers.len();
    stats.stale_keys_before = retired;
    stats.stale_keys_after = cached
        .keys()
        .filter(|key| !sources.view_phase_buffers.contains_key(*key))
        .count();
    stats.retired_keys = stats.retired_keys.saturating_add(retired as u64);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::render::view::RetainedViewEntity;
    use std::{any::TypeId, sync::Arc};

    struct OpaquePhase;
    struct MaskPhase;

    fn key<T: 'static>(
        entity: Entity,
        camera: Option<Entity>,
        subview_index: u32,
    ) -> BinUnpackingBuffersKey {
        BinUnpackingBuffersKey {
            phase: TypeId::of::<T>(),
            view: RetainedViewEntity::new(entity.into(), camera.map(Into::into), subview_index),
        }
    }

    #[test]
    fn retiring_views_releases_buffer_owners_and_preserves_live_phases_and_subviews() {
        let mut world = World::new();
        let camera = world.spawn_empty().id();
        let sun = world.spawn_empty().id();
        let mirror = world.spawn_empty().id();
        let stable = [
            key::<OpaquePhase>(camera, None, 0),
            key::<MaskPhase>(camera, None, 0),
            key::<OpaquePhase>(sun, Some(camera), 0),
            key::<OpaquePhase>(sun, Some(camera), 1),
            key::<OpaquePhase>(mirror, None, 0),
        ];
        let mut sources = BinUnpackingBuffers::default();
        let mut cached = HashMap::default();
        let buffers = || {
            let buffers: [Arc<()>; 4] = std::array::from_fn(|_| Arc::new(()));
            let weak = buffers.each_ref().map(Arc::downgrade);
            (buffers, weak)
        };
        let mut stable_buffers = Vec::new();
        for key in stable {
            let (bound, weak) = buffers();
            sources.view_phase_buffers.insert(key, Default::default());
            cached.insert(key, bound);
            stable_buffers.push(weak);
        }

        let mut previous_camera = None;
        let mut previous_buffers: Vec<[std::sync::Weak<()>; 4]> = Vec::new();
        for _ in 0..12 {
            if let Some(previous_camera) = previous_camera.take() {
                world.despawn(previous_camera);
            }
            // Entity generations, shadow auxiliary cameras, phases and subviews
            // all participate in the actual Bevy key. An old scene cannot match
            // a fresh camera simply because an entity index was reused.
            let next_camera = world.spawn_empty().id();
            let active = [
                key::<OpaquePhase>(next_camera, None, 0),
                key::<MaskPhase>(next_camera, None, 0),
                key::<OpaquePhase>(sun, Some(next_camera), 0),
                key::<OpaquePhase>(sun, Some(next_camera), 1),
            ];
            sources
                .view_phase_buffers
                .retain(|key, _| stable.contains(key));
            let mut next_buffers = Vec::new();
            for key in active {
                let (bound, weak) = buffers();
                sources.view_phase_buffers.insert(key, Default::default());
                cached.insert(key, bound);
                next_buffers.push(weak);
            }
            assert!(
                previous_buffers
                    .iter()
                    .flatten()
                    .all(|weak: &std::sync::Weak<()>| weak.upgrade().is_some())
            );
            assert_eq!(
                retain_source_keys(&mut cached, &sources),
                if previous_buffers.is_empty() { 0 } else { 4 }
            );
            assert_eq!(cached.len(), stable.len() + active.len());
            assert!(
                previous_buffers
                    .iter()
                    .flatten()
                    .all(|weak| weak.upgrade().is_none())
            );
            assert!(
                stable_buffers
                    .iter()
                    .chain(&next_buffers)
                    .flatten()
                    .all(|weak| weak.upgrade().is_some())
            );
            assert_eq!(retain_source_keys(&mut cached, &sources), 0);
            previous_camera = Some(next_camera);
            previous_buffers = next_buffers;
        }

        sources.view_phase_buffers.clear();
        assert_eq!(retain_source_keys(&mut cached, &sources), 9);
        assert!(cached.is_empty());
        assert!(
            stable_buffers
                .iter()
                .chain(&previous_buffers)
                .flatten()
                .all(|weak| weak.upgrade().is_none())
        );
    }

    #[test]
    fn renderer_without_gpu_preprocessing_keeps_retirement_optional() {
        let mut app = App::new();
        app.init_resource::<BinGroupCleanupStats>()
            .add_systems(Update, retire_bin_unpacking_bind_groups);
        app.update();
        assert_eq!(
            app.world().resource::<BinGroupCleanupStats>().retired_keys,
            0
        );
        // A menu/empty frame can still have the public resources, with no jobs.
        app.init_resource::<BinUnpackingBuffers>()
            .init_resource::<BinUnpackingBindGroups>();
        app.update();
        assert_eq!(
            app.world().resource::<BinGroupCleanupStats>().cached_keys,
            0
        );
    }
}
