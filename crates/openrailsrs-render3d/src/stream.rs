//! Streaming de tiles alrededor de la camara + marcador `TileContent`.
//!
//! Desired/diff de ventana vía [`openrailsrs_bevy_scenery::stream`] (#113);
//! spawn/despawn/eviction GPU siguen locales a render3d.

use std::collections::HashSet;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use openrailsrs_bevy_scenery::stream::{StreamWindowPolicy, TileBound, TileCoord};

use crate::objects::ObjectMarker;
use crate::or_scenery_material::OrSceneryMaterial;
use crate::or_terrain_material::OrTerrainMaterial;
use crate::world_spawn::{
    AssetIndex, ObjectSpawnCtx, TerrainSpawnCtx, TextureLoadStats, spawn_objects_batch,
    spawn_terrain_patches, spawn_tile_track,
};
use crate::{MstsRootDir, RouteDir, TileEntry};

fn log_stream_eviction(shapes: usize, terrain_mats: usize, terrain_tex: usize, meshes: usize) {
    if shapes == 0 && terrain_mats == 0 && terrain_tex == 0 && meshes == 0 {
        return;
    }
    bevy::log::info!(
        "openrailsrs-render3d: stream eviction — {shapes} shape(s), {meshes} terrain mesh(es), {terrain_mats} terrain mat(s), {terrain_tex} terrain tex(s)"
    );
}

pub use openrailsrs_bevy_scenery::stream::TILE_SIZE_M;

/// Tile y su contenido 3D spawneado (para despawn) — alias del `TileBound` compartido (#113).
pub type TileContent = TileBound;

#[derive(Resource, Clone)]
pub struct TileStreamConfig {
    pub center_tile: (i32, i32),
    /// Radio maximo alrededor de la camara (tiles Chebyshev), desde `--radius`.
    pub stream_radius: u32,
    /// Tiles cargados al arrancar (igual que `stream_radius`: todo el grid visible).
    pub initial_radius: u32,
    /// Extra Chebyshev tiles beyond [`Self::stream_radius`] before unload (#113).
    pub unload_hysteresis: u32,
}

impl TileStreamConfig {
    pub fn new(center: (i32, i32), stream_radius: u32) -> Self {
        let initial_radius = stream_radius;
        Self {
            center_tile: center,
            stream_radius,
            initial_radius,
            unload_hysteresis: 0,
        }
    }

    pub fn streaming_enabled(&self) -> bool {
        self.stream_radius > self.initial_radius
    }

    /// Shared Chebyshev load/unload policy for the live stream window.
    pub fn stream_policy(&self) -> StreamWindowPolicy {
        StreamWindowPolicy::chebyshev(self.stream_radius, self.unload_hysteresis)
    }

    pub fn tile_in_stream_radius(&self, cam_tile: (i32, i32), tile: (i32, i32)) -> bool {
        self.stream_policy()
            .should_load(TileCoord::from(cam_tile), TileCoord::from(tile))
    }

    pub fn tile_in_initial_radius(&self, tile: (i32, i32)) -> bool {
        StreamWindowPolicy::chebyshev(self.initial_radius, 0)
            .should_load(TileCoord::from(self.center_tile), TileCoord::from(tile))
    }
}

#[derive(Resource, Clone)]
pub struct TileCatalog {
    pub entries: Vec<TileEntry>,
}

/// Cached [`TileHeightIndex`] for the stream catalog (#63).
///
/// Rebuilt only when the set of catalog tile coords or scene center changes.
#[derive(Resource, Default)]
pub struct StreamHeightIndexCache {
    index: Option<crate::tdb_track::TileHeightIndex>,
    fingerprint: Vec<(i32, i32)>,
    center: (i32, i32),
    pub builds: u32,
}

impl StreamHeightIndexCache {
    pub fn get_or_build(
        &mut self,
        catalog: &TileCatalog,
        center: (i32, i32),
    ) -> &crate::tdb_track::TileHeightIndex {
        let mut keys: Vec<_> = catalog
            .entries
            .iter()
            .map(|e| (e.geometry.tile_x, e.geometry.tile_z))
            .collect();
        keys.sort_unstable();
        let reuse = self.index.is_some() && self.fingerprint == keys && self.center == center;
        if !reuse {
            self.builds += 1;
            self.fingerprint = keys;
            self.center = center;
            self.index = Some(crate::tdb_track::TileHeightIndex::from_tile_heights(
                catalog
                    .entries
                    .iter()
                    .map(|e| (e.geometry.tile_x, e.geometry.tile_z, &e.geometry.height)),
                center,
            ));
        }
        self.index
            .as_ref()
            .expect("StreamHeightIndexCache index after get_or_build")
    }
}

