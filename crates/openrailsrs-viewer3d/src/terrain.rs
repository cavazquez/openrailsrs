//! MSTS terrain tiles: heightfield meshes from `.y` + `_Y.RAW` (order 8 / issue #8).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use openrailsrs_formats::{
    ElevationGrid, TerrainFile, TerrainMeshData, build_tile_mesh_data, read_y_raw,
};

use crate::track::TrackScene;
use crate::world::MSTS_TILE_SIZE_M;

#[derive(Clone)]
struct TileElevation {
    grid: ElevationGrid,
    sample_size: f64,
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
            tiles.insert(
                (tile.tile_x, tile.tile_z),
                TileElevation {
                    grid,
                    sample_size: tile.samples.sample_size,
                },
            );
        }
        Self { tiles }
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    /// World-space elevation (metres) at `(x, z)`; `None` if no tile covers the point.
    pub fn sample_world_y(&self, x: f32, z: f32) -> Option<f32> {
        let tile_x = (x / MSTS_TILE_SIZE_M as f32).floor() as i32;
        let tile_z = (z / MSTS_TILE_SIZE_M as f32).floor() as i32;
        let tile = self.tiles.get(&(tile_x, tile_z))?;
        let lx = x - tile_x as f32 * MSTS_TILE_SIZE_M as f32;
        let lz = z - tile_z as f32 * MSTS_TILE_SIZE_M as f32;
        Some(
            tile.grid
                .sample_bilinear(lx as f64, lz as f64, tile.sample_size),
        )
    }
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

/// Scan `route_dir/TERRAIN/` (or `terrain/`) for `.y` tiles and parse metadata.
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

fn load_tile_mesh(_route_dir: &Path, tile_path: &Path) -> Option<Mesh> {
    let tile = TerrainFile::from_path(tile_path).ok()?;
    let raw_path = tile.y_raw_path(tile_path);
    if !raw_path.is_file() {
        eprintln!(
            "openrailsrs-viewer3d: terrain raw missing {}",
            raw_path.display()
        );
        return None;
    }
    let grid = read_y_raw(&raw_path, &tile.samples).ok()?;
    let data = build_tile_mesh_data(&grid, tile.samples.sample_size);
    Some(mesh_from_terrain_data(&data))
}

/// Spawn heightfield meshes for all terrain tiles; skips the flat ground plane caller when non-empty.
pub fn spawn_terrain_meshes(
    route_dir: Res<crate::shapes::RouteAssets>,
    terrain: Res<TerrainScene>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if terrain.is_empty() {
        return;
    }

    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.28, 0.42, 0.22),
        perceptual_roughness: 0.95,
        metallic: 0.0,
        double_sided: false,
        ..default()
    });

    let mut paths = discover_terrain_files(&route_dir.route_dir);
    paths.sort();

    let mut spawned = 0usize;
    for path in paths {
        let Some(mesh) = load_tile_mesh(&route_dir.route_dir, &path) else {
            continue;
        };
        let tile = match TerrainFile::from_path(&path) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let translation = Vec3::new(
            tile.tile_x as f32 * MSTS_TILE_SIZE_M as f32,
            0.0,
            tile.tile_z as f32 * MSTS_TILE_SIZE_M as f32,
        );

        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(translation),
            Name::new(format!("terrain:{}:{}", tile.tile_x, tile.tile_z)),
        ));
        spawned += 1;
    }

    if spawned > 0 {
        eprintln!("openrailsrs-viewer3d: {spawned} terrain tile(s) with heightfield mesh");
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
        assert_eq!(scene.tiles_loaded, 1);
        assert_eq!(scene.tiles[0].tile_x, 0);
        assert_eq!(scene.tiles[0].tile_z, 0);
    }

    #[test]
    fn smoke_tile_mesh_has_triangles() {
        let route_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        let path = route_dir.join("TERRAIN/+000000+000000.y");
        let mesh = load_tile_mesh(&route_dir, &path).expect("mesh");
        let verts = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .map(|a| a.len())
            .unwrap_or(0);
        assert!(verts > 0);
        assert!(mesh.indices().is_some());
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
}
