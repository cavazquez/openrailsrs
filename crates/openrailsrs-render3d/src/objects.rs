//! Capa 4a: objetos del `.w` como marcadores.
//!
//! Parsea el world tile (`.w`) con `openrailsrs-formats` y coloca un marcador
//! (pilar) por objeto en su posición y rotación, en el mismo espacio local
//! centrado que el terreno. El objetivo es validar la **ubicación** (que no
//! queden bajo el terreno ni desplazados) antes de cargar las mallas `.s` reales.

use std::path::{Path, PathBuf};

use bevy::math::{Mat3, Quat, Vec3};
use openrailsrs_bevy_scenery::{MstsAssetKind, MstsLoadCause, MstsLoadDiagnostics};
use openrailsrs_formats::{WorldFile, WorldItem, parse_world_w_tile_xz};

/// Tipo de objeto del `.w` (para colorear el marcador).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ObjectKind {
    Static,
    Track,
    Dyntrack,
    Signal,
    Forest,
    HWater,
    Pickup,
    Transfer,
    Hazard,
    Other,
}

impl ObjectKind {
    fn from_item(item: &WorldItem) -> Self {
        match item.kind() {
            "Static" => Self::Static,
            "TrackObj" => Self::Track,
            "Dyntrack" => Self::Dyntrack,
            "Signal" => Self::Signal,
            "Forest" => Self::Forest,
            "HWater" => Self::HWater,
            "Transfer" => Self::Transfer,
            "Pickup" => Self::Pickup,
            "Hazard" => Self::Hazard,
            _ => Self::Other,
        }
    }

    /// Color RGB del marcador.
    pub fn color(self) -> (f32, f32, f32) {
        match self {
            Self::Static => (0.95, 0.55, 0.15),
            Self::Track => (0.20, 0.80, 0.85),
            Self::Dyntrack => (0.85, 0.25, 0.85),
            Self::Signal => (0.90, 0.15, 0.15),
            Self::Forest => (0.20, 0.70, 0.25),
            Self::HWater => (0.20, 0.40, 0.95),
            Self::Pickup => (0.55, 0.45, 0.35),
            Self::Transfer => (0.45, 0.72, 0.38),
            Self::Hazard => (0.85, 0.35, 0.25),
            Self::Other => (0.65, 0.65, 0.65),
        }
    }
}

/// Con al menos un `TrackObj` se omiten cinta `.tdb` y shapes UKFS (Open Rails).
pub const TDB_RIBBON_SUPPRESS_TRACK_OBJ_MIN: usize = 1;

pub fn count_track_objects(objs: &[ObjectMarker]) -> usize {
    objs.iter().filter(|o| o.kind == ObjectKind::Track).count()
}

pub fn tile_suppresses_tdb_ribbon(objs: &[ObjectMarker]) -> bool {
    count_track_objects(objs) >= TDB_RIBBON_SUPPRESS_TRACK_OBJ_MIN
}

/// Longitud en metros del patrón MSTS `ukfs_*_1x1200m_*.s` (diagnóstico / tests).
#[allow(dead_code)]
pub fn ukfs_length_from_shape_name(file_name: &str) -> Option<f32> {
    let lower = file_name.to_ascii_lowercase();
    let start = lower.find("1x").map(|i| i + 2)?;
    let rest = lower.get(start..)?;
    let end = rest.find('m')?;
    rest[..end].parse().ok()
}

/// ¿Este objeto del `.w` debe instanciarse como shape `.s`?
pub fn object_wants_shape_mesh(obj: &ObjectMarker) -> bool {
    if matches!(
        obj.kind,
        ObjectKind::Forest | ObjectKind::Dyntrack | ObjectKind::HWater | ObjectKind::Transfer
    ) {
        return false;
    }
    if obj.kind == ObjectKind::Track {
        return shape_file_name_is_mesh(obj.file_name.as_deref());
    }
    shape_file_name_is_mesh(obj.file_name.as_deref())
}
fn shape_file_name_is_mesh(file_name: Option<&str>) -> bool {
    file_name.is_some_and(|f| {
        let lower = f.to_ascii_lowercase();
        if is_animated_scenery_shape(&lower) {
            return false;
        }
        lower.ends_with(".s") && !lower.ends_with(".sd")
    })
}

