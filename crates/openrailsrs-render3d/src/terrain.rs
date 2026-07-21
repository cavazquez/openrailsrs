//! Carga y malla de UN tile de terreno MSTS, desde cero.
//!
//! Reutiliza solo el *parser de datos* de `openrailsrs-formats` (`TerrainFile`,
//! `read_y_raw`, `ElevationGrid`). Toda la geometría se construye aquí, sin
//! depender del viewer3d viejo.
//!
//! Convención de coordenadas (igual a Open Rails / XNA, ver `coordinates.rs` del
//! viewer viejo): X = este, Y = arriba, Z mundo = negado. Para un solo tile
//! centramos la malla en el origen, así que solo importa la forma y la escala.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use openrailsrs_bevy_scenery::{
    TERRAIN_PATCH_SIZE_M, terrain_patch_offset_centered, terrain_shader_overlay_scale,
};
use openrailsrs_formats::{
    ElevationGrid, TerrainFile, TerrainShader, build_patch_mesh_data_sampled,
    msts_tile_name_from_xz, parse_world_w_tile_xz, read_y_raw, terrain_patches_per_side,
};

/// Geometría de un patch de terreno (128 m), local al patch y re-basada a Y=0,
/// más su posición dentro del tile centrado y la textura base a aplicar.
#[derive(Clone, Debug)]
pub struct PatchGeometry {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    /// Desplazamiento del patch dentro del tile (ya centrado en el origen).
    pub offset: [f32; 3],
    /// Nombre del archivo de textura base (`TERRTEX/*.ace`), si el patch tiene shader.
    pub texture: Option<String>,
    /// Textura de detalle (segundo texslot o `microtex.ace`).
    pub overlay_texture: Option<String>,
    /// Escala UV del overlay (`terrain_uvcalcs[1].d`, default 32).
    pub overlay_scale: f32,
}

/// Muestreador de altura del tile, en el mismo espacio local centrado que la
/// malla (X/Z en ±lado/2, Y re-basado a 0). Lo usa la capa de vía para apoyar
/// los raíles sobre el terreno.
#[derive(Clone)]
pub struct TileHeight {
    grid: ElevationGrid,
    sample_size: f64,
    base_y: f32,
    half: f32,
}

impl TileHeight {
    /// Altura MSL (m) que corresponde a Y=0 local (mínimo del tile). Permite
    /// convertir alturas absolutas de `.w` a este espacio: `local = msl - base_y`.
    pub fn base_y(&self) -> f32 {
        self.base_y
    }

    /// Altura local (re-basada a 0) del terreno en `(local_x, local_z)`.
    pub fn local_y(&self, local_x: f32, local_z: f32) -> f32 {
        let mx = (local_x + self.half) as f64;
        let mz = (local_z + self.half) as f64;
        self.grid.sample_or_triangle(mx, mz, self.sample_size) - self.base_y
    }
}

/// Un tile de terreno texturizado, listo para renderizar (capa 2).
#[derive(Clone)]
pub struct TileGeometry {
    pub tile_x: i32,
    pub tile_z: i32,
    pub side_m: f32,
    pub min_y: f32,
    pub max_y: f32,
    pub patches: Vec<PatchGeometry>,
    pub height: TileHeight,
}

/// Carpeta `TILES/` de la ruta (New Forest usa `Tiles/`, etc.).
fn tiles_dir(route_dir: &Path) -> PathBuf {
    for name in ["TILES", "Tiles", "tiles"] {
        let path = route_dir.join(name);
        if path.is_dir() {
            return path;
        }
    }
    route_dir.join("TILES")
}

/// Carpeta `WORLD/` de la ruta, si existe.
fn world_dir(route_dir: &Path) -> Option<PathBuf> {
    for name in ["WORLD", "world"] {
        let path = route_dir.join(name);
        if path.is_dir() {
            return Some(path);
        }
    }
    None
}

/// Resuelve qué tile cargar: el indicado por el usuario, o el centroide de los
/// tiles que aparecen en `WORLD/*.w` (zona poblada de la ruta).
pub fn resolve_tile(route_dir: &Path, tile: Option<(i32, i32)>) -> Result<(i32, i32)> {
    if let Some(t) = tile {
        return Ok(t);
    }
    centroid_world_tile(route_dir)
        .context("no pude elegir un tile por defecto (¿hay WORLD/*.w en la ruta?)")
}

/// Centroide (redondeado) de los tiles presentes en `WORLD/*.w` o `world/*.w`.
fn centroid_world_tile(route_dir: &Path) -> Option<(i32, i32)> {
    let world = world_dir(route_dir)?;
    let mut sum_x = 0i64;
    let mut sum_z = 0i64;
    let mut n = 0i64;
    for entry in std::fs::read_dir(&world).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("w") {
            continue;
        }
        if let Some((tx, tz)) = parse_world_w_tile_xz(&path) {
            sum_x += tx as i64;
            sum_z += tz as i64;
            n += 1;
        }
    }
    if n == 0 {
        return None;
    }
    Some(((sum_x / n) as i32, (sum_z / n) as i32))
}

