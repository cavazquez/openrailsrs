//! MSTS `CarSpawner` road traffic over `.rdb` (issue #32).
//!
//! v1: one deterministic car per spawner, posed on the RDB segment between the
//! two `TrItemId (1 …)` endpoints, with simple ping-pong motion at `CarAvSpeed`.

use std::collections::HashMap;
use std::path::PathBuf;

use bevy::prelude::*;

use crate::shapes::{
    RouteAssets, ShapeRenderAsset, load_shape_render_asset_from_path, resolve_shape_path,
};
use crate::viewer_log;
use crate::world::{
    RouteFocus, WorldObject, WorldScene, WorldTileBound, horizontal_distance_xz, visible_radius_m,
};

const COLOR_CAR_FALLBACK: Color = Color::srgb(0.75, 0.22, 0.18);
/// Convert `CarAvSpeed` (OR stores ~km/h style values used as m/s*scale in viewer).
/// OpenRails treats `CarAvSpeed` as m/s in RoadCars; Chiltern uses 20 → 20 m/s.
fn car_speed_mps(car_av_speed: f32) -> f32 {
    car_av_speed.clamp(1.0, 40.0)
}

/// Moving road car along a straight RDB chord (start→end).
#[derive(Component, Clone, Debug)]
pub struct RoadCarMotion {
    pub start: Vec3,
    pub end: Vec3,
    pub speed_mps: f32,
    pub length_m: f32,
    /// Phase in [0, 1) along the segment (ping-pong).
    pub t: f32,
    pub forward: bool,
}

/// Resolve Bevy endpoints for a CarSpawner from RDB `TrItemRData`.
pub fn spawner_rdb_endpoints(
    road_db: &openrailsrs_formats::TrackDbFile,
    rdb_ids: &[u32],
) -> Option<(Vec3, Vec3)> {
    let start_id = *rdb_ids.first()?;
    let end_id = if rdb_ids.len() >= 2 {
        rdb_ids[1]
    } else {
        start_id
    };
    let (sx, sy, sz) = road_db.item_bevy_position(start_id)?;
    let (ex, ey, ez) = road_db.item_bevy_position(end_id)?;
    let start = Vec3::new(sx, sy, sz);
    let end = Vec3::new(ex, ey, ez);
    if !start.is_finite() || !end.is_finite() {
        return None;
    }
    // Reject degenerate / origin glitches.
    if start.length_squared() < 1.0 || end.length_squared() < 1.0 {
        return None;
    }
    Some((start, end))
}

fn yaw_along(start: Vec3, end: Vec3) -> Quat {
    let dir = end - start;
    let flat = Vec3::new(dir.x, 0.0, dir.z);
    if flat.length_squared() < 1e-4 {
        return Quat::IDENTITY;
    }
    let forward = flat.normalize();
    Quat::from_rotation_arc(Vec3::NEG_Z, forward)
}

fn pose_on_segment(start: Vec3, end: Vec3, t: f32, car_length: f32) -> (Vec3, Quat) {
    let delta = end - start;
    let len = delta.length().max(1e-3);
    // Keep the car fully on the segment.
    let margin = (car_length * 0.5 / len).clamp(0.0, 0.45);
    let u = t.clamp(margin, 1.0 - margin);
    let pos = start + delta * u;
    (pos, yaw_along(start, end))
}

/// Spawn road cars for every `CarSpawner` in the world scene (startup).
#[allow(clippy::too_many_arguments)]
pub fn spawn_road_cars(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    world: Res<WorldScene>,
    assets: Res<RouteAssets>,
    focus: Res<RouteFocus>,
) {
    spawn_road_car_objects(
        &mut commands,
        &mut meshes,
        &mut images,
        &mut materials,
        &world.items,
        &assets,
        &focus,
        None,
    );
}

