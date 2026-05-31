//! MSTS terrain tiles: heightfield meshes from `.y` + `_Y.RAW` (order 8 / issue #8, PR2 textures).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use openrailsrs_formats::{
    ElevationGrid, FeatureGrid, TerrainFile, TerrainMeshData, build_patch_mesh_data_ex,
    build_tile_mesh_data, msts_display_tile_x_from_internal,
    msts_internal_tile_x_from_world_display, msts_tile_name_from_xz, msts_tile_world_origin,
    parse_tile_xz_from_filename, read_f_raw, read_y_raw,
};

use crate::shapes::RouteAssets;
use crate::terrain_assets::terrain_material_textures;
use crate::terrain_material::TerrainMaterial;
use crate::track::TrackScene;
use crate::world::MSTS_TILE_SIZE_M;
use crate::{log_step, viewer_log};

const COLOR_TERRAIN_FALLBACK: Color = Color::srgb(0.28, 0.42, 0.22);

/// World-space offset for a textured patch inside a tile.
#[inline]
pub(crate) fn terrain_patch_offset_in_tile(px: u32, pz: u32) -> Vec3 {
    Vec3::new(px as f32 * 128.0, 0.0, pz as f32 * 128.0)
}

#[derive(Clone)]
struct TileElevation {
    grid: ElevationGrid,
    sample_size: f64,
    features: Option<FeatureGrid>,
}

/// Cached elevation grids for runtime height sampling (trains, forests).
#[derive(Resource, Clone, Default)]
pub struct TerrainElevation {
    tiles: HashMap<(i32, i32), TileElevation>,
}

impl TerrainElevation {
    /// Load `_Y.RAW` grids for terrain tiles under the route (optionally within `radius_m` of `center`).
    pub fn load_from_route_dir(route_dir: &Path) -> Self {
        Self::load_from_route_dir_near(route_dir, None, f32::MAX)
    }

    pub fn load_from_route_dir_near(route_dir: &Path, center: Option<Vec3>, radius_m: f32) -> Self {
        let mut tiles = HashMap::new();
        let mut entries = discover_terrain_tile_entries(route_dir, center, radius_m);
        entries.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        for (display_x, display_z, path) in entries {
            let Ok(tile) = TerrainFile::from_path_with_coords(&path, display_x, display_z) else {
                continue;
            };
            let raw_path = tile.y_raw_path(&path);
            let Ok(grid) = read_y_raw(&raw_path, &tile.samples) else {
                continue;
            };
            let features = if tile.samples.f_buffer_file.trim().is_empty() {
                None
            } else {
                read_f_raw(&tile.f_raw_path(&path), &tile.samples).ok()
            };
            tiles.insert(
                (tile.tile_x, tile.tile_z),
                TileElevation {
                    grid,
                    sample_size: tile.samples.sample_size,
                    features,
                },
            );
        }
        Self { tiles }
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    fn sample_hidden(&self, display_x: i32, display_z: i32, x: f32, z: f32) -> bool {
        let internal_x = msts_internal_tile_x_from_world_display(display_x);
        let Some(tile) = self.tiles.get(&(internal_x, display_z)) else {
            return false;
        };
        let Some(features) = tile.features.as_ref() else {
            return false;
        };
        let (ox, oz) = msts_tile_world_origin(display_x, display_z);
        let lx = x - ox;
        let lz = z - oz;
        let ux = (lx / tile.sample_size as f32).round() as usize;
        let uz = (lz / tile.sample_size as f32).round() as usize;
        features.is_vertex_hidden(ux, uz)
    }

    /// World-space elevation (metres) at `(x, z)`; `None` if no tile covers the point or vertex is hidden.
    pub fn sample_world_y(&self, x: f32, z: f32) -> Option<f32> {
        let tile = MSTS_TILE_SIZE_M as f32;
        let display_x = (x / tile).floor() as i32;
        let display_z = (z / tile).floor() as i32;
        if self.sample_hidden(display_x, display_z, x, z) {
            return None;
        }
        let internal_x = msts_internal_tile_x_from_world_display(display_x);
        let tile = self.tiles.get(&(internal_x, display_z))?;
        let (ox, oz) = msts_tile_world_origin(display_x, display_z);
        let lx = x - ox;
        let lz = z - oz;
        Some(
            tile.grid
                .sample_bilinear(lx as f64, lz as f64, tile.sample_size),
        )
    }
}

/// Scenery anchor height: terrain sample plus a small clearance, else tile-local Y → MSL.
pub fn scenery_ground_y(
    terrain: Option<&TerrainElevation>,
    x: f32,
    z: f32,
    scene: &TrackScene,
    fallback_scene_y: f32,
    focus: &crate::world::RouteFocus,
) -> f32 {
    let lift = scene.bounds.edge_radius().max(1.0) * 0.04;
    terrain
        .and_then(|t| t.sample_world_y(x, z))
        .map(|h| h + lift)
        .unwrap_or_else(|| focus.scenery_y_to_msl(fallback_scene_y))
}

/// Train / marker height: terrain sample plus a small rail clearance, or graph lift fallback.
pub fn ground_y_at(terrain: Option<&TerrainElevation>, x: f32, z: f32, scene: &TrackScene) -> f32 {
    let rail_offset = scene.bounds.edge_radius() * 0.35;
    terrain
        .and_then(|t| t.sample_world_y(x, z))
        .map(|h| h + rail_offset)
        .unwrap_or(scene.bounds.node_radius() + scene.bounds.edge_radius() * 1.5)
}

/// One loaded terrain tile ready for GPU spawn.
#[derive(Clone, Debug)]
pub struct TerrainTile {
    pub tile_x: i32,
    pub tile_z: i32,
    pub translation: Vec3,
    pub path: PathBuf,
}

/// Terrain tiles discovered under a route's `TERRAIN/` folder.
#[derive(Resource, Clone, Default)]
pub struct TerrainScene {
    pub tiles_loaded: usize,
    pub tiles: Vec<TerrainTile>,
}

impl TerrainScene {
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }
}

