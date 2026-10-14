//! MSTS `CarSpawner` road traffic over `.rdb` (issue #32).
//!
//! A bounded pool follows authored RDB vector geometry between the two
//! `TrItemId (1 …)` endpoints at `CarAvSpeed`. Queues stop before active
//! crossings; a car reaching the end re-enters only when the entry is clear.
//! Shapes spawn **per prim_state part** (like train cars). Materials are unlit
//! with albedo ×1: lit PBR under station ambient washes pale ACE vans to white,
//! and legacy scenery unlit×4 clips the same cream albedos.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::shapes::{
    RouteAssets, ShapeRenderAsset, load_shape_render_asset_from_path, resolve_shape_path,
    texture_search_dirs_for_shape,
};
use crate::train::train_part_casts_shadow;
use crate::viewer_log;
use crate::world::{RouteFocus, WorldObject, WorldScene, WorldTileBound, horizontal_distance_xz};

const COLOR_CAR_FALLBACK: Color = Color::srgb(0.75, 0.22, 0.18);

#[derive(Clone, Copy, PartialEq, Eq)]
struct RoadJobKey {
    tile: (i32, i32),
    source_index: usize,
}
impl RoadJobKey {
    fn of(object: &WorldObject) -> Self {
        Self {
            tile: (object.tile_x, object.tile_z),
            source_index: object.source_index,
        }
    }
}

struct PreparedRoadModel {
    path: PathBuf,
    directories: Vec<PathBuf>,
    loaded: crate::shapes::LoadedShape,
    pixels: crate::shapes::PrefetchedTextures,
    pbr: Option<crate::shapes::ShapePbrSidecar>,
    flags: openrailsrs_bevy_scenery::textures::TextureFlags,
    environment: openrailsrs_bevy_scenery::textures::TextureEnvironment,
}
#[derive(Clone)]
struct RoadSpawnPlan {
    object: WorldObject,
    curve: std::sync::Arc<RoadCurve>,
    start: Vec3,
    end: Vec3,
    length_m: f32,
    speed_mps: f32,
    count: usize,
}
struct PreparedRoadSpawner {
    plan: Option<RoadSpawnPlan>,
    model_name: Option<String>,
    model_requested: bool,
    model: Option<PreparedRoadModel>,
}
struct RoadRequest {
    object: WorldObject,
    road_db: Option<std::sync::Arc<openrailsrs_formats::TrackDbFile>>,
    sections: std::sync::Arc<openrailsrs_formats::TSectionCatalog>,
    route_dir: PathBuf,
    focus: RouteFocus,
    model_name: Option<String>,
    indexed_path: Option<PathBuf>,
    length_m: f32,
    model_requested: bool,
}
struct CachedRoadModel {
    asset: Option<ShapeRenderAsset>,
    flags: openrailsrs_bevy_scenery::textures::TextureFlags,
    environment: openrailsrs_bevy_scenery::textures::TextureEnvironment,
    tiles: HashSet<(i32, i32)>,
    last_use: u64,
}
impl CachedRoadModel {
    fn current(&self) -> bool {
        self.environment == crate::shapes::scenery_texture_environment(self.flags)
    }
}

/// The owning WORLD stream generation is the only receiver. Resetting the
/// scene or unloading its tile drops that receiver; an old job cannot publish
/// GPU assets into the next scene. At most one preparation job is in flight.
#[derive(Default)]
pub(crate) struct RoadCarPreparation {
    task: Option<(
        RoadJobKey,
        std::sync::Mutex<std::sync::mpsc::Receiver<PreparedRoadSpawner>>,
    )>,
    ready: Option<(RoadJobKey, PreparedRoadSpawner)>,
    models: HashMap<String, CachedRoadModel>,
    next_use: u64,
    fallback: Option<(Handle<Mesh>, Handle<StandardMaterial>)>,
}
impl RoadCarPreparation {
    const MAX_MODELS: usize = 64;

    fn request(&self, object: WorldObject, assets: &RouteAssets, focus: RouteFocus) -> RoadRequest {
        let item = object.car_spawner.as_ref().and_then(|patch| {
            assets
                .carspawn()
                .pick_item(patch.list_name.as_deref(), patch.uid)
        });
        let model_name = item.map(|item| item.shape.clone());
        let indexed_path = model_name
            .as_deref()
            .and_then(|name| assets.resolve_shape(name));
        let model_requested = model_name
            .as_ref()
            .is_some_and(|name| self.models.get(name).is_none_or(|model| !model.current()));
        RoadRequest {
            object,
            road_db: assets.shared_road_db(),
            sections: assets.shared_tsection(),
            route_dir: assets.route_dir.clone(),
            focus,
            model_name,
            indexed_path,
            length_m: item.map_or(8., |item| item.length_m).clamp(3., 60.),
            model_requested,
        }
    }