/// Ruta del `.t` (hash MSTS) para un tile dentro de `TILES/` / `Tiles/`.
fn tile_dot_t_path(route_dir: &Path, tile_x: i32, tile_z: i32) -> PathBuf {
    let hash = msts_tile_name_from_xz(tile_x, tile_z).to_ascii_lowercase();
    tiles_dir(route_dir).join(format!("{hash}.t"))
}

/// Ruta de un `.ace` o `.dds` dentro de `TERRTEX/` (case-insensitive), si existe.
/// Primero intenta el nombre exacto, luego el equivalente `.dds` si el original
/// era `.ace` y no se encontró.
pub fn resolve_terrtex_path(route_dir: &Path, file_name: &str) -> Option<PathBuf> {
    let base = Path::new(file_name).file_name()?.to_str()?;
    // 1. Intentar el nombre exacto (.ace u otro)
    for subdir in ["TERRTEX", "terrtex"] {
        let path = route_dir.join(subdir).join(base);
        if path.is_file() {
            return Some(path);
        }
        if let Some(resolved) = openrailsrs_formats::resolve_path_case_insensitive(&path) {
            return Some(resolved);
        }
    }
    // 2. Fallback: si el nombre termina en .ace, intentar con .dds
    let path_obj = Path::new(base);
    if path_obj.extension().map(|e| e.to_ascii_lowercase()) == Some(std::ffi::OsString::from("ace"))
    {
        let dds_name = path_obj
            .with_extension("dds")
            .to_string_lossy()
            .into_owned();
        for subdir in ["TERRTEX", "terrtex"] {
            let path = route_dir.join(subdir).join(&dds_name);
            if path.is_file() {
                return Some(path);
            }
            if let Some(resolved) = openrailsrs_formats::resolve_path_case_insensitive(&path) {
                return Some(resolved);
            }
        }
    }
    None
}

/// Carga un tile completo como patches texturizados, centrado en el origen y
/// re-basado a Y=0. Cada patch lleva el nombre de su textura base (sin resolver
/// todavía a píxeles: eso lo hace el render).
pub fn load_tile_geometry(route_dir: &Path, tile_x: i32, tile_z: i32) -> Result<TileGeometry> {
    let t_path = tile_dot_t_path(route_dir, tile_x, tile_z);
    if !t_path.is_file() {
        return Err(anyhow!(
            "no existe el tile {} ({}). ¿Está copiado en TILES/?",
            t_path.display(),
            msts_tile_name_from_xz(tile_x, tile_z)
        ));
    }
    let tile = TerrainFile::from_path_with_coords(&t_path, tile_x, tile_z)
        .map_err(|e| anyhow!("parse de {}: {e}", t_path.display()))?;
    let y_path = tile.y_raw_path(&t_path);
    let grid = read_y_raw(&y_path, &tile.samples)
        .map_err(|e| anyhow!("lectura de {}: {e}", y_path.display()))?;
    Ok(tile_geometry_from_elevation(tile_x, tile_z, &tile, grid))
}

/// Build [`TileGeometry`] from already-parsed terrain + elevation (#53).
pub fn tile_geometry_from_elevation(
    tile_x: i32,
    tile_z: i32,
    tile: &TerrainFile,
    grid: ElevationGrid,
) -> TileGeometry {
    let sample_size = tile.samples.sample_size;
    let (min_y, max_y) = elevation_range(&grid);
    let base_y = if min_y.is_finite() { min_y } else { 0.0 };

    let patches_per_side = terrain_patches_per_side(grid.nsamples);
    let half = patches_per_side as f32 * TERRAIN_PATCH_SIZE_M * 0.5;
    let patch_set = tile.primary_patch_set();

    let mut patches = Vec::with_capacity((patches_per_side * patches_per_side) as usize);
    for pz in 0..patches_per_side {
        for px in 0..patches_per_side {
            let patch = patch_set.and_then(|ps| ps.patch_at(px, pz));
            if patch.is_some_and(|p| !p.drawing_enabled()) {
                continue;
            }
            let shader = patch
                .and_then(|p| {
                    tile.shaders
                        .get(p.shader_index as usize)
                        .or_else(|| tile.shaders.first())
                })
                .or_else(|| tile.shaders.first());
            let texture = shader
                .and_then(|sh| sh.texslots.first())
                .map(|ts| ts.filename.clone());
            let overlay_texture = shader
                .and_then(|sh| sh.texslots.get(1).map(|ts| ts.filename.clone()))
                .or_else(|| Some("microtex.ace".to_string()));
            let overlay_scale = shader.map(shader_overlay_scale).unwrap_or(32.0);

            let md = build_patch_mesh_data_sampled(
                sample_size,
                px,
                pz,
                patch,
                false,
                |ux, uz| grid.elevation_at_clamped(ux as isize, uz as isize),
                |_ux, _uz| false,
            );
            let positions = md
                .positions
                .iter()
                .map(|[x, y, z]| [*x, *y - base_y, *z])
                .collect();

            let offset = terrain_patch_offset_centered(px, pz, half);
            patches.push(PatchGeometry {
                positions,
                normals: md.normals,
                uvs: md.uvs,
                indices: md.indices,
                offset: offset.to_array(),
                texture,
                overlay_texture,
                overlay_scale,
            });
        }
    }

    TileGeometry {
        tile_x,
        tile_z,
        side_m: patches_per_side as f32 * TERRAIN_PATCH_SIZE_M,
        min_y,
        max_y,
        patches,
        height: TileHeight {
            grid,
            sample_size,
            base_y,
            half,
        },
    }
}