/// Spawn road cars for a slice of world objects (tile streaming).
#[allow(clippy::too_many_arguments)]
pub fn spawn_road_car_objects(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    items: &[WorldObject],
    assets: &RouteAssets,
    focus: &RouteFocus,
    cull_center: Option<Vec3>,
) {
    let spawners: Vec<_> = items
        .iter()
        .filter(|o| o.kind == "CarSpawner" && o.car_spawner.is_some())
        .collect();
    if spawners.is_empty() {
        return;
    }
    let Some(road_db) = assets.road_db() else {
        viewer_log!("openrailsrs-viewer3d: CarSpawner present but no .rdb loaded");
        return;
    };

    let cull_at = cull_center.unwrap_or(focus.center);
    let mut texture_cache: HashMap<PathBuf, Handle<Image>> = HashMap::new();
    let mut shape_cache: HashMap<String, Option<ShapeRenderAsset>> = HashMap::new();
    let fallback_mat = materials.add(StandardMaterial {
        base_color: COLOR_CAR_FALLBACK,
        perceptual_roughness: 0.7,
        ..default()
    });
    let fallback_mesh = meshes.add(Cuboid::new(4.0, 1.4, 1.8));

    let mut spawned = 0usize;
    let mut shaped = 0usize;
    let mut skipped = 0usize;

    for obj in spawners {
        if horizontal_distance_xz(cull_at, obj.position) > visible_radius_m() {
            continue;
        }
        let patch = obj.car_spawner.as_ref().expect("filtered");
        let Some((start, end)) = spawner_rdb_endpoints(road_db, &patch.rdb_tr_item_ids) else {
            skipped += 1;
            continue;
        };
        let item = assets
            .carspawn()
            .pick_item(patch.list_name.as_deref(), patch.uid);
        let car_length = item.map(|i| i.length_m).unwrap_or(8.0).clamp(3.0, 60.0);
        let t0 = ((patch.uid as f32 * 0.618_034) % 1.0).abs();
        let (world_pos, rot) = pose_on_segment(start, end, t0, car_length);
        if !world_pos.is_finite() {
            skipped += 1;
            continue;
        }

        let render = focus.to_render_surface(world_pos);
        let render_start = focus.to_render_surface(start);
        let render_end = focus.to_render_surface(end);

        let mut entity = commands.spawn((
            WorldTileBound {
                tile_x: obj.tile_x,
                tile_z: obj.tile_z,
            },
            RoadCarMotion {
                start: render_start,
                end: render_end,
                speed_mps: car_speed_mps(patch.car_av_speed),
                length_m: car_length,
                t: t0,
                forward: patch.uid % 2 == 0,
            },
            Transform {
                translation: render,
                rotation: rot,
                scale: Vec3::ONE,
            },
            Name::new(format!(
                "roadcar:{}:{}",
                patch.list_name.as_deref().unwrap_or("Default"),
                patch.uid
            )),
        ));

        let mut used_shape = false;
        if let Some(car) = item {
            let asset = shape_cache
                .entry(car.shape.clone())
                .or_insert_with(|| {
                    let path = resolve_shape_path(&assets.route_dir, &car.shape)
                        .or_else(|| assets.resolve_shape(&car.shape))?;
                    let dirs = [assets.route_dir.as_path()];
                    load_shape_render_asset_from_path(
                        &path,
                        &dirs,
                        Some(80.0),
                        meshes,
                        images,
                        materials,
                        &mut texture_cache,
                        COLOR_CAR_FALLBACK,
                        false,
                    )
                })
                .clone();
            if let Some(asset) = asset {
                used_shape = true;
                shaped += 1;
                let material = asset
                    .parts
                    .first()
                    .map(|p| p.material.clone())
                    .unwrap_or_else(|| fallback_mat.clone());
                entity.insert((
                    Mesh3d(asset.combined_mesh.clone()),
                    MeshMaterial3d(material),
                ));
            }
        }
        if !used_shape {
            entity.insert((
                Mesh3d(fallback_mesh.clone()),
                MeshMaterial3d(fallback_mat.clone()),
            ));
        }
        spawned += 1;
    }

    if spawned + skipped > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: {spawned} road car(s) ({shaped} shaped, {skipped} skipped)"
        );
    }
}