    pub(crate) fn advance(
        &mut self,
        object: &WorldObject,
        assets: &RouteAssets,
        focus: RouteFocus,
    ) -> bool {
        let key = RoadJobKey::of(object);
        if self.ready.as_ref().is_some_and(|(ready, _)| *ready == key) {
            return true;
        }
        if self
            .task
            .as_ref()
            .is_some_and(|(pending, _)| *pending != key)
        {
            self.task = None;
        }
        if let Some((_, receiver)) = &self.task {
            let result = receiver.lock().unwrap().try_recv();
            match result {
                Ok(prepared) => {
                    self.task = None;
                    // A day/night/season change during preparation is not a
                    // licence to publish a stale context or decode on ECS.
                    let current = prepared.model.as_ref().is_none_or(|model| {
                        model.environment == crate::shapes::scenery_texture_environment(model.flags)
                            && model.pixels.resolved.environment_current()
                    }) && (prepared.model_requested
                        || prepared.model_name.as_ref().is_none_or(|name| {
                            self.models.get(name).is_some_and(CachedRoadModel::current)
                        }));
                    if current {
                        self.ready = Some((key, prepared));
                        return true;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return false,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.task = None;
                    self.ready = Some((
                        key,
                        PreparedRoadSpawner {
                            plan: None,
                            model_name: None,
                            model_requested: false,
                            model: None,
                        },
                    ));
                    return true;
                }
            }
        }
        let request = self.request(object.clone(), assets, focus);
        let (sender, receiver) = std::sync::mpsc::channel();
        self.task = Some((key, std::sync::Mutex::new(receiver)));
        std::thread::spawn(move || {
            let _ = sender.send(prepare_road_spawner(request));
        });
        false
    }

    pub(crate) fn forget_tiles(&mut self, tiles: &HashSet<(i32, i32)>) {
        if self
            .task
            .as_ref()
            .is_some_and(|(key, _)| tiles.contains(&key.tile))
        {
            self.task = None;
        }
        if self
            .ready
            .as_ref()
            .is_some_and(|(key, _)| tiles.contains(&key.tile))
        {
            self.ready = None;
        }
        self.models.retain(|_, model| {
            model.tiles.retain(|tile| !tiles.contains(tile));
            !model.tiles.is_empty()
        });
    }

    pub(crate) fn publish(
        &mut self,
        object: &WorldObject,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
        origin: &crate::floating_origin::FloatingOrigin,
    ) {
        let Some((key, prepared)) = self.ready.take() else {
            return;
        };
        if key != RoadJobKey::of(object) {
            return;
        }
        let Some(plan) = prepared.plan else { return };
        let _timer = crate::performance::ShapeTimer::start(
            "publish_road_car",
            std::path::Path::new("CarSpawner"),
        );
        self.next_use = self.next_use.saturating_add(1);
        if prepared.model_requested
            && let Some(name) = prepared.model_name.as_ref()
        {
            let mut flags = openrailsrs_bevy_scenery::textures::TextureFlags::default();
            let mut environment = crate::shapes::scenery_texture_environment(flags);
            let asset = prepared.model.map(|model| {
                flags = model.flags;
                environment = model.environment;
                let references = model
                    .directories
                    .iter()
                    .map(PathBuf::as_path)
                    .collect::<Vec<_>>();
                let _timer =
                    crate::performance::ShapeTimer::start("publish_road_model", &model.path);
                let asset = crate::shapes::shape_render_asset_from_loaded_with_ace_cache(
                    model.loaded,
                    &references,
                    meshes,
                    images,
                    materials,
                    None,
                    &mut HashMap::new(),
                    &model.pixels,
                    COLOR_CAR_FALLBACK,
                    None,
                    false,
                    false,
                    model.pbr.as_ref(),
                    flags,
                );
                apply_road_car_albedo_materials(&asset, materials);
                asset
            });
            if self.models.len() >= Self::MAX_MODELS && !self.models.contains_key(name) {
                let oldest = self
                    .models
                    .iter()
                    .min_by_key(|(_, value)| value.last_use)
                    .map(|(key, _)| key.clone());
                if let Some(oldest) = oldest {
                    self.models.remove(&oldest);
                }
            }
            self.models.insert(
                name.clone(),
                CachedRoadModel {
                    asset,
                    flags,
                    environment,
                    tiles: HashSet::new(),
                    last_use: self.next_use,
                },
            );
        }
        let asset = prepared
            .model_name
            .as_ref()
            .and_then(|name| self.models.get_mut(name))
            .and_then(|cached| {
                cached.tiles.insert(key.tile);
                cached.last_use = self.next_use;
                cached.asset.clone()
            });
        let fallback = if asset.as_ref().is_none_or(|asset| asset.parts.is_empty()) {
            Some(
                self.fallback
                    .get_or_insert_with(|| {
                        (
                            meshes.add(Cuboid::new(4., 1.4, 1.8)),
                            materials.add(StandardMaterial {
                                base_color: COLOR_CAR_FALLBACK,
                                perceptual_roughness: 0.7,
                                ..default()
                            }),
                        )
                    })
                    .clone(),
            )
        } else {
            None
        };
        spawn_prepared_road_instances(commands, &plan, asset.as_ref(), fallback.as_ref(), origin);
    }
}

fn prepare_road_spawner(request: RoadRequest) -> PreparedRoadSpawner {
    let _timer = crate::performance::ShapeTimer::start(
        "prepare_road_car",
        std::path::Path::new("CarSpawner"),
    );
    let mut prepared = PreparedRoadSpawner {
        plan: None,
        model_name: request.model_name.clone(),
        model_requested: request.model_requested,
        model: None,
    };
    let Some(db) = request.road_db.as_deref() else {
        return prepared;
    };
    let Some(patch) = request.object.car_spawner.as_ref() else {
        return prepared;
    };
    let Some((start, end)) = spawner_rdb_endpoints(db, &patch.rdb_tr_item_ids) else {
        return prepared;
    };
    let Some(curve) = road_curve(
        db,
        &patch.rdb_tr_item_ids,
        &request.sections,
        &request.focus,
    ) else {
        return prepared;
    };
    let speed_mps = car_speed_mps(patch.car_av_speed);
    let count = ((curve.total / (patch.car_frequency.max(3.) * speed_mps + request.length_m))
        .floor() as usize)
        .clamp(1, 8);
    if request.model_requested
        && let Some(name) = request.model_name.as_deref()
        && let Some(path) = resolve_shape_path(&request.route_dir, name).or(request.indexed_path)
        && let Some((_, loaded)) = crate::shapes::load_shape_file_and_loaded(&path, Some(80.))
    {
        let directories = texture_search_dirs_for_shape(&path, &request.route_dir);
        let references = directories.iter().map(PathBuf::as_path).collect::<Vec<_>>();
        let flags = crate::shapes::texture_flags_for_shape(&path);
        let pbr = crate::shapes::load_shape_pbr_sidecar(&path);
        let resolved = crate::shapes::prepare_shape_texture_paths(
            &loaded,
            &[],
            &references,
            flags,
            pbr.as_ref(),
        );
        let mut paths = resolved.paths().cloned().collect::<Vec<_>>();
        paths.sort_unstable();
        paths.dedup();
        let mut pixels = crate::shapes::prefetch_ace_textures(&paths);
        pixels.resolved = resolved;
        prepared.model = Some(PreparedRoadModel {
            path,
            directories,
            loaded,
            pixels,
            pbr,
            flags,
            environment: crate::shapes::scenery_texture_environment(flags),
        });
    }
    prepared.plan = Some(RoadSpawnPlan {
        start: request.focus.to_render_surface(start),
        end: request.focus.to_render_surface(end),
        curve,
        object: request.object,
        length_m: request.length_m,
        speed_mps,
        count,
    });
    prepared
}

fn spawn_prepared_road_instances(
    commands: &mut Commands,
    plan: &RoadSpawnPlan,
    asset: Option<&ShapeRenderAsset>,
    fallback: Option<&(Handle<Mesh>, Handle<StandardMaterial>)>,
    origin: &crate::floating_origin::FloatingOrigin,
) {
    let patch = plan.object.car_spawner.as_ref().expect("prepared spawner");
    for slot in 0..plan.count {
        let t = (slot as f32 + 0.5) / plan.count as f32;
        let (position, rotation) = plan.curve.pose(t);
        if !position.is_finite() {
            continue;
        }
        let mut entity = commands.spawn((
            WorldTileBound {
                tile_x: plan.object.tile_x,
                tile_z: plan.object.tile_z,
            },
            RoadCarMotion {
                curve: Some(plan.curve.clone()),
                start: plan.start,
                end: plan.end,
                speed_mps: plan.speed_mps,
                length_m: plan.length_m,
                t,
                forward: true,
            },
            Transform {
                translation: crate::floating_origin::view_translation(position, origin),
                rotation,
                ..default()
            },
            Visibility::default(),
            Name::new(format!(
                "roadcar:{}:{}",
                patch.list_name.as_deref().unwrap_or("Default"),
                patch.uid
            )),
        ));
        if let Some(asset) = asset.filter(|asset| !asset.parts.is_empty()) {
            entity.with_children(|parent| {
                for (index, part) in asset.parts.iter().enumerate() {
                    let mut child = parent.spawn((
                        Mesh3d(part.mesh.clone()),
                        MeshMaterial3d(part.material.clone()),
                        Transform::default(),
                        Visibility::Visible,
                        Name::new(format!("roadcar:part:{index}:{}", part.prim_state_idx)),
                    ));
                    if !train_part_casts_shadow(part.is_transparent) {
                        child.insert(NotShadowCaster);
                    }
                }
            });
        } else if let Some((mesh, material)) = fallback {
            entity.insert((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone())));
        }
    }
}