#[derive(Resource, Clone)]
pub struct StreamWorldAssets {
    pub index: AssetIndex,
    pub terrain_ctx: TerrainSpawnCtx,
    pub obj_ctx: ObjectSpawnCtx,
    pub materials_lit: bool,
}

#[derive(Resource, Default)]
pub struct TileStreamState {
    pub loaded: HashSet<(i32, i32)>,
    pub last_camera_tile: Option<(i32, i32)>,
}

#[derive(Resource, Clone)]
pub struct SavedTerrainCtx(pub TerrainSpawnCtx);

/// Filtra entradas al radio inicial de carga (evita bloquear en grids grandes).
pub fn catalog_entries_for_initial_load(
    catalog: &TileCatalog,
    config: &TileStreamConfig,
) -> Vec<TileEntry> {
    catalog
        .entries
        .iter()
        .filter(|e| config.tile_in_initial_radius((e.geometry.tile_x, e.geometry.tile_z)))
        .cloned()
        .collect()
}

pub fn initial_loaded_tiles(
    config: &TileStreamConfig,
    entries: &[(i32, i32)],
) -> HashSet<(i32, i32)> {
    entries
        .iter()
        .filter(|(x, z)| config.tile_in_initial_radius((*x, *z)))
        .copied()
        .collect()
}

pub fn camera_tile(config: &TileStreamConfig, cam: &Transform) -> (i32, i32) {
    let dx = (cam.translation.x / TILE_SIZE_M).round() as i32;
    let dz = (cam.translation.z / TILE_SIZE_M).round() as i32;
    // Scene Z crece hacia el norte (render Z = -msts Z); tile_z decrece hacia el norte.
    (config.center_tile.0 + dx, config.center_tile.1 - dz)
}

fn find_catalog_entry(catalog: &TileCatalog, tile_x: i32, tile_z: i32) -> Option<&TileEntry> {
    catalog
        .entries
        .iter()
        .find(|e| e.geometry.tile_x == tile_x && e.geometry.tile_z == tile_z)
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_tile_entry(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    or_materials: &mut Assets<OrSceneryMaterial>,
    or_terrain_materials: &mut Assets<OrTerrainMaterial>,
    images: &mut Assets<Image>,
    assets: &mut StreamWorldAssets,
    route: &RouteDir,
    msts_root: &MstsRootDir,
    entry: &TileEntry,
    texture_env: &crate::textures::TextureEnvironment,
    viewer_pos: Vec3,
    tdb_track: Option<&crate::TdbTrackResource>,
    stream_config: &TileStreamConfig,
    catalog: &TileCatalog,
    height_cache: &mut StreamHeightIndexCache,
) {
    let tile_x = entry.geometry.tile_x;
    let tile_z = entry.geometry.tile_z;
    let offset = entry.world_offset;
    let mut tex_stats = TextureLoadStats::default();

    spawn_terrain_patches(
        commands,
        meshes,
        &mut assets.terrain_ctx,
        or_terrain_materials,
        images,
        &route.0,
        &entry.geometry,
        0,
        entry.geometry.patches.len(),
        offset,
        tile_x,
        tile_z,
    );

    if !crate::objects::tile_suppresses_tdb_ribbon(&entry.objects) {
        let center = stream_config.center_tile;
        let shaped = tdb_track
            .map(|tdb| {
                crate::tdb_track::collect_tdb_shaped_chords(
                    &tdb.ctx,
                    center.0,
                    center.1,
                    tdb.grid_radius,
                )
            })
            .unwrap_or_default();
        let height_index = height_cache.get_or_build(catalog, center);
        spawn_tile_track(
            commands,
            meshes,
            materials,
            or_materials,
            images,
            Some(&assets.index),
            Some(&mut assets.obj_ctx),
            &route.0,
            &msts_root.0,
            tdb_track.map(|r| &r.ctx),
            &shaped,
            &entry.track,
            &entry.objects,
            center,
            height_index,
            offset,
            assets.materials_lit,
            tile_x,
            tile_z,
            &mut tex_stats,
            texture_env,
            viewer_pos,
            None,
        );
    }

    let filtered: Vec<ObjectMarker> = entry
        .objects
        .iter()
        .filter(|o| crate::objects::object_wants_shape_mesh(o))
        .cloned()
        .collect();
    if !filtered.is_empty() {
        spawn_objects_batch(
            commands,
            meshes,
            materials,
            or_materials,
            images,
            &assets.index,
            &mut assets.obj_ctx,
            &route.0,
            &msts_root.0,
            &filtered,
            &mut tex_stats,
            offset,
            texture_env,
            viewer_pos,
            tile_x,
            tile_z,
            assets.materials_lit,
            None,
            tdb_track.map(|t| &t.ctx),
        );
    }

    let trackobj = crate::world_spawn::spawn_trackobj_procedural_for_objects(
        commands,
        meshes,
        materials,
        &entry.objects,
        offset,
        &assets.index.tsection,
        tdb_track.map(|t| &t.ctx),
        &assets.index,
        &route.0,
        &msts_root.0,
        assets.materials_lit,
        tile_x,
        tile_z,
        None,
    );
    let _ = trackobj;

    let (forests, waters) = crate::scenery::spawn_tile_scenery(
        commands,
        meshes,
        materials,
        images,
        &assets.index,
        &mut assets.obj_ctx,
        &route.0,
        &msts_root.0,
        &entry.objects,
        &entry.geometry.height,
        tile_x,
        tile_z,
        offset,
        &mut tex_stats,
        texture_env,
        assets.materials_lit,
    );
    let _ = (forests, waters);

    let dyntrack = crate::dyntrack::spawn_tile_dyntrack(
        commands,
        meshes,
        materials,
        &entry.objects,
        offset,
        assets.materials_lit,
        tile_x,
        tile_z,
    );
    let _ = dyntrack;

    let transfers = crate::transfer::spawn_tile_transfers(
        commands,
        meshes,
        materials,
        images,
        &assets.index,
        &route.0,
        &msts_root.0,
        &entry.objects,
        &entry.geometry.height,
        tile_x,
        tile_z,
        offset,
        &mut tex_stats,
        texture_env,
        assets.materials_lit,
    );
    let _ = transfers;
}

pub fn despawn_tile(
    commands: &mut Commands,
    tile_x: i32,
    tile_z: i32,
    query: &Query<(Entity, &TileContent)>,
) {
    for (entity, content) in query.iter() {
        if content.tile_x == tile_x && content.tile_z == tile_z {
            commands.entity(entity).despawn();
        }
    }
}

type TileEntityQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static TileContent,
        Option<&'static Mesh3d>,
        Option<&'static MeshMaterial3d<OrTerrainMaterial>>,
    ),
