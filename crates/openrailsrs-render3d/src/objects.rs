//! Capa 4a: objetos del `.w` como marcadores.
//!
//! Thin adapter over [`openrailsrs_bevy_scenery::MstsTileSnapshot`] / classified
//! WORLD items (#112). Places markers in the same centered local frame as terrain.

use std::path::Path;

use bevy::math::{Quat, Vec3};
use openrailsrs_bevy_scenery::{
    MstsClassifiedWorldItem, MstsForestPatch, MstsHWaterPatch, MstsLoadDiagnostics,
    MstsTransferPatch, MstsWorldItemKind, classify_world_file, load_msts_tile_world_snapshot,
};
use openrailsrs_formats::{WorldFile, parse_world_w_tile_xz};

/// Tipo de objeto del `.w` (alias del kind canónico #112).
pub type ObjectKind = MstsWorldItemKind;

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

pub type ForestPatch = MstsForestPatch;
pub type HWaterPatch = MstsHWaterPatch;
pub type TransferPatch = MstsTransferPatch;

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
    /// Authored Dyntrack subsections (#87).
    pub dyntrack_sections: Vec<openrailsrs_formats::DyntrackSection>,
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
    let snap = load_msts_tile_world_snapshot(route_dir, tile_x, tile_z);
    if let Some(d) = diag.as_deref_mut() {
        d.merge_from(&snap.diag);
    }
    let Some(world) = snap.world.as_ref() else {
        return Vec::new();
    };
    object_markers_from_classified(&world.items, base_y)
}

/// Materialize WORLD objects from an already-parsed [`WorldFile`] (#53 / #112).
pub fn objects_from_world_file(
    world: &WorldFile,
    route_dir: &Path,
    base_y: f32,
) -> Vec<ObjectMarker> {
    let items = classify_world_file(world, Some(route_dir));
    object_markers_from_classified(&items, base_y)
}

/// Convert classified snapshot items into render3d local-frame markers.
pub fn object_markers_from_classified(
    items: &[MstsClassifiedWorldItem],
    base_y: f32,
) -> Vec<ObjectMarker> {
    items
        .iter()
        .map(|item| {
            // `.w` es tile-local (X este, +Z "adelante"); render niega Z.
            let position = Vec3::new(
                item.position[0] as f32,
                item.position[1] as f32 - base_y,
                -(item.position[2] as f32),
            );
            ObjectMarker {
                position,
                rotation: item.rotation,
                scale: item.scale,
                kind: item.kind,
                file_name: item.file_name.clone(),
                section_idx: item.section_idx,
                dyntrack_sections: item.dyntrack_sections.clone(),
                forest: item.forest.clone(),
                hwater: item.hwater.clone(),
                transfer: item.transfer.clone(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn chiltern_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern")
    }

    #[test]
    fn chiltern_tile_objects_load_inside_tile() {
        let dir = chiltern_dir();
        // Tile poblado de Chiltern (w-006082+014925.w, ~2.3 MB).
        let (tx, tz) = (-6082, 14925);
        if openrailsrs_bevy_scenery::resolve_world_tile_path(&dir, tx, tz).is_none() {
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
        if openrailsrs_bevy_scenery::resolve_world_tile_path(&dir, tx, tz).is_none() {
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
            dyntrack_sections: Vec::new(),
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
            dyntrack_sections: Vec::new(),
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
            dyntrack_sections: Vec::new(),
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
            dyntrack_sections: Vec::new(),
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
            dyntrack_sections: Vec::new(),
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