/// MSTS carspawn atlases are fixed-function albedos (cream Escort vans, etc.).
/// Draw **unlit** with albedo ×1 and the ACE also on emissive so station ambient /
/// PBR cannot wash pale vans to solid white (and so missing-bind is obvious).
fn apply_road_car_albedo_materials(
    asset: &ShapeRenderAsset,
    materials: &mut Assets<StandardMaterial>,
) {
    for part in &asset.parts {
        let Some(mut mat) = materials.get_mut(&part.material) else {
            continue;
        };
        mat.unlit = true;
        mat.base_color = Color::WHITE;
        mat.metallic = 0.0;
        mat.reflectance = 0.5;
        mat.perceptual_roughness = 1.0;
        mat.fog_enabled = false;
        // Same atlas on emissive: survives lit/unlit path quirks under bright interiors.
        if let Some(tex) = mat.base_color_texture.clone() {
            mat.emissive = LinearRgba::WHITE;
            mat.emissive_texture = Some(tex);
        } else {
            mat.emissive = LinearRgba::BLACK;
            mat.emissive_texture = None;
        }
    }
}
/// Convert `CarAvSpeed` (OR stores ~km/h style values used as m/s*scale in viewer).
/// OpenRails treats `CarAvSpeed` as m/s in RoadCars; Chiltern uses 20 → 20 m/s.
fn car_speed_mps(car_av_speed: f32) -> f32 {
    car_av_speed.clamp(1.0, 40.0)
}