fn mesh_from_terrain_data(data: &TerrainMeshData, height_origin: f32) -> Mesh {
    let positions: Vec<[f32; 3]> = data
        .positions
        .iter()
        .map(|p| [p[0], p[1] - height_origin, p[2]])
        .collect();
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, data.normals.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, data.uvs.clone());
    mesh.insert_indices(Indices::U32(data.indices.clone()));
    mesh
}

/// Scan terrain folders and parse tile metadata (see [`discover_terrain_files`]).
pub fn load_terrain_from_route_dir(route_dir: &Path) -> TerrainScene {
    load_terrain_from_route_dir_near(route_dir, None, f32::MAX)
}

pub fn load_terrain_from_route_dir_near(
    route_dir: &Path,
    center: Option<Vec3>,
    radius_m: f32,
) -> TerrainScene {
    let mut entries = discover_terrain_tile_entries(route_dir, center, radius_m);
    entries.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

    let mut scene = TerrainScene::default();
    let mut skip_count = 0usize;
    for (tile_x, tile_z, path) in entries {
        match TerrainFile::from_path_with_coords(&path, tile_x, tile_z) {
            Ok(tile) => {
                scene.tiles_loaded += 1;
                let display_x = msts_display_tile_x_from_internal(tile.tile_x);
                let (wx, wz) = msts_tile_world_origin(display_x, tile.tile_z);
                scene.tiles.push(TerrainTile {
                    tile_x: tile.tile_x,
                    tile_z: tile.tile_z,
                    translation: Vec3::new(wx, 0.0, wz),
                    path,
                });
            }
            Err(err) => {
                skip_count += 1;
                if skip_count == 1 {
                    viewer_log!(
                        "openrailsrs-viewer3d: skip terrain {} ({err})",
                        path.display()
                    );
                }
            }
        }
    }
    if skip_count > 1 {
        viewer_log!("openrailsrs-viewer3d: skipped {skip_count} terrain tile(s)");
    }
    if scene.tiles.is_empty() {
        let tiles_dir = route_dir.join("TILES");
        if tiles_dir.is_dir()
            && std::fs::read_dir(&tiles_dir)
                .ok()
                .is_some_and(|rd| rd.flatten().any(|e| e.path().extension().is_some()))
        {
            viewer_log!(
                "openrailsrs-viewer3d: no terrain tiles near route focus (check TILES/ + *_y.raw)"
            );
        }
    }
    scene
}

