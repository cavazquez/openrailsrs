//! Capa 4a: objetos del `.w` como marcadores.
//!
//! Parsea el world tile (`.w`) con `openrailsrs-formats` y coloca un marcador
//! (pilar) por objeto en su posición y rotación, en el mismo espacio local
//! centrado que el terreno. El objetivo es validar la **ubicación** (que no
//! queden bajo el terreno ni desplazados) antes de cargar las mallas `.s` reales.

use std::path::{Path, PathBuf};

use bevy::math::{Mat3, Quat, Vec3};
use openrailsrs_formats::{
    WorldFile, WorldItem, parse_world_w_tile_xz, world_w_filename_from_tile_xz,
};

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
    Other,
}

impl ObjectKind {
    fn from_item(item: &WorldItem) -> Self {
        if let WorldItem::Other { tag, .. } = item {
            if tag.eq_ignore_ascii_case("Pickup") {
                return Self::Pickup;
            }
        }
        match item.kind() {
            "Static" => Self::Static,
            "TrackObj" => Self::Track,
            "Dyntrack" => Self::Dyntrack,
            "Signal" => Self::Signal,
            "Forest" => Self::Forest,
            "HWater" => Self::HWater,
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
            Self::Other => (0.65, 0.65, 0.65),
        }
    }
}

/// ¿Este objeto del `.w` debe instanciarse como shape `.s`?
pub fn wants_shape_mesh(kind: ObjectKind, file_name: Option<&str>) -> bool {
    if matches!(
        kind,
        ObjectKind::Forest | ObjectKind::Dyntrack | ObjectKind::HWater
    ) {
        return false;
    }
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
pub struct ObjectMarker {
    /// Posición en coords locales (X este, Y arriba re-basado, Z sur).
    pub position: Vec3,
    pub rotation: Quat,
    /// Escala no uniforme (de `Matrix3x3`); `ONE` para `QDirection`.
    pub scale: Vec3,
    pub kind: ObjectKind,
    /// Archivo de shape (`SHAPES/*.s`) a instanciar, si lo tiene.
    pub file_name: Option<String>,
}

/// Tile `(x, z)` del `.w` más grande de la ruta (el más poblado de objetos).
/// Útil como tile por defecto: suele tener terreno, vía y objetos juntos.
pub fn busiest_world_tile(route_dir: &Path) -> Option<(i32, i32)> {
    let mut best: Option<(u64, (i32, i32))> = None;
    for subdir in ["WORLD", "world"] {
        let Ok(rd) = std::fs::read_dir(route_dir.join(subdir)) else {
            continue;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().and_then(|x| x.to_str()) != Some("w") {
                continue;
            }
            let Some(tile) = parse_world_w_tile_xz(&path) else {
                continue;
            };
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            if best.is_none_or(|(s, _)| size > s) {
                best = Some((size, tile));
            }
        }
    }
    best.map(|(_, tile)| tile)
}

/// Ruta del `.w` del tile dentro de `WORLD/` (case-insensitive).
fn world_path(route_dir: &Path, tile_x: i32, tile_z: i32) -> Option<PathBuf> {
    let name = world_w_filename_from_tile_xz(tile_x, tile_z);
    for subdir in ["WORLD", "world"] {
        let path = route_dir.join(subdir).join(&name);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

/// Carga los objetos del `.w` del tile en coords locales centradas.
///
/// `base_y` es la altura MSL que corresponde a Y=0 local (mínimo del tile),
/// para convertir la Y absoluta de cada objeto al espacio del terreno.
pub fn load_objects(route_dir: &Path, tile_x: i32, tile_z: i32, base_y: f32) -> Vec<ObjectMarker> {
    let Some(path) = world_path(route_dir, tile_x, tile_z) else {
        return Vec::new();
    };
    let Ok(world) = WorldFile::from_path(&path) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for item in &world.items {
        let Some(p) = item.position() else {
            continue;
        };
        // `.w` es tile-local (X este, +Z "adelante"); render niega Z. Como el
        // tile está centrado, la parte de tile se cancela: local = (x, y, -z).
        let position = Vec3::new(p.x as f32, p.y as f32 - base_y, -(p.z as f32));
        let (rotation, scale) = item_transform(item);
        out.push(ObjectMarker {
            position,
            rotation,
            scale,
            kind: ObjectKind::from_item(item),
            file_name: item.file_name().map(str::to_string),
        });
    }
    out
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
    fn wants_shape_mesh_filters_non_shapes() {
        assert!(!wants_shape_mesh(ObjectKind::Forest, Some("MSBirch.ace")));
        assert!(!wants_shape_mesh(ObjectKind::HWater, Some("water.ace")));
        assert!(!wants_shape_mesh(ObjectKind::Static, None));
        assert!(!wants_shape_mesh(
            ObjectKind::Pickup,
            Some("rf_gw_watercolumn.s")
        ));
        assert!(!wants_shape_mesh(ObjectKind::Static, Some("smoke1.s")));
        assert!(!wants_shape_mesh(ObjectKind::Static, Some("smoke3.s")));
        assert!(wants_shape_mesh(ObjectKind::Pickup, Some("rf_gw_coal.s")));
    }
}