>;

/// Bundle stream GPU asset stores so `tile_stream_system` stays under Bevy's param limit.
#[derive(SystemParam)]
pub struct StreamGpuAssets<'w> {
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    or_materials: ResMut<'w, Assets<OrSceneryMaterial>>,
    or_terrain_materials: ResMut<'w, Assets<OrTerrainMaterial>>,
    images: ResMut<'w, Assets<Image>>,
}

#[allow(clippy::too_many_arguments)]
pub fn tile_stream_system(
    mut commands: Commands,
    config: Res<TileStreamConfig>,
    catalog: Res<TileCatalog>,
    assets: Option<ResMut<StreamWorldAssets>>,
    state: Option<ResMut<TileStreamState>>,
    mut height_cache: ResMut<StreamHeightIndexCache>,
    mut bundle_handles: ResMut<crate::tile_bundle::TileBundleHandles>,
    route: Res<RouteDir>,
    msts_root: Res<MstsRootDir>,
    texture_env: Res<crate::textures::TextureEnvironment>,
    tdb_track: Option<Res<crate::TdbTrackResource>>,
    camera: Query<&Transform, With<Camera3d>>,
    tile_entities: TileEntityQuery,
    mut gpu: StreamGpuAssets,
) {
    if !config.streaming_enabled() {
        return;
    }
    let Some(mut assets) = assets else {
        return;
    };
    let Some(mut state) = state else {
        return;
    };
    let Ok(cam_tf) = camera.single() else {
        return;
    };
    let cam_tile = camera_tile(&config, cam_tf);
    if state.last_camera_tile == Some(cam_tile) {
        return;
    }
    state.last_camera_tile = Some(cam_tile);

    let policy = config.stream_policy();
    let center = TileCoord::from(cam_tile);
    let loaded_coords: HashSet<TileCoord> =
        state.loaded.iter().copied().map(TileCoord::from).collect();
    let candidates = catalog
        .entries
        .iter()
        .map(|e| TileCoord::new(e.geometry.tile_x, e.geometry.tile_z));
    let stream_diff = policy.diff(center, &loaded_coords, candidates);

    let mut unloading = HashSet::new();
    for tile in &stream_diff.to_unload {
        let key = (tile.x, tile.z);
        unloading.insert(key);
        state.loaded.remove(&key);
    }

    if !unloading.is_empty() {
        // Exact tile unload for AssetServer-backed bundles (#53).
        bundle_handles.release_all(unloading.iter());
        // Despawn is deferred: treat unloading-tile entities as already gone (#51).
        let mut live_shape_meshes = HashSet::new();
        let mut live_terrain_mats = HashSet::new();
        let mut released_terrain_meshes = Vec::new();
        let mut to_despawn = Vec::new();
        for (entity, content, mesh3d, terrain_mat) in tile_entities.iter() {
            let key = (content.tile_x, content.tile_z);
            if unloading.contains(&key) {
                to_despawn.push(entity);
                if let (Some(mesh), Some(_)) = (mesh3d, terrain_mat) {
                    // Terrain patches are not shape-cached; free their meshes now.
                    released_terrain_meshes.push(mesh.id());
                }
                continue;
            }
            if let Some(mesh) = mesh3d {
                live_shape_meshes.insert(mesh.id());
            }
            if let Some(mat) = terrain_mat {
                live_terrain_mats.insert(mat.id());
            }
        }
        for entity in to_despawn {
            commands.entity(entity).despawn();
        }
        for key in &unloading {
            assets.obj_ctx.release_tile_shapes(TileCoord::from(*key));
        }
        let mut meshes_freed = 0usize;
        for id in released_terrain_meshes {
            if gpu.meshes.remove(id).is_some() {
                meshes_freed += 1;
            }
        }
        let shapes = assets.obj_ctx.evict_unreferenced_shapes(
            &live_shape_meshes,
            &mut gpu.meshes,
            &mut gpu.materials,
            &mut gpu.or_materials,
        );
        let (terrain_mats, terrain_tex) = assets.terrain_ctx.evict_unreferenced(
            &live_terrain_mats,
            &mut gpu.images,
            &mut gpu.or_terrain_materials,
        );
        log_stream_eviction(shapes, terrain_mats, terrain_tex, meshes_freed);
    }

    // Varios tiles por frame: el catálogo ya está en memoria; solo falta spawn 3D.
    const TILES_PER_FRAME: usize = 4;
    let mut spawned = 0usize;
    for tile in &stream_diff.to_load {
        let key = (tile.x, tile.z);
        if state.loaded.contains(&key) {
            continue;
        }
        let Some(entry) = find_catalog_entry(&catalog, key.0, key.1).cloned() else {
            continue;
        };
        spawn_tile_entry(
            &mut commands,
            &mut gpu.meshes,
            &mut gpu.materials,
            &mut gpu.or_materials,
            &mut gpu.or_terrain_materials,
            &mut gpu.images,
            &mut assets,
            &route,
            &msts_root,
            &entry,
            &texture_env,
            cam_tf.translation,
            tdb_track.as_deref(),
            &config,
            &catalog,
            &mut height_cache,
        );
        state.loaded.insert(key);
        spawned += 1;
        if spawned >= TILES_PER_FRAME {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::load_tile_geometry;
    use std::path::PathBuf;

    #[test]
    fn stream_height_index_builds_once_per_catalog() {
        let route = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        let tiles = [(-6082, 14925), (-6081, 14925), (-6082, 14924)];
        let mut entries = Vec::new();
        for (tx, tz) in tiles {
            let Ok(geom) = load_tile_geometry(&route, tx, tz) else {
                eprintln!("skip: Chiltern tile ({tx},{tz}) missing");
                return;
            };
            entries.push(crate::TileEntry {
                geometry: geom,
                world_offset: Vec3::ZERO,
                track: crate::track::TrackRibbon::default(),
                objects: Vec::new(),
            });
        }
        let catalog = TileCatalog { entries };
        let center = (-6082, 14925);
        let mut cache = StreamHeightIndexCache::default();
        let y0 = cache.get_or_build(&catalog, center).scene_base_y();
        let y1 = cache.get_or_build(&catalog, center).scene_base_y();
        let y2 = cache.get_or_build(&catalog, center).scene_base_y();
        assert_eq!(
            cache.builds, 1,
            "same catalog must build TileHeightIndex once"
        );
        assert_eq!(y0, y1);
        assert_eq!(y1, y2);

        // Different center → rebuild (scene_base_y may change).
        let _ = cache.get_or_build(&catalog, (-6081, 14925));
        assert_eq!(cache.builds, 2);
    }

    #[test]
    fn initial_radius_matches_stream_radius() {
        let cfg = TileStreamConfig::new((0, 0), 2);
        assert_eq!(cfg.initial_radius, 2);
        assert!(!cfg.streaming_enabled());
    }

    #[test]
    fn camera_tile_follows_world_offset() {
        let cfg = TileStreamConfig::new((10, 20), 1);
        let tf = Transform::from_xyz(2048.0, 0.0, 0.0);
        assert_eq!(camera_tile(&cfg, &tf), (11, 20));
        // Scene +Z = norte → tile_z decrece.
        let north = Transform::from_xyz(0.0, 0.0, 2048.0);
        assert_eq!(camera_tile(&cfg, &north), (10, 19));
        let south = Transform::from_xyz(0.0, 0.0, -2048.0);
        assert_eq!(camera_tile(&cfg, &south), (10, 21));
    }
}