/// Shapes que dependen de animación (humo, columnas de agua, etc.).
fn is_animated_scenery_shape(lower: &str) -> bool {
    lower.contains("watercolumn")
        || lower.starts_with("smoke")
        || (lower.contains("steam") && lower.ends_with(".s"))
}

#[derive(Clone, Debug)]
pub struct ForestPatch {
    pub uid: u32,
    pub population: u32,
    pub patch_half_x: f32,
    pub patch_half_z: f32,
    pub tree_width: f32,
    pub tree_height: f32,
    pub scale_min: f32,
    pub scale_max: f32,
    pub tree_texture: Option<String>,
}

#[derive(Clone, Debug)]
pub struct HWaterPatch {
    pub uid: u32,
    pub half_x: f32,
    pub half_z: f32,
    pub texture: Option<String>,
}

#[derive(Clone, Debug)]
pub struct TransferPatch {
    pub uid: u32,
    pub width: f32,
    pub height: f32,
    pub texture: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ObjectMarker {
    /// Posición en coords locales (X este, Y arriba re-basado, Z sur).
    pub position: Vec3,
    pub rotation: Quat,
    /// Escala no uniforme (de `Matrix3x3`); `ONE` para `QDirection`.
    pub scale: Vec3,
    pub kind: ObjectKind,
    /// Archivo de shape (`SHAPES/*.s`) a instanciar, si lo tiene.
    pub file_name: Option<String>,
    /// `TrackObj` → indice en `tsection.dat` cuando no hay `FileName`.
    pub section_idx: Option<u32>,
    pub forest: Option<ForestPatch>,
    pub hwater: Option<HWaterPatch>,
    pub transfer: Option<TransferPatch>,
}

/// Tile `(x, z)` del `.w` más grande de la ruta (el más poblado de objetos).
/// Útil como tile por defecto: suele tener terreno, vía y objetos juntos.
pub fn busiest_world_tile(route_dir: &Path) -> Option<(i32, i32)> {
    let mut best: Option<(u64, (i32, i32))> = None;
    for (_x, _z, path) in openrailsrs_formats::scan_world_tile_files(route_dir) {
        let Some(tile) = parse_world_w_tile_xz(&path) else {
            continue;
        };
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if best.is_none_or(|(s, _)| size > s) {
            best = Some((size, tile));
        }
    }
    best.map(|(_, tile)| tile)
}

/// Ruta del `.w` del tile dentro de `WORLD/` (case-insensitive).
fn world_path(route_dir: &Path, tile_x: i32, tile_z: i32) -> Option<PathBuf> {
    if let Some(path) = openrailsrs_formats::resolve_world_tile_file(route_dir, tile_x, tile_z) {
        return Some(path);
    }
    if tile_x == 0 && tile_z == 0 {
        // Algunas rutas de prueba usan `w-000000-000000.w` en lugar de `w+000000+000000.w`.
        for dir in openrailsrs_formats::world_subdirs(route_dir) {
            let candidate = dir.join("w-000000-000000.w");
            if let Some(resolved) = openrailsrs_formats::resolve_path_case_insensitive(&candidate) {
                if resolved.is_file() {
                    return Some(resolved);
                }
            }
        }
    }
    None
}

/// Carga los objetos del `.w` del tile en coords locales centradas.
///
/// `base_y` es la altura MSL que corresponde a Y=0 local (mínimo del tile),
/// para convertir la Y absoluta de cada objeto al espacio del terreno.
pub fn load_objects(route_dir: &Path, tile_x: i32, tile_z: i32, base_y: f32) -> Vec<ObjectMarker> {
    load_objects_with_diag(route_dir, tile_x, tile_z, base_y, None)
}

/// Like [`load_objects`], recording world tile success/failure into `#54` diagnostics.
pub fn load_objects_with_diag(
    route_dir: &Path,
    tile_x: i32,
    tile_z: i32,
    base_y: f32,
    mut diag: Option<&mut MstsLoadDiagnostics>,
) -> Vec<ObjectMarker> {
    // No `.w` for this tile is normal (sparse WORLD); do not count as a failed request.
    let Some(path) = world_path(route_dir, tile_x, tile_z) else {
        return Vec::new();
    };
    let world = match WorldFile::from_path(&path) {
        Ok(w) => {
            if let Some(d) = diag.as_deref_mut() {
                d.record_path_loaded(&path, MstsAssetKind::World);
            }
            w
        }
        Err(e) => {
            if let Some(d) = diag.as_deref_mut() {
                d.record_failed_at(
                    path.display().to_string(),
                    MstsAssetKind::World,
                    MstsLoadCause::Parse,
                    e.to_string(),
                    Some(tile_x),
                    Some(tile_z),
                );
            }
            return Vec::new();
        }
    };

    objects_from_world_file(&world, route_dir, base_y)
}

/// Materialize WORLD objects from an already-parsed [`WorldFile`] (#53).
pub fn objects_from_world_file(
    world: &WorldFile,
    route_dir: &Path,
    base_y: f32,
) -> Vec<ObjectMarker> {
    let mut out = Vec::new();
    for item in &world.items {
        let Some(p) = item.position() else {
            continue;
        };
        // `.w` es tile-local (X este, +Z "adelante"); render niega Z. Como el
        // tile está centrado, la parte de tile se cancela: local = (x, y, -z).
        let position = Vec3::new(p.x as f32, p.y as f32 - base_y, -(p.z as f32));
        let (rotation, scale) = item_transform(item);
        let (forest, hwater, transfer) = scenery_from_item(item);
        let file_name = match item {
            WorldItem::Hazard {
                haz_file: Some(haz),
                ..
            } => openrailsrs_formats::resolve_hazard_shape_name(route_dir, haz)
                .or_else(|| Some(haz.clone())),
            _ => item.file_name().map(str::to_string),
        };
        out.push(ObjectMarker {
            position,
            rotation,
            scale,
            kind: ObjectKind::from_item(item),
            file_name,
            section_idx: item.section_idx(),
            forest,
            hwater,
            transfer,
        });
    }
    out
}

fn scenery_from_item(
    item: &WorldItem,
) -> (
    Option<ForestPatch>,
    Option<HWaterPatch>,
    Option<TransferPatch>,
) {
    match item {
        WorldItem::Forest {
            uid,
            tree_texture,
            scale_range,
            patch_size,
            tree_size,
            population,
            ..
        } => {
            let (scale_min, scale_max) = scale_range
                .map(|r| (r[0].max(0.1) as f32, r[1].max(r[0] + 0.01) as f32))
                .unwrap_or((0.85, 1.15));
            let (patch_half_x, patch_half_z) = patch_size
                .map(|a| ((a[0] * 0.5) as f32, (a[1] * 0.5) as f32))
                .unwrap_or((128.0, 128.0));
            let (tree_width, tree_height) = tree_size
                .map(|s| (s[0].max(0.5) as f32, s[1].max(1.0) as f32))
                .unwrap_or((5.0, 12.0));
            (
                Some(ForestPatch {
                    uid: *uid,
                    population: *population,
                    patch_half_x,
                    patch_half_z,
                    tree_width,
                    tree_height,
                    scale_min,
                    scale_max,
                    tree_texture: tree_texture.clone(),
                }),
                None,
                None,
            )
        }
        WorldItem::HWater {
            uid,
            file_name,
            size,
            ..
        } => (
            None,
            Some(HWaterPatch {
                uid: *uid,
                half_x: (size[0].max(0.5) * 0.5) as f32,
                half_z: (size[1].max(0.5) * 0.5) as f32,
                texture: file_name.clone(),
            }),
            None,
        ),
        WorldItem::Transfer {
            uid,
            file_name,
            width,
            height,
            ..
        } => (
            None,
            None,
            Some(TransferPatch {
                uid: *uid,
                width: (*width).max(0.5) as f32,
                height: (*height).max(0.5) as f32,
                texture: file_name.clone(),
            }),
        ),
        _ => (None, None, None),
    }
}

/// Rotación + escala del objeto siguiendo la convención XNA de Open Rails.
fn item_transform(item: &WorldItem) -> (Quat, Vec3) {
    if let Some(m) = item.matrix3x3() {
        let (rot, scale) = matrix3x3_to_rotation_scale(&m);
        return (sanitize_quat(rot), scale);
    }
    let rot = item
        .qdirection()
        .map(|q| qdir_to_quat(&q))
        .unwrap_or(Quat::IDENTITY);
    (sanitize_quat(rot), Vec3::ONE)
}

/// Normaliza la rotación; si es inválida (NaN o casi nula), usa identidad.
/// Algunos objetos del `.w` traen matrices degeneradas.
fn sanitize_quat(q: Quat) -> Quat {
    if q.is_finite() && q.length_squared() > 1e-6 {
        q.normalize()
    } else {
        Quat::IDENTITY
    }
}

/// MSTS `QDirection` `[qx, qy, qz, qw]` → Bevy `Quat` (Z negada).
fn qdir_to_quat(q: &[f64; 4]) -> Quat {
    Quat::from_xyzw(q[0] as f32, q[1] as f32, -(q[2] as f32), q[3] as f32)
}

/// MSTS `Matrix3x3` → rotación Bevy + escala (convención XNA: Z de columnas X/Y
/// negada, X/Y de columna Z negadas).
fn matrix3x3_to_rotation_scale(m: &[f64; 9]) -> (Quat, Vec3) {
    let x = Vec3::new(m[0] as f32, m[1] as f32, -(m[2] as f32));
    let y = Vec3::new(m[3] as f32, m[4] as f32, -(m[5] as f32));
    let z = Vec3::new(-(m[6] as f32), -(m[7] as f32), m[8] as f32);
    let sx = x.length().max(1e-6);
    let sy = y.length().max(1e-6);
    let sz = z.length().max(1e-6);
    let rot = Quat::from_mat3(&Mat3::from_cols(x / sx, y / sy, z / sz));
    (rot, Vec3::new(sx, sy, sz))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chiltern_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern")
    }

