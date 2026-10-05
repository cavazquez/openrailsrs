//! Snow on explicitly opted-in train exteriors, sharing the original textures.
use bevy::{
    asset::AssetId,
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
};
use std::collections::{HashMap, HashSet};

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct SnowSurface {
    #[uniform(100)]
    pub cover: Vec4,
}
impl MaterialExtension for SnowSurface {
    fn fragment_shader() -> ShaderRef {
        "shaders/surface_weather.wgsl".into()
    }
    fn deferred_fragment_shader() -> ShaderRef {
        "shaders/surface_weather.wgsl".into()
    }
}
pub type SnowMaterial = ExtendedMaterial<StandardMaterial, SnowSurface>;

/// Snow is opt-in. Native WORLD shapes (including buildings and their roofs)
/// retain their authored materials in every rendering/LOD path.
#[derive(Component)]
pub struct SnowReceiver;

/// Keep the original material alive for LOD changes and wet-surface updates.
#[derive(Component)]
pub struct SnowSurfaceSource(pub Handle<StandardMaterial>);
#[derive(Resource, Default)]
pub struct SnowMaterials {
    by_source: HashMap<AssetId<StandardMaterial>, Handle<SnowMaterial>>,
    last_cover: f32,
}

pub fn sync(
    mut commands: Commands,
    wet: Res<crate::wet_surfaces::WetSurfaces>,
    originals: Res<Assets<StandardMaterial>>,
    mut snow: ResMut<Assets<SnowMaterial>>,
    mut cache: ResMut<SnowMaterials>,
    mut events: MessageReader<AssetEvent<StandardMaterial>>,
    outdoor: Query<
        (Entity, &MeshMaterial3d<StandardMaterial>),
        (
            With<SnowReceiver>,
            Without<crate::cab_view::CabInteriorMarker>,
        ),
    >,
) {
    let changed: HashSet<_> = events
        .read()
        .filter_map(|event| match event {
            AssetEvent::Added { id } | AssetEvent::Modified { id } => Some(*id),
            _ => None,
        })
        .collect();
    cache.by_source.retain(|id, _| originals.contains(*id));
    let cover_changed = (wet.snow_cover - cache.last_cover).abs() > 1e-4;
    if cover_changed || !changed.is_empty() {
        for (id, handle) in &cache.by_source {
            let Some(base) = originals.get(*id) else {
                continue;
            };
            if changed.contains(id) || cover_changed {
                // Share the original texture handles; never allocate new images.
                if let Some(mut material) = snow.get_mut(handle) {
                    material.base = base.clone();
                    material.extension.cover.x = wet.snow_cover;
                }
            }
        }
        cache.last_cover = wet.snow_cover;
    }
    if wet.snow_cover <= 0.0 {
        return;
    }
    for (entity, source) in &outdoor {
        let Some(base) = originals.get(&source.0) else {
            continue;
        };
        if !matches!(base.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_)) {
            continue;
        }
        let handle = cache
            .by_source
            .entry(source.id())
            .or_insert_with(|| {
                snow.add(SnowMaterial {
                    base: base.clone(),
                    extension: SnowSurface {
                        cover: Vec4::new(wet.snow_cover, 0.0, 0.0, 0.0),
                    },
                })
            })
            .clone();
        commands
            .entity(entity)
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert((MeshMaterial3d(handle), SnowSurfaceSource(source.0.clone())));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snow_keeps_cab_and_transparency_and_updates_shared_original_material() {
        let mut app = App::new();
        app.init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<SnowMaterial>>()
            .init_resource::<SnowMaterials>()
            .init_resource::<crate::wet_surfaces::WetSurfaces>()
            .add_message::<AssetEvent<StandardMaterial>>()
            .add_systems(Update, sync);
        app.world_mut()
            .resource_mut::<crate::wet_surfaces::WetSurfaces>()
            .snow_cover = 1.0;
        let opaque = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let transparent = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                alpha_mode: AlphaMode::Blend,
                ..default()
            });
        let first = app
            .world_mut()
            .spawn((SnowReceiver, MeshMaterial3d(opaque.clone())))
            .id();
        let second = app
            .world_mut()
            .spawn((SnowReceiver, MeshMaterial3d(opaque.clone())))
            .id();
        // Even when a building shares the train's source material, it must keep
        // the original appearance. New streamed WORLD/LOD meshes are opt-out.
        let building = app.world_mut().spawn(MeshMaterial3d(opaque.clone())).id();
        let cab = app
            .world_mut()
            .spawn((
                crate::cab_view::CabInteriorMarker,
                SnowReceiver,
                MeshMaterial3d(opaque.clone()),
            ))
            .id();
        let glass = app
            .world_mut()
            .spawn((SnowReceiver, MeshMaterial3d(transparent)))
            .id();
        app.update();
        let derived = app
            .world()
            .get::<MeshMaterial3d<SnowMaterial>>(first)
            .unwrap()
            .0
            .clone();
        assert_eq!(
            derived,
            app.world()
                .get::<MeshMaterial3d<SnowMaterial>>(second)
                .unwrap()
                .0
        );
        assert_eq!(app.world().resource::<Assets<SnowMaterial>>().len(), 1);
        // An opted-in mesh may replace its source material.
        // Rewrap that source rather than retaining the previous band's texture.
        let replacement = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                alpha_mode: AlphaMode::Mask(0.78),
                ..default()
            });
        app.world_mut()
            .entity_mut(first)
            .insert(MeshMaterial3d(replacement.clone()));
        app.update();
        let new_derived = &app
            .world()
            .get::<MeshMaterial3d<SnowMaterial>>(first)
            .unwrap()
            .0;
        assert_ne!(*new_derived, derived);
        assert_eq!(
            app.world()
                .resource::<Assets<SnowMaterial>>()
                .get(new_derived)
                .unwrap()
                .base
                .alpha_mode,
            AlphaMode::Mask(0.78)
        );
        assert_eq!(
            app.world().get::<SnowSurfaceSource>(first).unwrap().0,
            replacement
        );
        assert!(
            app.world()
                .get::<MeshMaterial3d<StandardMaterial>>(first)
                .is_none()
        );
        assert!(
            app.world()
                .get::<MeshMaterial3d<StandardMaterial>>(cab)
                .is_some()
        );
        assert!(
            app.world()
                .get::<MeshMaterial3d<StandardMaterial>>(glass)
                .is_some()
        );
        assert_eq!(
            app.world()
                .get::<MeshMaterial3d<StandardMaterial>>(building)
                .unwrap()
                .0,
            opaque
        );
        assert!(app.world().get::<SnowSurfaceSource>(building).is_none());
        assert_eq!(
            app.world().get::<SnowSurfaceSource>(second).unwrap().0,
            opaque
        );
        app.world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .get_mut(&opaque)
            .unwrap()
            .perceptual_roughness = 0.31;
        app.world_mut()
            .write_message(AssetEvent::<StandardMaterial>::Modified { id: opaque.id() });
        app.update();
        assert_eq!(
            app.world()
                .resource::<Assets<SnowMaterial>>()
                .get(&derived)
                .unwrap()
                .base
                .perceptual_roughness,
            0.31
        );
        assert_eq!(app.world().resource::<Assets<SnowMaterial>>().len(), 2);
    }
}
