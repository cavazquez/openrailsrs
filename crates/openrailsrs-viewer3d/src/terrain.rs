//! MSTS terrain tiles: heightfield meshes from `.y` + `_Y.RAW` (order 8 / issue #8, PR2 textures).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use openrailsrs_formats::{
    ElevationGrid, FeatureGrid, TerrainFile, TerrainMeshData, build_patch_mesh_data_ex,
    build_tile_mesh_data, read_f_raw, read_y_raw,
};

use crate::shapes::RouteAssets;
use crate::terrain_assets::terrain_material_textures;
use crate::terrain_material::TerrainMaterial;
use crate::track::TrackScene;
use crate::world::MSTS_TILE_SIZE_M;

const COLOR_TERRAIN_FALLBACK: Color = Color::srgb(0.28, 0.42, 0.22);

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
    /// Load `_Y.RAW` grids for every `.y` tile under the route.
    pub fn load_from_route_dir(route_dir: &Path) -> Self {
        let mut tiles = HashMap::new();
        let mut paths = discover_terrain_files(route_dir);
        paths.sort();
        for path in paths {
            let Ok(tile) = TerrainFile::from_path(&path) else {
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

    fn sample_hidden(&self, tile_x: i32, tile_z: i32, x: f32, z: f32) -> bool {
        let Some(tile) = self.tiles.get(&(tile_x, tile_z)) else {
            return false;
        };
        let Some(features) = tile.features.as_ref() else {
            return false;
        };
        let lx = x - tile_x as f32 * MSTS_TILE_SIZE_M as f32;
        let lz = z - tile_z as f32 * MSTS_TILE_SIZE_M as f32;
        let ux = (lx / tile.sample_size as f32).round() as usize;
        let uz = (lz / tile.sample_size as f32).round() as usize;
        features.is_vertex_hidden(ux, uz)
    }

    /// World-space elevation (metres) at `(x, z)`; `None` if no tile covers the point or vertex is hidden.
    pub fn sample_world_y(&self, x: f32, z: f32) -> Option<f32> {
        let tile_x = (x / MSTS_TILE_SIZE_M as f32).floor() as i32;
        let tile_z = (z / MSTS_TILE_SIZE_M as f32).floor() as i32;
        if self.sample_hidden(tile_x, tile_z, x, z) {
            return None;
        }
        let tile = self.tiles.get(&(tile_x, tile_z))?;
        let lx = x - tile_x as f32 * MSTS_TILE_SIZE_M as f32;
        let lz = z - tile_z as f32 * MSTS_TILE_SIZE_M as f32;
        Some(
            tile.grid
                .sample_bilinear(lx as f64, lz as f64, tile.sample_size),
        )
    }
}

/// Scenery anchor height: terrain sample plus a small clearance, else MSTS `Position.y`.
pub fn scenery_ground_y(
    terrain: Option<&TerrainElevation>,
    x: f32,
    z: f32,
    scene: &TrackScene,
    fallback_y: f32,
) -> f32 {
    let lift = scene.bounds.edge_radius().max(1.0) * 0.04;
    terrain
        .and_then(|t| t.sample_world_y(x, z))
        .map(|h| h + lift)
        .unwrap_or(fallback_y)
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

fn mesh_from_terrain_data(data: &TerrainMeshData) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, data.positions.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, data.normals.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, data.uvs.clone());
    mesh.insert_indices(Indices::U32(data.indices.clone()));
    mesh
}

/// Scan `route_dir/TERRAIN/` and `route_dir/terrain/` for `.y` tiles and parse metadata.
pub fn load_terrain_from_route_dir(route_dir: &Path) -> TerrainScene {
    let mut paths = discover_terrain_files(route_dir);
    paths.sort();

    let mut scene = TerrainScene::default();
    for path in paths {
        match TerrainFile::from_path(&path) {
            Ok(tile) => {
                scene.tiles_loaded += 1;
                scene.tiles.push(TerrainTile {
                    tile_x: tile.tile_x,
                    tile_z: tile.tile_z,
                    translation: Vec3::new(
                        tile.tile_x as f32 * MSTS_TILE_SIZE_M as f32,
                        0.0,
                        tile.tile_z as f32 * MSTS_TILE_SIZE_M as f32,
                    ),
                });
            }
            Err(err) => {
                eprintln!(
                    "openrailsrs-viewer3d: skip terrain {} ({err})",
                    path.display()
                );
            }
        }
    }
    scene
}

fn discover_terrain_files(route_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for subdir in ["TERRAIN", "terrain"] {
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
                .is_some_and(|e| e.eq_ignore_ascii_case("y"))
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
) -> (usize, usize) {
    let patch_set = match tile.primary_patch_set() {
        Some(set) => set,
        None => return (0, 0),
    };
    let tile_origin = Vec3::new(
        tile.tile_x as f32 * MSTS_TILE_SIZE_M as f32,
        0.0,
        tile.tile_z as f32 * MSTS_TILE_SIZE_M as f32,
    );
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

            let (cx, cz) = patch.patch_translation();
            commands.spawn((
                Mesh3d(meshes.add(mesh_from_terrain_data(&mesh_data))),
                MeshMaterial3d(material),
                Transform::from_translation(tile_origin + Vec3::new(cx, 0.0, cz)),
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
) {
    let data = build_tile_mesh_data(grid, tile.samples.sample_size);
    let translation = Vec3::new(
        tile.tile_x as f32 * MSTS_TILE_SIZE_M as f32,
        0.0,
        tile.tile_z as f32 * MSTS_TILE_SIZE_M as f32,
    );
    commands.spawn((
        Mesh3d(meshes.add(mesh_from_terrain_data(&data))),
        MeshMaterial3d(material.clone()),
        Transform::from_translation(translation),
        Name::new(format!("terrain:{}:{}", tile.tile_x, tile.tile_z)),
    ));
}

/// Spawn heightfield meshes for all terrain tiles; textured patches when `.y` includes patch sets.
pub fn spawn_terrain_meshes(
    route_dir: Res<RouteAssets>,
    terrain: Res<TerrainScene>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut std_materials: ResMut<Assets<StandardMaterial>>,
    mut terrain_materials: ResMut<Assets<TerrainMaterial>>,
) {
    if terrain.is_empty() {
        return;
    }

    let fallback_material = std_materials.add(StandardMaterial {
        base_color: COLOR_TERRAIN_FALLBACK,
        perceptual_roughness: 0.95,
        metallic: 0.0,
        double_sided: false,
        ..default()
    });
    let fallback_tex = fallback_terrain_image(&mut images);
    let mut texture_cache: HashMap<String, Handle<Image>> = HashMap::new();

    let mut paths = discover_terrain_files(&route_dir.route_dir);
    paths.sort();

    let mut spawned_tiles = 0usize;
    let mut spawned_patches = 0usize;
    let mut holed_patches = 0usize;

    for path in paths {
        let Ok(tile) = TerrainFile::from_path(&path) else {
            continue;
        };
        let Ok(grid) = read_y_raw(&tile.y_raw_path(&path), &tile.samples) else {
            continue;
        };
        let features = if tile.samples.f_buffer_file.trim().is_empty() {
            None
        } else {
            read_f_raw(&tile.f_raw_path(&path), &tile.samples).ok()
        };

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
            );
            if patches > 0 {
                spawned_patches += patches;
                holed_patches += holed;
                spawned_tiles += 1;
                continue;
            }
        }

        spawn_legacy_tile(&mut commands, &mut meshes, &tile, &grid, &fallback_material);
        spawned_tiles += 1;
    }

    if spawned_patches > 0 {
        eprintln!(
            "openrailsrs-viewer3d: {spawned_tiles} terrain tile(s), {spawned_patches} textured patch(es){}",
            if holed_patches > 0 {
                format!(" ({holed_patches} with holes)")
            } else {
                String::new()
            }
        );
    } else if spawned_tiles > 0 {
        eprintln!("openrailsrs-viewer3d: {spawned_tiles} terrain tile(s) with heightfield mesh");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_smoke_route_terrain_tile() {
        let route_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        let scene = load_terrain_from_route_dir(&route_dir);
        assert!(scene.tiles_loaded >= 1);
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
        let y = scenery_ground_y(Some(&elev), 120.0, 15.0, &scene, 0.0);
        let raw = elev.sample_world_y(120.0, 15.0).unwrap();
        assert!(y > raw);
    }

    #[test]
    fn scenery_ground_y_falls_back_without_terrain() {
        let scene = TrackScene::from_graph(openrailsrs_track::TrackGraph::new());
        let y = scenery_ground_y(None, 10.0, 10.0, &scene, 4.5);
        assert!((y - 4.5).abs() < 1e-5);
    }

    #[test]
    fn neighbor_tile_loads_for_seam_fixture() {
        let route_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        let scene = load_terrain_from_route_dir(&route_dir);
        assert!(scene.tiles.iter().any(|t| t.tile_x == 1 && t.tile_z == 0));
    }
}