    #[test]
    fn chiltern_tile_objects_load_inside_tile() {
        let dir = chiltern_dir();
        // Tile poblado de Chiltern (w-006082+014925.w, ~2.3 MB).
        let (tx, tz) = (-6082, 14925);
        if world_path(&dir, tx, tz).is_none() {
            eprintln!("skip: .w de Chiltern no disponible");
            return;
        }
        let objs = load_objects(&dir, tx, tz, 100.0);
        assert!(
            !objs.is_empty(),
            "el tile poblado debería tener objetos en su .w"
        );
        // Posiciones en coords locales sanas (no absolutas/render-world, que
        // estarían en millones) y rotaciones unitarias.
        for o in &objs {
            assert!(
                o.position.x.abs() <= 1500.0,
                "x fuera de tile: {}",
                o.position.x
            );
            assert!(
                o.position.z.abs() <= 1500.0,
                "z fuera de tile: {}",
                o.position.z
            );
            assert!(
                (o.rotation.length() - 1.0).abs() < 1e-3,
                "rotación no unitaria"
            );
        }
    }

    #[test]
    fn chiltern_water_columns_near_terrain() {
        use crate::terrain::load_tile_geometry;
        let dir = chiltern_dir();
        let (tx, tz) = (-6082, 14925);
        if world_path(&dir, tx, tz).is_none() {
            eprintln!("skip: .w no disponible");
            return;
        }
        let loaded = load_tile_geometry(&dir, tx, tz).expect("tile");
        let base = loaded.height.base_y();
        let objs = load_objects(&dir, tx, tz, base);
        let height = &loaded.height;
        for o in objs.iter().filter(|o| {
            o.file_name
                .as_deref()
                .is_some_and(|f| f.to_ascii_lowercase().contains("watercolumn"))
        }) {
            let terrain_y = height.local_y(o.position.x, o.position.z);
            let delta = o.position.y - terrain_y;
            eprintln!(
                "{:?} pos_y={:.2} terrain_y={:.2} delta={:.2}",
                o.file_name, o.position.y, terrain_y, delta
            );
            assert!(
                delta.abs() < 3.0,
                "columna de agua muy lejos del terreno (delta={delta:.2})"
            );
        }
    }