pub(crate) fn update_road_cars(
    time: Res<Time>,
    mut cars: Query<(&mut Transform, &mut RoadCarMotion)>,
) {
    let dt = time.delta_secs();
    for (mut tf, mut motion) in &mut cars {
        let span = (motion.end - motion.start).length().max(1e-3);
        let margin = (motion.length_m * 0.5 / span).clamp(0.0, 0.45);
        let usable = (1.0 - 2.0 * margin).max(0.05);
        let delta_t = (motion.speed_mps * dt) / span;
        if motion.forward {
            motion.t += delta_t;
            if motion.t >= 1.0 - margin {
                motion.t = 1.0 - margin;
                motion.forward = false;
            }
        } else {
            motion.t -= delta_t;
            if motion.t <= margin {
                motion.t = margin;
                motion.forward = true;
            }
        }
        let u = motion.t.clamp(margin, margin + usable);
        let world = motion.start + (motion.end - motion.start) * u;
        tf.translation = world;
        tf.rotation = if motion.forward {
            yaw_along(motion.start, motion.end)
        } else {
            yaw_along(motion.end, motion.start)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn chiltern_route() -> Option<PathBuf> {
        std::env::var_os("CHILTERN_ROUTE")
            .map(PathBuf::from)
            .or_else(|| {
                let home = std::env::var_os("HOME")?;
                let p = PathBuf::from(home)
                    .join("Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern");
                p.join("Chiltern.rdb").is_file().then_some(p)
            })
    }

    #[test]
    fn chiltern_fixture_spawner_endpoints_finite() {
        let Some(route) = chiltern_route() else {
            return;
        };
        let rdb =
            openrailsrs_formats::TrackDbFile::from_path(route.join("Chiltern.rdb")).expect("rdb");
        let posed = rdb.items.iter().filter(|i| i.world.is_some()).count();
        assert!(
            posed > 100,
            "expected many TrItemRData on Chiltern.rdb, got {posed}"
        );

        // Sample from w-006084+014930: TrItemId 753 / 754
        let (a, b) = spawner_rdb_endpoints(&rdb, &[753, 754]).expect("endpoints");
        assert!(a.is_finite() && b.is_finite());
        assert!(a.length() > 1000.0 && b.length() > 1000.0);
        assert!((a - b).length() > 1.0, "segment should have length");
        let (pos, _) = pose_on_segment(a, b, 0.5, 8.0);
        assert!(pos.is_finite());
        assert!(
            (pos - Vec3::ZERO).length() > 1000.0,
            "must not sit at origin"
        );
    }

    #[test]
    fn chiltern_fixture_tile_has_carspawners() {
        use crate::world::load_world_from_route_dir_near;

        let Some(route) = chiltern_route() else {
            return;
        };
        let (ox, oz) = openrailsrs_formats::msts_tile_world_origin(-6084, 14930);
        let center = Vec3::new(ox + 1024.0, 0.0, oz + 1024.0);
        let scene = load_world_from_route_dir_near(&route, Some(center), 50.0);
        let n = scene
            .items
            .iter()
            .filter(|o| o.kind == "CarSpawner" && o.tile_x == -6084 && o.tile_z == 14930)
            .count();
        assert_eq!(n, 2, "fixture tile should materialize 2 CarSpawner");
        for obj in scene
            .items
            .iter()
            .filter(|o| o.kind == "CarSpawner" && o.tile_x == -6084 && o.tile_z == 14930)
        {
            let p = obj.car_spawner.as_ref().expect("meta");
            assert_eq!(p.rdb_tr_item_ids.len(), 2);
            assert!(
                p.list_name.as_deref() == Some("London Inner"),
                "uid {:?} list_name={:?}",
                obj.uid,
                p.list_name
            );
        }
    }
}