fn tile_center_distance_m(display_x: i32, display_z: i32, center: Vec3) -> f32 {
    let tile = MSTS_TILE_SIZE_M as f32;
    let half = tile * 0.5;
    let (ox, oz) = msts_tile_world_origin(display_x, display_z);
    let tcx = ox + half;
    let tcz = oz + half;
    Vec2::new(tcx - center.x, tcz - center.z).length()
}

/// `(display_tile_x, display_tile_z, path)` for each terrain tile to load.
pub fn discover_terrain_tile_entries(
    route_dir: &Path,
    center: Option<Vec3>,
    radius_m: f32,
) -> Vec<(i32, i32, PathBuf)> {
    if uses_hash_tile_names(route_dir) {
        return discover_hash_terrain_tiles(route_dir, center, radius_m);
    }
    discover_terrain_files(route_dir)
        .into_iter()
        .map(|path| {
            let (x, z) = parse_tile_xz_from_filename(&path).unwrap_or((0, 0));
            (x, z, path)
        })
        .collect()
}

fn uses_hash_tile_names(route_dir: &Path) -> bool {
    let dir = route_dir.join("TILES");
    if !dir.is_dir() {
        return false;
    }
    std::fs::read_dir(&dir).ok().is_some_and(|rd| {
        rd.flatten().any(|e| {
            e.path()
                .file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|stem| stem.starts_with('-') && stem.len() >= 9)
        })
    })
}

fn discover_hash_terrain_tiles(
    route_dir: &Path,
    center: Option<Vec3>,
    radius_m: f32,
) -> Vec<(i32, i32, PathBuf)> {
    let tiles_dir = route_dir.join("TILES");
    let world_dir = route_dir.join("WORLD");
    let mut out = Vec::new();

    if let Some(c) = center {
        let tile = MSTS_TILE_SIZE_M as f32;
        let center_dx = (c.x / tile).floor() as i32;
        let center_dz = (c.z / tile).floor() as i32;
        let radius_tiles = (radius_m / tile).ceil() as i32 + 1;
        for dtx in -radius_tiles..=radius_tiles {
            for dtz in -radius_tiles..=radius_tiles {
                let display_x = center_dx + dtx;
                let display_z = center_dz + dtz;
                if tile_center_distance_m(display_x, display_z, c) > radius_m + tile {
                    continue;
                }
                push_hash_tile(&mut out, &tiles_dir, display_x, display_z);
            }
        }
        if out.is_empty() {
            push_hash_tiles_from_world_near(&mut out, &world_dir, &tiles_dir, c, radius_m);
        }
        return out;
    }

    if world_dir.is_dir() {
        for entry in std::fs::read_dir(&world_dir)
            .into_iter()
            .flatten()
            .flatten()
        {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("w") {
                continue;
            }
            let Some((display_x, display_z)) = parse_world_w_tile_display_xz(&path) else {
                continue;
            };
            push_hash_tile(&mut out, &tiles_dir, display_x, display_z);
        }
    }
    out
}