/// Road car on a sampled RDB path, retaining endpoints for pool identity.
#[derive(Component, Clone, Debug)]
pub struct RoadCarMotion {
    pub curve: Option<std::sync::Arc<RoadCurve>>,
    pub start: Vec3,
    pub end: Vec3,
    pub speed_mps: f32,
    pub length_m: f32,
    /// Fraction of travelled RDB path; wrapping is gated by the queue at entry.
    pub t: f32,
    pub forward: bool,
}
#[derive(Clone, Debug)]
pub struct RoadCurve {
    points: Vec<Vec3>,
    lengths: Vec<f32>,
    total: f32,
}
impl RoadCurve {
    fn new(points: Vec<Vec3>) -> Option<Self> {
        if points.len() < 2 || points.len() > 2048 || points.iter().any(|p| !p.is_finite()) {
            return None;
        }
        let mut lengths = vec![0.];
        for p in points.windows(2) {
            lengths.push(lengths.last().unwrap() + p[0].distance(p[1]));
        }
        let total = *lengths.last()?;
        (total > 1.).then_some(Self {
            points,
            lengths,
            total,
        })
    }
    fn pose(&self, t: f32) -> (Vec3, Quat) {
        let distance = t.clamp(0., 1.) * self.total;
        let i = self
            .lengths
            .partition_point(|s| *s < distance)
            .clamp(1, self.points.len() - 1);
        let frac =
            (distance - self.lengths[i - 1]) / (self.lengths[i] - self.lengths[i - 1]).max(0.001);
        (
            self.points[i - 1].lerp(self.points[i], frac),
            yaw_along(self.points[i - 1], self.points[i]),
        )
    }
    fn project(&self, p: Vec3) -> Option<f32> {
        self.points
            .windows(2)
            .enumerate()
            .filter_map(|(i, line)| {
                let delta = line[1] - line[0];
                let fraction =
                    ((p - line[0]).dot(delta) / delta.length_squared().max(0.001)).clamp(0., 1.);
                let distance = p.distance_squared(line[0] + delta * fraction);
                (distance < 36.).then_some((
                    distance,
                    (self.lengths[i] + fraction * delta.length()) / self.total,
                ))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|v| v.1)
    }
}
fn road_curve(
    db: &openrailsrs_formats::TrackDbFile,
    ids: &[u32],
    catalog: &openrailsrs_formats::TSectionCatalog,
    focus: &RouteFocus,
) -> Option<std::sync::Arc<RoadCurve>> {
    use openrailsrs_formats::TrackNodeKind;
    let a = *ids.first()?;
    let b = *ids.get(1)?;
    let start = db.host_vector_for_item(a)?;
    let end = db.host_vector_for_item(b)?;
    let nodes = db
        .nodes
        .iter()
        .map(|n| (n.id, n))
        .collect::<std::collections::HashMap<_, _>>();
    let mut queue = std::collections::VecDeque::from([start]);
    let mut previous = std::collections::HashMap::from([(start, start)]);
    while let Some(at) = queue.pop_front() {
        if at == end {
            break;
        }
        let node = nodes.get(&at)?;
        let mut neighbours = node.pin_refs.iter().map(|p| p.node_id).collect::<Vec<_>>();
        match &node.kind {
            TrackNodeKind::Vector { pins, .. } => neighbours.extend([pins.0, pins.1]),
            TrackNodeKind::Junction { pins } => neighbours.extend(pins.iter().map(|p| p.node_id)),
            _ => {}
        }
        neighbours.sort_unstable();
        neighbours.dedup();
        for next in neighbours {
            if nodes.contains_key(&next) && !previous.contains_key(&next) {
                previous.insert(next, at);
                queue.push_back(next);
            }
        }
        if previous.len() > 10000 {
            return None;
        }
    }
    if !previous.contains_key(&end) {
        return None;
    }
    let mut route = vec![end];
    while *route.last()? != start {
        route.push(*previous.get(route.last()?)?);
    }
    route.reverse();
    let frame = bevy::math::DVec3::new(
        f64::from(focus.center.x),
        f64::from(focus.height_origin),
        f64::from(focus.center.z),
    );
    let mut points = Vec::<Vec3>::new();
    for (index, id) in route.iter().enumerate() {
        let node = *nodes.get(id)?;
        let TrackNodeKind::Vector { pins, .. } = &node.kind else {
            continue;
        };
        let path =
            openrailsrs_bevy_scenery::spawn::tdb_track::TrackVectorPath::new(node, Some(catalog))?;
        let from = if *id == start {
            db.item_by_id(a)?.distance_m
        } else if route.get(index.wrapping_sub(1)) == Some(&pins.0) {
            0.
        } else {
            path.length_m()
        };
        let to = if *id == end {
            db.item_by_id(b)?.distance_m
        } else if route.get(index + 1) == Some(&pins.0) {
            0.
        } else {
            path.length_m()
        };
        let samples = (((to - from).abs() / 10.).ceil() as usize).clamp(1, 2047);
        for step in 0..=samples {
            let point = path
                .pose_in_frame(from + (to - from) * step as f64 / samples as f64, frame)
                .position;
            if step == 0 && points.last().is_some_and(|old| old.distance(point) > 20.) {
                return None;
            }
            if points
                .last()
                .is_none_or(|old| old.distance_squared(point) > 0.0001)
            {
                points.push(point);
            }
            if points.len() > 2048 {
                return None;
            }
        }
    }
    RoadCurve::new(points).map(std::sync::Arc::new)
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
        &crate::floating_origin::FloatingOrigin::default(),
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
    origin: &crate::floating_origin::FloatingOrigin,
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
    let mut texture_cache: HashMap<(PathBuf, i32), Handle<Image>> = HashMap::new();
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
        if spawned >= 128 {
            break;
        }
        if horizontal_distance_xz(cull_at, obj.position) > crate::world::shape_mesh_radius_m() {
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
        let Some(curve) = road_curve(road_db, &patch.rdb_tr_item_ids, assets.tsection(), focus)
        else {
            skipped += 1;
            continue;
        };
        let count = ((curve.total
            / (patch.car_frequency.max(3.) * car_speed_mps(patch.car_av_speed) + car_length))
            .floor() as usize)
            .clamp(1, 8);
        for slot in 0..count {
            let t0 = (slot as f32 + 0.5) / count as f32;
            let (world_pos, _rot) = pose_on_segment(start, end, t0, car_length);
            if !world_pos.is_finite() {
                skipped += 1;
                continue;
            }

            let render_start = focus.to_render_surface(start);
            let render_end = focus.to_render_surface(end);
            let (render, rot) = curve.pose(t0);
            let curve = Some(curve.clone());

            let mut entity = commands.spawn((
                WorldTileBound {
                    tile_x: obj.tile_x,
                    tile_z: obj.tile_z,
                },
                RoadCarMotion {
                    curve,
                    start: render_start,
                    end: render_end,
                    speed_mps: car_speed_mps(patch.car_av_speed),
                    length_m: car_length,
                    t: t0,
                    forward: true,
                },
                Transform {
                    translation: crate::floating_origin::view_translation(render, origin),
                    rotation: rot,
                    scale: Vec3::ONE,
                },
                Visibility::default(),
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
                        // Same TEXTURES/GLOBAL search as WORLD/train (not just route_dir).
                        let tex_dirs = texture_search_dirs_for_shape(&path, &assets.route_dir);
                        let dir_refs: Vec<&std::path::Path> =
                            tex_dirs.iter().map(PathBuf::as_path).collect();
                        let asset = load_shape_render_asset_from_path(
                            &path,
                            &dir_refs,
                            Some(80.0),
                            meshes,
                            images,
                            materials,
                            &mut texture_cache,
                            COLOR_CAR_FALLBACK,
                            false,
                        )?;
                        apply_road_car_albedo_materials(&asset, materials);
                        let textured = asset.parts.iter().filter(|p| p.has_texture).count();
                        viewer_log!(
                            "openrailsrs-viewer3d: roadcar shape {} — {} part(s), {} textured",
                            path.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
                            asset.parts.len(),
                            textured
                        );
                        Some(asset)
                    })
                    .clone();
                if let Some(asset) = asset {
                    if asset.parts.is_empty() {
                        // Keep fallback below.
                    } else {
                        used_shape = true;
                        shaped += 1;
                        // One MeshMaterial3d on combined_mesh painted the whole car with the
                        // first prim_state (often glass) → full-bright white on the platform.
                        entity.with_children(|parent| {
                            for (pi, part) in asset.parts.iter().enumerate() {
                                let mut part_entity = parent.spawn((
                                    Mesh3d(part.mesh.clone()),
                                    MeshMaterial3d(part.material.clone()),
                                    Transform::default(),
                                    Visibility::Visible,
                                    Name::new(format!("roadcar:part:{pi}:{}", part.prim_state_idx)),
                                ));
                                if !train_part_casts_shadow(part.is_transparent) {
                                    part_entity.insert(NotShadowCaster);
                                }
                            }
                        });
                    }
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
    }

    if spawned + skipped > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: {spawned} road car(s) ({shaped} shaped, {skipped} skipped)"
        );
    }
}

pub(crate) fn update_road_cars(
    time: Res<Time>,
    origin: Res<crate::floating_origin::FloatingOrigin>,
    crossings: Option<Res<crate::crossing_visual::RoadCrossings>>,
    live: Option<Res<crate::live::LiveDrive>>,
    mut cars: Query<(Entity, &mut Transform, &mut RoadCarMotion)>,
) {
    let _profile = crate::performance::profile_system("road_cars");
    let dt = if live.as_ref().is_some_and(|l| l.paused) {
        0.
    } else {
        time.delta_secs() * live.as_ref().map_or(1., |l| l.session.speed_mul as f32)
    };
    let mut snapshots = cars
        .iter()
        .map(|(e, _, m)| (e, m.start, m.end, m.t, m.forward, m.length_m))
        .collect::<Vec<_>>();
    for (entity, mut tf, mut motion) in &mut cars {
        let span = motion
            .curve
            .as_ref()
            .map_or((motion.end - motion.start).length(), |c| c.total)
            .max(1e-3);
        let margin = (motion.length_m * 0.5 / span).clamp(0.0, 0.45);
        let usable = (1.0 - 2.0 * margin).max(0.05);
        let mut delta_t = (motion.speed_mps * dt) / span;
        for (other, start, end, t, forward, length) in &snapshots {
            if *other == entity
                || *forward != motion.forward
                || start.distance(motion.start) > 0.1
                || end.distance(motion.end) > 0.1
            {
                continue;
            }
            let ahead = if motion.forward {
                *t - motion.t
            } else {
                motion.t - *t
            };
            if ahead > 0. {
                delta_t = delta_t
                    .min((ahead - (motion.length_m + length) * 0.5 / span - 2. / span).max(0.));
            }
        }
        for &(point, closed) in crossings.as_ref().map_or(&[][..], |c| c.stops.as_slice()) {
            if !closed {
                continue;
            }
            let at = motion
                .curve
                .as_ref()
                .and_then(|c| c.project(point))
                .or_else(|| {
                    let delta = motion.end - motion.start;
                    let at = (point - motion.start).dot(delta) / delta.length_squared().max(0.001);
                    ((0. ..=1.).contains(&at)
                        && point.distance_squared(motion.start + delta * at) < 36.)
                        .then_some(at)
                });
            if let Some(at) = at {
                let ahead = if motion.forward {
                    at - motion.t
                } else {
                    motion.t - at
                };
                if ahead >= 0. {
                    delta_t = delta_t.min((ahead - (10. + motion.length_m * 0.5) / span).max(0.));
                }
            }
        }
        if motion.forward {
            motion.t += delta_t;
            if dt > 0. && motion.t >= 1. - margin {
                let clear = snapshots
                    .iter()
                    .all(|(other, start, end, t, forward, length)| {
                        *other == entity
                            || *forward != motion.forward
                            || start.distance(motion.start) > 0.1
                            || end.distance(motion.end) > 0.1
                            || *t > margin + (motion.length_m + length) * 0.5 / span + 2. / span
                    });
                motion.t = if clear { margin } else { 1. - margin };
            }
        } else {
            motion.t -= delta_t;
            if dt > 0. && motion.t <= margin {
                let clear = snapshots
                    .iter()
                    .all(|(other, start, end, t, forward, length)| {
                        *other == entity
                            || *forward != motion.forward
                            || start.distance(motion.start) > 0.1
                            || end.distance(motion.end) > 0.1
                            || *t
                                < 1. - margin - (motion.length_m + length) * 0.5 / span - 2. / span
                    });
                motion.t = if clear { 1. - margin } else { margin };
            }
        }
        // Publish each newly claimed entry into this tick's queue view. Two
        // cars reaching the exit together must not wrap onto the same point.
        if let Some(record) = snapshots.iter_mut().find(|(other, ..)| *other == entity) {
            record.3 = motion.t;
        }
        let u = motion.t.clamp(margin, margin + usable);
        let (world, curve_rotation) = motion.curve.as_ref().map_or(
            (
                motion.start + (motion.end - motion.start) * u,
                yaw_along(motion.start, motion.end),
            ),
            |c| c.pose(u),
        );
        tf.translation = crate::floating_origin::view_translation(world, &origin);
        tf.rotation = if motion.forward {
            curve_rotation
        } else {
            curve_rotation * Quat::from_rotation_y(std::f32::consts::PI)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn preparation_fixture(directory: &std::path::Path) -> RoadRequest {
        use openrailsrs_formats::{
            TrItem, TrItemKind, TrItemWorldPose, TrackDbFile, TrackDbNode, TrackNodeKind,
            TrackVectorGeometry, TrackVectorPoint,
        };
        let point = |x| TrackVectorPoint {
            tile_x: 0,
            tile_z: 0,
            x,
            y: 10.,
            z: 0.,
        };
        let db = TrackDbFile {
            nodes: vec![TrackDbNode {
                junction_shape_index: None,
                id: 2,
                position: None,
                pin_refs: vec![],
                kind: TrackNodeKind::Vector {
                    length_m: 100.,
                    speed_limit_mps: 20.,
                    pins: (1, 3),
                    item_ids: vec![10, 11],
                    sections: vec![],
                    geometry: Some(TrackVectorGeometry {
                        start: point(0.),
                        end: point(100.),
                    }),
                },
            }],
            items: [(10, 20.), (11, 80.)]
                .map(|(id, x)| TrItem {
                    id,
                    kind: TrItemKind::Other,
                    distance_m: x,
                    world: Some(TrItemWorldPose {
                        x,
                        y: 10.,
                        z: 0.,
                        tile_x: 0,
                        tile_z: 0,
                    }),
                    speed_post_flags: None,
                    speed_post_angle_rad: None,
                })
                .to_vec(),
        };
        let object = WorldObject {
            crossing: None,
            kind: "CarSpawner",
            uid: Some(8),
            source_index: 0,
            label: "prepared road".into(),
            shape_file: None,
            section_idx: None,
            dyntrack_sections: vec![],
            position: Vec3::new(40., 10., 0.),
            position_precision_offset: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
            linear: None,
            tile_x: 0,
            tile_z: 0,
            forest: None,
            water: None,
            transfer: None,
            car_spawner: Some(crate::world::CarSpawnerPatch {
                uid: 8,
                list_name: None,
                car_frequency: 3.,
                car_av_speed: 5.,
                rdb_tr_item_ids: vec![10, 11],
            }),
            signal: None,
            pickup: None,
            tr_item_ids: vec![],
            static_detail_level: 0,
            loop_animation: false,
        };
        RoadRequest {
            object,
            road_db: Some(std::sync::Arc::new(db)),
            sections: std::sync::Arc::new(Default::default()),
            route_dir: directory.to_path_buf(),
            focus: RouteFocus {
                center: Vec3::ZERO,
                height_origin: 10.,
            },
            model_name: Some("car.s".into()),
            indexed_path: Some(directory.join("SHAPES/car.s")),
            length_m: 4.,
            model_requested: true,
        }
    }

    #[test]
    fn worker_preparation_publishes_original_parts_without_source_reads_and_reuses_models() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("SHAPES")).unwrap();
        std::fs::create_dir(directory.path().join("TEXTURES")).unwrap();
        let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        std::fs::copy(
            fixture_root.join("openrailsrs-formats/tests/fixtures/minimal.s"),
            directory.path().join("SHAPES/car.s"),
        )
        .unwrap();
        std::fs::copy(
            fixture_root.join("openrailsrs-ace/tests/fixtures/rgba8_4x4.ace"),
            directory.path().join("TEXTURES/wagon.ace"),
        )
        .unwrap();
        let path = directory.path().join("SHAPES/car.s");
        let dirs = texture_search_dirs_for_shape(&path, directory.path());
        let refs = dirs.iter().map(PathBuf::as_path).collect::<Vec<_>>();
        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let expected = load_shape_render_asset_from_path(
            &path,
            &refs,
            Some(80.),
            &mut meshes,
            &mut images,
            &mut materials,
            &mut HashMap::new(),
            COLOR_CAR_FALLBACK,
            false,
        )
        .unwrap();
        apply_road_car_albedo_materials(&expected, &mut materials);
        let request = preparation_fixture(directory.path());
        let object = request.object.clone();
        let prepared = std::thread::spawn(move || prepare_road_spawner(request))
            .join()
            .unwrap();
        let plan = prepared.plan.as_ref().unwrap().clone();
        assert_eq!(plan.count, 3);
        assert!((plan.curve.total - 60.).abs() < 0.01);
        // Publication must only consume the completed worker result, even when
        // the author sources have disappeared since its preparation.
        std::fs::remove_dir_all(directory.path().join("SHAPES")).unwrap();
        std::fs::remove_dir_all(directory.path().join("TEXTURES")).unwrap();
        let mut state = RoadCarPreparation {
            ready: Some((RoadJobKey::of(&object), prepared)),
            ..Default::default()
        };
        let mut world = World::new();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        {
            let mut commands = Commands::new(&mut queue, &world);
            state.publish(
                &object,
                &mut commands,
                &mut meshes,
                &mut images,
                &mut materials,
                &Default::default(),
            );
        }
        queue.apply(&mut world);
        let actual = state.models["car.s"].asset.as_ref().unwrap();
        assert_eq!(actual.parts.len(), expected.parts.len());
        for (actual, expected) in actual.parts.iter().zip(&expected.parts) {
            let a = meshes.get(&actual.mesh).unwrap();
            let e = meshes.get(&expected.mesh).unwrap();
            for attribute in [
                Mesh::ATTRIBUTE_POSITION,
                Mesh::ATTRIBUTE_NORMAL,
                Mesh::ATTRIBUTE_UV_0,
            ] {
                assert_eq!(
                    format!("{:?}", a.attribute(attribute)),
                    format!("{:?}", e.attribute(attribute))
                );
            }
            assert_eq!(format!("{:?}", a.indices()), format!("{:?}", e.indices()));
            assert_eq!(actual.has_texture, expected.has_texture);
            let a = materials.get(&actual.material).unwrap();
            let e = materials.get(&expected.material).unwrap();
            assert_eq!(a.alpha_mode, e.alpha_mode);
            assert_eq!(a.unlit, e.unlit);
            assert_eq!(a.base_color, e.base_color);
            let a = images.get(a.base_color_texture.as_ref().unwrap()).unwrap();
            let e = images.get(e.base_color_texture.as_ref().unwrap()).unwrap();
            assert_eq!(a.data, e.data);
            assert_eq!(a.texture_descriptor.format, e.texture_descriptor.format);
        }
        let cars = world
            .query::<(&RoadCarMotion, &Transform)>()
            .iter(&world)
            .collect::<Vec<_>>();
        assert_eq!(cars.len(), 3);
        for (motion, transform) in cars {
            assert!((transform.translation - plan.curve.pose(motion.t).0).length() < 0.001);
        }
        let counts = (meshes.len(), images.len(), materials.len());
        let mut object2 = object.clone();
        object2.source_index = 1;
        state.ready = Some((
            RoadJobKey::of(&object2),
            PreparedRoadSpawner {
                plan: Some(RoadSpawnPlan {
                    object: object2.clone(),
                    ..plan
                }),
                model_name: Some("car.s".into()),
                model_requested: false,
                model: None,
            },
        ));
        {
            let mut commands = Commands::new(&mut queue, &world);
            state.publish(
                &object2,
                &mut commands,
                &mut meshes,
                &mut images,
                &mut materials,
                &Default::default(),
            );
        }
        queue.apply(&mut world);
        assert_eq!(
            (meshes.len(), images.len(), materials.len()),
            counts,
            "second spawner shares existing GPU owners"
        );
        assert_eq!(world.query::<&RoadCarMotion>().iter(&world).count(), 6);
    }

    #[test]
    fn unloading_and_reset_drop_receivers_before_an_old_worker_can_publish() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let key = RoadJobKey {
            tile: (4, -2),
            source_index: 9,
        };
        let mut state = RoadCarPreparation {
            task: Some((key, std::sync::Mutex::new(receiver))),
            ..default()
        };
        state.forget_tiles(&HashSet::from([key.tile]));
        assert!(state.task.is_none());
        assert!(
            sender
                .send(PreparedRoadSpawner {
                    plan: None,
                    model_name: None,
                    model_requested: false,
                    model: None
                })
                .is_err()
        );
        let (sender, receiver) = std::sync::mpsc::channel();
        state.task = Some((key, std::sync::Mutex::new(receiver)));
        drop(state);
        assert!(
            sender
                .send(PreparedRoadSpawner {
                    plan: None,
                    model_name: None,
                    model_requested: false,
                    model: None
                })
                .is_err()
        );
        let next_scene = RoadCarPreparation::default();
        assert!(
            next_scene.ready.is_none() && next_scene.task.is_none() && next_scene.models.is_empty()
        );
    }