    #[test]
    fn smoke_world_tile_parses_forest_and_hwater() {
        let route =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        let w = route.join("WORLD/w-000000-000000.w");
        if !w.is_file() {
            return;
        }
        let objs = load_objects(&route, 0, 0, 0.0);
        assert!(objs.iter().any(|o| o.forest.is_some()));
        assert!(objs.iter().any(|o| o.hwater.is_some()));
    }

    #[test]
    fn watersnake_loaded_grid_trackobj_counts() {
        let route = std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|h| h.join("routes/NewForestRouteV3/Routes/Watersnake"))
            .filter(|p| p.join("world").is_dir());
        let Some(route) = route else {
            return;
        };
        let (cx, cz) = (-6144, 14900);
        let mut suppressed = 0usize;
        for dz in -2..=2 {
            for dx in -2..=2 {
                let (tx, tz) = (cx + dx, cz + dz);
                let objs = load_objects(&route, tx, tz, 0.0);
                let track = count_track_objects(&objs);
                if tile_suppresses_tdb_ribbon(&objs) {
                    suppressed += 1;
                }
                if track > 0 || objs.len() > 50 {
                    eprintln!("({tx},{tz}): total={} track={track}", objs.len());
                }
            }
        }
        eprintln!("watersnake grid: {suppressed} tiles suprimirían .tdb");
    }

    #[test]
    fn tile_suppresses_tdb_ribbon_with_enough_trackobj() {
        let mk = |kind| ObjectMarker {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
            kind,
            file_name: Some("ukfs.s".into()),
            section_idx: None,
            forest: None,
            hwater: None,
            transfer: None,
        };
        let objs: Vec<_> = (0..TDB_RIBBON_SUPPRESS_TRACK_OBJ_MIN)
            .map(|_| mk(ObjectKind::Track))
            .collect();
        assert!(tile_suppresses_tdb_ribbon(&objs));
        assert!(!tile_suppresses_tdb_ribbon(&[]));
    }

    #[test]
    fn wants_shape_mesh_filters_non_shapes() {
        let mk = |kind, file: Option<&str>, section_idx| ObjectMarker {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
            kind,
            file_name: file.map(str::to_string),
            section_idx,
            forest: None,
            hwater: None,
            transfer: None,
        };
        assert!(!object_wants_shape_mesh(&mk(
            ObjectKind::Forest,
            Some("MSBirch.ace"),
            None
        )));
        assert!(!object_wants_shape_mesh(&mk(
            ObjectKind::HWater,
            Some("water.ace"),
            None
        )));
        assert!(!object_wants_shape_mesh(&mk(
            ObjectKind::Static,
            None,
            None
        )));
        assert!(!object_wants_shape_mesh(&mk(
            ObjectKind::Pickup,
            Some("rf_gw_watercolumn.s"),
            None
        )));
        assert!(!object_wants_shape_mesh(&mk(
            ObjectKind::Static,
            Some("smoke1.s"),
            None
        )));
        assert!(!object_wants_shape_mesh(&mk(
            ObjectKind::Static,
            Some("smoke3.s"),
            None
        )));
        assert!(object_wants_shape_mesh(&mk(
            ObjectKind::Pickup,
            Some("rf_gw_coal.s"),
            None
        )));
    }

    #[test]
    fn trackobj_with_section_idx_uses_procedural_not_full_ukfs_mesh() {
        let obj = ObjectMarker {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
            kind: ObjectKind::Track,
            file_name: None,
            section_idx: Some(42),
            forest: None,
            hwater: None,
            transfer: None,
        };
        assert!(
            !object_wants_shape_mesh(&obj),
            "TrackObj sin FileName no instancia shape directo"
        );
        assert!(object_wants_shape_mesh(&ObjectMarker {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
            kind: ObjectKind::Track,
            file_name: Some("ukfs_s_1x25m.s".into()),
            section_idx: None,
            forest: None,
            hwater: None,
            transfer: None,
        }));
        assert!(object_wants_shape_mesh(&ObjectMarker {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
            kind: ObjectKind::Track,
            file_name: Some("ukfs_c_1x1200m_5d.s".into()),
            section_idx: Some(38597),
            forest: None,
            hwater: None,
            transfer: None,
        }));
        assert_eq!(
            ukfs_length_from_shape_name("ukfs_c_1x1200m_5d.s"),
            Some(1200.0)
        );
    }

    #[test]
    fn watersnake_trackobjs_spawn_ukfs_shapes_not_procedural() {
        use crate::terrain::load_tile_geometry;
        use crate::world_spawn::AssetIndex;

        let route = std::env::var_os("NEW_FOREST_V3_ROUTE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
                    .join("routes/NewForestRouteV3/Routes/Watersnake")
            });
        if !route.is_dir() {
            return;
        }
        let msts = route.ancestors().nth(2).unwrap_or(&route);
        let index = AssetIndex::build(&route, msts);
        let (tx, tz) = (-6144, 14900);
        let tile = load_tile_geometry(&route, tx, tz).expect("tile");
        let objs = load_objects(&route, tx, tz, tile.height.base_y());
        let tracks: Vec<_> = objs
            .iter()
            .filter(|o| o.kind == ObjectKind::Track)
            .collect();
        assert!(!tracks.is_empty(), "tile túnel debería tener TrackObj");
        for t in tracks {
            assert!(
                object_wants_shape_mesh(t),
                "TrackObj {:?} debería usar shape UKFS modular",
                t.file_name
            );
            assert!(
                !crate::world_spawn::trackobj_prefers_procedural_mesh(t, &index, &route, msts),
                "shape resuelve en GLOBAL: {:?}",
                t.file_name
            );
        }
    }

    #[test]
    fn tunnel_tile_tunnel_cliff_heights() {
        use crate::terrain::load_tile_geometry;

        let route = std::env::var_os("NEW_FOREST_V3_ROUTE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
                    .join("routes/NewForestRouteV3/Routes/Watersnake")
            });
        if !route.is_dir() {
            return;
        }
        let (tx, tz) = (-6144, 14900);
        let tile = load_tile_geometry(&route, tx, tz).expect("tile");
        let base = tile.height.base_y();
        let objs = load_objects(&route, tx, tz, base);
        let h = &tile.height;
        eprintln!("tile base_y={base:.2}");
        for key in ["chalkcliff", "ij_tunnel", "transfer"] {
            eprintln!("\n-- {key} --");
            for o in objs.iter().filter(|o| {
                o.file_name
                    .as_deref()
                    .is_some_and(|f| f.to_ascii_lowercase().contains(key))
                    || (key == "transfer" && o.kind == ObjectKind::Transfer)
            }) {
                let terrain = h.local_y(o.position.x, o.position.z);
                let delta = o.position.y - terrain;
                eprintln!(
                    "  {:?} kind={:?} pos=({:.1},{:.1},{:.1}) terrain_y={:.1} delta={:.1} scale={:?}",
                    o.file_name,
                    o.kind,
                    o.position.x,
                    o.position.y,
                    o.position.z,
                    terrain,
                    delta,
                    o.scale
                );
            }
        }
    }
}