/// Display tile coords from `WORLD/w-006074+014924.w` (also accepts `w-001000-001000`).
fn parse_world_w_tile_display_xz(path: &Path) -> Option<(i32, i32)> {
    let stem = path.file_stem()?.to_str()?;
    let rest = stem.strip_prefix('w')?;
    let coords = rest.trim_start_matches('-');
    if let Some((x, z)) = coords.split_once('+') {
        return Some((x.parse().ok()?, z.parse().ok()?));
    }
    let mut parts = rest.split(['-', '_']).filter(|p| !p.is_empty());
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

fn push_hash_tile(
    out: &mut Vec<(i32, i32, PathBuf)>,
    tiles_dir: &Path,
    display_x: i32,
    display_z: i32,
) {
    let internal_x = msts_internal_tile_x_from_world_display(display_x);
    let hash = msts_tile_name_from_xz(internal_x, display_z).to_ascii_lowercase();
    let path = tiles_dir.join(format!("{hash}.t"));
    if path.is_file() {
        out.push((internal_x, display_z, path));
    }
}

fn push_hash_tiles_from_world_near(
    out: &mut Vec<(i32, i32, PathBuf)>,
    world_dir: &Path,
    tiles_dir: &Path,
    center: Vec3,
    radius_m: f32,
) {
    if !world_dir.is_dir() {
        return;
    }
    let tile = MSTS_TILE_SIZE_M as f32;
    for entry in std::fs::read_dir(world_dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("w") {
            continue;
        }
        let Some((display_x, display_z)) = parse_world_w_tile_display_xz(&path) else {
            continue;
        };
        if tile_center_distance_m(display_x, display_z, center) > radius_m + tile {
            continue;
        }
        push_hash_tile(out, tiles_dir, display_x, display_z);
    }
}

/// Scan `TERRAIN/` (`.y`) and `TILES/` (`.t`) under the route (legacy `+000000+000000` names).
pub fn discover_terrain_files(route_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for subdir in ["TERRAIN", "terrain", "TILES", "tiles"] {
        let dir = route_dir.join(subdir);
        if !dir.is_dir() {
            continue;
        }
        let Ok(read_dir) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("y") || e.eq_ignore_ascii_case("t"))
            {
                out.push(path);
            }
        }
    }
    out
}

fn fallback_terrain_image(images: &mut Assets<Image>) -> Handle<Image> {
    let mut img = Image::new_fill(
        Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[70, 107, 56, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..default()
    });
    images.add(img)
}

#[allow(clippy::too_many_arguments)]
fn spawn_textured_patches(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<TerrainMaterial>,
    images: &mut Assets<Image>,
    route_dir: &Path,
    tile: &TerrainFile,
    grid: &ElevationGrid,
    features: Option<&FeatureGrid>,
    texture_cache: &mut HashMap<String, Handle<Image>>,
    fallback_tex: &Handle<Image>,
    render_origin: Vec3,
    height_origin: f32,
) -> (usize, usize) {
    let patch_set = match tile.primary_patch_set() {
        Some(set) => set,
        None => return (0, 0),
    };
    let display_x = msts_display_tile_x_from_internal(tile.tile_x);
    let (wx, wz) = msts_tile_world_origin(display_x, tile.tile_z);
    let tile_origin = Vec3::new(wx - render_origin.x, 0.0, wz - render_origin.z);
    let mut spawned = 0usize;
    let mut holed = 0usize;

    for pz in 0..patch_set.npatches {
        for px in 0..patch_set.npatches {
            let Some(patch) = patch_set.patch_at(px, pz) else {
                continue;
            };
            if !patch.drawing_enabled() {
                continue;
            }
            let shader = tile
                .shaders
                .get(patch.shader_index as usize)
                .or_else(|| tile.shaders.first());
            let Some(shader) = shader else {
                continue;
            };

            let mesh_data = build_patch_mesh_data_ex(
                grid,
                tile.samples.sample_size,
                px,
                pz,
                Some(patch),
                features,
                true,
            );
            if features.is_some_and(|f| f.patch_has_hidden_vertices(px, pz)) {
                holed += 1;
            }

            let (base, overlay, overlay_scale) = terrain_material_textures(
                route_dir,
                images,
                texture_cache,
                shader,
                fallback_tex.clone(),
            );
            let material = materials.add(TerrainMaterial {
                overlay_scale,
                base_texture: base,
                overlay_texture: overlay,
            });

            let patch_offset = terrain_patch_offset_in_tile(px, pz);
            commands.spawn((
                Mesh3d(meshes.add(mesh_from_terrain_data(&mesh_data, height_origin))),
                MeshMaterial3d(material),
                Transform::from_translation(tile_origin + patch_offset),
                Name::new(format!(
                    "terrain-patch:{}:{}:{}:{}",
                    tile.tile_x, tile.tile_z, px, pz
                )),
            ));
            spawned += 1;
        }
    }
    (spawned, holed)
}

