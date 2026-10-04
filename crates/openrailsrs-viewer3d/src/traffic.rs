//! Each live service owns its track pose; native meshes and textures are shared.
use crate::{
    floating_origin::{FloatingOrigin, view_position},
    live::LiveDrive,
    rolling_stock::{ConsistVehicleVisual, TrainConsistScene},
    shapes::{RouteAssets, ShapeRenderAsset},
    terrain::TerrainElevation,
    track::TrackScene,
    track_position::{TrackPositionResolver, vehicle_pose_on_graph_edge},
    world::{RouteFocus, RouteWorldOffset},
};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Resource, Default)]
pub struct TrainRenderCache {
    pub shapes: HashMap<PathBuf, Option<(ShapeRenderAsset, Arc<openrailsrs_formats::ShapeFile>)>>,
    pub textures: HashMap<(PathBuf, i32), Handle<Image>>,
}

#[derive(Component)]
pub struct TrafficTrainMarker(pub usize);

fn spawn_generic_car(
    train: &mut ChildSpawnerCommands,
    vehicle: &ConsistVehicleVisual,
    index: usize,
    mesh: &Handle<Mesh>,
    material: &Handle<StandardMaterial>,
) {
    train
        .spawn((
            Transform::from_xyz(vehicle.offset_m, 0.0, 0.0),
            Visibility::Inherited,
            crate::rolling_stock_anim::TrainCarTrackOffset {
                offset_m: vehicle.offset_m,
                track_index: index,
                flipped: vehicle.flipped,
            },
            Name::new(format!("traffic:generic:{}", vehicle.name)),
        ))
        .with_children(|car| {
            car.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(0.0, 1.8, 0.0).with_scale(Vec3::new(
                    vehicle.length_m.max(1.0),
                    3.4,
                    3.0,
                )),
                Visibility::Inherited,
            ));
        });
}

pub fn spawn_traffic(
    mut commands: Commands,
    live: Res<LiveDrive>,
    consist: Res<TrainConsistScene>,
    assets: Res<RouteAssets>,
    scene: Res<TrackScene>,
    offset: Res<RouteWorldOffset>,
    focus: Res<RouteFocus>,
    terrain: Option<Res<TerrainElevation>>,
    mut cache: ResMut<TrainRenderCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dirs = consist.shape_search_dirs(&assets.route_dir);
    let dirs: Vec<_> = dirs.iter().map(|p| p.as_path()).collect();
    let resolver = assets
        .track_db()
        .map(|tdb| TrackPositionResolver::from_track_scene(tdb, Some(assets.tsection()), &scene));
    let mut textures = std::mem::take(&mut cache.textures);
    let generic_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let generic_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.35, 0.4, 0.5),
        ..default()
    });
    for (index, service) in live.traffic.services.iter().enumerate() {
        let session = &service.session;
        let Some((pos, rotation)) = session.current_edge_id().and_then(|edge| {
            vehicle_pose_on_graph_edge(
                &scene.graph,
                edge,
                session.pos_on_edge_m(),
                resolver.as_ref(),
                &scene,
                offset.delta,
                &focus,
                terrain.as_deref(),
            )
        }) else {
            continue;
        };
        let head = Transform::from_translation(pos).with_rotation(rotation);
        commands
            .spawn((
                TrafficTrainMarker(index + 1),
                head,
                Visibility::Hidden,
                Name::new(format!("traffic:{}", service.id)),
            ))
            .with_children(|train| {
                for (car_index, vehicle) in consist.vehicles_for(&service.id).iter().enumerate() {
                    let Some(path) = vehicle.shape_file.as_deref().and_then(|name| {
                        crate::shapes::resolve_vehicle_shape_path(&dirs, name, &assets.route_dir)
                    }) else {
                        spawn_generic_car(
                            train,
                            vehicle,
                            index + 1,
                            &generic_mesh,
                            &generic_material,
                        );
                        continue;
                    };
                    let texture_dirs =
                        crate::shapes::vehicle_texture_search_dirs(&path, &assets.route_dir);
                    let texture_dirs: Vec<_> = texture_dirs.iter().map(|p| p.as_path()).collect();
                    let Some((asset, shape)) = cache
                        .shapes
                        .entry(path.clone())
                        .or_insert_with(|| {
                            crate::shapes::load_shape_render_asset_and_file_from_path(
                                &path,
                                &texture_dirs,
                                Some(crate::launch::LIVE_TRAIN_LOD_DISTANCE_M),
                                &mut meshes,
                                &mut images,
                                &mut materials,
                                &mut textures,
                                Color::srgb(0.55, 0.58, 0.62),
                                true,
                            )
                            .map(|(asset, shape)| (asset, Arc::new(shape)))
                        })
                        .clone()
                    else {
                        spawn_generic_car(
                            train,
                            vehicle,
                            index + 1,
                            &generic_mesh,
                            &generic_material,
                        );
                        continue;
                    };
                    let transform = meshes
                        .get(&asset.combined_mesh)
                        .map(|mesh| {
                            crate::shapes::vehicle_shape_local_transform(
                                mesh,
                                vehicle.offset_m,
                                vehicle.length_m,
                                vehicle.flipped,
                            )
                        })
                        .unwrap_or_default();
                    train
                        .spawn((
                            transform,
                            Visibility::Inherited,
                            crate::rolling_stock_anim::TrainCarTrackOffset {
                                offset_m: vehicle.offset_m,
                                track_index: index + 1,
                                flipped: vehicle.flipped,
                            },
                            Name::new(format!("traffic:{}:car:{car_index}", service.id)),
                        ))
                        .with_children(|car| {
                            for part in &asset.parts {
                                let mut entity = car.spawn((
                                    Mesh3d(part.mesh.clone()),
                                    MeshMaterial3d(part.material.clone()),
                                    Transform::IDENTITY,
                                    Visibility::Inherited,
                                ));
                                if !crate::train::train_part_casts_shadow(part.is_transparent) {
                                    entity.insert(NotShadowCaster);
                                }
                                crate::rolling_stock_anim::insert_part_anim(
                                    &mut entity,
                                    &shape,
                                    part.prim_state_idx,
                                    crate::rolling_stock_anim::DEFAULT_WHEEL_RADIUS_M,
                                );
                            }
                        });
                }
            });
    }
    cache.textures = textures;
}

pub fn update_traffic_poses(
    live: Res<LiveDrive>,
    scene: Res<TrackScene>,
    assets: Res<RouteAssets>,
    resolver_cache: Res<crate::track_position::TrackPositionResolverCache>,
    offset: Res<RouteWorldOffset>,
    focus: Res<RouteFocus>,
    origin: Res<FloatingOrigin>,
    terrain: Option<Res<TerrainElevation>>,
    mut trains: Query<(&TrafficTrainMarker, &mut Transform, &mut Visibility)>,
) {
    let resolver = assets
        .track_db()
        .map(|tdb| resolver_cache.resolver(tdb, Some(assets.tsection())));
    for (marker, mut transform, mut visibility) in &mut trains {
        let Some(service) = live.traffic.services.get(marker.0 - 1) else {
            continue;
        };
        *visibility = if service.departed {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let Some((edge, position)) = live.visual_position_for_service(marker.0, 0.0, 0.0) else {
            continue;
        };
        if let Some((point, rotation)) = vehicle_pose_on_graph_edge(
            &scene.graph,
            &edge,
            position,
            resolver.as_ref(),
            &scene,
            offset.delta,
            &focus,
            terrain.as_deref(),
        ) {
            transform.set_if_neq(
                Transform::from_translation(view_position(point, &origin)).with_rotation(rotation),
            );
        }
    }
}