fn elevation_range(grid: &ElevationGrid) -> (f32, f32) {
    grid.elevations
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &h| {
            (lo.min(h), hi.max(h))
        })
}

/// Escala UV del overlay desde `terrain_uvcalcs[1].d` (paridad OR TerrainMaterial).
pub fn shader_overlay_scale(shader: &TerrainShader) -> f32 {
    terrain_shader_overlay_scale(shader)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chiltern_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern")
    }

    #[test]
    fn elevation_range_tracks_min_and_max() {
        let grid = ElevationGrid {
            nsamples: 2,
            elevations: vec![100.0, 110.0, 120.0, 130.0],
        };
        let (min_y, max_y) = elevation_range(&grid);
        assert!((min_y - 100.0).abs() < 1e-4);
        assert!((max_y - 130.0).abs() < 1e-4);
    }

    #[test]
    fn chiltern_start_tile_loads_textured_patches() {
        let dir = chiltern_dir();
        // Tile del área de inicio (n3) de Chiltern; -11cf297c.t.
        let (tx, tz) = (-6084, 14924);
        if !tile_dot_t_path(&dir, tx, tz).is_file() {
            eprintln!("skip: tile de Chiltern no disponible");
            return;
        }
        let tile = load_tile_geometry(&dir, tx, tz).expect("cargar tile de inicio");
        assert!(
            (tile.side_m - 2048.0).abs() < 1.0,
            "lado ~2 km, fue {}",
            tile.side_m
        );
        assert!(tile.max_y > tile.min_y, "el terreno debe tener relieve");
        assert!(!tile.patches.is_empty(), "el tile debe tener patches");

        for p in &tile.patches {
            assert!(p.positions.iter().all(|v| v.iter().all(|c| c.is_finite())));
            assert_eq!(p.normals.len(), p.positions.len());
            assert_eq!(p.uvs.len(), p.positions.len());
            assert!(p.indices.iter().all(|&i| (i as usize) < p.positions.len()));
        }
        // Al menos un patch referencia una textura y esa textura existe en TERRTEX/.
        let textured = tile
            .patches
            .iter()
            .filter_map(|p| p.texture.as_deref())
            .next()
            .expect("algún patch con textura base");
        assert!(
            resolve_terrtex_path(&dir, textured).is_some(),
            "no encontré la textura {textured} en TERRTEX/"
        );
        let with_overlay = tile
            .patches
            .iter()
            .find(|p| p.overlay_texture.is_some())
            .expect("overlay en patches");
        assert_eq!(with_overlay.overlay_scale, 32.0);
    }

    #[test]
    fn resolve_default_tile_is_inside_route() {
        let dir = chiltern_dir();
        if !dir.join("WORLD").is_dir() {
            return;
        }
        let (tx, tz) = resolve_tile(&dir, None).expect("centroide");
        // Chiltern está alrededor de (-6080, 14930).
        assert!((-6200..=-5900).contains(&tx), "tile_x raro: {tx}");
        assert!((14700..=15100).contains(&tz), "tile_z raro: {tz}");
    }

    #[test]
    fn scene_tile_z_offset_matches_render_space() {
        use openrailsrs_formats::msts_tile_world_origin;

        let (cx, cz) = (-6144, 14900);
        let tile_size = 2048.0_f32;
        for (tz, label) in [(14899, "north"), (14901, "south")] {
            let (_, oz) = msts_tile_world_origin(cx, tz);
            let (_, oz_center) = msts_tile_world_origin(cx, cz);
            let render_delta = oz - oz_center;
            let scene_offset_z = (cz - tz) as f32 * tile_size;
            assert!(
                (render_delta - scene_offset_z).abs() < 0.01,
                "{label}: render Δz={render_delta} scene_offset={scene_offset_z}"
            );
        }
    }

    #[test]
    fn new_forest_tiles_subdir_loads_geometry() {
        let dir = std::env::var_os("NEW_FOREST_V3_ROUTE")
            .map(PathBuf::from)
            .or_else(|| {
                let home = std::env::var_os("HOME")?;
                let p = PathBuf::from(home).join("routes/NewForestRouteV3/Routes/Watersnake");
                p.join("Tiles").is_dir().then_some(p)
            });
        let Some(dir) = dir else {
            return;
        };
        // Tile del `.w` más grande en `world/` (busiest_world_tile).
        load_tile_geometry(&dir, -6131, 14898).expect("tile con .t en Tiles/");
    }
}