fn spawn_legacy_tile(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    tile: &TerrainFile,
    grid: &ElevationGrid,
    material: &Handle<StandardMaterial>,
    render_origin: Vec3,
    height_origin: f32,
) {
    let data = build_tile_mesh_data(grid, tile.samples.sample_size);
    let display_x = msts_display_tile_x_from_internal(tile.tile_x);
    let (wx, wz) = msts_tile_world_origin(display_x, tile.tile_z);
    let translation = Vec3::new(wx - render_origin.x, 0.0, wz - render_origin.z);
    commands.spawn((
        Mesh3d(meshes.add(mesh_from_terrain_data(&data, height_origin))),
        MeshMaterial3d(material.clone()),
        Transform::from_translation(translation),
        Name::new(format!("terrain:{}:{}", tile.tile_x, tile.tile_z)),
    ));
}

/// Spawn heightfield meshes for all terrain tiles; textured patches when `.y` includes patch sets.
#[allow(clippy::too_many_arguments)]
pub fn spawn_terrain_meshes(
    route_dir: Res<RouteAssets>,
    terrain: Res<TerrainScene>,
    focus: Res<crate::world::RouteFocus>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut std_materials: ResMut<Assets<StandardMaterial>>,
    mut terrain_materials: ResMut<Assets<TerrainMaterial>>,
) {
    if terrain.is_empty() {
        return;
    }

    viewer_log!(
        "openrailsrs-viewer3d: spawning terrain meshes ({} tile(s))",
        terrain.tiles.len()
    );
    let spawn_start = Instant::now();

    let fallback_material = std_materials.add(StandardMaterial {
        base_color: COLOR_TERRAIN_FALLBACK,
        perceptual_roughness: 0.95,
        metallic: 0.0,
        double_sided: false,
        ..default()
    });
    let fallback_tex = fallback_terrain_image(&mut images);
    let mut texture_cache: HashMap<String, Handle<Image>> = HashMap::new();

    let render_origin = focus.center;
    let height_origin = focus.height_origin;
    let mut spawned_tiles = 0usize;
    let mut spawned_patches = 0usize;
    let mut holed_patches = 0usize;

    for terrain_tile in &terrain.tiles {
        let path = &terrain_tile.path;
        let Ok(tile) =
            TerrainFile::from_path_with_coords(path, terrain_tile.tile_x, terrain_tile.tile_z)
        else {
            continue;
        };
        let Ok(grid) = read_y_raw(&tile.y_raw_path(path), &tile.samples) else {
            continue;
        };
        let features = if tile.samples.f_buffer_file.trim().is_empty() {
            None
        } else {
            read_f_raw(&tile.f_raw_path(path), &tile.samples).ok()
        };

        if std::env::var("OPENRAILSRS_TERRAIN_DEBUG").is_ok() {
            let min_h = grid.elevations.iter().cloned().fold(f32::MAX, f32::min);
            let max_h = grid.elevations.iter().cloned().fold(f32::MIN, f32::max);
            viewer_log!(
                "openrailsrs-viewer3d: terrain-debug tile {}:{} floor={:.2} scale={:.6} size={:.1} elev=[{:.1}..{:.1}] range={:.1}m",
                terrain_tile.tile_x,
                terrain_tile.tile_z,
                tile.samples.sample_floor,
                tile.samples.sample_scale,
                tile.samples.sample_size,
                min_h,
                max_h,
                max_h - min_h,
            );
        }

        if tile.has_textured_patches() {
            let (patches, holed) = spawn_textured_patches(
                &mut commands,
                &mut meshes,
                &mut terrain_materials,
                &mut images,
                &route_dir.route_dir,
                &tile,
                &grid,
                features.as_ref(),
                &mut texture_cache,
                &fallback_tex,
                render_origin,
                height_origin,
            );
            if patches > 0 {
                spawned_patches += patches;
                holed_patches += holed;
                spawned_tiles += 1;
                continue;
            }
        }

        spawn_legacy_tile(
            &mut commands,
            &mut meshes,
            &tile,
            &grid,
            &fallback_material,
            render_origin,
            height_origin,
        );
        spawned_tiles += 1;
    }

    if spawned_patches > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: {spawned_tiles} terrain tile(s), {spawned_patches} textured patch(es){}",
            if holed_patches > 0 {
                format!(" ({holed_patches} with holes)")
            } else {
                String::new()
            }
        );
    } else if spawned_tiles > 0 {
        viewer_log!("openrailsrs-viewer3d: {spawned_tiles} terrain tile(s) with heightfield mesh");
    }
    log_step("spawned terrain meshes", spawn_start);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    #[test]
    fn load_smoke_route_terrain_tile() {
        let route_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        let scene = load_terrain_from_route_dir(&route_dir);
        assert!(scene.tiles_loaded >= 1);
    }

    #[test]
    fn parse_world_w_tile_display_xz_chiltern_name() {
        let path = PathBuf::from("w-006074+014924.w");
        assert_eq!(parse_world_w_tile_display_xz(&path), Some((6074, 14924)));
    }

    #[test]
    fn chiltern_hash_tiles_discovered_from_world_and_center_grid() {
        let route_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        if !route_dir.join("TILES").is_dir() || !route_dir.join("WORLD").is_dir() {
            return;
        }
        let from_world = discover_terrain_tile_entries(&route_dir, None, f32::MAX);
        assert!(
            from_world.len() >= 50,
            "expected TILES from WORLD/*.w names, got {}",
            from_world.len()
        );
        // RouteFocus-style centre (positive display/world coords from `.w` bbox).
        let center = Vec3::new(
            6100.0 * MSTS_TILE_SIZE_M as f32 + 1024.0,
            0.0,
            14941.0 * MSTS_TILE_SIZE_M as f32 + 1024.0,
        );
        let near = discover_terrain_tile_entries(&route_dir, Some(center), 8_000.0);
        assert!(
            !near.is_empty(),
            "expected hash TILES near display tile (6100,14941), got {}",
            near.len()
        );
        let has_6100 = near
            .iter()
            .any(|(ix, iz, _)| msts_display_tile_x_from_internal(*ix) == 6100 && *iz == 14941);
        assert!(has_6100, "expected tile hash for display (6100,14941)");
    }

    #[test]
    fn chiltern_terrain_loads_near_route_focus() {
        let route_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        if !route_dir.join("TILES").is_dir() {
            return;
        }
        let world = crate::world::load_world_from_route_dir(&route_dir);
        let graph = openrailsrs_track::TrackGraph::new();
        let scene = TrackScene::from_graph(graph);
        let elevation = TerrainElevation::load_from_route_dir_near(&route_dir, None, f32::MAX);
        let focus = crate::world::RouteFocus::from_scene_world_and_elevation(
            &scene,
            &world,
            Some(&elevation),
        );
        let terrain = load_terrain_from_route_dir_near(&route_dir, Some(focus.center), 8_000.0);
        assert!(
            terrain.tiles_loaded >= 10,
            "expected terrain near focus {:?}, got {}",
            focus.center,
            terrain.tiles_loaded
        );
        // height_origin should come from the terrain sample, not the scenery bbox Y.
        // Once prefixed MSTS world tiles are parsed fully, the scenery bbox can move
        // to a low-elevation part of the route, so only require a finite terrain MSL.
        assert!(
            focus.height_origin.is_finite() && focus.height_origin >= 0.0,
            "Chiltern height_origin should be a terrain MSL value, got {}",
            focus.height_origin
        );
    }

    #[test]
    fn smoke_tile_has_textured_patches() {
        let route_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        let path = route_dir.join("TERRAIN/+000000+000000.y");
        let tile = TerrainFile::from_path(&path).expect("parse");
        assert!(tile.has_textured_patches());
        assert_eq!(tile.shaders[0].texslots.len(), 2);
    }

    #[test]
    fn elevation_samples_smoke_tile() {
        let route_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        let elev = TerrainElevation::load_from_route_dir(&route_dir);
        assert!(!elev.is_empty());
        let y = elev.sample_world_y(100.0, 100.0).expect("sample");
        assert!(y.is_finite());
    }

    #[test]
    fn hidden_vertex_returns_none_for_elevation() {
        let route_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        let elev = TerrainElevation::load_from_route_dir(&route_dir);
        assert!(elev.sample_world_y(112.0, 112.0).is_none());
    }

    #[test]
    fn scenery_ground_y_uses_terrain_when_available() {
        let route_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        let elev = TerrainElevation::load_from_route_dir(&route_dir);
        let scene = TrackScene::from_graph(openrailsrs_track::TrackGraph::new());
        let focus = crate::world::RouteFocus {
            center: Vec3::ZERO,
            height_origin: 0.0,
        };
        let y = scenery_ground_y(Some(&elev), 120.0, 15.0, &scene, 0.0, &focus);
        let raw = elev.sample_world_y(120.0, 15.0).unwrap();
        assert!(y > raw);
    }

    #[test]
    fn scenery_ground_y_falls_back_without_terrain() {
        let scene = TrackScene::from_graph(openrailsrs_track::TrackGraph::new());
        let focus = crate::world::RouteFocus {
            center: Vec3::ZERO,
            height_origin: 0.0,
        };
        let y = scenery_ground_y(None, 10.0, 10.0, &scene, 4.5, &focus);
        assert!((y - 4.5).abs() < 1e-5);
    }

    #[test]
    fn scenery_ground_y_fallback_converts_scenery_y_to_msl() {
        let scene = TrackScene::from_graph(openrailsrs_track::TrackGraph::new());
        let focus = crate::world::RouteFocus {
            center: Vec3::new(12_494_846.0, 82.0, 30_600_240.0),
            height_origin: 13_184.0,
        };
        let y = scenery_ground_y(None, 10.0, 10.0, &scene, 55.0, &focus);
        assert!(
            (y - 13_157.0).abs() < 1.0,
            "tile-local scenery y must map to MSL, got {y}"
        );
    }

    #[test]
    fn neighbor_tile_loads_for_seam_fixture() {
        let route_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        let scene = load_terrain_from_route_dir(&route_dir);
        assert!(scene.tiles.iter().any(|t| t.tile_x == 1 && t.tile_z == 0));
    }

    #[test]
    fn terrain_patch_offset_is_index_times_128m() {
        assert_eq!(terrain_patch_offset_in_tile(0, 0), Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(
            terrain_patch_offset_in_tile(1, 0),
            Vec3::new(128.0, 0.0, 0.0)
        );
        assert_eq!(
            terrain_patch_offset_in_tile(0, 1),
            Vec3::new(0.0, 0.0, 128.0)
        );
    }

    #[test]
    fn mesh_from_terrain_data_rebases_msl_y() {
        let data = TerrainMeshData {
            positions: vec![[10.0, 13_200.0, 20.0]],
            normals: vec![[0.0, 1.0, 0.0]],
            uvs: vec![[0.0, 0.0]],
            indices: vec![0, 0, 0],
        };
        let mesh = mesh_from_terrain_data(&data, 13_184.0);
        let pos = mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap();
        let VertexAttributeValues::Float32x3(vals) = pos else {
            panic!("expected positions");
        };
        assert!((vals[0][1] - 16.0).abs() < 1e-3);
    }

    #[test]
    fn tile_translation_only_rebases_xz() {
        let render_origin = Vec3::new(12_494_846.0, 82.0, 30_600_240.0);
        let wx = 12_494_000.0;
        let wz = 30_599_000.0;
        let t = Vec3::new(wx - render_origin.x, 0.0, wz - render_origin.z);
        assert!(t.x.abs() < 2000.0 && t.z.abs() < 2000.0);
        assert_eq!(t.y, 0.0);
    }
}