    #[test]
    fn road_car_motion_keeps_world_position_after_multiple_origin_shifts() {
        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .init_resource::<crate::floating_origin::FloatingOrigin>()
            .add_systems(Update, update_road_cars);
        let car = app
            .world_mut()
            .spawn((
                Transform::IDENTITY,
                RoadCarMotion {
                    curve: None,
                    start: Vec3::new(4000.0, 3.0, -2000.0),
                    end: Vec3::new(4200.0, 3.0, -2000.0),
                    speed_mps: 0.0,
                    length_m: 4.0,
                    t: 0.5,
                    forward: true,
                },
            ))
            .id();
        for shift in [
            Vec3::ZERO,
            Vec3::new(3700.0, 0.0, -1800.0),
            Vec3::new(4050.0, 0.0, -2100.0),
        ] {
            app.world_mut()
                .resource_mut::<crate::floating_origin::FloatingOrigin>()
                .shift = shift;
            app.update();
            let tf = app.world().get::<Transform>(car).unwrap();
            assert_eq!(tf.translation + shift, Vec3::new(4100.0, 3.0, -2000.0));
        }
    }

    fn traffic_app() -> App {
        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .init_resource::<crate::floating_origin::FloatingOrigin>()
            .init_resource::<crate::crossing_visual::RoadCrossings>()
            .add_systems(Update, update_road_cars);
        app
    }
    fn car(app: &mut App, t: f32) -> Entity {
        app.world_mut()
            .spawn((
                Transform::default(),
                RoadCarMotion {
                    curve: Some(std::sync::Arc::new(
                        RoadCurve::new(vec![
                            Vec3::ZERO,
                            Vec3::new(100., 0., 0.),
                            Vec3::new(100., 0., 100.),
                        ])
                        .unwrap(),
                    )),
                    start: Vec3::ZERO,
                    end: Vec3::new(100., 0., 100.),
                    speed_mps: 20.,
                    length_m: 4.,
                    t,
                    forward: true,
                },
            ))
            .id()
    }
    fn tick(app: &mut App, dt: f64) {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f64(dt));
        app.update();
    }
    #[test]
    fn queues_follow_the_rdb_curve_wait_before_closed_crossings_then_clear_without_overlap() {
        let mut app = traffic_app();
        let lead = car(&mut app, 0.2);
        let tail = car(&mut app, 0.1);
        app.world_mut()
            .resource_mut::<crate::crossing_visual::RoadCrossings>()
            .stops = vec![(Vec3::new(100., 0., 20.), true)];
        for _ in 0..200 {
            tick(&mut app, 0.1);
        }
        let a = app.world().get::<RoadCarMotion>(lead).unwrap().t;
        let b = app.world().get::<RoadCarMotion>(tail).unwrap().t;
        assert!(
            (a - 0.54).abs() < 0.001,
            "front bumper stops 10 m before gate: {a}"
        );
        assert!((a - b) * 200. >= 5.99);
        let position = app.world().get::<Transform>(lead).unwrap().translation;
        assert!((position.x - 100.).abs() < 0.001 && position.z > 0.);
        app.world_mut()
            .resource_mut::<crate::crossing_visual::RoadCrossings>()
            .stops[0]
            .1 = false;
        for _ in 0..5 {
            tick(&mut app, 0.1);
        }
        assert!(app.world().get::<RoadCarMotion>(lead).unwrap().t > a);
        assert!(app.world().get::<RoadCarMotion>(tail).unwrap().t > b);
        let before = app.world().get::<RoadCarMotion>(lead).unwrap().t;
        tick(&mut app, 0.);
        assert_eq!(before, app.world().get::<RoadCarMotion>(lead).unwrap().t);
    }
    #[test]
    fn simultaneous_exit_cars_cannot_claim_the_same_entry() {
        let mut app = traffic_app();
        let a = car(&mut app, 0.99);
        let b = car(&mut app, 0.99);
        tick(&mut app, 0.1);
        let a = app.world().get::<RoadCarMotion>(a).unwrap().t;
        let b = app.world().get::<RoadCarMotion>(b).unwrap().t;
        assert!(
            (a - b).abs() > 0.9,
            "one car wraps while the other waits: {a}, {b}"
        );
    }
    #[test]
    fn rdb_spawner_follows_connected_vectors_and_keeps_the_surface_height_datum() {
        use openrailsrs_formats::{
            TrItem, TrItemKind, TrackDbFile, TrackDbNode, TrackNodeKind, TrackVectorGeometry,
            TrackVectorPoint,
        };
        let point = |x, z| TrackVectorPoint {
            tile_x: 0,
            tile_z: 0,
            x,
            y: 12.,
            z,
        };
        let vector = |id, pins, items, start, end| TrackDbNode {
            junction_shape_index: None,
            id,
            position: None,
            pin_refs: vec![],
            kind: TrackNodeKind::Vector {
                length_m: 100.,
                speed_limit_mps: 20.,
                pins,
                item_ids: items,
                sections: vec![],
                geometry: Some(TrackVectorGeometry { start, end }),
            },
        };
        let db = TrackDbFile {
            nodes: vec![
                vector(2, (1, 4), vec![10], point(0., 0.), point(100., 0.)),
                vector(4, (2, 5), vec![11], point(100., 0.), point(100., -100.)),
            ],
            items: vec![
                TrItem {
                    id: 10,
                    kind: TrItemKind::Other,
                    distance_m: 20.,
                    world: None,
                    speed_post_flags: None,
                    speed_post_angle_rad: None,
                },
                TrItem {
                    id: 11,
                    kind: TrItemKind::Other,
                    distance_m: 80.,
                    world: None,
                    speed_post_flags: None,
                    speed_post_angle_rad: None,
                },
            ],
        };
        let focus = RouteFocus {
            center: Vec3::new(50., 100., 50.),
            height_origin: 10.,
        };
        let curve = super::road_curve(
            &db,
            &[10, 11],
            &openrailsrs_formats::TSectionCatalog::default(),
            &focus,
        )
        .unwrap();
        assert!((curve.total - 160.).abs() < 0.01);
        assert!((curve.pose(0.75).0 - Vec3::new(50., 2., -10.)).length() < 0.01);
    }
    #[test]
    fn sampling_a_curved_road_uses_chainage_instead_of_the_endpoint_chord() {
        let curve = RoadCurve::new(vec![
            Vec3::ZERO,
            Vec3::new(100., 0., 0.),
            Vec3::new(100., 0., 100.),
        ])
        .unwrap();
        assert_eq!(curve.total, 200.);
        assert_eq!(curve.pose(0.75).0, Vec3::new(100., 0., 50.));
        assert_eq!(curve.project(Vec3::new(102., 0., 50.)), Some(0.75));
        assert_eq!(curve.project(Vec3::new(80., 0., 50.)), None);
    }
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
    #[ignore = "requires the original author's installed Chiltern v4"]
    fn native_chiltern_rdb_vectors_reach_both_authored_spawner_items() {
        let route = std::env::var_os("CHILTERN_ROUTE")
            .map(PathBuf::from)
            .expect("CHILTERN_ROUTE original install required");
        let rdb = openrailsrs_formats::TrackDbFile::from_path(route.join("Chiltern.rdb")).unwrap();
        let catalog = openrailsrs_formats::TSectionCatalog::load_for_route(&route).unwrap();
        let (a, b) = super::spawner_rdb_endpoints(&rdb, &[753, 754]).unwrap();
        let focus = RouteFocus {
            center: (a + b) * 0.5,
            height_origin: a.y,
        };
        let curve = super::road_curve(&rdb, &[753, 754], &catalog, &focus)
            .expect("connected authored RDB vector geometry");
        assert!(curve.pose(0.).0.distance(focus.to_render_surface(a)) < 3.0);
        assert!(curve.pose(1.).0.distance(focus.to_render_surface(b)) < 3.0);
        assert!(curve.total >= a.distance(b) - 3.0);
        for step in 0..=100 {
            assert!(curve.pose(step as f32 / 100.).0.is_finite());
        }
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

    /// Mini1 (carspawn.dat) has body + glass; a single combined material paints everything white.
    #[test]
    fn chiltern_mini1_road_car_has_distinct_part_materials() {
        let Some(route) = chiltern_route() else {
            return;
        };
        let shape = route.join("SHAPES/MINI1.S");
        if !shape.is_file() || !route.join("TEXTURES/MiniTexture1.ace").is_file() {
            return;
        }

        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mut texture_cache = HashMap::new();
        let tex_dirs = texture_search_dirs_for_shape(&shape, &route);
        let dir_refs: Vec<&std::path::Path> = tex_dirs.iter().map(PathBuf::as_path).collect();
        let asset = load_shape_render_asset_from_path(
            &shape,
            &dir_refs,
            Some(80.0),
            &mut meshes,
            &mut images,
            &mut materials,
            &mut texture_cache,
            COLOR_CAR_FALLBACK,
            false,
        )
        .expect("MINI1.S load");
        assert!(
            asset.parts.len() >= 2,
            "expected multi-part Mini1, got {}",
            asset.parts.len()
        );
        assert!(
            asset.parts.iter().any(|p| p.has_texture),
            "expected at least one textured part"
        );
        let first = asset.parts[0].material.id();
        assert!(
            asset.parts.iter().any(|p| p.material.id() != first),
            "parts must not share a single material handle (combined-mesh bug)"
        );
    }

    /// Escort van ACE is pale cream; lit station wash / unlit×4 both read as solid white.
    #[test]
    fn chiltern_tbescvan_road_car_materials_are_unlit_albedo() {
        let Some(route) = chiltern_route() else {
            return;
        };
        let shape = route.join("SHAPES/TBescvan.s");
        if !shape.is_file() || !route.join("TEXTURES/Escortvan.ace").is_file() {
            return;
        }
        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mut texture_cache = HashMap::new();
        let tex_dirs = texture_search_dirs_for_shape(&shape, &route);
        let dir_refs: Vec<&std::path::Path> = tex_dirs.iter().map(PathBuf::as_path).collect();
        let asset = load_shape_render_asset_from_path(
            &shape,
            &dir_refs,
            Some(80.0),
            &mut meshes,
            &mut images,
            &mut materials,
            &mut texture_cache,
            COLOR_CAR_FALLBACK,
            false,
        )
        .expect("TBescvan.s load");
        apply_road_car_albedo_materials(&asset, &mut materials);
        let mat = materials.get(&asset.parts[0].material).expect("material");
        assert!(mat.unlit, "road cars must sample ACE unlit");
        assert!(mat.base_color_texture.is_some());
        assert!(mat.emissive_texture.is_some(), "ACE also on emissive");
        let c = mat.base_color.to_linear();
        assert!(
            (c.red - 1.0).abs() < 0.01 && (c.green - 1.0).abs() < 0.01,
            "albedo tint must be ×1 (not scenery×4), got {:?}",
            mat.base_color
        );
        assert!((mat.emissive.red - 1.0).abs() < 0.01);
    }
}
